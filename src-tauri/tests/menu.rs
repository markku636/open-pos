//! 商品維護的整合測試。
//!
//! 重點不在 CRUD 會不會動，而在幾個**做錯了店家會恨你**的行為：
//! 刪分類不會連帶刪商品、改價會留下可查的稽核、一個品項只能有一個預設規格。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::menu::{self, CategoryInput, ItemInput, VariantInput};

static SEQ: AtomicU32 = AtomicU32::new(0);

struct Env {
    root: std::path::PathBuf,
    ctx: Ctx,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn env(tag: &str) -> Env {
    let root = std::env::temp_dir().join(format!(
        "openpos_menu_{}_{}_{}",
        tag,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let layout = DataLayout::new(root.clone());
    layout.ensure().unwrap();

    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    let now = Stamp::now();
    let mut uow = db.begin_write().await.unwrap();
    open_pos::services::seed::apply(&mut uow, &now)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let actor = open_pos::services::seed::default_actor(&db).await.unwrap();

    Env {
        root,
        ctx: Arc::new(AppCtx {
            db,
            layout,
            started_at: now.at,
            actor,
        }),
    }
}

fn cat(name: &str) -> CategoryInput {
    CategoryInput {
        id: None,
        name: name.into(),
        color: None,
        sort_order: None,
        is_active: None,
    }
}

fn item(name: &str, price: i64, category_id: Option<String>) -> ItemInput {
    ItemInput {
        id: None,
        category_id,
        name: name.into(),
        short_name: None,
        base_price: price,
        tax_code: None,
        is_open_price: None,
        sold_out_until: None,
        sort_order: None,
        is_active: None,
    }
}

async fn audit_rows(ctx: &Ctx, action: &str) -> Vec<(String, Option<i64>)> {
    sqlx::query_as::<_, (String, Option<i64>)>(
        "SELECT entity_id, amount_delta FROM audit_logs WHERE action = ?1 ORDER BY created_at, id",
    )
    .bind(action)
    .fetch_all(ctx.db.reader())
    .await
    .unwrap()
}

#[tokio::test]
async fn create_category_and_item_shows_up_in_the_tree() {
    let e = env("create").await;
    let c = menu::upsert_category(&e.ctx, cat("飲料")).await.unwrap();
    menu::upsert_item(&e.ctx, item("珍珠奶茶", 60, Some(c.id.clone())))
        .await
        .unwrap();

    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    assert_eq!(tree.categories.len(), 1);
    assert_eq!(tree.categories[0].category.name, "飲料");
    assert_eq!(tree.categories[0].items.len(), 1);
    assert_eq!(tree.categories[0].items[0].name, "珍珠奶茶");
    assert_eq!(tree.categories[0].items[0].base_price, 60);
    assert!(tree.uncategorized.is_empty());
}

/// ★ 刪分類**不會**連帶刪商品。
///
/// 老闆按錯一個鍵就整批商品消失，是不可接受的。品項會落到「未分類」，
/// 而「未分類」在畫面上是看得見的一格 —— 藏起來等於刪掉。
#[tokio::test]
async fn deleting_a_category_moves_its_items_to_uncategorized() {
    let e = env("delcat").await;
    let c = menu::upsert_category(&e.ctx, cat("誤建的分類"))
        .await
        .unwrap();
    menu::upsert_item(&e.ctx, item("滷肉飯", 55, Some(c.id.clone())))
        .await
        .unwrap();
    menu::upsert_item(&e.ctx, item("貢丸湯", 30, Some(c.id.clone())))
        .await
        .unwrap();

    menu::delete_category(&e.ctx, c.id.clone()).await.unwrap();

    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    assert!(tree.categories.is_empty(), "分類應該不見了");
    assert_eq!(tree.uncategorized.len(), 2, "商品必須還在，只是沒有分類");
    let mut names: Vec<&str> = tree.uncategorized.iter().map(|i| i.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["滷肉飯", "貢丸湯"]);
}

/// ★ 改價要留下可查的稽核，而且金額差要記在 amount_delta 上。
///
/// 「這個月誰把哪些商品調過價、幅度多少」必須一個 SUM 就查得出來 ——
/// 防弊的價值全在統計上，單看一筆調價永遠是合理的。
#[tokio::test]
async fn changing_a_price_records_an_auditable_delta() {
    let e = env("price").await;
    let c = menu::upsert_category(&e.ctx, cat("主食")).await.unwrap();
    let it = menu::upsert_item(&e.ctx, item("牛肉麵", 180, Some(c.id.clone())))
        .await
        .unwrap();

    let mut update = item("牛肉麵", 200, Some(c.id.clone()));
    update.id = Some(it.id.clone());
    menu::upsert_item(&e.ctx, update).await.unwrap();

    let rows = audit_rows(&e.ctx, "price_override").await;
    assert_eq!(rows.len(), 1, "調價要留下一筆 price_override");
    assert_eq!(rows[0].0, it.id);
    assert_eq!(rows[0].1, Some(20), "amount_delta 應為 +20");

    // 沒改價的儲存不該被記成調價 —— 否則統計會被雜訊淹沒。
    let mut rename = item("牛肉麵（大）", 200, Some(c.id));
    rename.id = Some(it.id.clone());
    menu::upsert_item(&e.ctx, rename).await.unwrap();
    assert_eq!(audit_rows(&e.ctx, "price_override").await.len(), 1);
    assert_eq!(audit_rows(&e.ctx, "update").await.len(), 1);
}

#[tokio::test]
async fn deleting_an_item_also_hides_its_variants() {
    let e = env("delitem").await;
    let c = menu::upsert_category(&e.ctx, cat("飲料")).await.unwrap();
    let it = menu::upsert_item(&e.ctx, item("紅茶", 30, Some(c.id)))
        .await
        .unwrap();

    menu::upsert_variant(
        &e.ctx,
        VariantInput {
            id: None,
            item_id: it.id.clone(),
            code: "L".into(),
            name: "大杯".into(),
            price_mode: "delta".into(),
            price: None,
            price_delta: Some(10),
            is_default: Some(true),
            sort_order: None,
            is_active: None,
        },
    )
    .await
    .unwrap();

    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    assert_eq!(tree.categories[0].items[0].variants.len(), 1);

    menu::delete_item(&e.ctx, it.id).await.unwrap();
    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    assert!(tree.categories[0].items.is_empty());
}

/// 資料庫有 partial unique index 擋著「一個品項只能有一個預設規格」，
/// 所以服務層必須先把其他的取消，否則會撞唯一索引。
#[tokio::test]
async fn setting_a_new_default_variant_clears_the_previous_one() {
    let e = env("default").await;
    let c = menu::upsert_category(&e.ctx, cat("飲料")).await.unwrap();
    let it = menu::upsert_item(&e.ctx, item("綠茶", 25, Some(c.id)))
        .await
        .unwrap();

    for (code, name, default) in [("M", "中杯", true), ("L", "大杯", true)] {
        menu::upsert_variant(
            &e.ctx,
            VariantInput {
                id: None,
                item_id: it.id.clone(),
                code: code.into(),
                name: name.into(),
                price_mode: "delta".into(),
                price: None,
                price_delta: Some(if code == "L" { 10 } else { 0 }),
                is_default: Some(default),
                sort_order: None,
                is_active: None,
            },
        )
        .await
        .unwrap();
    }

    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    let variants = &tree.categories[0].items[0].variants;
    assert_eq!(variants.len(), 2);
    let defaults: Vec<&str> = variants
        .iter()
        .filter(|v| v.is_default)
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(
        defaults,
        vec!["大杯"],
        "只能有一個預設規格，且是最後設定的那個"
    );
}

#[tokio::test]
async fn validation_rejects_bad_input_with_actionable_messages() {
    let e = env("validate").await;
    let c = menu::upsert_category(&e.ctx, cat("測試")).await.unwrap();

    let empty = menu::upsert_item(&e.ctx, item("   ", 10, Some(c.id.clone())))
        .await
        .unwrap_err();
    assert!(empty.message().contains("不能是空的"));

    let negative = menu::upsert_item(&e.ctx, item("負價", -1, Some(c.id.clone())))
        .await
        .unwrap_err();
    assert!(negative.message().contains("負數"));

    // 多按一個 0 是收銀員最常見的手誤，訊息要直接點出來。
    let typo = menu::upsert_item(&e.ctx, item("打錯", 9_999_999, Some(c.id.clone())))
        .await
        .unwrap_err();
    assert!(
        typo.message().contains("多按了一個 0"),
        "{}",
        typo.message()
    );

    // 出單機一行放不下的品名，廚師會看不出是哪一道菜。
    let long = menu::upsert_item(&e.ctx, item(&"很長".repeat(30), 10, Some(c.id)))
        .await
        .unwrap_err();
    assert!(long.message().contains("出單機一行放不下"));
}

#[tokio::test]
async fn updating_a_missing_item_reports_not_found() {
    let e = env("missing").await;
    let mut ghost = item("不存在", 10, None);
    ghost.id = Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into());
    let err = menu::upsert_item(&e.ctx, ghost).await.unwrap_err();
    assert_eq!(err.code(), "ERR_NOT_FOUND");
}
