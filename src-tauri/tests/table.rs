//! 桌位的整合測試。
//!
//! 內用的錢全部掛在桌上，所以這裡守的每一條都是「錢會不會不見」：
//!
//! 1. **一桌同時只有一個未關的 session** —— 兩張單要疊在同一桌上，不是各開一桌
//! 2. **有未結帳的單不能清桌** —— 否則「清桌」是一個把帳丟掉的按鈕
//! 3. **有客人的桌不能刪** —— 刪掉之後那一桌的單會變成孤兒，而客人還坐在那
//! 4. **結完帳才釋放桌位，而且要是最後一張單** —— 同桌其他未結的單不能跟著消失

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::{demo, menu, order, table};

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
        "openpos_table_{}_{}_{}",
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

    let e = Env {
        root,
        ctx: Arc::new(AppCtx {
            db,
            layout,
            started_at: now.at,
            actor,
        }),
    };
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e
}

impl Env {
    async fn table(&self, code: &str) -> table::TableView {
        table::list_tables(&self.ctx)
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.code == code)
            .unwrap_or_else(|| panic!("找不到桌位 {code}"))
    }

    /// 在某一桌開一張單並點一樣東西。
    async fn order_on(&self, table_id: &str, item: &str) -> order::OrderView {
        let tree = menu::menu_tree(&self.ctx).await.unwrap();
        let item_id = tree
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| i.name == item)
            .unwrap()
            .id
            .clone();

        let o = order::open_order(
            &self.ctx,
            order::OpenOrderReq {
                channel: Channel::DineIn,
                table_id: Some(table_id.to_string()),
                guest_count: Some(2),
                client_id: None,
            },
        )
        .await
        .unwrap();

        order::add_lines(
            &self.ctx,
            order::AddLinesReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                lines: vec![order::NewLine {
                    item_id,
                    variant_id: None,
                    modifier_ids: vec![],
                    qty_milli: None,
                    note: None,
                }],
            },
        )
        .await
        .unwrap()
    }

    async fn settle(&self, o: &order::OrderView) -> order::SettleResult {
        order::settle(
            &self.ctx,
            order::SettleReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                payments: vec![order::PaymentReq {
                    method_code: "cash".into(),
                    amount: o.grand_total,
                    tendered: Some(o.grand_total),
                    ref_no: None,
                }],
                idem_key: open_pos::core::ids::Id::new().to_string(),
                split: None,
            },
        )
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn a_seated_table_shows_its_money_and_its_clock() {
    let e = env("seated").await;
    let a1 = e.table("A1").await;
    assert!(a1.session.is_none(), "剛開店的桌應該是空的");
    assert_eq!(a1.area_name.as_deref(), Some("大廳"));

    let o = e.order_on(&a1.id, "滷肉飯").await;

    let a1 = e.table("A1").await;
    let s = a1.session.expect("開單之後這一桌應該有人");
    assert_eq!(s.guest_count, 2);
    assert_eq!(s.order_count, 1);
    assert_eq!(s.total, o.grand_total, "桌位圖上的金額要跟單一致");
    // 剛開的桌坐了 0 分鐘，而不是一個亂數。
    assert!(s.seated_seconds < 5);

    e.ctx.db.close().await;
}

/// ★ 一桌只有一個未關的 session：第二張單疊上去，不是另開一桌。
#[tokio::test]
async fn two_orders_on_one_table_share_one_session() {
    let e = env("share").await;
    let a1 = e.table("A1").await;

    let first = e.order_on(&a1.id, "滷肉飯").await;
    let second = e.order_on(&a1.id, "珍珠奶茶").await;
    assert_ne!(first.id, second.id, "這是兩張不同的單");

    let s = e.table("A1").await.session.expect("桌上有人");
    assert_eq!(s.order_count, 2);
    assert_eq!(
        s.total,
        first.grand_total + second.grand_total,
        "同一桌的金額要加總 —— 客人問「多少錢」問的是整桌"
    );

    e.ctx.db.close().await;
}

/// ★ 有未結帳的單不能清桌。
///
/// 否則「清桌」會變成一個把帳丟掉的按鈕，而那是最容易被拿來吃單的操作。
#[tokio::test]
async fn a_table_with_an_unpaid_order_refuses_to_be_cleared() {
    let e = env("unpaid").await;
    let a1 = e.table("A1").await;
    let o = e.order_on(&a1.id, "滷肉飯").await;

    let err = table::close_table(&e.ctx, a1.id.clone()).await.unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    assert!(
        err.to_string().contains("1"),
        "訊息要說清楚還有幾張單：{err}"
    );
    assert!(e.table("A1").await.session.is_some(), "桌沒有被清掉");

    // 結完帳之後桌位自動釋放 —— 不必再按一次「清桌」。
    e.settle(&o).await;
    assert!(e.table("A1").await.session.is_none());

    // 已經空了的桌再清一次是「找不到」，不是靜默成功。
    let err = table::close_table(&e.ctx, a1.id).await.unwrap_err();
    assert_eq!(err.code(), "ERR_NOT_FOUND");

    e.ctx.db.close().await;
}

