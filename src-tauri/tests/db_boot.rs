//! 資料庫啟動與交易邊界的整合測試。
//!
//! **一律用暫存檔，絕不用 `sqlite::memory:`** —— in-memory 的每一條連線都是
//! 各自獨立的資料庫，雙池設計下 reader 會看到一個空的 DB，測試會綠得毫無意義。
//!
//! 這些測試沒有任何外部相依（不需要 Docker、不需要 DATABASE_URL），
//! `cargo test` 直接就能跑 —— 這是開源專案第一印象的一部分。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::infra::db::sqlite::SqliteDb;
use sqlx::Row;

static SEQ: AtomicU32 = AtomicU32::new(0);

/// 每支測試一個獨立的暫存目錄，drop 時清掉。
struct TempDb {
    dir: PathBuf,
    db: Arc<SqliteDb>,
}

impl TempDb {
    async fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "openpos_it_{}_{}_{}",
            tag,
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = SqliteDb::open(&dir.join("pos.db"), Some(4))
            .await
            .expect("開啟資料庫應成功");
        Self {
            dir,
            db: Arc::new(db),
        }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn migrations_run_and_create_expected_tables() {
    let t = TempDb::new("migrate").await;

    let rows = sqlx::query(
        "SELECT name FROM sqlite_master
          WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx_%'
          ORDER BY name",
    )
    .fetch_all(t.db.reader())
    .await
    .unwrap();

    let names: Vec<String> = rows.iter().map(|r| r.get::<String, _>("name")).collect();
    for expected in ["app_settings", "idempotency_keys", "stores", "terminals"] {
        assert!(
            names.contains(&expected.to_string()),
            "缺少表 {expected}；實際有 {names:?}"
        );
    }
}

#[tokio::test]
async fn wal_mode_and_foreign_keys_are_actually_on() {
    // 開機自檢已經驗過一次，這裡再從 reader 側確認一次 ——
    // journal_mode 是資料庫層級屬性，兩個池必須看到同一個值。
    let t = TempDb::new("pragma").await;

    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(t.db.reader())
        .await
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");

    let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(t.db.reader())
        .await
        .unwrap();
    assert_eq!(fk, 1, "外鍵必須是開的，否則參照完整性只能靠應用層自律");
}

#[tokio::test]
async fn committed_work_is_visible_to_readers() {
    let t = TempDb::new("commit").await;

    let mut uow = t.db.begin_write().await.unwrap();
    sqlx::query("INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)")
        .bind("greeting")
        .bind(r#""hello""#)
        .bind("2026-09-06T00:00:00.000Z")
        .execute(uow.conn())
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let v: String = sqlx::query_scalar("SELECT value_json FROM app_settings WHERE key = ?1")
        .bind("greeting")
        .fetch_one(t.db.reader())
        .await
        .unwrap();
    assert_eq!(v, r#""hello""#);
}

#[tokio::test]
async fn rolled_back_work_leaves_no_trace() {
    let t = TempDb::new("rollback").await;

    let mut uow = t.db.begin_write().await.unwrap();
    sqlx::query("INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)")
        .bind("ghost")
        .bind("1")
        .bind("2026-09-06T00:00:00.000Z")
        .execute(uow.conn())
        .await
        .unwrap();
    uow.rollback().await.unwrap();

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_settings WHERE key = ?1")
        .bind("ghost")
        .fetch_one(t.db.reader())
        .await
        .unwrap();
    assert_eq!(n, 0);
}

/// ★ 這支測試守的是雙池設計最危險的失敗模式。
///
/// 寫入池只有**一條**連線。如果一個 UnitOfWork 被丟棄時沒有 commit / rollback，
/// 那條連線會帶著一個開啟中的交易回到池子 —— 從此全店的每一筆寫入都在那個殭屍交易裡，
/// 而且不會有任何錯誤訊息。
///
/// 解法是寫入池的 `after_release` hook 無條件下 ROLLBACK。這支測試證明它有效：
/// 丟棄一個未結束的 UoW 之後，資料庫仍然乾淨、而且下一筆寫入正常。
#[tokio::test]
async fn dropped_unit_of_work_does_not_poison_the_single_writer_connection() {
    let t = TempDb::new("drop_uow").await;

    {
        let mut uow = t.db.begin_write().await.unwrap();
        sqlx::query("INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)")
            .bind("leaked")
            .bind("1")
            .bind("2026-09-06T00:00:00.000Z")
            .execute(uow.conn())
            .await
            .unwrap();
        // 刻意不 commit 也不 rollback，直接讓它離開作用域。
    }

    // ① 未提交的資料不該存在。
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_settings WHERE key = ?1")
        .bind("leaked")
        .fetch_one(t.db.reader())
        .await
        .unwrap();
    assert_eq!(n, 0, "未提交的交易必須被回捲");

    // ② 更重要的：唯一那條寫入連線必須仍然可用。
    let mut uow = t.db.begin_write().await.unwrap();
    sqlx::query("INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)")
        .bind("after")
        .bind("2")
        .bind("2026-09-06T00:00:01.000Z")
        .execute(uow.conn())
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let v: String = sqlx::query_scalar("SELECT value_json FROM app_settings WHERE key = ?1")
        .bind("after")
        .fetch_one(t.db.reader())
        .await
        .unwrap();
    assert_eq!(v, "2");
}

/// 寫入必須被序列化。這是 writer pool-of-1 的存在理由，也是它的驗收條件。
#[tokio::test]
async fn concurrent_writes_are_serialized_without_busy_errors() {
    let t = TempDb::new("concurrent").await;
    let db = t.db.clone();

    let mut handles = Vec::new();
    for i in 0..20 {
        let db = db.clone();
        handles.push(tokio::spawn(async move {
            let mut uow = db.begin_write().await.expect("取得寫入交易");
            sqlx::query(
                "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)",
            )
            .bind(format!("k{i}"))
            .bind(i.to_string())
            .bind("2026-09-06T00:00:00.000Z")
            .execute(uow.conn())
            .await
            .expect("寫入不應撞 SQLITE_BUSY");
            uow.commit().await.expect("提交");
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_settings")
        .fetch_one(db.reader())
        .await
        .unwrap();
    assert_eq!(n, 20, "20 筆並行寫入應全部落地，一筆不漏");
}

#[tokio::test]
async fn schema_fingerprint_is_stable_and_non_empty() {
    let t = TempDb::new("fingerprint").await;
    let a = t.db.schema_fingerprint().await.unwrap();
    let b = t.db.schema_fingerprint().await.unwrap();
    assert!(!a.is_empty());
    assert_eq!(a, b, "指紋必須可重現，否則無法當 golden test");
    assert!(a.contains("stores"), "指紋應涵蓋表定義");
    assert!(
        !a.contains("_sqlx_migrations"),
        "指紋不該包含 migration 記錄表 —— 它的內容隨時間變動"
    );
}
