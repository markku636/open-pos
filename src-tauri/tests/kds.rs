//! 廚房顯示的整合測試。
//!
//! 兩件事要守住：
//!
//! 1. **看板上只出現該做的東西**（已上菜、已作廢的不該還在上面）
//! 2. **狀態只能往前推** —— 讓它可以往回，就會有人把已經上桌的東西改回
//!    「未完成」，而那會讓整面板不再可信

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::{demo, kds, menu, order};
use tower::ServiceExt;

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
        "openpos_kds_{}_{}_{}",
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

impl Env {
    async fn order_with(&self, items: &[&str]) -> order::OrderView {
        let tree = menu::menu_tree(&self.ctx).await.unwrap();
        let find = |name: &str| {
            tree.categories
                .iter()
                .flat_map(|c| c.items.iter())
                .find(|i| i.name == name)
                .unwrap()
                .id
                .clone()
        };
        let o = order::open_order(
            &self.ctx,
            order::OpenOrderReq {
                channel: Channel::Takeout,
                table_id: None,
                guest_count: None,
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
                lines: items
                    .iter()
                    .map(|n| order::NewLine {
                        item_id: find(n),
                        variant_id: None,
                        modifier_ids: vec![],
                        qty_milli: None,
                        note: None,
                    })
                    .collect(),
            },
        )
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn the_board_shows_what_the_kitchen_still_has_to_make() {
    let e = env("board").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();

    assert!(kds::board(&e.ctx).await.unwrap().tickets.is_empty());

    e.order_with(&["珍珠奶茶", "滷肉飯"]).await;
    let board = kds::board(&e.ctx).await.unwrap();
    assert_eq!(board.tickets.len(), 1);
    assert_eq!(board.tickets[0].lines.len(), 2);
    // 看板給的是代碼不是「外帶」兩個字 —— 顯示文字由廚房平板那一端決定。
    assert_eq!(board.tickets[0].channel, "takeout");
    // 剛開的單等待時間應該接近 0，而不是一個亂數。
    assert!(board.tickets[0].waiting_seconds < 5);

    e.ctx.db.close().await;
}

/// ★ 已經上菜的不該還留在看板上。
#[tokio::test]
async fn a_served_line_leaves_the_board() {
    let e = env("served").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶", "滷肉飯"]).await;

    let bubble = o
        .lines
        .iter()
        .find(|l| l.name == "珍珠奶茶")
        .unwrap()
        .id
        .clone();
    kds::advance(&e.ctx, bubble.clone(), "ready".into())
        .await
        .unwrap();
    let board = kds::advance(&e.ctx, bubble, "served".into()).await.unwrap();

    assert_eq!(board.tickets.len(), 1);
    assert_eq!(board.tickets[0].lines.len(), 1, "上菜的那一項應該消失");
    assert_eq!(board.tickets[0].lines[0].name, "滷肉飯");

    e.ctx.db.close().await;
}

/// 作廢的單整張消失。
#[tokio::test]
async fn a_voided_order_leaves_the_board() {
    let e = env("voided").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶"]).await;
    assert_eq!(kds::board(&e.ctx).await.unwrap().tickets.len(), 1);

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

    assert!(kds::board(&e.ctx).await.unwrap().tickets.is_empty());
    e.ctx.db.close().await;
}

/// ★ 狀態只能往前推。
///
/// 讓它可以往回，就會有人把已經上桌的東西改回「未完成」——
/// 而那會讓整面板不再可信。
#[tokio::test]
async fn a_line_can_only_move_forward() {
    let e = env("forward").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶"]).await;
    let line = o.lines[0].id.clone();

    let board = kds::advance(&e.ctx, line.clone(), "ready".into())
        .await
        .unwrap();
    assert_eq!(board.tickets[0].lines[0].status, "ready");

    // 往回推：安靜忽略，不報錯 ——
    // 兩台平板同時點同一張單是正常的，「按了沒反應」比跳錯誤訊息好。
    let board = kds::advance(&e.ctx, line.clone(), "cooking".into())
        .await
        .unwrap();
    assert_eq!(board.tickets[0].lines[0].status, "ready", "狀態被推回去了");

    // 重複推同一格也一樣。
    let board = kds::advance(&e.ctx, line, "ready".into()).await.unwrap();
    assert_eq!(board.tickets[0].lines[0].status, "ready");

    e.ctx.db.close().await;
}

#[tokio::test]
async fn an_unknown_status_is_refused() {
    let e = env("badstatus").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶"]).await;

    let err = kds::advance(&e.ctx, o.lines[0].id.clone(), "burnt".into())
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    e.ctx.db.close().await;
}

/// KDS 的兩個指令要真的掛在區網上 —— 廚房平板是從瀏覽器連進來的。
#[tokio::test]
async fn the_kds_endpoints_are_reachable_over_the_lan() {
    let e = env("lan").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶"]).await;

    let app = open_pos::lan::router::build(e.ctx.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/rpc/kds_board")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let app = open_pos::lan::router::build(e.ctx.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/rpc/kds_advance")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"lineId":"{}","to":"ready"}}"#,
                    o.lines[0].id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(
        kds::board(&e.ctx).await.unwrap().tickets[0].lines[0].status,
        "ready"
    );

    // 管理類指令仍然不在區網上。這條界線不能因為加了 KDS 就鬆掉。
    let app = open_pos::lan::router::build(e.ctx.clone());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/rpc/list_printers")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    e.ctx.db.close().await;
}
