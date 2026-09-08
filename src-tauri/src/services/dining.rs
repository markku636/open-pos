//! 吃到飽 / 無限暢飲方案。
//!
//! 設計依據與被推翻的兩個設計，見 `docs/dining-modes.md`（查了 25 套市售產品）。
//! 這裡只記與程式碼直接相關的三件事。
//!
//! # 一、方案本身就是一個商品
//!
//! 不是另一種東西。這是所有查過的產品的共識 —— Airレジ 的「放題プラン名商品」、
//! スマレジ 的『プラン』、Eats365 的 representing item、Square 的 セット。
//!
//! 所以 `dining_plans.item_id` 指向一筆既有的 `items`，而人頭費就是
//! **那個商品點 N 份**。這樣免費得到：人頭分級走 `item_variants`
//! （大人 / 小孩 / 長者）、平假日不同價走既有的 `price_rules`、
//! 品項排行與稅別與廚房分區全部自動適用。
//!
//! # 二、方案內的品項是 0 元，但仍然是一行
//!
//! Airレジ 在手持機上把方案內商品印成「（放）」與 ¥0。它們必須留在單上，
//! 因為**廚房要知道要做什麼** —— 只是不收錢。
//!
//! 0 元的行經過定價引擎不會有任何問題：`gross = 0`，攤到的稅也是 0，
//! 而 `Σ taxable_amount == grand_total` 依然成立。
//!
//! # 三、不在方案裡的東西照常收錢
//!
//! 這是「加價品」（和牛 +200、酒水另計）。不需要特別處理 —— 沒被判定為
//! 方案成員的品項就走原本的價格，這是預設行為而不是額外邏輯。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::infra::db::sqlite::SqliteUow;
use crate::services::rbac;

const PERM_MENU: &str = "settings.item";

/// 一個方案，以及它涵蓋哪些東西。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiningPlan {
    pub id: String,
    /// 有價的那個商品。人頭費 = 這個商品 × 人數。
    pub item_id: String,
    pub item_name: String,
    pub name: String,
    /// 用餐時限（分鐘）。0 = 不限時。
    pub limit_minutes: i64,
    /// 提前多久先提醒。
    pub notice_minutes: i64,
    pub print_members_on_bill: bool,
    pub is_active: bool,
    /// 方案涵蓋的品項與分類。
    pub member_items: Vec<String>,
    pub member_categories: Vec<String>,
}

impl DiningPlan {
    /// 這個品項在方案裡嗎（吃它不另外收錢）？
    ///
    /// 品項與分類都算。用分類的理由很實際 ——「所有飲料無限暢飲」不該要
    /// 一個一個勾，而且新增一款飲料時不該還要記得回來加。
    ///
    /// **方案的代表商品本身不是成員**：它就是要收錢的那一行。
    pub fn covers(&self, item_id: &str, category_id: Option<&str>) -> bool {
        if item_id == self.item_id {
            return false;
        }
        if self.member_items.iter().any(|i| i == item_id) {
            return true;
        }
        match category_id {
            Some(c) => self.member_categories.iter().any(|x| x == c),
            None => false,
        }
    }
}

/// 這張單所屬的桌現在套用哪個方案。
///
/// 綁在 session 而不是 order 上：一桌可能有好幾張單（分開結帳、續攤），
/// 但「這桌是吃到飽」是整桌的事。Airレジ 也是綁桌。
pub async fn active_plan_for_order(
    uow: &mut SqliteUow,
    order_id: &str,
) -> AppResult<Option<DiningPlan>> {
    let row = sqlx::query(
        "SELECT s.dining_plan_id
           FROM orders o
           JOIN table_sessions s ON s.id = o.table_session_id
          WHERE o.id = ?1 AND s.dining_plan_id IS NOT NULL",
    )
    .bind(order_id)
    .fetch_optional(uow.conn())
    .await?;

    let Some(r) = row else { return Ok(None) };
    let plan_id: String = r.get("dining_plan_id");
    load_in(uow, &plan_id).await
}

async fn load_in(uow: &mut SqliteUow, plan_id: &str) -> AppResult<Option<DiningPlan>> {
    let Some(p) = sqlx::query(
        "SELECT p.id, p.item_id, p.name, p.limit_minutes, p.notice_minutes,
                p.print_members_on_bill, p.is_active, i.name AS item_name
           FROM dining_plans p JOIN items i ON i.id = p.item_id
          WHERE p.id = ?1 AND p.deleted_at IS NULL",
    )
    .bind(plan_id)
    .fetch_optional(uow.conn())
    .await?
    else {
        return Ok(None);
    };

    let members =
        sqlx::query("SELECT target_type, target_id FROM dining_plan_items WHERE plan_id = ?1")
            .bind(plan_id)
            .fetch_all(uow.conn())
            .await?;

    let mut member_items = Vec::new();
    let mut member_categories = Vec::new();
    for m in &members {
        let t: String = m.get("target_type");
        let id: String = m.get("target_id");
        if t == "category" {
            member_categories.push(id);
        } else {
            member_items.push(id);
        }
    }

    Ok(Some(DiningPlan {
        id: p.get("id"),
        item_id: p.get("item_id"),
        item_name: p.get("item_name"),
        name: p.get("name"),
        limit_minutes: p.get("limit_minutes"),
        notice_minutes: p.get("notice_minutes"),
        print_members_on_bill: p.get::<i64, _>("print_members_on_bill") == 1,
        is_active: p.get::<i64, _>("is_active") == 1,
        member_items,
        member_categories,
    }))
}

