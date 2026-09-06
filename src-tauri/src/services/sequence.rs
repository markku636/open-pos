//! 人看的單號配號。
//!
//! ULID 是內部主鍵，但沒有人唸得出「01M1TS48SKG4MDQTC62D87VE1M」。
//! 店員在電話裡要能說「四十二號單」，客人取餐時要能對號碼，
//! 所以另外配一組短的、按營業日重新編號的序號。
//!
//! # 為什麼配號一定要在交易內
//!
//! `UPDATE ... RETURNING` 是原子的，但「取號」與「用這個號建立訂單」必須是
//! 同一筆交易 —— 否則會出現「號碼發出去了但訂單沒建立」的空洞，
//! 而營業日結算時那個洞會被當成遺失的單去查。

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::error::{AppError, AppResult};
use crate::infra::db::sqlite::SqliteUow;

/// 序號用途。分開計數，讓每一種單各自從 1 開始。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Order,
    Bill,
    Shift,
}

impl Scope {
    fn key(self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Bill => "bill",
            Self::Shift => "shift",
        }
    }
    /// 單號的前綴字母。店員報號時「A 開頭的是訂單」很好認。
    fn prefix(self) -> &'static str {
        match self {
            Self::Order => "A",
            Self::Bill => "B",
            Self::Shift => "S",
        }
    }
}

/// 取下一個序號，回傳格式化後的單號（`A-20260906-0042`）。
///
/// `scope_key` 通常是營業日 —— 每天從 1 重新開始，號碼才不會愈來愈長。
pub async fn next_no(
    uow: &mut SqliteUow,
    store_id: &str,
    scope: Scope,
    scope_key: &str,
    now: &Stamp,
) -> AppResult<String> {
    // 先確保這一組計數器存在。ON CONFLICT DO NOTHING 讓它可以安全地重複執行。
    sqlx::query(
        "INSERT INTO sequences (id, store_id, scope, scope_key, prefix, next_value, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
         ON CONFLICT(store_id, scope, scope_key) DO NOTHING",
    )
    .bind(Id::new().as_str())
    .bind(store_id)
    .bind(scope.key())
    .bind(scope_key)
    .bind(scope.prefix())
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    // UPDATE ... RETURNING 是原子的：兩個並行的交易不可能拿到同一個號碼。
    // （而且寫入池只有一條連線，所以連競爭都不會發生 —— 這是第二道保險。）
    let value: Option<i64> = sqlx::query_scalar(
        "UPDATE sequences SET next_value = next_value + 1, updated_at = ?4
          WHERE store_id = ?1 AND scope = ?2 AND scope_key = ?3
      RETURNING next_value - 1",
    )
    .bind(store_id)
    .bind(scope.key())
    .bind(scope_key)
    .bind(now.iso())
    .fetch_optional(uow.conn())
    .await?;

    let n = value.ok_or_else(|| AppError::Internal("配號失敗：找不到計數器".into()))?;
    // 日期去掉連字號，讓單號短一點也好唸。
    let date = scope_key.replace('-', "");
    Ok(format!("{}-{date}-{n:04}", scope.prefix()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_distinct_so_staff_can_tell_them_apart() {
        assert_eq!(Scope::Order.prefix(), "A");
        assert_eq!(Scope::Bill.prefix(), "B");
        assert_eq!(Scope::Shift.prefix(), "S");
    }
}
