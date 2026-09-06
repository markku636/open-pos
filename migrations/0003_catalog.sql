-- 0003_catalog：分類、品項、規格、加購選項、可用時段、價格規則、折扣定義。
--
-- 兩個貫穿整份 schema 的慣例在這裡第一次出現：
--
-- ① 主檔一律**軟刪除**（deleted_at），唯一索引一律寫成 partial index。
--    沒有 partial 的話，刪掉的「珍奶」會永遠擋住新建同名品項 —— 而店家改菜單
--    是每週都在做的事。
--
-- ② 金額欄位一律 INTEGER 整數元，百分比一律 basis point（8500 = 85 折）。
--    由 tests/ddl_lint.rs 強制。

CREATE TABLE categories (
  id          TEXT NOT NULL PRIMARY KEY,
  store_id    TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  parent_id   TEXT REFERENCES categories(id) ON DELETE SET NULL,
  code        TEXT,
  name        TEXT NOT NULL,
  color       TEXT,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  deleted_at  TEXT
);
CREATE INDEX idx_categories_tree ON categories(store_id, parent_id, sort_order) WHERE deleted_at IS NULL;

CREATE TABLE items (
  id             TEXT NOT NULL PRIMARY KEY,
  store_id       TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  category_id    TEXT REFERENCES categories(id) ON DELETE SET NULL,
  sku            TEXT,
  name           TEXT NOT NULL,
  short_name     TEXT,
  description    TEXT,
  image_path     TEXT,
  tax_code       TEXT NOT NULL DEFAULT 'TAXABLE' REFERENCES tax_rates(code),
  base_price     INTEGER NOT NULL DEFAULT 0,
  is_open_price  INTEGER NOT NULL DEFAULT 0 CHECK (is_open_price IN (0, 1)),
  prep_seconds   INTEGER NOT NULL DEFAULT 0,
  sold_out_until TEXT,
  sort_order     INTEGER NOT NULL DEFAULT 0,
  is_active      INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL,
  deleted_at     TEXT
);
CREATE INDEX idx_items_category ON items(store_id, category_id, sort_order) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uq_items_sku ON items(store_id, sku) WHERE sku IS NOT NULL AND deleted_at IS NULL;

-- 規格 / 尺寸（大中小）。
--
-- price_mode 分成 delta 與 absolute 兩種而不是單一欄位帶正負：語意不同。
-- 老闆把 base_price 從 60 改成 65 時，delta 型的「大杯 +10」要跟著變成 75，
-- absolute 型的「大杯固定 80」不能動。這是他們心裡真正的區分。
CREATE TABLE item_variants (
  id             TEXT NOT NULL PRIMARY KEY,
  item_id        TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  code           TEXT NOT NULL,
  name           TEXT NOT NULL,
  price_mode     TEXT NOT NULL DEFAULT 'delta' CHECK (price_mode IN ('delta', 'absolute')),
  price          INTEGER NOT NULL DEFAULT 0,
  price_delta    INTEGER NOT NULL DEFAULT 0,
  sku            TEXT,
  sold_out_until TEXT,
  is_default     INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
  sort_order     INTEGER NOT NULL DEFAULT 0,
  is_active      INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL,
  deleted_at     TEXT
);
CREATE UNIQUE INDEX uq_item_variants_code ON item_variants(item_id, code) WHERE deleted_at IS NULL;
-- 一個品項最多一個預設規格，由資料庫保證而不是靠 UI 自律。
CREATE UNIQUE INDEX uq_item_variants_default ON item_variants(item_id) WHERE is_default = 1 AND deleted_at IS NULL;

-- 加購選項群組（甜度 / 冰塊 / 加料）。
CREATE TABLE modifier_groups (
  id             TEXT NOT NULL PRIMARY KEY,
  store_id       TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  name           TEXT NOT NULL,
  selection_type TEXT NOT NULL DEFAULT 'single' CHECK (selection_type IN ('single', 'multiple')),
  min_select     INTEGER NOT NULL DEFAULT 0,
  max_select     INTEGER NOT NULL DEFAULT 1,
  sort_order     INTEGER NOT NULL DEFAULT 0,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL,
  deleted_at     TEXT
);
CREATE INDEX idx_modifier_groups_store ON modifier_groups(store_id, sort_order) WHERE deleted_at IS NULL;

