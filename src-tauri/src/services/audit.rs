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

use serde::Serialize;

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