/// ★ 結一張單不能把同桌其他未結的單一起關掉。
///
/// 一桌可以有很多張單（分開結帳、續攤、加點開新單）。「結完帳就關檯」很直覺，
/// 但它會把剩下那些單留在一個已關的 session 上：從桌位圖消失，帳卻還在。
#[tokio::test]
async fn settling_one_order_does_not_release_a_table_that_still_owes() {
    let e = env("partial").await;
    let a1 = e.table("A1").await;

    let first = e.order_on(&a1.id, "滷肉飯").await;
    let second = e.order_on(&a1.id, "珍珠奶茶").await;

    e.settle(&first).await;

    let s = e
        .table("A1")
        .await
        .session
        .expect("還有一張沒結的單，桌不該被釋放");
    assert_eq!(s.order_count, 2, "已結的那張仍然算在這一桌的帳上");

    // 兩張都結完，桌才真的空出來。
    let second = order::get_order(&e.ctx, &second.id).await.unwrap();
    e.settle(&second).await;
    assert!(e.table("A1").await.session.is_none());

    e.ctx.db.close().await;
}

/// ★ 有客人的桌不能刪：刪掉之後那一桌的單會變成孤兒，而客人還在座位上。
#[tokio::test]
async fn a_seated_table_cannot_be_deleted() {
    let e = env("delete").await;
    let a1 = e.table("A1").await;
    let o = e.order_on(&a1.id, "滷肉飯").await;

    let err = table::delete_table(&e.ctx, a1.id.clone())
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    assert!(e.table("A1").await.session.is_some());

    e.settle(&o).await;
    table::delete_table(&e.ctx, a1.id).await.unwrap();
    assert!(
        table::list_tables(&e.ctx)
            .await
            .unwrap()
            .iter()
            .all(|t| t.code != "A1"),
        "刪掉的桌不該還在圖上"
    );

    e.ctx.db.close().await;
}

/// 改桌位：區域用名字對應，沒有就建一個。
///
/// 大部分店只有一個區域，逼他們先建「區域」再建桌，那一層對他們是純粹的負擔。
#[tokio::test]
async fn an_area_is_created_from_its_name() {
    let e = env("area").await;

    let t = table::upsert_table(
        &e.ctx,
        table::TableInput {
            id: None,
            code: "C1".into(),
            name: Some("露台".into()),
            seats: Some(2),
            area_name: Some("戶外".into()),
            is_active: Some(true),
        },
    )
    .await
    .unwrap();
    assert_eq!(t.area_name.as_deref(), Some("戶外"));

    // 第二桌用同一個區域名，不該再建一個同名區域。
    let t2 = table::upsert_table(
        &e.ctx,
        table::TableInput {
            id: None,
            code: "C2".into(),
            name: None,
            seats: Some(2),
            area_name: Some("戶外".into()),
            is_active: Some(true),
        },
    )
    .await
    .unwrap();
    assert_eq!(t2.area_name.as_deref(), Some("戶外"));

    let areas: Vec<String> = table::list_tables(&e.ctx)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|t| t.area_name)
        .filter(|a| a == "戶外")
        .collect();
    assert_eq!(areas.len(), 2, "兩桌共用一個區域");

    // 改名走同一支：帶 id 就是更新，不是再建一桌。
    let renamed = table::upsert_table(
        &e.ctx,
        table::TableInput {
            id: Some(t.id.clone()),
            code: "C1".into(),
            name: Some("露台靠海".into()),
            seats: Some(4),
            area_name: Some("戶外".into()),
            is_active: Some(true),
        },
    )
    .await
    .unwrap();
    assert_eq!(renamed.id, t.id);
    assert_eq!(renamed.name.as_deref(), Some("露台靠海"));
    assert_eq!(renamed.seats, 4);

    e.ctx.db.close().await;
}

#[tokio::test]
async fn a_blank_table_code_is_refused() {
    let e = env("blank").await;
    let err = table::upsert_table(
        &e.ctx,
        table::TableInput {
            id: None,
            code: "   ".into(),
            name: None,
            seats: None,
            area_name: None,
            is_active: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");
    e.ctx.db.close().await;
}

/// 作廢的單不算在桌上。否則一桌被作廢的單會讓「清桌」永遠按不下去。
#[tokio::test]
async fn a_voided_order_stops_holding_the_table() {
    let e = env("voided").await;
    let a1 = e.table("A1").await;
    let o = e.order_on(&a1.id, "滷肉飯").await;

    order::void_order(
        &e.ctx,
        order::VoidOrderReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            reason_id: None,
            note: None,
            approver_id: None,
        },
    )
    .await
    .unwrap();

    let s = e.table("A1").await.session.expect("session 還開著");
    assert_eq!(s.order_count, 0, "作廢的單不算在這一桌的帳上");
    assert_eq!(s.total, 0);

    // 沒有欠帳了，清桌就該過。
    table::close_table(&e.ctx, a1.id).await.unwrap();
    assert!(e.table("A1").await.session.is_none());

    e.ctx.db.close().await;
}
