-- 0001_core：最小可用骨架。
--
-- 撰寫規則（由 tests/ddl_lint.rs 強制，見 docs/adr/0003-single-source-ddl.md）：
--   * 禁用 SQLite 專有語法：AUTOINCREMENT / WITHOUT ROWID / STRICT / COLLATE NOCASE /
--     IFNULL / datetime('now') / strftime / PRAGMA / GLOB / INSERT OR IGNORE
--   * 型別只用 TEXT / INTEGER；金額欄位一律 INTEGER（整數元），對應 PG BIGINT
--   * 時間一律由 Rust 寫入 ISO-8601 UTC 文字，不用資料庫預設值
--   * 不用 IF NOT EXISTS（migration 有版本控管，它只會掩蓋「重跑」這種真正的錯誤）
--   * 索引一律 idx_<表>_<用途> / uq_<表>_<用途>，全域唯一

-- 單例設定。key/value 形式，值一律 JSON 文字。
CREATE TABLE app_settings (
  key        TEXT NOT NULL PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- 店家。v1 只會有一列，但先做成表 —— 多分店時不必改 schema。
CREATE TABLE stores (
  id                  TEXT NOT NULL PRIMARY KEY,
  code                TEXT NOT NULL,
  name                TEXT NOT NULL,
  tax_id              TEXT,
  address             TEXT,
  phone               TEXT,
  tz                  TEXT NOT NULL DEFAULT 'Asia/Taipei',
  business_day_cutoff TEXT NOT NULL DEFAULT '05:00',
  currency            TEXT NOT NULL DEFAULT 'TWD',
  tax_rate_bp         INTEGER NOT NULL DEFAULT 500,
  service_charge_rate_bp INTEGER NOT NULL DEFAULT 0,
  rounding_policy     TEXT NOT NULL DEFAULT 'none',
  created_at          TEXT NOT NULL,
  updated_at          TEXT NOT NULL,
  deleted_at          TEXT
);
CREATE UNIQUE INDEX uq_stores_code ON stores(code) WHERE deleted_at IS NULL;

-- 終端註冊。發票字軌的號碼池區塊是租給「終端」的，所以它必須在最早期就存在。
CREATE TABLE terminals (
  id           TEXT NOT NULL PRIMARY KEY,
  store_id     TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  code         TEXT NOT NULL,
  name         TEXT NOT NULL,
  kind         TEXT NOT NULL DEFAULT 'pos' CHECK (kind IN ('pos','kds','kiosk','mobile')),
  is_active    INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
  last_seen_at TEXT,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_terminals_code ON terminals(store_id, code);

-- 冪等鍵。所有寫入 RPC 都必須帶前端產生的 UUID。
-- 這不是為了「主機掛掉」，是為了每天都在發生的 Wi-Fi 抖動：
-- 廚房的不鏽鋼 + 2.4GHz 干擾 + 平板省電，重送是常態，重複收款不能是。
CREATE TABLE idempotency_keys (
  key           TEXT NOT NULL PRIMARY KEY,
  operation     TEXT NOT NULL,
  response_json TEXT NOT NULL,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_idempotency_keys_created ON idempotency_keys(created_at);
