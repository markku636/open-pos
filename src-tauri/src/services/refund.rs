//! 退款。
//!
//! # 退款不是作廢
//!
//! 作廢的語意是「這筆交易沒有發生過」；退款的語意是「發生過，然後退回來」。
//! 兩者在稅務上是不同的東西（銷貨退回要開折讓，作廢是作廢），在報表上也是：
//! 作廢的單不該出現在營業額裡，退款的單該出現、然後另外扣掉。
//!
//! 少了退款，店家只能用「結帳後作廢」處理「三杯飲料有一杯做錯了」——
//! 那會把整筆銷售抹掉，而且那是餐飲業最大的防弊點，不該天天被用到。
//!
//! # 原路退回
//!
//! 一筆退款一定指向**一筆收款**。刷卡收的錢用現金退，是最經典的一種內神通
//! 外鬼：帳面上兩邊都平，抽屜裡少了錢而信用卡那筆還在。所以這裡不提供
//! 「用別的方式退」的選項 —— 要退哪一筆，就退回那一筆的來路。
//!
//! # 退款算在今天
//!
//! 昨天的帳單今天退，錢是今天離開抽屜的。`refunds.business_date` 存的是
//! **今天**，不是原帳單的日期 —— 否則今天關班一定短少，而昨天的 Z 報表
//! 會在事後被改動。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::infra::db::sqlite::SqliteUow;
use crate::services::audit::{self, AuditAction, AuditEntry};
use crate::services::rbac;

const PERM_REFUND: &str = "payment.refund";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillPaymentView {
    pub id: String,
    pub method_code: String,
    pub method_name: String,
    /// 這一筆收了多少（沖銷金額，不含找回去的錢）。
    pub amount: i64,
    /// 這一筆已經退了多少。
    pub refunded: i64,
    /// 還可以退多少。
    pub refundable: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillView {
    pub id: String,
    pub bill_no: String,
    pub order_no: String,
    pub business_date: String,
    pub settled_at: Option<String>,
    pub status: String,
    pub grand_total: i64,
    pub refunded_total: i64,
    pub refundable: i64,
    /// 分帳的那一份：「2／4」。整單結帳是 None。
    pub split_label: Option<String>,
    pub payments: Vec<BillPaymentView>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindBillsReq {
    /// 哪一個營業日的帳單。省略＝今天。
    pub business_date: Option<String>,
    /// 單號片段（收銀員手上通常只有客人那張收據的末幾碼）。
    pub bill_no: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundReq {
    pub bill_id: String,
    /// 退哪一筆收款。只有一筆的話可以省略。
    pub payment_id: Option<String>,
    pub amount: i64,
    /// 退款一定要有原因。這是防弊的一半，另一半是有人要簽名。
    pub reason_id: Option<String>,
    pub note: Option<String>,
    /// 主管的使用者 id（收銀員預設沒有這個權限）。
    pub approver_id: Option<String>,
    pub idem_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundResult {
    pub bill_no: String,
    pub amount: i64,
    pub method_name: String,
    /// 這張帳單累計退了多少。
    pub refunded_total: i64,
    /// 還可以退多少。
    pub refundable: i64,
    pub bill_status: String,
}

/// 找帳單。
///
/// 收銀員手上通常只有客人那張收據，所以單號要能**用末幾碼搜**——
/// 要求他把 `B-20260906-0042` 一字不差打完，實際上就是要求他別用這個功能。
pub async fn find_bills(ctx: &Ctx, req: FindBillsReq) -> AppResult<Vec<BillView>> {
    let now = Stamp::now();
    let date = match req.business_date {
        Some(d) => d,
        None => crate::services::shift::today(ctx, &now).await?,
    };

    let like = req.bill_no.as_deref().map(|s| format!("%{}%", s.trim()));
    let rows = sqlx::query(
        "SELECT b.id, b.bill_no, b.business_date, b.settled_at, b.status, b.grand_total,
                b.split_mode, b.split_index, b.split_count, o.order_no
           FROM bills b JOIN orders o ON o.id = b.order_id
          WHERE b.status IN ('settled', 'partially_refunded', 'refunded')
            AND (?1 IS NULL OR b.business_date = ?1)
            AND (?2 IS NULL OR b.bill_no LIKE ?2)
          ORDER BY b.settled_at DESC
          LIMIT 100",
    )
    // 有搜尋字串時就跨日找 —— 客人拿著三天前的收據回來是常態。
    .bind(if like.is_some() { None } else { Some(&date) })
    .bind(&like)
    .fetch_all(ctx.db.reader())
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: String = r.get("id");
        let payments = payments_of(ctx, &id).await?;
        let refunded_total: i64 = payments.iter().map(|p| p.refunded).sum();
        let grand_total: i64 = r.get("grand_total");
        let split_mode: String = r.get("split_mode");
        let count: i64 = r.get("split_count");
        out.push(BillView {
            id,
            bill_no: r.get("bill_no"),
            order_no: r.get("order_no"),
            business_date: r.get("business_date"),
            settled_at: r.get("settled_at"),
            status: r.get("status"),
            grand_total,
            refunded_total,
            refundable: (grand_total - refunded_total).max(0),
            split_label: if split_mode == "none" {
                None
            } else {
                let index: i64 = r.get("split_index");
                Some(if count > 0 {
                    format!("{index}／{count}")
                } else {
                    format!("第 {index} 筆")
                })
            },
            payments,
        });
    }
    Ok(out)
}

async fn payments_of(ctx: &Ctx, bill_id: &str) -> AppResult<Vec<BillPaymentView>> {
    let rows = sqlx::query(
        "SELECT p.id, p.method_code_snapshot AS code, p.method_name_snapshot AS name, p.amount,
                COALESCE((SELECT SUM(r.amount) FROM refunds r WHERE r.payment_id = p.id), 0)
                  AS refunded
           FROM payments p
          WHERE p.bill_id = ?1 AND p.status IN ('captured', 'refunded')
          ORDER BY p.id",
    )
    .bind(bill_id)
    .fetch_all(ctx.db.reader())
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let amount: i64 = r.get("amount");
            let refunded: i64 = r.get("refunded");
            BillPaymentView {
                id: r.get("id"),
                method_code: r.get("code"),
                method_name: r.get("name"),
                amount,
                refunded,
                refundable: (amount - refunded).max(0),
            }
        })
        .collect())
}

