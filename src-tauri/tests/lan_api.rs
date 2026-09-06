//! 區網 HTTP 端點的整合測試。
//!
//! 用 tower 的 oneshot 直接把請求餵給 Router，不真的綁 port ——
//! 測試因此是確定性的，也不會在 CI 上互相搶連接埠。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use open_pos::ctx::AppCtx;
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use serde_json::Value;
use tower::ServiceExt;

static SEQ: AtomicU32 = AtomicU32::new(0);

async fn ctx(tag: &str) -> (DataLayout, open_pos::ctx::Ctx) {
    let root = std::env::temp_dir().join(format!(
        "openpos_lan_{}_{}_{}",
        tag,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let layout = DataLayout::new(root);
    layout.ensure().unwrap();
    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    // 種子資料要跑，否則沒有預設店長帳號可當 actor。
    let now = open_pos::core::clock::Stamp::now();
    let mut uow = db.begin_write().await.unwrap();
    open_pos::services::seed::apply(&mut uow, &now)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let actor = open_pos::services::seed::default_actor(&db).await.unwrap();

    let ctx = Arc::new(AppCtx {
        db,
        layout: DataLayout::new(layout.root.clone()),
        started_at: now.at,
        actor,
    });
    (layout, ctx)
}

async fn body_json(res: axum::response::Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn app_info_returns_version_and_data_dir() {
    let (layout, c) = ctx("info").await;
    let app = open_pos::lan::router::build(c.clone());

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/rpc/app_info")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let v = body_json(res).await;
    // 成功時直接回 T 的 JSON，**不包 envelope** —— 這樣才能與 Tauri 的
    // invoke resolve 出來的形狀一致，前端的 api.ts 才有辦法只有一份。
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert!(v["schemaVersion"].as_i64().unwrap() > 0);
    assert!(!v["dataDir"].as_str().unwrap().is_empty());
    assert!(v.get("error").is_none());

    c.db.close().await;
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[tokio::test]
async fn unknown_command_returns_404_with_a_structured_error() {
    let (layout, c) = ctx("unknown").await;
    let app = open_pos::lan::router::build(c.clone());

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/rpc/definitely_not_a_command")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let v = body_json(res).await;
    // 失敗時包 { error: AppError }，形狀與 Tauri invoke 的 reject 一致。
    assert_eq!(v["error"]["code"], "ERR_NOT_FOUND");
    assert_eq!(v["error"]["retryable"], false);

    c.db.close().await;
    let _ = std::fs::remove_dir_all(&layout.root);
}

/// 管理類指令刻意不存在於區網端點上。
///
/// 就算這個 server 有漏洞，攻擊面也只到「亂送單」，到不了「看營業額 / 改價格」。
#[tokio::test]
async fn admin_commands_are_not_exposed_on_the_lan() {
    let (layout, c) = ctx("admin").await;

    for name in [
        "menu_update",
        "report_daily",
        "shift_close",
        "printer_settings",
        "settings_store",
    ] {
        let app = open_pos::lan::router::build(c.clone());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/rpc/{name}"))
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::NOT_FOUND,
            "{name} 不該出現在區網端點上"
        );
    }

    c.db.close().await;
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[tokio::test]
async fn health_reports_database_ok_but_flags_missing_backup() {
    let (layout, c) = ctx("health").await;
    let app = open_pos::lan::router::build(c.clone());

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 全新的資料庫還沒有任何備份 —— 這**應該**是不健康的。
    // 「以為有在備份、其實隨身碟三個月前就拔掉了」是很常見的情形，
    // 所以預設就要亮紅燈，而不是等使用者自己想到要檢查。
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let v = body_json(res).await;
    assert_eq!(v["ok"], false);

    let items = v["items"].as_array().unwrap();
    let db_item = items.iter().find(|i| i["name"] == "資料庫").unwrap();
    assert_eq!(db_item["ok"], true);
    let backup_item = items.iter().find(|i| i["name"] == "備份").unwrap();
    assert_eq!(backup_item["ok"], false);
    assert!(
        backup_item["detail"].as_str().unwrap().contains("隨身碟"),
        "不健康時要說得出該怎麼辦：{}",
        backup_item["detail"]
    );

    c.db.close().await;
    let _ = std::fs::remove_dir_all(&layout.root);
}