/// 這一批要加的品項裡，有沒有哪一個是某個方案的代表商品？
///
/// 有的話，點它就等於宣告「這桌吃到飽」（見 `order::add_lines` 的說明）。
/// 只看**啟用中**的方案 —— 老闆停用了它就不該再被意外觸發。
pub async fn plan_for_any_item(
    uow: &mut SqliteUow,
    lines: &[crate::services::order::NewLine],
) -> AppResult<Option<DiningPlan>> {
    for l in lines {
        let found = sqlx::query_scalar::<_, String>(
            "SELECT id FROM dining_plans
              WHERE item_id = ?1 AND deleted_at IS NULL AND is_active = 1",
        )
        .bind(&l.item_id)
        .fetch_optional(uow.conn())
        .await?;
        if let Some(pid) = found {
            return load_in(uow, &pid).await;
        }
    }
    Ok(None)
}

/// 把方案綁到這張單所在的桌上，並開始計時。
///
/// 外帶沒有 session，所以綁不上 —— 那是對的：吃到飽本來就是內用的事，
/// 而外帶那一份仍然會照方案商品的原價收錢（它就是一個 599 的便當）。
pub async fn bind_to_order_session(
    uow: &mut SqliteUow,
    order_id: &str,
    plan_id: &str,
    now: &Stamp,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE table_sessions
            SET dining_plan_id = ?2, plan_started_at = ?3, updated_at = ?3
          WHERE id = (SELECT table_session_id FROM orders WHERE id = ?1)
            AND status <> 'closed'
            AND dining_plan_id IS NULL",
    )
    .bind(order_id)
    .bind(plan_id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    Ok(())
}

/// 這一行的來源，決定收據怎麼印、報表怎麼算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineOrigin {
    /// 一般點的商品。
    Item,
    /// 吃到飽方案本身（有價，人頭費就是這個 × 人數）。
    Plan,
    /// 方案內的品項（0 元）。
    PlanMember,
    /// 開桌費 / お通し（× 人數）。
    Cover,
}

impl LineOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Plan => "plan",
            Self::PlanMember => "plan_member",
            Self::Cover => "cover",
        }
    }
}