/// 退款。
///
/// 需要 `payment.refund` 權限（收銀員預設沒有），而且一定要選原因並留下簽核
/// 紀錄 —— 退款是把現金交出去，跟結帳後作廢屬於同一類風險。
pub async fn refund(ctx: &Ctx, req: RefundReq) -> AppResult<RefundResult> {
    if let Some(cached) = load_idempotent::<RefundResult>(ctx, &req.idem_key).await? {
        return Ok(cached);
    }
    if req.amount <= 0 {
        return Err(AppError::Validation("退款金額要大於 0".into()));
    }
    if req.reason_id.is_none() {
        return Err(AppError::Validation("退款必須選一個原因。".into()));
    }

    let now = Stamp::now();
    let today = crate::services::shift::today(ctx, &now).await?;

    // 權限與簽核在交易外先確認 —— 它會讀 rbac 快取與另一個使用者，
    // 而寫入池只有一條連線，在交易裡再去拿連線就是自我死鎖。
    let approver = crate::services::order::load_actor(ctx, req.approver_id.as_deref()).await?;
    let authorized =
        rbac::require_with_approval(&ctx.db, &ctx.actor, approver.as_ref(), PERM_REFUND).await?;

    let mut uow = ctx.db.begin_write().await?;
    crate::services::shift::ensure_day_open(&mut uow, &today).await?;

    let bill = sqlx::query(
        "SELECT b.bill_no, b.grand_total, b.status, b.store_id, o.id AS order_id
           FROM bills b JOIN orders o ON o.id = b.order_id
          WHERE b.id = ?1",
    )
    .bind(&req.bill_id)
    .fetch_optional(uow.conn())
    .await?
    .ok_or_else(|| AppError::NotFound("找不到這張帳單".into()))?;

    let bill_status: String = bill.get("status");
    if bill_status == "voided" {
        return Err(AppError::Conflict(
            "這張帳單已經作廢了，作廢的單不需要退款。".into(),
        ));
    }

    // 哪一筆收款。
    let payments = sqlx::query(
        "SELECT p.id, p.method_code_snapshot AS code, p.method_name_snapshot AS name, p.amount,
                COALESCE((SELECT SUM(r.amount) FROM refunds r WHERE r.payment_id = p.id), 0)
                  AS refunded
           FROM payments p
          WHERE p.bill_id = ?1 AND p.status IN ('captured', 'refunded')
          ORDER BY p.id",
    )
    .bind(&req.bill_id)
    .fetch_all(uow.conn())
    .await?;
    if payments.is_empty() {
        return Err(AppError::Conflict("這張帳單上沒有收款紀錄".into()));
    }
    let target = match &req.payment_id {
        Some(id) => payments
            .iter()
            .find(|p| p.get::<String, _>("id") == *id)
            .ok_or_else(|| AppError::NotFound("這張帳單上沒有那一筆收款".into()))?,
        None if payments.len() == 1 => &payments[0],
        // 混合支付時不替店家決定退哪一筆 —— 猜錯的話帳面兩邊都平，
        // 但抽屜裡的錢對不上。
        None => {
            return Err(AppError::Validation(
                "這張帳單有多筆收款，請選要退回哪一筆。".into(),
            ))
        }
    };

    let payment_id: String = target.get("id");
    let method_code: String = target.get("code");
    let method_name: String = target.get("name");
    let paid: i64 = target.get("amount");
    let already: i64 = target.get("refunded");
    let refundable = (paid - already).max(0);
    if req.amount > refundable {
        return Err(AppError::Validation(format!(
            "「{method_name}」這一筆只收了 {paid} 元、已經退過 {already} 元，最多只能再退 {refundable} 元"
        )));
    }

    let bill_total: i64 = bill.get("grand_total");
    let bill_refunded: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(amount), 0) FROM refunds WHERE bill_id = ?1")
            .bind(&req.bill_id)
            .fetch_one(uow.conn())
            .await?;
    let refunded_total = bill_refunded + req.amount;
    let fully = refunded_total >= bill_total;

    let shift_id = crate::services::shift::current_shift_id(&mut uow).await?;
    let store_id: String = bill.get("store_id");
    let bill_no: String = bill.get("bill_no");

    sqlx::query(
        "INSERT INTO refunds (id, bill_id, payment_id, store_id, shift_id, business_date, kind,
                              amount, reason_id, note, approved_by, created_by, refunded_at,
                              created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
    )
    .bind(Id::new().as_str())
    .bind(&req.bill_id)
    .bind(&payment_id)
    .bind(&store_id)
    .bind(&shift_id)
    // ★ 今天的營業日，不是原帳單的 —— 錢是今天離開抽屜的。
    .bind(&today)
    .bind(if fully { "full" } else { "partial" })
    .bind(req.amount)
    .bind(&req.reason_id)
    .bind(&req.note)
    .bind(&authorized.user_id)
    .bind(&ctx.actor.user_id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    sqlx::query("UPDATE bills SET status = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(&req.bill_id)
        .bind(if fully {
            "refunded"
        } else {
            "partially_refunded"
        })
        .bind(now.iso())
        .execute(uow.conn())
        .await?;

    // 收款那一筆整筆退完才改狀態。部分退款時它仍然是 captured ——
    // 因為那筆錢確實還有一部分留在店裡。
    if refundable == req.amount {
        sqlx::query("UPDATE payments SET status = 'refunded' WHERE id = ?1")
            .bind(&payment_id)
            .execute(uow.conn())
            .await?;
    }

    sqlx::query(
        "INSERT INTO approvals (id, action_code, ref_type, ref_id, amount, reason_id, note,
                                requested_by, approved_by, auth_method, business_date,
                                approved_at, created_at)
         VALUES (?1, ?2, 'bill', ?3, ?4, ?5, ?6, ?7, ?8, 'pin', ?9, ?10, ?10)",
    )
    .bind(Id::new().as_str())
    .bind(PERM_REFUND)
    .bind(&req.bill_id)
    .bind(req.amount)
    .bind(&req.reason_id)
    .bind(&req.note)
    .bind(&ctx.actor.user_id)
    .bind(&authorized.user_id)
    .bind(&today)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    let mut entry = AuditEntry::new("Bill", &req.bill_id, AuditAction::Refund)
        .to(&bill_no)
        // 稽核看的是「動了多少錢」，退款是負的。
        .amount(-req.amount)
        // 誰批准的要跟金額記在同一列 —— 「這筆錢是誰放行的」是同一個問題。
        .approved_by(&authorized.user_id)
        .on(&today);
    if let Some(r) = &req.reason_id {
        entry = entry.reason(r);
    }
    audit::write_in(&mut uow, entry, &ctx.actor, &now).await?;

    let result = RefundResult {
        bill_no,
        amount: req.amount,
        method_name,
        refunded_total,
        refundable: (bill_total - refunded_total).max(0),
        bill_status: if fully {
            "refunded".into()
        } else {
            "partially_refunded".into()
        },
    };

    enqueue_slip(&mut uow, ctx, &result, &method_code, &today, &now).await?;
    save_idempotent(&mut uow, &req.idem_key, &result, &now).await?;
    uow.commit().await?;
    Ok(result)
}

