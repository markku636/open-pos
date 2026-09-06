//! 原因代碼。
//!
//! 作廢、折扣、招待、退款、現金收支各有一組。**它們存在的理由是事後查得出來**：
//! 「今天為什麼少了 800 元」在有原因代碼時是一句話就能回答的問題，沒有的話
//! 要翻一整天的單，而通常沒有人會去翻。
//!
//! 所以原因是選的、不是打字的 —— 自由文字的欄位在報表上沒有辦法分組統計。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::ctx::Ctx;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonCode {
    pub id: String,
    pub code: String,
    pub name: String,
    /// 選了這個原因就一定要補一句說明。
    pub requires_note: bool,
}

/// 某一類的原因代碼。`kind`：void / discount / comp / refund / cash_in / cash_out。
pub async fn list_reasons(ctx: &Ctx, kind: String) -> AppResult<Vec<ReasonCode>> {
    let rows = sqlx::query(
        "SELECT id, code, name, requires_note FROM reason_codes
          WHERE kind = ?1 AND is_active = 1
          ORDER BY sort_order, name",
    )
    .bind(&kind)
    .fetch_all(ctx.db.reader())
    .await?;
    Ok(rows
        .iter()
        .map(|r| ReasonCode {
            id: r.get("id"),
            code: r.get("code"),
            name: r.get("name"),
            requires_note: r.get::<i64, _>("requires_note") == 1,
        })
        .collect())
}