/// 把一桌切換成吃到飽（或取消）。
///
/// `plan_id = None` 就是恢復單點 —— 已經點過的 0 元行**不會**跟著漲回原價，
/// 那些是當時的事實。之後點的才會照原價收。
///
/// # 計時從這一刻開始，不是從入座開始
///
/// 客人常常先坐下看菜單再決定。Airレジ 的 L.O. 也是「放題プラン注文から
/// XX 分後」—— 從點方案那一刻算，不是入座。
pub async fn apply_to_session(
    ctx: &Ctx,
    session_id: String,
    plan_id: Option<String>,
) -> AppResult<()> {
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;

    if let Some(pid) = &plan_id {
        // 停用或刪掉的方案不該還能套上去 —— 否則老闆停用了它，
        // 外場還是套得到，而他不會知道為什麼。
        let ok = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM dining_plans
              WHERE id = ?1 AND deleted_at IS NULL AND is_active = 1",
        )
        .bind(pid)
        .fetch_one(uow.conn())
        .await?;
        if ok == 0 {
            return Err(AppError::NotFound("這個方案已經停用了".into()));
        }
    }

    let n = sqlx::query(
        "UPDATE table_sessions
            SET dining_plan_id = ?2,
                plan_started_at = CASE WHEN ?2 IS NULL THEN NULL ELSE ?3 END,
                updated_at = ?3
          WHERE id = ?1 AND status <> 'closed'",
    )
    .bind(&session_id)
    .bind(&plan_id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();

    if n == 0 {
        return Err(AppError::NotFound("找不到這一桌，或它已經清桌了".into()));
    }
    uow.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------- 維護

pub async fn list(ctx: &Ctx) -> AppResult<Vec<DiningPlan>> {
    let rows = sqlx::query(
        "SELECT id FROM dining_plans
          WHERE store_id = (SELECT id FROM stores LIMIT 1) AND deleted_at IS NULL
          ORDER BY sort_order, name",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let mut uow = ctx.db.begin_write().await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        if let Some(p) = load_in(&mut uow, &r.get::<String, _>("id")).await? {
            out.push(p);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInput {
    pub id: Option<String>,
    pub item_id: String,
    pub name: String,
    pub limit_minutes: Option<i64>,
    pub notice_minutes: Option<i64>,
    pub print_members_on_bill: Option<bool>,
    pub is_active: Option<bool>,
    #[serde(default)]
    pub member_items: Vec<String>,
    #[serde(default)]
    pub member_categories: Vec<String>,
}

pub async fn upsert(ctx: &Ctx, input: PlanInput) -> AppResult<DiningPlan> {
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;
    if input.name.trim().is_empty() {
        return Err(AppError::Validation("方案名稱不能空白".into()));
    }
    let limit = input.limit_minutes.unwrap_or(0);
    let notice = input.notice_minutes.unwrap_or(0);
    if notice > 0 && limit > 0 && notice >= limit {
        // 事前通知比時限還晚就永遠不會響，而店家不會發現 ——
        // 他只會覺得「這個提醒好像沒有用」。
        return Err(AppError::Validation(
            "事前通知要比用餐時限短，否則永遠不會提醒".into(),
        ));
    }

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());
    let mut uow = ctx.db.begin_write().await?;

    if input.id.is_none() {
        sqlx::query(
            "INSERT INTO dining_plans (id, store_id, item_id, name, limit_minutes,
                                       notice_minutes, print_members_on_bill, is_active,
                                       sort_order, created_at, updated_at)
             VALUES (?1, (SELECT id FROM stores LIMIT 1), ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)",
        )
        .bind(&id)
        .bind(&input.item_id)
        .bind(input.name.trim())
        .bind(limit)
        .bind(notice)
        .bind(i64::from(input.print_members_on_bill.unwrap_or(false)))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        sqlx::query(
            "UPDATE dining_plans SET item_id = ?2, name = ?3, limit_minutes = ?4,
                                     notice_minutes = ?5, print_members_on_bill = ?6,
                                     is_active = ?7, updated_at = ?8
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(&input.item_id)
        .bind(input.name.trim())
        .bind(limit)
        .bind(notice)
        .bind(i64::from(input.print_members_on_bill.unwrap_or(false)))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    // 成員整批換掉。逐筆 diff 沒有意義 —— 這張表沒有自己的歷史，
    // 而訂單那一側存的是快照，不會受影響。
    sqlx::query("DELETE FROM dining_plan_items WHERE plan_id = ?1")
        .bind(&id)
        .execute(uow.conn())
        .await?;
    for (t, ids) in [
        ("item", &input.member_items),
        ("category", &input.member_categories),
    ] {
        for target in ids {
            sqlx::query(
                "INSERT INTO dining_plan_items (id, plan_id, target_type, target_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(Id::new().to_string())
            .bind(&id)
            .bind(t)
            .bind(target)
            .bind(now.iso())
            .execute(uow.conn())
            .await?;
        }
    }

    let out = load_in(&mut uow, &id).await?;
    uow.commit().await?;
    out.ok_or_else(|| AppError::Internal("方案存好了卻讀不回來".into()))
}

pub async fn delete(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_MENU).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    // 軟刪除：已經開過的桌還指著它，帳要看得出當時是哪個方案。
    let n = sqlx::query("UPDATE dining_plans SET deleted_at = ?2, is_active = 0 WHERE id = ?1 AND deleted_at IS NULL")
        .bind(&id)
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到方案 {id}")));
    }
    uow.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> DiningPlan {
        DiningPlan {
            id: "P1".into(),
            item_id: "ITEM_BUFFET".into(),
            item_name: "吃到飽".into(),
            name: "晚餐吃到飽".into(),
            limit_minutes: 120,
            notice_minutes: 30,
            print_members_on_bill: false,
            is_active: true,
            member_items: vec!["ITEM_TEA".into()],
            member_categories: vec!["CAT_DRINKS".into()],
        }
    }

    #[test]
    fn the_plan_item_itself_is_never_free() {
        // 代表商品就是要收錢的那一行。把它算成成員的話，
        // 整桌吃到飽會變成 0 元 —— 而且不會有任何錯誤訊息。
        let p = plan();
        assert!(!p.covers("ITEM_BUFFET", Some("CAT_DRINKS")));
    }

    #[test]
    fn membership_covers_items_and_whole_categories() {
        let p = plan();
        assert!(p.covers("ITEM_TEA", None), "直接列名的品項");
        assert!(p.covers("ITEM_ANYTHING", Some("CAT_DRINKS")), "整個分類");
        assert!(
            !p.covers("ITEM_STEAK", Some("CAT_MAINS")),
            "不在方案裡的照常收錢"
        );
        // 加價品：不在方案裡就是原價，這是預設行為不是特例。
        assert!(!p.covers("ITEM_WAGYU", None));
    }

    #[test]
    fn an_item_with_no_category_only_matches_by_id() {
        let p = plan();
        assert!(p.covers("ITEM_TEA", None));
        assert!(!p.covers("ITEM_OTHER", None));
    }

    #[test]
    fn line_origin_round_trips() {
        for o in [
            LineOrigin::Item,
            LineOrigin::Plan,
            LineOrigin::PlanMember,
            LineOrigin::Cover,
        ] {
            assert!(!o.as_str().is_empty());
        }
        assert_eq!(LineOrigin::PlanMember.as_str(), "plan_member");
    }
}