-- 選項（加蛋 +10 / 去冰 +0 / 半糖 +0）。price 為 0 表示免費選項。
CREATE TABLE modifiers (
  id             TEXT NOT NULL PRIMARY KEY,
  group_id       TEXT NOT NULL REFERENCES modifier_groups(id) ON DELETE CASCADE,
  name           TEXT NOT NULL,
  price          INTEGER NOT NULL DEFAULT 0,
  max_qty        INTEGER NOT NULL DEFAULT 1,
  sku            TEXT,
  sold_out_until TEXT,
  is_default     INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
  sort_order     INTEGER NOT NULL DEFAULT 0,
  is_active      INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL,
  deleted_at     TEXT
);
CREATE INDEX idx_modifiers_group ON modifiers(group_id, sort_order) WHERE deleted_at IS NULL;

-- 品項與加購群組的關聯，可逐項覆寫群組的 min/max。
CREATE TABLE item_modifier_groups (
  item_id    TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  group_id   TEXT NOT NULL REFERENCES modifier_groups(id) ON DELETE CASCADE,
  min_select INTEGER,
  max_select INTEGER,
  sort_order INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (item_id, group_id)
);

-- 可用時段（早餐限定 06:00-10:30、平日午間套餐）。
--
-- target 是多型的（品項 / 規格 / 分類），而兩種資料庫都不支援多型外鍵，
-- 所以參照完整性由 Rust 端保證。這是刻意的取捨：另一條路是為每種 target 各開一張表，
-- 查詢時要 UNION 三次，複雜度更高而收益只有一個外鍵。
CREATE TABLE availability_rules (
  id          TEXT NOT NULL PRIMARY KEY,
  store_id    TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  target_type TEXT NOT NULL CHECK (target_type IN ('item', 'variant', 'category')),
  target_id   TEXT NOT NULL,
  day_mask    INTEGER NOT NULL DEFAULT 127,
  start_time  TEXT NOT NULL DEFAULT '00:00',
  end_time    TEXT NOT NULL DEFAULT '23:59',
  valid_from  TEXT,
  valid_to    TEXT,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE INDEX idx_availability_rules_target ON availability_rules(store_id, target_type, target_id);

-- 價格規則：內用 / 外帶不同價、時段優惠。
-- priority 大者優先，**命中即停不疊加** —— 疊加語意留給 discount，
-- 兩者混在一起會讓「為什麼這杯是 55 元」變得沒人解釋得清楚。
CREATE TABLE price_rules (
  id           TEXT NOT NULL PRIMARY KEY,
  store_id     TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  scope_type   TEXT NOT NULL CHECK (scope_type IN ('all', 'category', 'item', 'variant')),
  scope_id     TEXT,
  channel      TEXT NOT NULL DEFAULT 'any' CHECK (channel IN ('any', 'dine_in', 'takeout', 'delivery')),
  day_mask     INTEGER NOT NULL DEFAULT 127,
  start_time   TEXT NOT NULL DEFAULT '00:00',
  end_time     TEXT NOT NULL DEFAULT '23:59',
  valid_from   TEXT,
  valid_to     TEXT,
  adjust_type  TEXT NOT NULL CHECK (adjust_type IN ('fixed_price', 'amount_off', 'percent_off')),
  adjust_value INTEGER NOT NULL,
  priority     INTEGER NOT NULL DEFAULT 0,
  is_active    INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE INDEX idx_price_rules_active ON price_rules(store_id, is_active, priority);

-- 折扣定義。
--
-- comp（招待）刻意與 percent/amount 分開：它在報表上要進「招待統計」而不是
-- 「折扣統計」。老闆看折扣是看行銷成效，看招待是看有沒有人在送人情。
CREATE TABLE discount_defs (
  id                            TEXT NOT NULL PRIMARY KEY,
  store_id                      TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  code                          TEXT NOT NULL,
  name                          TEXT NOT NULL,
  level                         TEXT NOT NULL CHECK (level IN ('line', 'order')),
  adjust_type                   TEXT NOT NULL CHECK (adjust_type IN ('percent', 'amount', 'comp')),
  value                         INTEGER NOT NULL DEFAULT 0,
  max_amount                    INTEGER,
  requires_reason               INTEGER NOT NULL DEFAULT 0 CHECK (requires_reason IN (0, 1)),
  requires_manager              INTEGER NOT NULL DEFAULT 0 CHECK (requires_manager IN (0, 1)),
  applies_before_service_charge INTEGER NOT NULL DEFAULT 1 CHECK (applies_before_service_charge IN (0, 1)),
  is_active                     INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  sort_order                    INTEGER NOT NULL DEFAULT 0,
  created_at                    TEXT NOT NULL,
  updated_at                    TEXT NOT NULL,
  deleted_at                    TEXT
);
CREATE UNIQUE INDEX uq_discount_defs_code ON discount_defs(store_id, code) WHERE deleted_at IS NULL;
