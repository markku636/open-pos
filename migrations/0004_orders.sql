-- 0004_orders：桌位、開桌、訂單、明細、折扣、事件。
--
-- 這是整份 schema 的核心，也是「補做會很痛」的東西集中的地方：
-- 明細快照、append-only 事件、business_date、shift_id、冪等鍵。
-- 這五樣沒有在第一天做進去，之後就再也拿不回來（見 docs/adr/ 與 README）。

CREATE TABLE areas (
  id         TEXT NOT NULL PRIMARY KEY,
  store_id   TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE INDEX idx_areas_store ON areas(store_id, sort_order) WHERE deleted_at IS NULL;

-- 桌位。
-- **刻意不叫 tables** —— PostgreSQL 有 information_schema.tables，
-- 在查詢與對話裡「tables 這張表」永遠會造成混淆。
CREATE TABLE dining_tables (
  id         TEXT NOT NULL PRIMARY KEY,
  store_id   TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  area_id    TEXT REFERENCES areas(id) ON DELETE SET NULL,
  code       TEXT NOT NULL,
  name       TEXT,
  seats      INTEGER NOT NULL DEFAULT 4,
  pos_x      INTEGER NOT NULL DEFAULT 0,
  pos_y      INTEGER NOT NULL DEFAULT 0,
  shape      TEXT NOT NULL DEFAULT 'rect' CHECK (shape IN ('rect', 'round')),
  is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE UNIQUE INDEX uq_dining_tables_code ON dining_tables(store_id, code) WHERE deleted_at IS NULL;
CREATE INDEX idx_dining_tables_area ON dining_tables(store_id, area_id) WHERE deleted_at IS NULL;

-- 開桌時段。一次「入座到清桌」是一個 session。
--
-- ★ partial unique index 硬性保證「一桌同時只有一個未關的 session」。
--   兩個收銀員同時點同一桌是真實會發生的事（一個在櫃檯、一個拿平板在外場），
--   這種競態不能靠應用層先查再寫來擋 —— 那中間就是競態窗口。
CREATE TABLE table_sessions (
  id            TEXT NOT NULL PRIMARY KEY,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  table_id      TEXT NOT NULL REFERENCES dining_tables(id) ON DELETE RESTRICT,
  business_date TEXT NOT NULL,
  guest_count   INTEGER NOT NULL DEFAULT 1,
  status        TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'bill_requested', 'closed')),
  opened_at     TEXT NOT NULL,
  opened_by     TEXT,
  closed_at     TEXT,
  closed_by     TEXT,
  merged_into_session_id TEXT REFERENCES table_sessions(id) ON DELETE SET NULL,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_table_sessions_open ON table_sessions(table_id) WHERE status <> 'closed';
CREATE INDEX idx_table_sessions_date ON table_sessions(business_date, status);

-- 訂單。
CREATE TABLE orders (
  id               TEXT NOT NULL PRIMARY KEY,
  store_id         TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
  terminal_id      TEXT REFERENCES terminals(id) ON DELETE SET NULL,
  shift_id         TEXT,
  business_date    TEXT NOT NULL,
  order_no         TEXT NOT NULL,
  -- 冪等鍵：由送單端（收銀機 / 顧客手機 / 離線 KDS）產生的 ULID。
  -- 這不是為了「主機掛掉」，是為了每天都在發生的 Wi-Fi 抖動。
  client_id        TEXT,
  -- 樂觀鎖。讀取回應帶 rev，寫入請求帶 expected_rev，不符回 409 讓前端重讀。
  rev              INTEGER NOT NULL DEFAULT 0,
  channel          TEXT NOT NULL DEFAULT 'dine_in' CHECK (channel IN ('dine_in', 'takeout', 'delivery')),
  source           TEXT NOT NULL DEFAULT 'pos' CHECK (source IN ('pos', 'kiosk', 'qr', 'online')),
  table_id         TEXT REFERENCES dining_tables(id) ON DELETE SET NULL,
  table_session_id TEXT REFERENCES table_sessions(id) ON DELETE SET NULL,
  guest_count      INTEGER NOT NULL DEFAULT 1,
  status           TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'placed', 'in_progress', 'ready', 'served', 'settled', 'voided')),
  -- 金額（整數元）。全部由 Rust 的定價引擎算完一次寫入；
  -- 資料庫端不做 GENERATED / TRIGGER 計算 —— 兩種資料庫語法不同且難測。
  subtotal             INTEGER NOT NULL DEFAULT 0,
  line_discount_total  INTEGER NOT NULL DEFAULT 0,
  order_discount_total INTEGER NOT NULL DEFAULT 0,
  service_charge       INTEGER NOT NULL DEFAULT 0,
  rounding_adjustment  INTEGER NOT NULL DEFAULT 0,
  grand_total          INTEGER NOT NULL DEFAULT 0,
  sales_amount         INTEGER NOT NULL DEFAULT 0,
  tax_amount           INTEGER NOT NULL DEFAULT 0,
  tax_code             TEXT NOT NULL DEFAULT 'TAXABLE',
  paid_total           INTEGER NOT NULL DEFAULT 0,
  change_total         INTEGER NOT NULL DEFAULT 0,
  refunded_total       INTEGER NOT NULL DEFAULT 0,
  opened_at        TEXT NOT NULL,
  placed_at        TEXT,
  completed_at     TEXT,
  settled_at       TEXT,
  voided_at        TEXT,
  void_reason_id   TEXT REFERENCES reason_codes(id),
  void_note        TEXT,
  void_by          TEXT,
  void_approved_by TEXT,
  note             TEXT,
  customer_name    TEXT,
  customer_phone   TEXT,
  external_ref     TEXT,
  created_by       TEXT,
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL,
  -- 財政部對發票的硬檢核。放在資料庫層當最後一道防線：
  -- 就算定價引擎哪天被改壞，也不可能寫進一筆銷售額加稅額對不上總額的單。
  CHECK (sales_amount + tax_amount = grand_total)
);
CREATE UNIQUE INDEX uq_orders_no ON orders(store_id, order_no);
CREATE UNIQUE INDEX uq_orders_client ON orders(store_id, client_id) WHERE client_id IS NOT NULL;
CREATE INDEX idx_orders_date_status ON orders(business_date, status);
CREATE INDEX idx_orders_shift ON orders(shift_id);
CREATE INDEX idx_orders_open_table ON orders(table_id) WHERE status NOT IN ('settled', 'voided');
CREATE INDEX idx_orders_settled ON orders(business_date, settled_at) WHERE status = 'settled';

