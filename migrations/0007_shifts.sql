-- 0007_shifts：班別、現金收支、面額盤點、營業日、日結快照。
--
-- 這一組是「補做會很痛」清單裡的第 5 項。沒有 shift_id 掛在交易列上，
-- 過去的 Z 報表就永遠重建不出來，現金差異也查不了。

CREATE TABLE shifts (
  id            TEXT NOT NULL PRIMARY KEY,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
  terminal_id   TEXT REFERENCES terminals(id) ON DELETE SET NULL,
  business_date TEXT NOT NULL,
  shift_no      TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closing', 'closed', 'reviewed')),
  opened_by     TEXT NOT NULL,
  opened_at     TEXT NOT NULL,
  opening_float INTEGER NOT NULL DEFAULT 0,
  closed_by     TEXT,
  closed_at     TEXT,
  handover_to   TEXT,
  -- 關班快照：關班當下算好寫入，之後**永不重算**。
  -- 否則事後補一張單，三個月前的 Z 報表數字就會跟著變，稽核上完全站不住。
  expected_cash INTEGER,
  counted_cash  INTEGER,
  cash_variance INTEGER,
  summary_json  TEXT,
  note          TEXT,
  reviewed_by   TEXT,
  reviewed_at   TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_shifts_no ON shifts(store_id, shift_no);
-- 一台終端同時只能有一個未關的班。
CREATE UNIQUE INDEX uq_shifts_open ON shifts(terminal_id) WHERE status IN ('open', 'closing');
CREATE INDEX idx_shifts_date ON shifts(business_date, status);

-- 現金收支。
-- paid_out = 拿錢出去買菜 / 叫瓦斯；drop = 把大鈔投進保險箱。
-- amount 恆為正，方向由 kind 決定 —— 讓「負數代表什麼」不需要靠註解解釋。
CREATE TABLE cash_movements (
  id            TEXT NOT NULL PRIMARY KEY,
  shift_id      TEXT NOT NULL REFERENCES shifts(id) ON DELETE CASCADE,
  business_date TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('opening_float', 'paid_in', 'paid_out', 'drop', 'adjust')),
  amount        INTEGER NOT NULL CHECK (amount >= 0),
  reason_id     TEXT REFERENCES reason_codes(id),
  note          TEXT,
  ref_no        TEXT,
  actor_id      TEXT NOT NULL,
  approved_by   TEXT,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_cash_movements_shift ON cash_movements(shift_id, kind);

-- 面額盤點明細。
-- 只記總額的話，查不出「是不是少了一張千元」。有明細才查得動。
-- denomination 以元為單位（1000 = 一千元鈔）。
CREATE TABLE shift_counts (
  id           TEXT NOT NULL PRIMARY KEY,
  shift_id     TEXT NOT NULL REFERENCES shifts(id) ON DELETE CASCADE,
  phase        TEXT NOT NULL CHECK (phase IN ('open', 'close')),
  denomination INTEGER NOT NULL,
  count        INTEGER NOT NULL DEFAULT 0,
  subtotal     INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_shift_counts_denomination ON shift_counts(shift_id, phase, denomination);

-- 營業日。closed 之後拒絕任何交易寫入（由 repo 層 guard）。
CREATE TABLE business_days (
  id            TEXT NOT NULL PRIMARY KEY,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
  business_date TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closing', 'closed', 'locked')),
  opened_at     TEXT,
  opened_by     TEXT,
  closed_at     TEXT,
  closed_by     TEXT,
  z_report_no   TEXT,
  summary_json  TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_business_days_date ON business_days(store_id, business_date);

-- 日結快照（不可變）。
--
-- ★ 報表只讀這張聚合表，不掃 order_items。
--   三年後 order_items 上看百萬列，每次開報表都全表掃會愈跑愈慢，
--   而且愈忙的店愈慢 —— 剛好是最不能慢的那些店。
--
-- 日結後永不重算：之後的補單與退款計入「後續調整」而不是改寫歷史。
CREATE TABLE daily_summaries (
  id              TEXT NOT NULL PRIMARY KEY,
  business_day_id TEXT NOT NULL REFERENCES business_days(id) ON DELETE CASCADE,
  business_date   TEXT NOT NULL,
  metric          TEXT NOT NULL,
  dim_key         TEXT NOT NULL DEFAULT '',
  -- 維度的顯示名稱也要快照。分類改名之後，歷史報表仍要顯示當時的名字。
  dim_label       TEXT NOT NULL DEFAULT '',
  qty             INTEGER NOT NULL DEFAULT 0,
  amount          INTEGER NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_daily_summaries_metric ON daily_summaries(business_day_id, metric, dim_key);
CREATE INDEX idx_daily_summaries_date ON daily_summaries(business_date, metric);
