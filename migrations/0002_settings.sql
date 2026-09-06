-- 0002_settings：稅別、單號配號、原因碼、營業時段。
--
-- 這幾張表看起來很雜，但它們是後面所有東西的前提：
-- 稅別決定發票怎麼開、sequences 決定人看的單號、reason_codes 是防弊的骨幹
-- （沒有原因碼，「誰為什麼作廢這張單」就只能靠自由文字，事後查不出東西）。

-- 稅別。mig_tax_type 對映財政部 MIG 的 TaxType 欄位（1=應稅 2=零稅率 3=免稅 4=特種 9=混合）。
-- 現在還用不到發票，但稅別是算錢的前提，而且改稅別會動到歷史資料，先定下來。
CREATE TABLE tax_rates (
  code          TEXT NOT NULL PRIMARY KEY,
  name          TEXT NOT NULL,
  rate_bp       INTEGER NOT NULL,
  mig_tax_type  INTEGER NOT NULL CHECK (mig_tax_type IN (1, 2, 3, 4, 9)),
  is_default    INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
  sort_order    INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

-- 人看的單號原子配號（A-20260906-0042）。
--
-- ULID 是內部主鍵，人看的是單號，兩者刻意分開：ULID 唸不出來也記不住，
-- 而店員在電話裡要能講出「四十二號單」。
--
-- 配號用 `UPDATE ... RETURNING next_value` 一句完成，SQLite 3.35+ 與 PG 都支援。
-- scope_key 讓同一種單號能按營業日重新編號（scope='order', scope_key='2026-09-06'）。
CREATE TABLE sequences (
  id         TEXT NOT NULL PRIMARY KEY,
  store_id   TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  scope      TEXT NOT NULL,
  scope_key  TEXT NOT NULL DEFAULT '',
  prefix     TEXT NOT NULL DEFAULT '',
  next_value INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_sequences_scope ON sequences(store_id, scope, scope_key);

-- 作廢 / 折扣 / 退款 / 現金收支的原因主檔。
--
-- 為什麼要有這張表而不是讓店員自由輸入：報表要能回答「這個月因為『客人反悔』
-- 作廢了幾單、金額多少」。自由文字做不到分組統計，而防弊的價值全在統計上。
CREATE TABLE reason_codes (
  id            TEXT NOT NULL PRIMARY KEY,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL CHECK (kind IN ('void', 'discount', 'refund', 'comp', 'cash_in', 'cash_out')),
  code          TEXT NOT NULL,
  name          TEXT NOT NULL,
  requires_note INTEGER NOT NULL DEFAULT 0 CHECK (requires_note IN (0, 1)),
  is_active     INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  sort_order    INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  deleted_at    TEXT
);
CREATE UNIQUE INDEX uq_reason_codes_code ON reason_codes(store_id, kind, code) WHERE deleted_at IS NULL;
CREATE INDEX idx_reason_codes_kind ON reason_codes(store_id, kind, sort_order) WHERE deleted_at IS NULL;

-- 營業時段。end_time 小於 start_time 表示跨午夜（22:00-02:00）。
CREATE TABLE business_hours (
  id          TEXT NOT NULL PRIMARY KEY,
  store_id    TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  day_of_week INTEGER NOT NULL CHECK (day_of_week BETWEEN 0 AND 6),
  start_time  TEXT NOT NULL DEFAULT '00:00',
  end_time    TEXT NOT NULL DEFAULT '23:59',
  is_closed   INTEGER NOT NULL DEFAULT 0 CHECK (is_closed IN (0, 1)),
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_business_hours_day ON business_hours(store_id, day_of_week);
