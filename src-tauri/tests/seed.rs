//! 種子資料的整合測試。
//!
//! 這裡最重要的兩支測試不是「有沒有塞進去」，而是：
//! ① 重複跑不會長出重複資料（每次啟動都會跑）
//! ② 收銀員拿不到高風險權限（預設值就是這個專案的安全立場）

use std::sync::atomic::{AtomicU32, Ordering};

use open_pos::core::clock::Stamp;
use open_pos::infra::db::sqlite::SqliteDb;

static SEQ: AtomicU32 = AtomicU32::new(0);

async fn fresh(tag: &str) -> (std::path::PathBuf, SqliteDb) {
    let dir = std::env::temp_dir().join(format!(
        "openpos_seed_{}_{}_{}",
        tag,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db = SqliteDb::open(&dir.join("pos.db"), Some(2)).await.unwrap();
    (dir, db)
}

async fn run_seed(db: &SqliteDb) -> bool {
    let now = Stamp::now();
    let mut uow = db.begin_write().await.unwrap();
    let created = open_pos::services::seed::apply(&mut uow, &now)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    created
}

async fn count(db: &SqliteDb, table: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(db.reader())
        .await
        .unwrap()
}

async fn role_perms(db: &SqliteDb, role: &str) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT p.code FROM role_permissions rp
           JOIN roles r ON r.id = rp.role_id
           JOIN permissions p ON p.id = rp.permission_id
          WHERE r.name = ?1 ORDER BY p.code",
    )
    .bind(role)
    .fetch_all(db.reader())
    .await
    .unwrap()
}

#[tokio::test]
async fn first_run_creates_store_and_reference_data() {
    let (dir, db) = fresh("first").await;
    let created = run_seed(&db).await;
    assert!(created, "第一次跑應該建立店家");

    assert_eq!(count(&db, "stores").await, 1);
    assert_eq!(count(&db, "terminals").await, 1);
    assert!(count(&db, "permissions").await >= 20);
    assert_eq!(count(&db, "roles").await, 5);
    assert_eq!(count(&db, "tax_rates").await, 3);
    assert_eq!(count(&db, "payment_methods").await, 5);
    assert!(count(&db, "reason_codes").await >= 10);
    assert_eq!(count(&db, "business_hours").await, 7);

    db.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn seeding_is_idempotent() {
    // seed 在**每次啟動**都會跑。跑三次長出三倍資料是最容易犯又最難發現的錯，
    // 因為它只在使用者用了一陣子之後才顯現。
    let (dir, db) = fresh("idem").await;
    assert!(run_seed(&db).await);

    let before: Vec<i64> = futures_snapshot(&db).await;
    assert!(!run_seed(&db).await, "第二次不該再建立店家");
    assert!(!run_seed(&db).await);
    let after: Vec<i64> = futures_snapshot(&db).await;

    assert_eq!(before, after, "重複執行 seed 不該改變任何筆數");
    db.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

async fn futures_snapshot(db: &SqliteDb) -> Vec<i64> {
    let mut v = Vec::new();
    for t in [
        "stores",
        "terminals",
        "permissions",
        "roles",
        "role_permissions",
        "tax_rates",
        "payment_methods",
        "reason_codes",
        "business_hours",
    ] {
        v.push(count(db, t).await);
    }
    v
}

/// ★ 預設值就是這個專案的安全立場 —— 90% 的店家不會去調整它。
#[tokio::test]
async fn cashier_cannot_void_after_settlement_or_refund() {
    let (dir, db) = fresh("perms").await;
    run_seed(&db).await;

    let cashier = role_perms(&db, "cashier").await;
    assert!(
        !cashier.contains(&"order.void.after_settle".to_string()),
        "結帳後作廢等於把現金放進口袋，收銀員不該有這個權限"
    );
    assert!(!cashier.contains(&"payment.refund".to_string()));
    assert!(!cashier.contains(&"discount.order".to_string()));
    assert!(cashier.contains(&"order.create".to_string()));
    assert!(cashier.contains(&"shift.close".to_string()));

    // 領班多一些，但仍然拿不到結帳後作廢與退款。
    let lead = role_perms(&db, "shift_lead").await;
    assert!(lead.contains(&"order.void".to_string()));
    assert!(!lead.contains(&"order.void.after_settle".to_string()));
    assert!(!lead.contains(&"payment.refund".to_string()));

    // 老闆與店長是全部。
    let all = count(&db, "permissions").await as usize;
    assert_eq!(role_perms(&db, "owner").await.len(), all);
    assert_eq!(role_perms(&db, "manager").await.len(), all);

    db.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// 店家自己停用的東西，不該在下次開機時復活。
#[tokio::test]
async fn store_data_is_not_resurrected_after_the_owner_removes_it() {
    let (dir, db) = fresh("noresurrect").await;
    run_seed(&db).await;

    let mut uow = db.begin_write().await.unwrap();
    sqlx::query("DELETE FROM payment_methods WHERE code = 'easycard'")
        .execute(uow.conn())
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(count(&db, "payment_methods").await, 4);

    run_seed(&db).await;
    assert_eq!(
        count(&db, "payment_methods").await,
        4,
        "老闆刪掉的付款方式不該在下次啟動時又冒出來"
    );

    db.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// 但參照資料要跟著版本升級 —— 新版加的權限碼必須自動出現。
#[tokio::test]
async fn reference_data_is_refreshed_on_every_boot() {
    let (dir, db) = fresh("refdata").await;
    run_seed(&db).await;
    let before = count(&db, "permissions").await;

    // 模擬「上一版沒有這個權限碼」：手動刪掉一個，再跑一次 seed。
    let mut uow = db.begin_write().await.unwrap();
    sqlx::query("DELETE FROM permissions WHERE code = 'report.audit'")
        .execute(uow.conn())
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(count(&db, "permissions").await, before - 1);

    run_seed(&db).await;
    assert_eq!(
        count(&db, "permissions").await,
        before,
        "升級後新增的權限碼要自動出現，不需要 migration"
    );

    db.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}