/// 退款單。
///
/// 印出來讓客人簽名，店家留存。這是退款唯一的紙本憑證 ——
/// 沒有它，「我明明退了」跟「我沒收到」之間沒有第三方。
async fn enqueue_slip(
    uow: &mut SqliteUow,
    ctx: &Ctx,
    r: &RefundResult,
    method_code: &str,
    business_date: &str,
    now: &Stamp,
) -> AppResult<()> {
    use crate::receipt::{Cell, Finish, PaperWidth, ReceiptDoc, TextStyle};

    let row =
        sqlx::query("SELECT name, tz FROM stores WHERE deleted_at IS NULL ORDER BY id LIMIT 1")
            .fetch_optional(uow.conn())
            .await?;
    let (store, tz) = match &row {
        Some(r) => (
            r.get::<String, _>("name"),
            r.get::<String, _>("tz")
                .parse()
                .unwrap_or(chrono_tz::Asia::Taipei),
        ),
        None => (String::new(), chrono_tz::Asia::Taipei),
    };

    let doc = ReceiptDoc::new(PaperWidth::Mm80)
        .styled(&store, TextStyle::centered())
        .styled("退 款 單", TextStyle::centered())
        .feed(1)
        .columns(
            vec![
                Cell::left(format!("原單 {}", r.bill_no)),
                Cell::right(crate::core::clock::for_humans(now.at, tz)),
            ],
            vec![1, 1],
        )
        .rule()
        .columns(
            vec![
                Cell::left(format!("退款方式　{}", r.method_name)),
                Cell::right(r.amount.to_string()),
            ],
            vec![3, 2],
        )
        .columns(
            vec![
                Cell::left("本單累計退款"),
                Cell::right(r.refunded_total.to_string()),
            ],
            vec![3, 2],
        )
        .rule()
        .text("經手人：____________")
        .feed(1)
        // 客人簽名是這張單存在的理由。
        .text("客人簽名：__________")
        .feed(2)
        .finish(Finish {
            open_drawer: method_code == "cash",
            ..Finish::default()
        });

    let payload = serde_json::json!({ "billNo": r.bill_no, "doc": doc });
    crate::services::order::enqueue_print(uow, "print.refund", business_date, &payload, now)
        .await?;
    let _ = ctx;
    Ok(())
}

// ---------------------------------------------------------------- 冪等

async fn load_idempotent<T: serde::de::DeserializeOwned>(
    ctx: &Ctx,
    key: &str,
) -> AppResult<Option<T>> {
    let row: Option<String> =
        sqlx::query_scalar("SELECT response_json FROM idempotency_keys WHERE key = ?1")
            .bind(key)
            .fetch_optional(ctx.db.reader())
            .await?;
    match row {
        Some(json) => Ok(serde_json::from_str(&json).ok()),
        None => Ok(None),
    }
}

async fn save_idempotent<T: Serialize>(
    uow: &mut SqliteUow,
    key: &str,
    value: &T,
    now: &Stamp,
) -> AppResult<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO idempotency_keys (key, operation, response_json, created_at)
         VALUES (?1, 'refund', ?2, ?3)",
    )
    .bind(key)
    .bind(serde_json::to_string(value).unwrap_or_default())
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    Ok(())
}
