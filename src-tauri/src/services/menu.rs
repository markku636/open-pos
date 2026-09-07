//! 商品維護：分類、品項、規格。
//!
//! # 為什麼刪除是軟刪除
//!
//! 歷史訂單的明細雖然有品名與單價的快照（見 `order_items`），但報表仍然需要
//! JOIN 回主檔取分類樹（「這個月飲料類賣了多少」）。真的 DELETE 會讓那些
//! 歷史訂單的分類歸屬變成 NULL，過去的報表數字就跟著改變 ——
//! 而「昨天看到的數字今天不一樣」是稽核上最難解釋的一種問題。
//!
//! 唯一索引因此一律寫成 partial（`WHERE deleted_at IS NULL`），
//! 否則刪掉的「珍奶」會永遠擋住新建同名品項，而改菜單是每週都在做的事。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::services::audit::{self, AuditAction, AuditEntry};
use crate::services::rbac;

const PERM_MENU: &str = "settings.item";

// ---------------------------------------------------------------- DTO

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub id: String,
    pub item_id: String,
    pub code: String,
    pub name: String,
    /// `delta` = 跟著品項基本價加減；`absolute` = 自己一個固定價。
    /// 分兩種是因為老闆改基本價時，兩者的預期行為不同。
    pub price_mode: String,
    pub price: i64,
    pub price_delta: i64,
    pub is_default: bool,
    pub sort_order: i64,
    pub is_active: bool,
}

