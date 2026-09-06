//! 備份與還原的整合測試。
//!
//! 這組測試對應 README 上對店家的承諾：「不會掉資料」。
//! 它們跑在真實暫存檔上，涵蓋「備份 → 繼續營業 → 還原 → 資料回到備份當下」
//! 這條真正會被用到的路徑，以及三種必須被擋下的還原。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use open_pos::infra::backup::{self, BackupBucket};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;

static SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_layout(tag: &str) -> DataLayout {
    let root = std::env::temp_dir().join(format!(
        "openpos_bk_{}_{}_{}",
        tag,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let layout = DataLayout::new(root);
    layout.ensure().unwrap();
    layout
}

async fn put(db: &SqliteDb, key: &str, value: &str) {
    let mut uow = db.begin_write().await.unwrap();
    sqlx::query(
        "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
    )
    .bind(key)
    .bind(value)
    .bind("2026-09-06T00:00:00.000Z")
    .execute(uow.conn())
    .await
    .unwrap();
    uow.commit().await.unwrap();
}

async fn keys(db: &SqliteDb) -> Vec<String> {
    sqlx::query_scalar::<_, String>("SELECT key FROM app_settings ORDER BY key")
        .fetch_all(db.reader())
        .await
        .unwrap()
}

#[tokio::test]
async fn backup_produces_a_valid_and_complete_snapshot() {
    let layout = temp_layout("valid");
    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    put(&db, "a", "1").await;
    put(&db, "b", "2").await;

    let dst = backup::backup_path(&layout, BackupBucket::Daily, "2026-09-06");
    let res = backup::backup_to(&db, &dst).await.unwrap();
    assert!(res.bytes > 0);
    assert!(dst.exists());
    // 產出前已經驗過一次，這裡再獨立驗一次確保對外的 API 也是對的。
    backup::validate_backup_file(&dst).await.unwrap();

    // 備份必須含有備份當下已提交的全部資料。
    // 用 fs::copy 而不是 VACUUM INTO 的話，最近幾筆還在 -wal 裡的交易會遺失，
    // 而這正是這支測試要擋住的失敗模式。
    let restored = SqliteDb::open(&dst, Some(2)).await.unwrap();
    assert_eq!(keys(&restored).await, vec!["a", "b"]);
    restored.close().await;
    db.close().await;
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[tokio::test]
async fn backup_leaves_no_partial_file_behind() {
    let layout = temp_layout("partial");
    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    let dst = backup::backup_path(&layout, BackupBucket::Hourly, "2026-09-06_15");
    backup::backup_to(&db, &dst).await.unwrap();

    let leftovers: Vec<PathBuf> = std::fs::read_dir(dst.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "partial").unwrap_or(false))
        .collect();
    assert!(leftovers.is_empty(), "不該留下 .partial：{leftovers:?}");

    db.close().await;
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[tokio::test]
async fn rotate_keeps_the_newest_n() {
    let layout = temp_layout("rotate");
    let dir = layout.backups_dir().join("hourly");
    std::fs::create_dir_all(&dir).unwrap();
    // 檔名帶時間戳，字典序即時序 —— rotate 刻意不看 mtime，
    // 因為備份一旦被複製到 USB，mtime 就不再可靠。
    for h in 0..6 {
        std::fs::write(dir.join(format!("2026-09-06_{h:02}.db")), b"x").unwrap();
    }
    let removed = backup::rotate(&dir, 3).unwrap();
    assert_eq!(removed, 3);

    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        vec!["2026-09-06_03.db", "2026-09-06_04.db", "2026-09-06_05.db"]
    );
    let _ = std::fs::remove_dir_all(&layout.root);
}

/// ★ 端到端：備份 → 繼續營業 → 還原 → 資料回到備份當下，且還原前的資料沒被刪掉。
#[tokio::test]
async fn restore_rolls_back_to_the_snapshot_and_keeps_the_previous_data() {
    let layout = temp_layout("restore");
    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    put(&db, "before-backup", "1").await;

    let snapshot = backup::backup_path(&layout, BackupBucket::Shift, "2026-09-06_1430");
    backup::backup_to(&db, &snapshot).await.unwrap();

    // 備份之後又做了生意。
    put(&db, "after-backup", "2").await;
    assert_eq!(keys(&db).await, vec!["after-backup", "before-backup"]);

    // 還原前必須先關閉連線池 —— restore_from 只碰檔案，不碰連線。
    db.close().await;
    drop(db);

    let aside = backup::restore_from(&layout, &snapshot, SqliteDb::max_migration_version())
        .await
        .unwrap();
    assert!(aside.exists(), "還原前的資料必須保留下來，不是刪除");

    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    assert_eq!(
        keys(&db).await,
        vec!["before-backup"],
        "還原後應回到備份當下的狀態"
    );
    db.close().await;
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[tokio::test]
async fn restore_refuses_a_file_that_is_not_a_database() {
    let layout = temp_layout("notdb");
    let bogus = layout.backups_dir().join("daily").join("photo.db");
    std::fs::create_dir_all(bogus.parent().unwrap()).unwrap();
    std::fs::write(
        &bogus,
        b"\x89PNG\r\n\x1a\n this is a picture, not a database",
    )
    .unwrap();

    let err = backup::restore_from(&layout, &bogus, SqliteDb::max_migration_version())
        .await
        .unwrap_err();
    assert!(
        err.message().contains("不是 SQLite"),
        "訊息要說得出問題在哪：{}",
        err.message()
    );
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[tokio::test]
async fn restore_refuses_a_backup_from_a_newer_version() {
    // 把新版的 DB 還原到舊版的程式，SQLite 不會攔你 —— 它會在某個查詢時
    // 才噴 no such column，而那時店家已經開了半天的單。所以必須事前擋。
    let layout = temp_layout("newer");
    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    let snapshot = backup::backup_path(&layout, BackupBucket::Daily, "2026-09-06");
    backup::backup_to(&db, &snapshot).await.unwrap();
    db.close().await;
    drop(db);

    // 假裝這個程式只認識到 migration 1（備份裡有更高的版本）。
    let err = backup::restore_from(&layout, &snapshot, 1)
        .await
        .unwrap_err();
    assert!(
        err.message().contains("較新的版本"),
        "訊息要告訴店家該怎麼辦：{}",
        err.message()
    );
    let _ = std::fs::remove_dir_all(&layout.root);
}
