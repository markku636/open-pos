//! 稽核紀錄。
//!
//! # 與 kanban 的一個刻意分歧
//!
//! kanban 的 `audit-log-service.ts` 用 try/catch 吞掉稽核寫入失敗 ——
//! 對看板系統來說「功能可用」比「紀錄完整」重要，那是合理的取捨。
//!
//! **POS 反過來。** 這裡的 `write_in` 接收 `&mut SqliteUow`，錯誤直接往上拋，
//! 呼叫端不得吞掉 —— 稽核寫不進去就讓整筆交易 rollback。
//! 一筆沒有紀錄的免單，比一次結帳失敗糟糕得多：前者要到月底對帳才發現，
//! 而且發現時已經不知道是誰做的。
//!
//! # amount_delta 是這張表最有價值的欄位
//!
//! 「這個月誰總共免掉了多少錢」一個 SUM 就查得出來。防弊的價值全在統計上 ——
//! 單看一筆折扣永遠是合理的，看一整個月的分布才看得出問題。

use serde::{Deserialize, Serialize};

use sqlx::Row;

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::error::AppResult;
use crate::infra::db::sqlite::SqliteUow;
use crate::services::rbac::Actor;

/// 稽核動作。用 enum 而不是自由字串，因為報表要能分組統計 ——
/// 打錯一個字的 "discout" 會安靜地從統計裡消失。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Create,
    Update,
    Delete,
    Void,
    VoidAfterSettle,
    Discount,
    Comp,
    PriceOverride,
    Refund,
    Reprint,
    DrawerOpen,
    ShiftOpen,
    ShiftClose,
    SettingsChange,
    Restore,
}

impl AuditAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Void => "void",
            Self::VoidAfterSettle => "void_after_settle",
            Self::Discount => "discount",
            Self::Comp => "comp",
            Self::PriceOverride => "price_override",
            Self::Refund => "refund",
            Self::Reprint => "reprint",
            Self::DrawerOpen => "drawer_open",
            Self::ShiftOpen => "shift_open",
            Self::ShiftClose => "shift_close",
            Self::SettingsChange => "settings_change",
            Self::Restore => "restore",
        }
    }

    /// 這個動作是否會動到錢。會的話 `amount_delta` 必須填。
    pub fn touches_money(self) -> bool {
        matches!(
            self,
            Self::Void
                | Self::VoidAfterSettle
                | Self::Discount
                | Self::Comp
                | Self::PriceOverride
                | Self::Refund
        )
    }
}

/// 一筆稽核。
pub struct AuditEntry<'a> {
    pub entity_type: &'a str,
    pub entity_id: &'a str,
    pub action: AuditAction,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    /// 金額影響（整數元，可負）。動到錢的動作必須填。
    pub amount_delta: Option<i64>,
    pub reason_id: Option<&'a str>,
    pub approved_by: Option<&'a str>,
    pub terminal_id: Option<&'a str>,
    pub shift_id: Option<&'a str>,
    pub business_date: Option<&'a str>,
}

impl<'a> AuditEntry<'a> {
    pub fn new(entity_type: &'a str, entity_id: &'a str, action: AuditAction) -> Self {
        Self {
            entity_type,
            entity_id,
            action,
            old_value: None,
            new_value: None,
            amount_delta: None,
            reason_id: None,
            approved_by: None,
            terminal_id: None,
            shift_id: None,
            business_date: None,
        }
    }
    pub fn amount(mut self, delta: i64) -> Self {
        self.amount_delta = Some(delta);
        self
    }
    pub fn reason(mut self, reason_id: &'a str) -> Self {
        self.reason_id = Some(reason_id);
        self
    }
    pub fn approved_by(mut self, approver_id: &'a str) -> Self {
        self.approved_by = Some(approver_id);
        self
    }
    pub fn on(mut self, business_date: &'a str) -> Self {
        self.business_date = Some(business_date);
        self
    }
    pub fn at_terminal(mut self, terminal_id: &'a str) -> Self {
        self.terminal_id = Some(terminal_id);
        self
    }
    pub fn during_shift(mut self, shift_id: &'a str) -> Self {
        self.shift_id = Some(shift_id);
        self
    }
    pub fn from(mut self, old: impl Serialize) -> Self {
        self.old_value = serde_json::to_string(&old).ok();
        self
    }
    pub fn to(mut self, new: impl Serialize) -> Self {
        self.new_value = serde_json::to_string(&new).ok();
        self
    }
}

