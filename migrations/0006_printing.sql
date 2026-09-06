-- 0006_printing：印表機、出單分區、路由、列印佇列。
--
-- ★ 這裡最重要的設計是「三層間接」：品項/分類 → 出單分區 → 印表機。
--
-- 把品項直接綁印表機是新手 POS 最常見的設計錯誤：換一台機器、加一台備援機，
-- 就要去改幾百筆菜單。分區這一層讓「飲料吧」是一個穩定的概念，
-- 底下綁哪台機器是設定問題。

CREATE TABLE printers (
  id            TEXT NOT NULL PRIMARY KEY,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  -- Transport 的 JSON 序列化（見 infra::printer::Transport，serde tag = "kind"）。
  -- 存 JSON 而不是攤平成欄位：USB 要 vid/pid、網路要 host/port、藍牙要 addr，
  -- 攤平會變成一堆互斥的可空欄位，而它們的組合合法性 Rust 端本來就要驗。
  transport     TEXT NOT NULL,
  -- PrinterCaps 的 JSON。紙寬、點寬、欄數、有無切刀、raster 分帶列數、中文碼頁。
  -- 便宜機的規格書常常對不上，所有欄位都要能在 UI 手動校正。
  caps          TEXT NOT NULL,
  -- raster 為預設：不依賴印表機內建字庫，換任何機出來都一樣，而且能做像素級快照測試。
  render_mode   TEXT NOT NULL DEFAULT 'raster' CHECK (render_mode IN ('raster', 'text')),
  is_active     INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  last_probe_at TEXT,
  last_probe_ok INTEGER,
  last_error    TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  deleted_at    TEXT
);
CREATE INDEX idx_printers_store ON printers(store_id) WHERE deleted_at IS NULL;

-- 出單分區＝廚房的一個工作站（飲料吧 / 熱炒區 / 燒烤區 / 櫃檯）。
CREATE TABLE print_stations (
  id             TEXT NOT NULL PRIMARY KEY,
  store_id       TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  name           TEXT NOT NULL,
  template       TEXT NOT NULL DEFAULT 'kitchen' CHECK (template IN ('kitchen', 'drink', 'receipt', 'label')),
  -- 每品項一張。燒烤與貼杯標籤的站常這樣（一張單跟著一份餐走）。
  split_per_item INTEGER NOT NULL DEFAULT 0 CHECK (split_per_item IN (0, 1)),
  sort_order     INTEGER NOT NULL DEFAULT 0,
  is_active      INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL,
  deleted_at     TEXT
);
CREATE INDEX idx_print_stations_store ON print_stations(store_id, sort_order) WHERE deleted_at IS NULL;

-- 分區 → 印表機（多對多 + 優先序 = 備援鏈）。
--
-- 「飲料吧的單優先印飲料吧那台，掛了就印櫃檯那台」是真實需求：
-- 現場寧可讓櫃檯多印一張跑腿送過去，也不能讓單消失。
-- mode = 'always' 則是「熱炒區要一份、櫃檯留底一份」。
CREATE TABLE station_printers (
  station_id TEXT NOT NULL REFERENCES print_stations(id) ON DELETE CASCADE,
  printer_id TEXT NOT NULL REFERENCES printers(id) ON DELETE CASCADE,
  priority   INTEGER NOT NULL DEFAULT 0,
  mode       TEXT NOT NULL DEFAULT 'failover' CHECK (mode IN ('failover', 'always')),
  created_at TEXT NOT NULL,
  PRIMARY KEY (station_id, printer_id)
);

-- 品項與分類的出單分區。三層預設：品項 > 分類 > 全域。
-- 只放品項層的話，新增 200 個飲料要設 200 次；只放分類層則店長設定不出來。
ALTER TABLE items ADD COLUMN station_id TEXT REFERENCES print_stations(id);
ALTER TABLE categories ADD COLUMN default_station_id TEXT REFERENCES print_stations(id);

-- 列印佇列。
CREATE TABLE print_jobs (
  id            TEXT NOT NULL PRIMARY KEY,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  printer_id    TEXT NOT NULL REFERENCES printers(id) ON DELETE RESTRICT,
  station_id    TEXT REFERENCES print_stations(id) ON DELETE SET NULL,
  terminal_id   TEXT REFERENCES terminals(id) ON DELETE SET NULL,
  business_date TEXT NOT NULL,
  order_id      TEXT REFERENCES orders(id) ON DELETE SET NULL,
  doc_type      TEXT NOT NULL CHECK (doc_type IN ('receipt', 'kitchen', 'label', 'shift_report', 'day_report', 'test')),
  -- 加點與退點在廚房是完全不同的動作。混在一起印會讓廚師重做整桌，
  -- 所以出單原因必須是一級欄位而不是塞在 payload 裡。
  reason        TEXT NOT NULL CHECK (reason IN ('new_order', 'add_items', 'void', 'reprint', 'settle')),
  -- ★ 存已排版的 ReceiptDoc 快照，而不是 order_id。
  --   這是資料一致性的關鍵，不是效能最佳化：補印時若重跑業務邏輯，
  --   印出來的會是「訂單被改過之後」的內容 —— 而廚房單是「當時的指令」。
  --   （收據重印才是重新排版，因為收據是「當前的事實」。）
  doc           TEXT NOT NULL,
  doc_sha256    TEXT NOT NULL,
  -- 防「收銀員手快連按兩次送出」。注意它防的是入隊重複，
  -- 不防傳輸層重試的重複 —— 後者是刻意接受的取捨：寧可重複，不可漏印。
  idempotency_key TEXT NOT NULL,
  priority      INTEGER NOT NULL DEFAULT 0,
  status        TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'printing', 'done', 'failed', 'dead', 'cancelled')),
  attempts      INTEGER NOT NULL DEFAULT 0,
  next_attempt_at TEXT NOT NULL,
  last_error    TEXT,
  -- transient（網路斷）/ needs_attention（缺紙）/ permanent（排版失敗）。
  -- 分三類而不是兩類：店員看到「飲料吧缺紙」會去換紙，看到「連線失敗」會去看網路線。
  last_error_class TEXT CHECK (last_error_class IN ('transient', 'needs_attention', 'permanent')),
  reprint_of_id TEXT REFERENCES print_jobs(id) ON DELETE SET NULL,
  reprint_seq   INTEGER NOT NULL DEFAULT 0,
  reprint_reason TEXT,
  created_by    TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  done_at       TEXT
);
CREATE UNIQUE INDEX uq_print_jobs_idempotency ON print_jobs(idempotency_key);
-- worker 熱路徑：撈「這台機該做的、時間到的」。
CREATE INDEX idx_print_jobs_ready ON print_jobs(printer_id, status, next_attempt_at);
CREATE INDEX idx_print_jobs_order ON print_jobs(order_id, created_at);
-- 需要人為處理的（死信 / 缺紙）。這個清單必須在 UI 上發出聲音 ——
-- POS 最常見的客訴是「廚房沒收到單」，而技術根因幾乎都是
-- 「系統知道印失敗了但沒告訴任何人」。
CREATE INDEX idx_print_jobs_attention ON print_jobs(store_id, status) WHERE status IN ('dead', 'failed');