-- 訂單明細。
--
-- ★ 快照欄位是這張表最重要的設計。
--   老闆早上把「珍珠奶茶」從 60 改 65、下午停售、明天改名「黑糖珍奶」。
--   三個月後查昨天的訂單，若只存 item_id 再 JOIN 回主檔，會看到「黑糖珍奶 65 元」——
--   帳目對不起來、稅務查核解釋不清、將來開折讓時品名與原發票不符會被平台退件。
--   item_id 仍然保留（供「該品項歷史銷量」報表 JOIN 回分類樹），
--   但**任何顯示、重印、開發票一律讀快照欄位**。
CREATE TABLE order_items (
  id             TEXT NOT NULL PRIMARY KEY,
  order_id       TEXT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
  line_no        INTEGER NOT NULL,
  parent_line_id TEXT REFERENCES order_items(id) ON DELETE CASCADE,
  item_id        TEXT REFERENCES items(id),
  variant_id     TEXT REFERENCES item_variants(id),
  name_snapshot          TEXT NOT NULL,
  short_name_snapshot    TEXT,
  sku_snapshot           TEXT,
  variant_name_snapshot  TEXT,
  category_id_snapshot   TEXT,
  category_name_snapshot TEXT,
  unit_price_snapshot    INTEGER NOT NULL,
  tax_code_snapshot      TEXT NOT NULL,
  -- 數量用千分之一為單位的定點數，支援「半份」這種真實需求（500 = 0.5 份）。
  qty_milli       INTEGER NOT NULL DEFAULT 1000,
  uom             TEXT NOT NULL DEFAULT '份',
  unit_price      INTEGER NOT NULL,
  -- 未稅 / 秤重單價需要小數，另存 1/1,000,000 元刻度（見 core::money::Micros）。
  unit_price_micros INTEGER NOT NULL DEFAULT 0,
  modifier_amount INTEGER NOT NULL DEFAULT 0,
  gross_amount    INTEGER NOT NULL DEFAULT 0,
  line_discount   INTEGER NOT NULL DEFAULT 0,
  net_amount      INTEGER NOT NULL DEFAULT 0,
  allocated_order_discount INTEGER NOT NULL DEFAULT 0,
  allocated_service_charge INTEGER NOT NULL DEFAULT 0,
  -- Σ taxable_amount 必須嚴格等於 orders.grand_total，發票品項才對得起來。
  taxable_amount  INTEGER NOT NULL DEFAULT 0,
  note            TEXT,
  station_id      TEXT,
  kitchen_status  TEXT NOT NULL DEFAULT 'pending' CHECK (kitchen_status IN ('pending', 'fired', 'cooking', 'ready', 'served', 'cancelled')),
  fired_at        TEXT,
  ready_at        TEXT,
  served_at       TEXT,
  voided_at       TEXT,
  void_reason_id  TEXT REFERENCES reason_codes(id),
  void_by         TEXT,
  void_approved_by TEXT,
  created_by      TEXT,
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_order_items_line ON order_items(order_id, line_no);
CREATE INDEX idx_order_items_order ON order_items(order_id);
CREATE INDEX idx_order_items_item ON order_items(item_id);
CREATE INDEX idx_order_items_kitchen ON order_items(station_id, kitchen_status, fired_at) WHERE kitchen_status IN ('fired', 'cooking');

-- 明細的加購項。同樣要快照 —— 加料價也會改。
CREATE TABLE order_item_modifiers (
  id                 TEXT NOT NULL PRIMARY KEY,
  order_item_id      TEXT NOT NULL REFERENCES order_items(id) ON DELETE CASCADE,
  modifier_id        TEXT REFERENCES modifiers(id),
  group_id           TEXT REFERENCES modifier_groups(id),
  group_name_snapshot TEXT NOT NULL,
  name_snapshot      TEXT NOT NULL,
  unit_price_snapshot INTEGER NOT NULL,
  qty                INTEGER NOT NULL DEFAULT 1,
  amount             INTEGER NOT NULL DEFAULT 0,
  created_at         TEXT NOT NULL
);
CREATE INDEX idx_order_item_modifiers_line ON order_item_modifiers(order_item_id);

-- 折扣套用紀錄。order_item_id 為 NULL 表示整單折扣。
CREATE TABLE order_discounts (
  id               TEXT NOT NULL PRIMARY KEY,
  order_id         TEXT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
  order_item_id    TEXT REFERENCES order_items(id) ON DELETE CASCADE,
  discount_def_id  TEXT REFERENCES discount_defs(id),
  name_snapshot    TEXT NOT NULL,
  type_snapshot    TEXT NOT NULL,
  value_snapshot   INTEGER NOT NULL,
  amount           INTEGER NOT NULL,
  reason_id        TEXT REFERENCES reason_codes(id),
  note             TEXT,
  approved_by      TEXT,
  created_by       TEXT,
  created_at       TEXT NOT NULL
);
CREATE INDEX idx_order_discounts_order ON order_discounts(order_id);
CREATE INDEX idx_order_discounts_def ON order_discounts(discount_def_id);

-- 訂單事件。**append-only：永不 UPDATE、永不 DELETE。**
--
-- 這是「誰在幾點把這張單改成什麼」的唯一真相。訂單本身的欄位會被覆寫
-- （狀態、金額都會變），只有這張表保留完整過程。對帳爭議與防弊查核都靠它。
CREATE TABLE order_events (
  id           TEXT NOT NULL PRIMARY KEY,
  order_id     TEXT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
  seq          INTEGER NOT NULL,
  event_type   TEXT NOT NULL,
  from_status  TEXT,
  to_status    TEXT,
  payload_json TEXT,
  actor_id     TEXT,
  terminal_id  TEXT,
  created_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_order_events_seq ON order_events(order_id, seq);
CREATE INDEX idx_order_events_type ON order_events(event_type, created_at);

-- 轉桌 / 併桌 / 拆單。
CREATE TABLE order_transfers (
  id             TEXT NOT NULL PRIMARY KEY,
  kind           TEXT NOT NULL CHECK (kind IN ('move_table', 'merge', 'split')),
  business_date  TEXT NOT NULL,
  from_order_id  TEXT REFERENCES orders(id),
  to_order_id    TEXT REFERENCES orders(id),
  from_table_id  TEXT REFERENCES dining_tables(id),
  to_table_id    TEXT REFERENCES dining_tables(id),
  item_ids_json  TEXT,
  reason         TEXT,
  actor_id       TEXT,
  approved_by    TEXT,
  created_at     TEXT NOT NULL
);
CREATE INDEX idx_order_transfers_date ON order_transfers(business_date, kind);