/// 在**呼叫端的交易裡**寫入一筆稽核。
///
/// 簽章刻意吃 `&mut SqliteUow` 而不是 `&SqliteDb`：稽核與它記錄的那件事
/// 必須是同一筆交易，否則會出現「單作廢了但沒有紀錄」或反之的狀態。
///
/// 錯誤直接往上拋。**呼叫端不得用 `let _ =` 吞掉。**
pub async fn write_in(
    uow: &mut SqliteUow,
    entry: AuditEntry<'_>,
    actor: &Actor,
    now: &Stamp,
) -> AppResult<()> {
    debug_assert!(
        !entry.action.touches_money() || entry.amount_delta.is_some(),
        "{} 會動到錢，amount_delta 必須填 —— 否則防弊報表會漏掉這一筆",
        entry.action.as_str()
    );

    sqlx::query(
        "INSERT INTO audit_logs
           (id, actor_id, actor_code, actor_name, entity_type, entity_id, action,
            old_value, new_value, amount_delta, reason_id, approved_by,
            terminal_id, shift_id, business_date, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )
    .bind(Id::new().as_str())
    .bind(&actor.user_id)
    .bind(&actor.code)
    .bind(&actor.name)
    .bind(entry.entity_type)
    .bind(entry.entity_id)
    .bind(entry.action.as_str())
    .bind(entry.old_value)
    .bind(entry.new_value)
    .bind(entry.amount_delta)
    .bind(entry.reason_id)
    .bind(entry.approved_by)
    .bind(entry.terminal_id)
    .bind(entry.shift_id)
    .bind(entry.business_date)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    Ok(())
}

/// 記錄一次主管授權。與 `write_in` 同交易，理由相同。
#[allow(clippy::too_many_arguments)]
pub async fn record_approval(
    uow: &mut SqliteUow,
    action_code: &str,
    ref_type: &str,
    ref_id: &str,
    amount: Option<i64>,
    requester: &Actor,
    approver: &Actor,
    business_date: &str,
    now: &Stamp,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO approvals
           (id, action_code, ref_type, ref_id, amount, requested_by, approved_by,
            auth_method, business_date, approved_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pin', ?8, ?9, ?9)",
    )
    .bind(Id::new().as_str())
    .bind(action_code)
    .bind(ref_type)
    .bind(ref_id)
    .bind(amount)
    .bind(&requester.user_id)
    .bind(&approver.user_id)
    .bind(business_date)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_touching_actions_are_marked() {
        assert!(AuditAction::VoidAfterSettle.touches_money());
        assert!(AuditAction::Refund.touches_money());
        assert!(AuditAction::Comp.touches_money());
        assert!(!AuditAction::Reprint.touches_money());
        assert!(!AuditAction::ShiftOpen.touches_money());
    }

    #[test]
    fn action_strings_are_stable() {
        // 這些字串會進資料庫並被報表 GROUP BY，改動等同於破壞歷史資料。
        assert_eq!(AuditAction::VoidAfterSettle.as_str(), "void_after_settle");
        assert_eq!(AuditAction::PriceOverride.as_str(), "price_override");
    }
}

// ---------------------------------------------------------------- 查詢

/// 查稽核紀錄的條件。
///
/// 全部可選，因為使用者查的問題形狀不固定：
/// 「這個月的免單」「小美今天做了什麼」「上禮拜三為什麼少 800」。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    /// 營業日起（含）。省略＝今天。
    pub from: Option<String>,
    /// 營業日迄（含）。省略＝跟 from 一樣。
    pub to: Option<String>,
    /// 只看某一種動作。
    pub action: Option<String>,
    /// 只看某個人做的。
    pub actor_id: Option<String>,
    /// 只看動到錢的。**這是最常按的一個開關** ——
    /// 「誰改了什麼設定」跟「誰免掉了多少錢」不是同一個問題。
    #[serde(default)]
    pub money_only: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
    pub id: String,
    pub at: String,
    pub business_date: Option<String>,
    pub actor_name: Option<String>,
    pub action: String,
    pub action_label: String,
    pub entity_type: String,
    pub entity_id: String,
    /// 人看得懂的對象（單號、帳單號、品名）。
    pub label: Option<String>,
    pub amount_delta: Option<i64>,
    pub reason_name: Option<String>,
    /// 誰簽的核。有值代表這是一個需要授權的動作。
    pub approved_by_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditGroup {
    pub key: String,
    pub label: String,
    pub count: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReport {
    pub from: String,
    pub to: String,
    pub rows: Vec<AuditRow>,
    /// 這段期間動到的錢總共多少（負數＝從店裡出去的）。
    pub total_amount: i64,
    /// 依動作分組。**看一整段時間的分布才看得出問題** ——
    /// 單看一筆折扣永遠是合理的。
    pub by_action: Vec<AuditGroup>,
    /// 依操作者分組。
    pub by_actor: Vec<AuditGroup>,
    /// 超過上限時為 true，畫面上要說「還有更多」而不是假裝就這些。
    pub truncated: bool,
}

/// 一次最多回幾筆。太多筆的畫面沒有人會捲完，而分組統計已經回答了大問題。
const MAX_ROWS: i64 = 500;

pub async fn query(ctx: &crate::ctx::Ctx, q: AuditQuery) -> AppResult<AuditReport> {
    crate::services::rbac::require(&ctx.db, &ctx.actor, "report.audit").await?;
    let now = Stamp::now();
    let from = match q.from.clone() {
        Some(d) => d,
        None => crate::services::shift::today(ctx, &now).await?,
    };
    let to = q.to.clone().unwrap_or_else(|| from.clone());

    let mut sql = String::from(
        "SELECT a.id, a.created_at, a.business_date, a.actor_name, a.action, a.entity_type,
                a.entity_id, a.new_value, a.amount_delta, r.name AS reason_name,
                u.name AS approver_name
           FROM audit_logs a
           LEFT JOIN reason_codes r ON r.id = a.reason_id
           LEFT JOIN users u ON u.id = a.approved_by
          WHERE a.business_date >= ?1 AND a.business_date <= ?2",
    );
    if q.action.is_some() {
        sql.push_str(" AND a.action = ?3");
    }
    if q.actor_id.is_some() {
        sql.push_str(" AND a.actor_id = ?4");
    }
    if q.money_only {
        sql.push_str(" AND a.amount_delta IS NOT NULL AND a.amount_delta <> 0");
    }
    sql.push_str(" ORDER BY a.created_at DESC LIMIT ?5");

    let rows = sqlx::query(&sql)
        .bind(&from)
        .bind(&to)
        .bind(&q.action)
        .bind(&q.actor_id)
        .bind(MAX_ROWS + 1)
        .fetch_all(ctx.db.reader())
        .await?;

    let truncated = rows.len() as i64 > MAX_ROWS;
    let mut out = Vec::with_capacity(rows.len().min(MAX_ROWS as usize));
    for r in rows.iter().take(MAX_ROWS as usize) {
        let action: String = r.get("action");
        out.push(AuditRow {
            id: r.get("id"),
            at: r.get("created_at"),
            business_date: r.get("business_date"),
            actor_name: r.get("actor_name"),
            action_label: action_label(&action).to_string(),
            action,
            entity_type: r.get("entity_type"),
            entity_id: r.get("entity_id"),
            // new_value 是 JSON 字串（多半就是一個單號）。剝掉引號讓它像人話。
            label: r
                .get::<Option<String>, _>("new_value")
                .map(|v| v.trim_matches('"').to_string()),
            amount_delta: r.get("amount_delta"),
            reason_name: r.get("reason_name"),
            approved_by_name: r.get("approver_name"),
        });
    }

    // 分組統計走另一支查詢，**不是拿上面那 500 筆算的** ——
    // 一份「只統計到前 500 筆」的防弊報表比沒有更糟，因為它看起來是完整的。
    let by_action = group_by(ctx, "a.action", &from, &to, &q).await?;
    let by_actor = group_by(ctx, "COALESCE(a.actor_name, a.actor_code)", &from, &to, &q).await?;
    let total_amount = by_action.iter().map(|g| g.amount).sum();

    Ok(AuditReport {
        from,
        to,
        rows: out,
        total_amount,
        by_action,
        by_actor,
        truncated,
    })
}

async fn group_by(
    ctx: &crate::ctx::Ctx,
    expr: &str,
    from: &str,
    to: &str,
    q: &AuditQuery,
) -> AppResult<Vec<AuditGroup>> {
    let mut sql = format!(
        "SELECT {expr} AS k, COUNT(*) AS n, COALESCE(SUM(a.amount_delta), 0) AS amount
           FROM audit_logs a
          WHERE a.business_date >= ?1 AND a.business_date <= ?2"
    );
    if q.action.is_some() {
        sql.push_str(" AND a.action = ?3");
    }
    if q.actor_id.is_some() {
        sql.push_str(" AND a.actor_id = ?4");
    }
    if q.money_only {
        sql.push_str(" AND a.amount_delta IS NOT NULL AND a.amount_delta <> 0");
    }
    sql.push_str(&format!(" GROUP BY {expr} ORDER BY amount ASC, n DESC"));

    let rows = sqlx::query(&sql)
        .bind(from)
        .bind(to)
        .bind(&q.action)
        .bind(&q.actor_id)
        .fetch_all(ctx.db.reader())
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let key: Option<String> = r.get("k");
            let key = key.unwrap_or_else(|| "—".into());
            AuditGroup {
                label: action_label(&key).to_string(),
                key,
                count: r.get("n"),
                amount: r.get("amount"),
            }
        })
        .collect())
}

/// 動作代碼的中文。認不得的就原樣回去 —— 顯示 `foo` 比顯示「其他」有用。
fn action_label(code: &str) -> &str {
    match code {
        "create" => "建立",
        "update" => "修改",
        "delete" => "刪除",
        "void" => "作廢",
        "void_after_settle" => "結帳後作廢",
        "discount" => "折扣",
        "comp" => "招待",
        "price_override" => "改價",
        "refund" => "退款",
        "reprint" => "補印",
        "drawer_open" => "開錢箱",
        "shift_open" => "開班",
        "shift_close" => "關班",
        "settings_change" => "改設定",
        "restore" => "還原備份",
        other => other,
    }
}
