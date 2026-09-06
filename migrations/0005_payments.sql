-- 0005_payments：帳單、分帳、付款方式、收款、退款。
--
-- 為什麼付款不直接掛在 orders 上，中間要多一層 bills：
-- 分帳時每一份要各自收款、各自可能退款、將來各自開一張發票。
-- 發票是掛在「帳單」而不是「訂單」上的，所以這層間接在 v1.0 就得存在，
-- 否則 v1.4 接發票時要動到所有已經寫好的收款流程。

-- 付款方式主檔。
CREATE TABLE payment_methods (
  id             TEXT NOT NULL PRIMARY KEY,
  store_id       TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  code           TEXT NOT NULL,
  name           TEXT NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN ('cash', 'card', 'mobile', 'stored_value', 'voucher', 'external', 'on_account')),
  opens_drawer   INTEGER NOT NULL DEFAULT 0 CHECK (opens_drawer IN (0, 1)),
  -- 只有現金能找零。刷卡「找零」是不存在的東西，讓資料庫記住這件事，
  -- 免得每個呼叫端各自判斷。
  allows_change  INTEGER NOT NULL DEFAULT 0 CHECK (allows_change IN (0, 1)),
  allows_tip     INTEGER NOT NULL DEFAULT 0 CHECK (allows_tip IN (0, 1)),
  needs_ref      INTEGER NOT NULL DEFAULT 0 CHECK (needs_ref IN (0, 1)),
  -- 是否計入關班時的「應有現金」。悠遊卡、LINE Pay 都不算。
  counts_as_cash INTEGER NOT NULL DEFAULT 0 CHECK (counts_as_cash IN (0, 1)),
  sort_order     INTEGER NOT NULL DEFAULT 0,
  is_active      INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL,
  deleted_at     TEXT
);
CREATE UNIQUE INDEX uq_payment_methods_code ON payment_methods(store_id, code) WHERE deleted_at IS NULL;

-- 帳單。不分帳時結帳會自動建一張（split_count = 1）。
CREATE TABLE bills (
  id            TEXT NOT NULL PRIMARY KEY,
  order_id      TEXT NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
  shift_id      TEXT,
  business_date TEXT NOT NULL,
  bill_no       TEXT NOT NULL,
  split_mode    TEXT NOT NULL DEFAULT 'none' CHECK (split_mode IN ('none', 'even', 'by_item', 'by_amount')),
  split_index   INTEGER NOT NULL DEFAULT 1,
  split_count   INTEGER NOT NULL DEFAULT 1,
  subtotal            INTEGER NOT NULL DEFAULT 0,
  discount_total      INTEGER NOT NULL DEFAULT 0,
  service_charge      INTEGER NOT NULL DEFAULT 0,
  rounding_adjustment INTEGER NOT NULL DEFAULT 0,
  grand_total         INTEGER NOT NULL DEFAULT 0,
  sales_amount        INTEGER NOT NULL DEFAULT 0,
  tax_amount          INTEGER NOT NULL DEFAULT 0,
  paid_total          INTEGER NOT NULL DEFAULT 0,
  change_total        INTEGER NOT NULL DEFAULT 0,
  status        TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'settled', 'voided', 'refunded', 'partially_refunded')),
  settled_at    TEXT,
  settled_by    TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  CHECK (sales_amount + tax_amount = grand_total)
);
CREATE UNIQUE INDEX uq_bills_no ON bills(store_id, bill_no);
CREATE INDEX idx_bills_order ON bills(order_id);
CREATE INDEX idx_bills_date ON bills(business_date, status);

-- 分項分帳：支援「這道菜兩個人平分」。
-- amount 用最大餘數法分攤，同一張訂單所有 bill_lines 的加總必須等於訂單總額。
CREATE TABLE bill_lines (
  id            TEXT NOT NULL PRIMARY KEY,
  bill_id       TEXT NOT NULL REFERENCES bills(id) ON DELETE CASCADE,
  order_item_id TEXT NOT NULL REFERENCES order_items(id) ON DELETE RESTRICT,
  qty_milli     INTEGER NOT NULL DEFAULT 1000,
  amount        INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_bill_lines_bill ON bill_lines(bill_id);
CREATE INDEX idx_bill_lines_item ON bill_lines(order_item_id);

-- 收款。一張帳單可以有多筆（現金 200 + LINE Pay 350 的混合支付）。
CREATE TABLE payments (
  id            TEXT NOT NULL PRIMARY KEY,
  bill_id       TEXT NOT NULL REFERENCES bills(id) ON DELETE RESTRICT,
  order_id      TEXT NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
  shift_id      TEXT,
  terminal_id   TEXT REFERENCES terminals(id) ON DELETE SET NULL,
  business_date TEXT NOT NULL,
  payment_method_id    TEXT NOT NULL REFERENCES payment_methods(id),
  method_code_snapshot TEXT NOT NULL,
  method_name_snapshot TEXT NOT NULL,
  -- amount 是實際沖銷金額；tendered 是客人給的（只有現金會不一樣）。
  amount        INTEGER NOT NULL,
  tendered      INTEGER NOT NULL DEFAULT 0,
  change_amount INTEGER NOT NULL DEFAULT 0,
  tip_amount    INTEGER NOT NULL DEFAULT 0,
  currency      TEXT NOT NULL DEFAULT 'TWD',
  status        TEXT NOT NULL DEFAULT 'captured' CHECK (status IN ('pending', 'authorized', 'captured', 'voided', 'refunded', 'failed')),
  ref_no        TEXT,
  auth_code     TEXT,
  gateway_payload TEXT,
  paid_at       TEXT NOT NULL,
  created_by    TEXT,
  note          TEXT,
  created_at    TEXT NOT NULL,
  CHECK (change_amount >= 0),
  CHECK (tendered = 0 OR tendered >= amount)
);
CREATE INDEX idx_payments_bill ON payments(bill_id);
CREATE INDEX idx_payments_shift ON payments(shift_id, status);
CREATE INDEX idx_payments_date ON payments(business_date, payment_method_id);

-- 退款。
-- approved_by 是 NOT NULL —— 退款一律要主管授權，沒有例外。
-- 把它做成資料庫約束而不是 UI 規則，是因為 UI 規則遲早會被繞過。
CREATE TABLE refunds (
  id            TEXT NOT NULL PRIMARY KEY,
  bill_id       TEXT NOT NULL REFERENCES bills(id) ON DELETE RESTRICT,
  payment_id    TEXT REFERENCES payments(id) ON DELETE RESTRICT,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
  shift_id      TEXT,
  business_date TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('full', 'partial')),
  amount        INTEGER NOT NULL,
  reason_id     TEXT REFERENCES reason_codes(id),
  note          TEXT,
  approved_by   TEXT NOT NULL,
  created_by    TEXT NOT NULL,
  refunded_at   TEXT NOT NULL,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_refunds_date ON refunds(business_date);
CREATE INDEX idx_refunds_bill ON refunds(bill_id);
