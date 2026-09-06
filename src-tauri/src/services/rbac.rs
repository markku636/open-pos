//! 權限檢查。
//!
//! # 快取什麼、不快取什麼
//!
//! 這裡有一個刻意的不對稱，值得寫清楚：
//!
//! * **權限集合會快取**（5 分鐘）。它要 join 三張表（user_roles → role_permissions
//!   → permissions），而它幾乎不變 —— 店家設好角色之後幾個月才動一次。
//! * **「這個人還在職嗎」不快取**，每次都查資料庫。
//!
//! 理由是「離職員工的 PIN 必須立刻失效」是一個安全性質，不該取決於一個 TTL。
//! 老闆按下停用之後還要等五分鐘才生效，在他眼裡就是系統壞了。
//! 代價只是一次有索引的單列查詢，對 POS 的量級完全無感。
//!
//! 這也是「昂貴的部分快取、安全關鍵的部分不快取」這個原則的一個具體落點。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{AppError, AppResult};
use crate::infra::db::sqlite::SqliteDb;

const CACHE_TTL: Duration = Duration::from_secs(300);

/// 操作者。所有會動到錢或動到紀錄的 use case 都要帶著它。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Actor {
    pub user_id: String,
    pub code: String,
    pub name: String,
}

struct Entry {
    at: Instant,
    perms: Arc<HashSet<String>>,
}

static CACHE: Lazy<RwLock<HashMap<String, Entry>>> = Lazy::new(|| RwLock::new(HashMap::new()));

/// 清空整個快取。角色或權限被異動之後呼叫。
pub fn clear_cache() {
    CACHE.write().clear();
}

/// 清掉單一使用者的快取。指派 / 移除角色之後呼叫。
pub fn clear_cache_for(user_id: &str) {
    CACHE.write().remove(user_id);
}

/// 取得使用者的權限集合（可能來自快取）。
pub async fn permissions_of(db: &SqliteDb, user_id: &str) -> AppResult<Arc<HashSet<String>>> {
    if let Some(e) = CACHE.read().get(user_id) {
        if e.at.elapsed() < CACHE_TTL {
            return Ok(e.perms.clone());
        }
    }

    let codes: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT p.code
           FROM user_roles ur
           JOIN role_permissions rp ON rp.role_id = ur.role_id
           JOIN permissions p ON p.id = rp.permission_id
          WHERE ur.user_id = ?1",
    )
    .bind(user_id)
    .fetch_all(db.reader())
    .await?;

    let perms = Arc::new(codes.into_iter().collect::<HashSet<_>>());
    CACHE.write().insert(
        user_id.to_string(),
        Entry {
            at: Instant::now(),
            perms: perms.clone(),
        },
    );
    Ok(perms)
}

/// 這個人現在還能操作嗎。**刻意不快取**（見模組說明）。
async fn assert_still_employed(db: &SqliteDb, actor: &Actor) -> AppResult<()> {
    let ok: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM users WHERE id = ?1 AND is_active = 1 AND deleted_at IS NULL",
    )
    .bind(&actor.user_id)
    .fetch_optional(db.reader())
    .await?;

    if ok.is_none() {
        // 快取也一併清掉，免得同一個人在別處還撐著一份有效的權限集合。
        clear_cache_for(&actor.user_id);
        return Err(AppError::Unauthorized);
    }
    Ok(())
}

/// 檢查權限。通過回 `Ok(())`，否則回 `Forbidden` 並在訊息裡指出缺哪一個碼。
///
/// 訊息裡帶權限碼是刻意的：店長看到「缺少 order.void.after_settle」才知道
/// 該去設定頁的哪一欄打勾。只說「權限不足」等於要他猜。
pub async fn require(db: &SqliteDb, actor: &Actor, code: &str) -> AppResult<()> {
    assert_still_employed(db, actor).await?;
    let perms = permissions_of(db, &actor.user_id).await?;
    if perms.contains(code) {
        return Ok(());
    }
    Err(AppError::Forbidden(format!(
        "{}（{}）沒有「{code}」的權限",
        actor.name, actor.code
    )))
}

/// 檢查「主管授權」。用於高風險操作：操作者自己沒有權限，但現場有主管來刷。
///
/// 回傳授權者，呼叫端必須把它寫進 `approvals` 與 `audit_logs`。
/// 沒有留下授權紀錄的主管授權等於沒有授權 —— 事後查不出是誰放行的。
pub async fn require_with_approval(
    db: &SqliteDb,
    actor: &Actor,
    approver: Option<&Actor>,
    code: &str,
) -> AppResult<Actor> {
    if require(db, actor, code).await.is_ok() {
        return Ok(actor.clone());
    }
    let approver =
        approver.ok_or_else(|| AppError::Forbidden(format!("這個操作需要主管授權（{code}）")))?;
    require(db, approver, code).await?;
    Ok(approver.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_is_cleared_per_user() {
        let perms: Arc<HashSet<String>> = Arc::new(["a".to_string()].into_iter().collect());
        CACHE.write().insert(
            "u1".into(),
            Entry {
                at: Instant::now(),
                perms: perms.clone(),
            },
        );
        CACHE.write().insert(
            "u2".into(),
            Entry {
                at: Instant::now(),
                perms,
            },
        );
        clear_cache_for("u1");
        assert!(!CACHE.read().contains_key("u1"));
        assert!(CACHE.read().contains_key("u2"));
        clear_cache();
        assert!(CACHE.read().is_empty());
    }
}