/// 選項群組（甜度 / 冰塊 / 加購）。
///
/// # 為什麼是群組而不是一堆平的選項
///
/// 因為「甜度」要**必選一個**，而「加購」可以**選很多個、也可以不選**。
/// 這兩件事的差別只有群組層級表達得出來，而它直接決定點餐畫面長什麼樣子。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModifierGroup {
    pub id: String,
    pub name: String,
    /// `single` = 單選（甜度）；`multiple` = 複選（加購）。
    pub selection_type: String,
    /// 最少要選幾個。1 = 必選。
    pub min_select: i64,
    pub max_select: i64,
    pub sort_order: i64,
    pub options: Vec<Modifier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Modifier {
    pub id: String,
    pub group_id: String,
    pub name: String,
    /// 加價。0 = 免費選項（半糖、去冰）。
    pub price: i64,
    /// 預設勾起來的那一個。
    pub is_default: bool,
    pub sold_out_until: Option<String>,
    pub sort_order: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub category_id: Option<String>,
    pub name: String,
    pub short_name: Option<String>,
    /// 整數元。
    pub base_price: i64,
    pub tax_code: String,
    pub is_open_price: bool,
    /// 售完到什麼時候（ISO 文字）。None = 有貨。
    pub sold_out_until: Option<String>,
    pub sort_order: i64,
    pub is_active: bool,
    pub variants: Vec<Variant>,
    /// 這個品項掛了哪幾組選項。**只放 id** ——
    /// 群組本身在 `MenuTree.modifier_groups` 裡各一份，
    /// 不然一份三十個品項的菜單會把同一組「甜度」複製三十次。
    pub modifier_group_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryNode {
    #[serde(flatten)]
    pub category: Category,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuTree {
    pub categories: Vec<CategoryNode>,
    /// 沒有分類的品項。**刻意單獨列出來而不是藏起來** ——
    /// 刪掉分類之後品項會落到這裡，如果不顯示，老闆會以為商品不見了。
    pub uncategorized: Vec<Item>,
    /// 店裡所有的選項群組，各一份。品項用 id 指過來。
    #[serde(default)]
    pub modifier_groups: Vec<ModifierGroup>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInput {
    /// None = 新增。
    pub id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInput {
    pub id: Option<String>,
    pub category_id: Option<String>,
    pub name: String,
    pub short_name: Option<String>,
    pub base_price: i64,
    pub tax_code: Option<String>,
    pub is_open_price: Option<bool>,
    pub sold_out_until: Option<String>,
    pub sort_order: Option<i64>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModifierGroupInput {
    pub id: Option<String>,
    pub name: String,
    /// single / multiple。
    pub selection_type: Option<String>,
    pub min_select: Option<i64>,
    pub max_select: Option<i64>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModifierInput {
    pub id: Option<String>,
    pub group_id: String,
    pub name: String,
    pub price: Option<i64>,
    pub is_default: Option<bool>,
    pub sold_out_until: Option<String>,
    pub sort_order: Option<i64>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantInput {
    pub id: Option<String>,
    pub item_id: String,
    pub code: String,
    pub name: String,
    pub price_mode: String,
    pub price: Option<i64>,
    pub price_delta: Option<i64>,
    pub is_default: Option<bool>,
    pub sort_order: Option<i64>,
    pub is_active: Option<bool>,
}

// ---------------------------------------------------------------- 讀

pub async fn menu_tree(ctx: &Ctx) -> AppResult<MenuTree> {
    let cats = sqlx::query(
        "SELECT id, name, color, sort_order, is_active
           FROM categories WHERE deleted_at IS NULL ORDER BY sort_order, name",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let items = sqlx::query(
        "SELECT id, category_id, name, short_name, base_price, tax_code,
                is_open_price, sold_out_until, sort_order, is_active
           FROM items WHERE deleted_at IS NULL ORDER BY sort_order, name",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let variants = sqlx::query(
        "SELECT id, item_id, code, name, price_mode, price, price_delta,
                is_default, sort_order, is_active
           FROM item_variants WHERE deleted_at IS NULL ORDER BY sort_order, name",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let mut by_item: std::collections::HashMap<String, Vec<Variant>> = Default::default();
    for r in &variants {
        let v = Variant {
            id: r.get("id"),
            item_id: r.get("item_id"),
            code: r.get("code"),
            name: r.get("name"),
            price_mode: r.get("price_mode"),
            price: r.get("price"),
            price_delta: r.get("price_delta"),
            is_default: r.get::<i64, _>("is_default") == 1,
            sort_order: r.get("sort_order"),
            is_active: r.get::<i64, _>("is_active") == 1,
        };
        by_item.entry(v.item_id.clone()).or_default().push(v);
    }

    // 選項群組與它們的選項。整份菜單共用同一批群組，所以各讀一次就好。
    let group_rows = sqlx::query(
        "SELECT id, name, selection_type, min_select, max_select, sort_order
           FROM modifier_groups WHERE deleted_at IS NULL ORDER BY sort_order, name",
    )
    .fetch_all(ctx.db.reader())
    .await?;
    let option_rows = sqlx::query(
        "SELECT id, group_id, name, price, is_default, sold_out_until, sort_order, is_active
           FROM modifiers WHERE deleted_at IS NULL ORDER BY sort_order, name",
    )
    .fetch_all(ctx.db.reader())
    .await?;
    let mut options_by_group: std::collections::HashMap<String, Vec<Modifier>> = Default::default();
    for r in &option_rows {
        let m = Modifier {
            id: r.get("id"),
            group_id: r.get("group_id"),
            name: r.get("name"),
            price: r.get("price"),
            is_default: r.get::<i64, _>("is_default") == 1,
            sold_out_until: r.get("sold_out_until"),
            sort_order: r.get("sort_order"),
            is_active: r.get::<i64, _>("is_active") == 1,
        };
        options_by_group
            .entry(m.group_id.clone())
            .or_default()
            .push(m);
    }
    let modifier_groups: Vec<ModifierGroup> = group_rows
        .iter()
        .map(|r| {
            let id: String = r.get("id");
            ModifierGroup {
                options: options_by_group.remove(&id).unwrap_or_default(),
                id,
                name: r.get("name"),
                selection_type: r.get("selection_type"),
                min_select: r.get("min_select"),
                max_select: r.get("max_select"),
                sort_order: r.get("sort_order"),
            }
        })
        .collect();

    let links = sqlx::query(
        "SELECT img.item_id, img.group_id FROM item_modifier_groups img
           JOIN modifier_groups g ON g.id = img.group_id AND g.deleted_at IS NULL
          ORDER BY img.sort_order",
    )
    .fetch_all(ctx.db.reader())
    .await?;
    let mut groups_by_item: std::collections::HashMap<String, Vec<String>> = Default::default();
    for r in &links {
        groups_by_item
            .entry(r.get("item_id"))
            .or_default()
            .push(r.get("group_id"));
    }

    let mut by_cat: std::collections::HashMap<String, Vec<Item>> = Default::default();
    let mut uncategorized = Vec::new();
    for r in &items {
        let id: String = r.get("id");
        let category_id: Option<String> = r.get("category_id");
        let item = Item {
            variants: by_item.remove(&id).unwrap_or_default(),
            modifier_group_ids: groups_by_item.remove(&id).unwrap_or_default(),
            id,
            category_id: category_id.clone(),
            name: r.get("name"),
            short_name: r.get("short_name"),
            base_price: r.get("base_price"),
            tax_code: r.get("tax_code"),
            is_open_price: r.get::<i64, _>("is_open_price") == 1,
            sold_out_until: r.get("sold_out_until"),
            sort_order: r.get("sort_order"),
            is_active: r.get::<i64, _>("is_active") == 1,
        };
        match category_id {
            Some(c) => by_cat.entry(c).or_default().push(item),
            None => uncategorized.push(item),
        }
    }

    let categories = cats
        .iter()
        .map(|r| {
            let id: String = r.get("id");
            CategoryNode {
                items: by_cat.remove(&id).unwrap_or_default(),
                category: Category {
                    id,
                    name: r.get("name"),
                    color: r.get("color"),
                    sort_order: r.get("sort_order"),
                    is_active: r.get::<i64, _>("is_active") == 1,
                },
            }
        })
        .collect();

    Ok(MenuTree {
        categories,
        uncategorized,
        modifier_groups,
    })
}

// ---------------------------------------------------------------- 寫

pub async fn upsert_category(ctx: &Ctx, input: CategoryInput) -> AppResult<Category> {
    validate_name(&input.name, "分類名稱")?;
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());
    let creating = input.id.is_none();

    let mut uow = ctx.db.begin_write().await?;
    if creating {
        sqlx::query(
            "INSERT INTO categories (id, store_id, name, color, sort_order, is_active, created_at, updated_at)
             SELECT ?1, s.id, ?2, ?3, ?4, ?5, ?6, ?6 FROM stores s ORDER BY s.id LIMIT 1",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.color)
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE categories SET name = ?2, color = ?3, sort_order = ?4, is_active = ?5, updated_at = ?6
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.color)
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到分類 {id}")));
        }
    }

    audit::write_in(
        &mut uow,
        AuditEntry::new("Category", &id, action(creating)).to(&input.name),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;

    Ok(Category {
        id,
        name: input.name,
        color: input.color,
        sort_order: input.sort_order.unwrap_or(0),
        is_active: input.is_active.unwrap_or(true),
    })
}

pub async fn delete_category(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;

    let n =
        sqlx::query("UPDATE categories SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL")
            .bind(&id)
            .bind(now.iso())
            .execute(uow.conn())
            .await?
            .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到分類 {id}")));
    }

    // 品項不跟著刪 —— 它們會落到「未分類」並在畫面上顯示出來。
    // 連帶刪除的話，老闆按錯一個鍵就會以為整批商品消失了。
    sqlx::query("UPDATE items SET category_id = NULL, updated_at = ?2 WHERE category_id = ?1")
        .bind(&id)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;

    audit::write_in(
        &mut uow,
        AuditEntry::new("Category", &id, AuditAction::Delete),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;
    Ok(())
}

pub async fn upsert_item(ctx: &Ctx, input: ItemInput) -> AppResult<Item> {
    validate_name(&input.name, "品項名稱")?;
    validate_price(input.base_price)?;
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());
    let creating = input.id.is_none();
    let tax_code = input.tax_code.clone().unwrap_or_else(|| "TAXABLE".into());

    let mut uow = ctx.db.begin_write().await?;

    // 改價要留下前後值。這是 audit_logs.amount_delta 的用途之一：
    // 「這個月誰把哪些商品調過價、幅度多少」一次查得出來。
    let old_price: Option<i64> = if creating {
        None
    } else {
        sqlx::query_scalar("SELECT base_price FROM items WHERE id = ?1 AND deleted_at IS NULL")
            .bind(&id)
            .fetch_optional(uow.conn())
            .await?
    };

    if creating {
        sqlx::query(
            "INSERT INTO items (id, store_id, category_id, name, short_name, base_price, tax_code,
                                is_open_price, sold_out_until, sort_order, is_active, created_at, updated_at)
             SELECT ?1, s.id, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11
               FROM stores s ORDER BY s.id LIMIT 1",
        )
        .bind(&id)
        .bind(&input.category_id)
        .bind(&input.name)
        .bind(&input.short_name)
        .bind(input.base_price)
        .bind(&tax_code)
        .bind(i64::from(input.is_open_price.unwrap_or(false)))
        .bind(&input.sold_out_until)
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE items SET category_id = ?2, name = ?3, short_name = ?4, base_price = ?5,
                              tax_code = ?6, is_open_price = ?7, sold_out_until = ?8,
                              sort_order = ?9, is_active = ?10, updated_at = ?11
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(&input.category_id)
        .bind(&input.name)
        .bind(&input.short_name)
        .bind(input.base_price)
        .bind(&tax_code)
        .bind(i64::from(input.is_open_price.unwrap_or(false)))
        .bind(&input.sold_out_until)
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到品項 {id}")));
        }
    }

    let mut entry = AuditEntry::new("Item", &id, action(creating)).to(&input.name);
    if let Some(old) = old_price {
        if old != input.base_price {
            entry = AuditEntry::new("Item", &id, AuditAction::PriceOverride)
                .from(old)
                .to(input.base_price)
                .amount(input.base_price - old);
        }
    }
    audit::write_in(&mut uow, entry, &ctx.actor, &now).await?;
    uow.commit().await?;

    Ok(Item {
        id,
        category_id: input.category_id,
        name: input.name,
        short_name: input.short_name,
        base_price: input.base_price,
        tax_code,
        is_open_price: input.is_open_price.unwrap_or(false),
        sold_out_until: input.sold_out_until,
        sort_order: input.sort_order.unwrap_or(0),
        is_active: input.is_active.unwrap_or(true),
        // 這一支回的是剛存好的那一筆，掛在它身上的規格與選項群組要
        // 重讀菜單樹才拿得到。呼叫端本來就會重讀。
        variants: Vec::new(),
        modifier_group_ids: Vec::new(),
    })
}

pub async fn delete_item(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;

    let n = sqlx::query("UPDATE items SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL")
        .bind(&id)
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到品項 {id}")));
    }
    sqlx::query(
        "UPDATE item_variants SET deleted_at = ?2 WHERE item_id = ?1 AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    audit::write_in(
        &mut uow,
        AuditEntry::new("Item", &id, AuditAction::Delete),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;
    Ok(())
}

pub async fn upsert_variant(ctx: &Ctx, input: VariantInput) -> AppResult<Variant> {
    validate_name(&input.name, "規格名稱")?;
    if !matches!(input.price_mode.as_str(), "delta" | "absolute") {
        return Err(AppError::Validation(
            "價格模式只能是 delta（跟著基本價加減）或 absolute（固定價）".into(),
        ));
    }
    let price = input.price.unwrap_or(0);
    let delta = input.price_delta.unwrap_or(0);
    if input.price_mode == "absolute" {
        validate_price(price)?;
    }
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());
    let creating = input.id.is_none();
    let is_default = input.is_default.unwrap_or(false);

    let mut uow = ctx.db.begin_write().await?;

    // 一個品項只能有一個預設規格 —— 資料庫有 partial unique index 擋著，
    // 所以要先把其他的取消，否則會撞唯一索引。
    if is_default {
        sqlx::query(
            "UPDATE item_variants SET is_default = 0, updated_at = ?3
              WHERE item_id = ?1 AND id <> ?2 AND deleted_at IS NULL",
        )
        .bind(&input.item_id)
        .bind(&id)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    if creating {
        sqlx::query(
            "INSERT INTO item_variants (id, item_id, code, name, price_mode, price, price_delta,
                                        is_default, sort_order, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        )
        .bind(&id)
        .bind(&input.item_id)
        .bind(&input.code)
        .bind(&input.name)
        .bind(&input.price_mode)
        .bind(price)
        .bind(delta)
        .bind(i64::from(is_default))
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE item_variants SET code = ?2, name = ?3, price_mode = ?4, price = ?5,
                                      price_delta = ?6, is_default = ?7, sort_order = ?8,
                                      is_active = ?9, updated_at = ?10
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(&input.code)
        .bind(&input.name)
        .bind(&input.price_mode)
        .bind(price)
        .bind(delta)
        .bind(i64::from(is_default))
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到規格 {id}")));
        }
    }

    audit::write_in(
        &mut uow,
        AuditEntry::new("ItemVariant", &id, action(creating)).to(&input.name),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;

    Ok(Variant {
        id,
        item_id: input.item_id,
        code: input.code,
        name: input.name,
        price_mode: input.price_mode,
        price,
        price_delta: delta,
        is_default,
        sort_order: input.sort_order.unwrap_or(0),
        is_active: input.is_active.unwrap_or(true),
    })
}

pub async fn delete_variant(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;

    let n = sqlx::query(
        "UPDATE item_variants SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到規格 {id}")));
    }
    audit::write_in(
        &mut uow,
        AuditEntry::new("ItemVariant", &id, AuditAction::Delete),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------- 驗證

fn action(creating: bool) -> AuditAction {
    if creating {
        AuditAction::Create
    } else {
        AuditAction::Update
    }
}

fn validate_name(name: &str, what: &str) -> AppResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(format!("{what}不能是空的").into()));
    }
    // 出單機一行只有 32（58mm）或 48（80mm）個半形字元，中文佔兩格。
    // 太長的品名在廚房單上會被截掉，廚師就看不出是哪一道菜了。
    if trimmed.chars().count() > 40 {
        return Err(AppError::Validation(
            format!(
                "{what}太長（{} 字）—— 出單機一行放不下，請用 20 字以內",
                trimmed.chars().count()
            )
            .into(),
        ));
    }
    Ok(())
}

