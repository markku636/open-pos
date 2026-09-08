//! 示範資料的整合測試。
//!
//! 只有一條真正重要的性質：**冪等**。示範資料是唯一一種「程式自己寫進
//! 正式資料庫」的東西，它一旦會重複，就會變成「示範資料汙染真實菜單」——
//! 而店家不會知道那四個「珍珠奶茶」是誰建的。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::{demo, menu};

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
        "openpos_demo_{}_{}_{}",
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

fn total_items(t: &menu::MenuTree) -> usize {
    t.categories.iter().map(|c| c.items.len()).sum::<usize>() + t.uncategorized.len()
}

#[tokio::test]
async fn a_fresh_install_gets_a_menu_you_can_actually_click() {
    let e = env("fresh").await;

    assert!(demo::seed_demo_menu(&e.ctx).await.unwrap());
    let tree = menu::menu_tree(&e.ctx).await.unwrap();

    assert_eq!(tree.categories.len(), 4, "分類數不對");
    // 用 demo_menu_size() 而不是寫死數字：畫面上顯示的就是這個函式的回傳值，
    // 兩邊對不上的症狀是「說建了 38 個、實際 39 個」，而那正是要防的。
    let (_, expected_items) = demo::demo_menu_size();
    assert_eq!(
        total_items(&tree),
        expected_items,
        "品項數與 demo_menu_size() 對不上"
    );
    // 未分類必須是空的 —— 示範資料自己都放不進分類的話，
    // 使用者第一眼看到的就是一個「未分類」按鈕，那是最糟的第一印象。
    assert!(tree.uncategorized.is_empty());

    let names: Vec<&str> = tree
        .categories
        .iter()
        .flat_map(|c| c.items.iter().map(|i| i.name.as_str()))
        .collect();
    assert!(names.contains(&"珍珠奶茶"));
    assert!(names.contains(&"滷肉飯"));

    e.ctx.db.close().await;
}

/// ★ 冪等。不然每次帶 `--demo` 重開 daemon 就多一份珍珠奶茶。
#[tokio::test]
async fn seeding_twice_changes_nothing() {
    let e = env("twice").await;

    assert!(demo::seed_demo_menu(&e.ctx).await.unwrap());
    let before = menu::menu_tree(&e.ctx).await.unwrap();

    assert!(
        !demo::seed_demo_menu(&e.ctx).await.unwrap(),
        "第二次應該回報「沒做事」"
    );
    let after = menu::menu_tree(&e.ctx).await.unwrap();

    assert_eq!(total_items(&before), total_items(&after));
    assert_eq!(before.categories.len(), after.categories.len());

    e.ctx.db.close().await;
}

/// 店家已經自己建過菜單時，`--demo` 必須完全不動手。
#[tokio::test]
async fn an_existing_menu_is_never_touched() {
    let e = env("existing").await;

    menu::upsert_item(
        &e.ctx,
        menu::ItemInput {
            id: None,
            category_id: None,
            name: "老闆自己建的招牌".into(),
            short_name: None,
            base_price: 88,
            tax_code: None,
            is_open_price: None,
            sold_out_until: None,
            sort_order: None,
            is_active: Some(true),
        },
    )
    .await
    .unwrap();

    assert!(!demo::seed_demo_menu(&e.ctx).await.unwrap());
    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    assert_eq!(total_items(&tree), 1);

    e.ctx.db.close().await;
}
