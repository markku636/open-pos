//! 備份與還原的整合測試。
//!
//! 這是最不能只靠人工驗的功能：**它只在最壞的一天才會被用到**，
//! 而那一天沒有人有心情除錯。所以「備份 → 弄壞 → 還原 → 資料回來了」
//! 必須是一條自動跑的路徑。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::backup::BackupBucket;
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::infra::settings::AppSettings;
use open_pos::paths::DataLayout;
use open_pos::services::{backup, demo, menu};

static SEQ: AtomicU32 = AtomicU32::new(0);

struct Env {
    root: std::path::PathBuf,
    layout: DataLayout,
    ctx: Ctx,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn env(tag: &str) -> Env {
    let root = std::env::temp_dir().join(format!(
        "openpos_backup_{}_{}_{}",
        tag,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let layout = DataLayout::new(root.clone());
    layout.ensure().unwrap();
    Env {
        ctx: open_ctx(&layout).await,
        root,
        layout,
    }
}

async fn open_ctx(layout: &DataLayout) -> Ctx {
    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    let now = Stamp::now();
    let mut uow = db.begin_write().await.unwrap();
    open_pos::services::seed::apply(&mut uow, &now)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let actor = open_pos::services::seed::default_actor(&db).await.unwrap();
    Arc::new(AppCtx {
        db,
        layout: layout.clone(),
        started_at: now.at,
        actor,
    })
}

async fn item_count(ctx: &Ctx) -> usize {
    let t = menu::menu_tree(ctx).await.unwrap();
    t.categories.iter().map(|c| c.items.len()).sum::<usize>() + t.uncategorized.len()
}

/// ★ 備份 → 弄壞 → 還原 → 資料回來了。
///
/// 「還原」在 UI 上只是把備份放到暫存位置並留下標記；真正的替換發生在
/// 下一次啟動，因為資料庫檔案在程式跑的時候是開著的。這條測試走的就是
/// 那兩步。
#[tokio::test]
async fn a_backup_can_actually_be_restored() {
    let e = env("roundtrip").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let before = item_count(&e.ctx).await;
    assert!(before > 0);

    let result = backup::run_backup(&e.ctx, BackupBucket::Daily)
        .await
        .unwrap();
    assert!(result.size_bytes > 0);
    assert!(std::path::Path::new(&result.local_path).exists());

    // 之後又賣了東西、又改了菜單 —— 還原時這些都會回到備份當時。
    menu::upsert_item(
        &e.ctx,
        menu::ItemInput {
            id: None,
            category_id: None,
            name: "備份之後才建的品項".into(),
            short_name: None,
            base_price: 99,
            tax_code: None,
            is_open_price: None,
            sold_out_until: None,
            sort_order: None,
            is_active: Some(true),
        },
    )
    .await
    .unwrap();
    assert_eq!(item_count(&e.ctx).await, before + 1);

    let msg = backup::stage_restore(&e.ctx, result.local_path.clone())
        .await
        .unwrap();
    assert!(msg.contains("重新開啟"), "{msg}");
    assert!(backup::pending_restore(&e.ctx).await.unwrap().is_some());

    // 模擬「關掉程式再開起來」。
    e.ctx.db.close().await;
    let aside = backup::apply_pending_restore(&e.layout).await.unwrap();
    let aside = aside.expect("應該要有東西被搬到一旁");
    // 現在的資料必須被保留而不是刪掉 —— 還原是人在慌張的時候做的事。
    assert!(aside.exists(), "原本的資料不見了：{}", aside.display());

    let ctx2 = open_ctx(&e.layout).await;
    assert_eq!(
        item_count(&ctx2).await,
        before,
        "還原之後的品項數不對 —— 備份沒有真的被套用"
    );
    let names: Vec<String> = menu::menu_tree(&ctx2)
        .await
        .unwrap()
        .categories
        .iter()
        .flat_map(|c| c.items.iter().map(|i| i.name.clone()))
        .collect();
    assert!(!names.iter().any(|n| n == "備份之後才建的品項"));

    // 標記要被清掉，不然每次開機都會再還原一次。
    assert!(backup::pending_restore(&ctx2).await.unwrap().is_none());
    ctx2.db.close().await;
}

/// 沒有待處理的還原時，開機不該做任何事。
#[tokio::test]
async fn booting_without_a_pending_restore_changes_nothing() {
    let e = env("noop").await;
    assert!(backup::apply_pending_restore(&e.layout)
        .await
        .unwrap()
        .is_none());
    e.ctx.db.close().await;
}

/// 外接位置也要有一份 —— 那才是備份真正有用的部分。
#[tokio::test]
async fn the_backup_also_lands_on_the_second_location() {
    let e = env("external").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();

    let usb = e.root.join("fake-usb");
    std::fs::create_dir_all(&usb).unwrap();
    let mut s = AppSettings::default();
    s.backup.external_dir = Some(usb.to_string_lossy().into_owned());
    backup::save_settings(&e.ctx, s).await.unwrap();

    let r = backup::run_backup(&e.ctx, BackupBucket::Daily)
        .await
        .unwrap();
    assert!(r.external_error.is_none(), "{:?}", r.external_error);
    let external = r.external_path.expect("外接那一份沒有寫出來");
    assert!(std::path::Path::new(&external).exists());

    // 兩份都要出現在清單裡，而且外接那一份要標記出來。
    let files = backup::list_backups(&e.ctx).await.unwrap();
    assert!(files.iter().any(|f| f.external));
    assert!(files.iter().any(|f| !f.external));

    e.ctx.db.close().await;
}

/// ★ 隨身碟沒插不該讓備份整個失敗。
#[tokio::test]
async fn a_missing_usb_stick_does_not_fail_the_whole_backup() {
    let e = env("nousb").await;

    // 直接把設定寫進去（save_settings 會擋掉不存在的資料夾，那是刻意的），
    // 模擬「儲存時隨身碟在，之後被拔掉」。
    let mut s = AppSettings::default();
    s.backup.external_dir = Some(
        e.root
            .join("unplugged")
            .join("gone")
            .to_string_lossy()
            .into_owned(),
    );
    open_pos::infra::settings::save(&e.layout.settings_file(), &s)
        .await
        .unwrap();

    let r = backup::run_backup(&e.ctx, BackupBucket::Hourly).await;
    // 本機那一份還是要成功。這是整條設計的重點：外接失敗只是警告。
    let r = r.expect("本機備份不該因為隨身碟不在而失敗");
    assert!(std::path::Path::new(&r.local_path).exists());

    e.ctx.db.close().await;
}

/// 儲存設定時就要驗證位置寫不寫得進去。
///
/// 等到半夜自動備份才發現沒有寫入權限，就是「以為有在備份、其實三個月
/// 沒備了」的來源。
#[tokio::test]
async fn saving_an_unreachable_backup_folder_is_refused_immediately() {
    let e = env("badfolder").await;
    let mut s = AppSettings::default();
    s.backup.external_dir = Some(e.root.join("does-not-exist").to_string_lossy().into_owned());
    let err = backup::save_settings(&e.ctx, s).await.unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");
    assert!(err.message().contains("找不到"), "{}", err.message());

    e.ctx.db.close().await;
}

/// 用「比程式新」的備份還原會被擋下來。
#[tokio::test]
async fn a_backup_from_a_newer_version_is_refused() {
    let e = env("newer").await;
    let r = backup::run_backup(&e.ctx, BackupBucket::Daily)
        .await
        .unwrap();

    // 把備份裡的 migration 版本改成一個未來的數字。
    let path = std::path::PathBuf::from(&r.local_path);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "sqlite://{}",
            path.display().to_string().replace('\\', "/")
        ))
        .await
        .unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (99999, 'from the future', datetime('now'), 1, X'00', 0)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let err = backup::stage_restore(&e.ctx, r.local_path)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");
    // 訊息要說出該怎麼辦，不能只說「不支援」。
    assert!(err.message().contains("更新"), "{}", err.message());

    e.ctx.db.close().await;
}