fn validate_price(price: i64) -> AppResult<()> {
    if price < 0 {
        return Err(AppError::Validation("價格不能是負數".into()));
    }
    // 金額是整數元（見 ADR 0001），這裡順手擋掉離譜的輸入 ——
    // 收銀員多按一個 0 是很常見的手誤。
    if price > 1_000_000 {
        return Err(AppError::Validation(
            format!("價格 {price} 元看起來不對，請確認是否多按了一個 0").into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- 選項群組

/// 建立或更新一組選項（甜度 / 冰塊 / 加購）。
pub async fn upsert_modifier_group(
    ctx: &Ctx,
    input: ModifierGroupInput,
) -> AppResult<ModifierGroup> {
    validate_name(&input.name, "群組名稱")?;
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;

    let selection = match input.selection_type.as_deref() {
        Some("multiple") => "multiple",
        _ => "single",
    };
    // 單選群組的 max_select 只能是 1 —— 讓它變成 3 會做出一個
    // 「單選但可以選三個」的東西，而點餐畫面沒有辦法呈現那種狀態。
    let max = if selection == "single" {
        1
    } else {
        input.max_select.unwrap_or(0).max(0)
    };
    let min = input.min_select.unwrap_or(0).max(0);

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());
    let mut uow = ctx.db.begin_write().await?;

    if input.id.is_none() {
        sqlx::query(
            "INSERT INTO modifier_groups (id, store_id, name, selection_type, min_select,
                                          max_select, sort_order, created_at, updated_at)
             SELECT ?1, s.id, ?2, ?3, ?4, ?5, ?6, ?7, ?7 FROM stores s ORDER BY s.id LIMIT 1",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(selection)
        .bind(min)
        .bind(max)
        .bind(input.sort_order.unwrap_or(0))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE modifier_groups SET name = ?2, selection_type = ?3, min_select = ?4,
                                        max_select = ?5, sort_order = ?6, updated_at = ?7
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(selection)
        .bind(min)
        .bind(max)
        .bind(input.sort_order.unwrap_or(0))
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到選項群組 {id}")));
        }
    }
    uow.commit().await?;

    menu_tree(ctx)
        .await?
        .modifier_groups
        .into_iter()
        .find(|g| g.id == id)
        .ok_or_else(|| AppError::Internal("群組存好了卻讀不回來".into()))
}

/// 刪掉一組選項。
///
/// 已經賣出去的訂單不受影響 —— `order_item_modifiers` 存的是**當時的名稱與
/// 價格快照**，不是外鍵。這正是快照存在的理由：三個月後查昨天的單，
/// 看到的必須是當時那份。
pub async fn delete_modifier_group(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    let n = sqlx::query(
        "UPDATE modifier_groups SET deleted_at = ?2, updated_at = ?2
          WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到選項群組 {id}")));
    }
    // 掛在品項上的關聯一起拆掉，否則點餐時會找到一個已刪除的群組。
    sqlx::query("DELETE FROM item_modifier_groups WHERE group_id = ?1")
        .bind(&id)
        .execute(uow.conn())
        .await?;
    uow.commit().await?;
    Ok(())
}

pub async fn upsert_modifier(ctx: &Ctx, input: ModifierInput) -> AppResult<Modifier> {
    validate_name(&input.name, "選項名稱")?;
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());
    let price = input.price.unwrap_or(0);
    let mut uow = ctx.db.begin_write().await?;

    if input.id.is_none() {
        sqlx::query(
            "INSERT INTO modifiers (id, group_id, name, price, is_default, sold_out_until,
                                    sort_order, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        )
        .bind(&id)
        .bind(&input.group_id)
        .bind(&input.name)
        .bind(price)
        .bind(i64::from(input.is_default.unwrap_or(false)))
        .bind(&input.sold_out_until)
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE modifiers SET name = ?2, price = ?3, is_default = ?4, sold_out_until = ?5,
                                  sort_order = ?6, is_active = ?7, updated_at = ?8
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(price)
        .bind(i64::from(input.is_default.unwrap_or(false)))
        .bind(&input.sold_out_until)
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到選項 {id}")));
        }
    }

    // 單選群組只能有一個預設 —— 兩個預設的話點餐畫面不知道要勾哪一個。
    if input.is_default.unwrap_or(false) {
        sqlx::query(
            "UPDATE modifiers SET is_default = 0, updated_at = ?3
              WHERE group_id = ?1 AND id <> ?2
                AND (SELECT selection_type FROM modifier_groups g WHERE g.id = ?1) = 'single'",
        )
        .bind(&input.group_id)
        .bind(&id)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }
    uow.commit().await?;

    menu_tree(ctx)
        .await?
        .modifier_groups
        .into_iter()
        .flat_map(|g| g.options)
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::Internal("選項存好了卻讀不回來".into()))
}

pub async fn delete_modifier(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    let n = sqlx::query(
        "UPDATE modifiers SET deleted_at = ?2, updated_at = ?2
          WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到選項 {id}")));
    }
    uow.commit().await?;
    Ok(())
}

/// 這個品項要問哪幾組選項。傳整份清單（不是加一個／刪一個）——
/// 畫面上是一排勾選框，整批送比較不會出現「勾了但沒存到」的狀態。
pub async fn set_item_modifier_groups(
    ctx: &Ctx,
    item_id: String,
    group_ids: Vec<String>,
) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;
    let mut uow = ctx.db.begin_write().await?;
    sqlx::query("DELETE FROM item_modifier_groups WHERE item_id = ?1")
        .bind(&item_id)
        .execute(uow.conn())
        .await?;
    for (i, gid) in group_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO item_modifier_groups (item_id, group_id, sort_order) VALUES (?1, ?2, ?3)",
        )
        .bind(&item_id)
        .bind(gid)
        .bind(i as i64)
        .execute(uow.conn())
        .await?;
    }
    uow.commit().await?;
    Ok(())
}
