-- 0011_dining_plans：吃到飽 / 飲料無限暢飲方案，以及人頭費。
--
-- 設計依據見 docs/dining-modes.md —— 那份文件記錄了 25 套市售產品的做法，
-- 以及查完之後被推翻的兩個設計（店家層級營業型態、低消自動補行）。
--
-- ★ 這裡**沒有**新增「營業型態」欄位。
--   市場共識是「吃到飽」是一桌（或一張單）的屬性，不是店的屬性 ——
--   WEIBY 微碧那句「同桌不同價消費者也能點」說明了為什麼：
--   一桌可以有大人吃到飽、小孩單點兒童餐。做成店家開關就做不出來。
--
-- ★ 這裡也**沒有**低消的補差額結構。
--   查過的產品裡只有 WETOM 一家打出低消，而它是「提醒」不是自動補行。
--   低消做成結帳提醒（stores.min_charge_per_head），不進明細。

-- 明細行的來源。
--
-- 人頭費、開桌費都是**明細行**而不是訂單上的欄位 —— 定價引擎把 taxable_amount
-- 攤到每一行並保證 Σ 嚴格等於 grand_total（發票的品項加總要對得上總計，
-- 差一元整批退件）。做成欄位的話，稅額拆分、收據、分帳、報表四件事全部要重寫。
--
-- 市場也是這樣做的：Airレジ 的「放題プラン名商品」、Eats365 的 representing item、
-- Loyverse 建議「把服務費做成一個商品」。
--
-- origin 用來讓收據與報表分得出來：
--   item        一般點的商品
--   plan        吃到飽方案本身（有價，人頭費就是這個 × 人數）
--   plan_member 方案內的品項（0 元，Airレジ 印成「（放）」）
--   cover       開桌費 / お通し（× 人數）
ALTER TABLE order_items ADD COLUMN line_origin TEXT NOT NULL DEFAULT 'item';

-- 這一行屬於哪一個方案（origin 是 plan 或 plan_member 時才有值）。
-- 方案結束後這些行仍然留著 —— 帳要看得出當時是吃到飽。
ALTER TABLE order_items ADD COLUMN dining_plan_id TEXT;

CREATE INDEX idx_order_items_origin ON order_items(order_id, line_origin);

-- 吃到飽 / 無限暢飲方案。
--
-- ★ 方案本身就是一個既有的 items 記錄，不是另一種東西。
--   這是所有查過的產品的共識（Airレジ 放題プラン名商品 / スマレジ プラン /
--   Eats365 representing item / Square セット）。
--
--   這樣免費得到四件事：
--   * 人頭分級（大人 / 小孩 / 長者）直接用 item_variants
--     —— Toast 明講 size pricing 就是拿來做這個的
--   * 平日／假日、午餐／晚餐不同價直接用既有的 price_rules
--   * 品項排行、稅別、廚房分區全部自動適用
--   * 不必為「方案」另做一套定價
CREATE TABLE dining_plans (
  id         TEXT NOT NULL PRIMARY KEY,
  store_id   TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  -- 有價的那個商品。人頭費 = 這個商品 × 人數。
  item_id    TEXT NOT NULL REFERENCES items(id) ON DELETE RESTRICT,
  name       TEXT NOT NULL,
  -- 用餐時限（分鐘）。0 = 不限時。
  --
  -- ★ 到期**只提醒，不加價、不擋單**。這是全市場的一致做法：
  --   Square / Clover / Lightspeed 只變顏色，スマレジ 推播到手持機，
  --   Eats365 顯示 OT，iCHEF 顯示警示圖示。
  --   查過的 25 套產品裡**沒有一套**會自動加價。
  limit_minutes  INTEGER NOT NULL DEFAULT 0,
  -- 提前多久先提醒（分鐘）。スマレジ 的『事前通知』：
  -- 「120分制で終了30分前にも通知を出したい場合、制限時間に120、事前通知に30」
  notice_minutes INTEGER NOT NULL DEFAULT 0,
  -- 方案內的品項要不要印在客人的帳單上。
  -- スマレジ 每個菜單都有「印刷対象伝票」勾選；0 元的方案內品項通常只印廚房單。
  print_members_on_bill INTEGER NOT NULL DEFAULT 0 CHECK (print_members_on_bill IN (0, 1)),
  is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE INDEX idx_dining_plans_store ON dining_plans(store_id, is_active, sort_order);
-- 一個商品只能是一個方案的代表商品 —— 否則「點這個商品」要進哪個方案沒有答案。
CREATE UNIQUE INDEX uq_dining_plans_item ON dining_plans(item_id) WHERE deleted_at IS NULL;

-- 方案包含哪些品項（吃這些不另外收錢）。
--
-- target 是多型的（品項 / 分類），跟 availability_rules 同一個取捨：
-- 兩種資料庫都不支援多型外鍵，參照完整性由 Rust 端保證。
-- 用分類的理由很實際 —— 「所有飲料無限暢飲」不該要一個一個勾。
CREATE TABLE dining_plan_items (
  id          TEXT NOT NULL PRIMARY KEY,
  plan_id     TEXT NOT NULL REFERENCES dining_plans(id) ON DELETE CASCADE,
  target_type TEXT NOT NULL CHECK (target_type IN ('item', 'category')),
  target_id   TEXT NOT NULL,
  created_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_dining_plan_items ON dining_plan_items(plan_id, target_type, target_id);
CREATE INDEX idx_dining_plan_items_target ON dining_plan_items(target_type, target_id);

-- 這一桌現在套用哪個方案。
--
-- 綁在 session 上而不是 order 上：一桌可能有好幾張單（分開結帳、續攤），
-- 但「這桌是吃到飽」是整桌的事。Airレジ 也是綁桌（點了方案商品那桌就進入方案）。
ALTER TABLE table_sessions ADD COLUMN dining_plan_id TEXT REFERENCES dining_plans(id) ON DELETE SET NULL;
-- 方案的計時起點。**不等於入座時間** —— 客人可能先坐下看菜單再決定吃到飽。
-- Airレジ 的 L.O. 是「放題プラン注文から XX 分後」，從點方案那一刻算。
ALTER TABLE table_sessions ADD COLUMN plan_started_at TEXT;

-- 低消（每人）。0 = 沒有低消。
--
-- ★ 這是**提醒**用的，不會自動補一行差額。
--   查過的產品裡只有 WETOM 一家有低消，而它是「桌位最低消費提醒」。
--   日本五套全部沒有，改用固定的 per-head お通し 代替。
--   自動補差額要處理它的稅額、服務費基數、退款、分帳 ——
--   而市場證明店家並不需要這些。
ALTER TABLE stores ADD COLUMN min_charge_per_head INTEGER NOT NULL DEFAULT 0;
