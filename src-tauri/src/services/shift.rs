//! 班別與日結。
//!
//! # 盲盤（blind count）
//!
//! 關班時**不先顯示應有現金**。收銀員照面額數完、輸入之後，系統才揭曉差異。
//!
//! 這一條不是防呆設計，是防弊設計：先把應有金額顯示出來，短少的人會直接照抄，
//! 而那正是現金差異永遠是零的原因 —— 不是因為沒有問題，是因為看不到問題。
//! X 報表（會顯示現金）掛在 `report.daily` 權限下，收銀員預設拿不到。
//!
//! # 快照，永不重算
//!
//! 關班與日結當下算好的數字直接寫進 `shifts.summary_json` 與 `daily_summaries`，
//! 之後**永不重算**。否則隔天補一張單，三個月前的 Z 報表數字就會跟著變 ——
//! 而一份會自己變的報表在稽核上完全站不住。
//!
//! # 日結之後鎖定
//!
//! `business_days` 關掉之後，那個營業日拒絕任何交易寫入。少了這道鎖，
//! 「日結完成」就只是一個沒有意義的時間戳。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::business_date::BusinessDate;
use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::infra::db::sqlite::uow::SqliteUow;
use crate::services::audit::{self, AuditAction, AuditEntry};
use crate::services::rbac;
use crate::services::sequence::{self, Scope};

const PERM_OPEN: &str = "shift.open";
const PERM_CLOSE: &str = "shift.close";
const PERM_REPORT: &str = "report.daily";
const PERM_Z: &str = "report.z";

// ---------------------------------------------------------------- 型別

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftView {
    pub id: String,
    pub shift_no: String,
    pub business_date: String,
    pub status: String,
    pub opened_at: String,
    pub opened_by: String,
    pub opening_float: i64,
    pub closed_at: Option<String>,
    /// **關班之前不會有值** —— 盲盤的重點就在這裡。
    pub expected_cash: Option<i64>,
    pub counted_cash: Option<i64>,
    pub cash_variance: Option<i64>,
    pub note: Option<String>,
}

/// 面額盤點的一列。`denomination` 以元為單位（1000 = 一千元鈔）。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenomCount {
    pub denomination: i64,
    pub count: i64,
}

impl DenomCount {
    fn subtotal(&self) -> i64 {
        self.denomination.saturating_mul(self.count)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesTotals {
    pub bills: i64,
    pub subtotal: i64,
    pub discount: i64,
    pub service_charge: i64,
    pub rounding: i64,
    /// 未稅銷售額。
    pub sales: i64,
    pub tax: i64,
    /// 含稅總額。`sales + tax` 必須等於它 —— 那是財政部的硬檢核。
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentTotal {
    pub code: String,
    pub name: String,
    pub count: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashSummary {
    pub opening_float: i64,
    pub cash_sales: i64,
    pub paid_in: i64,
    pub paid_out: i64,
    /// 應有現金 = 開班準備金 + 現金銷售 + 收入 − 支出。
    pub expected: i64,
    pub counted: Option<i64>,
    /// 正數 = 溢收，負數 = 短少。
    pub variance: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoidTotals {
    pub voided_lines: i64,
    pub voided_amount: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftReport {
    pub shift_no: String,
    pub business_date: String,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub sales: SalesTotals,
    pub payments: Vec<PaymentTotal>,
    pub cash: CashSummary,
    pub voids: VoidTotals,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayReport {
    pub business_date: String,
    pub z_report_no: String,
    pub closed_at: String,
    pub sales: SalesTotals,
    pub payments: Vec<PaymentTotal>,
    pub voids: VoidTotals,
    /// 各班的現金差異。日結時最該被看的一欄。
    pub shifts: Vec<ShiftView>,
    pub top_items: Vec<ItemLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemLine {
    pub name: String,
    pub qty_milli: i64,
    pub amount: i64,
}

// ---------------------------------------------------------------- 共用

async fn store_id_and_date(ctx: &Ctx, now: &Stamp) -> AppResult<(String, String)> {
    let r = sqlx::query(
        "SELECT id, tz, business_day_cutoff FROM stores
          WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_optional(ctx.db.reader())
    .await?
    .ok_or_else(|| AppError::NotFound("還沒有建立店家資料".into()))?;

    let tz: String = r.get("tz");
    let cutoff: String = r.get("business_day_cutoff");
    let cfg = crate::core::business_date::BusinessDayConfig {
        tz: tz.parse().unwrap_or(chrono_tz::Asia::Taipei),
        cutoff: cutoff
            .parse::<chrono::NaiveTime>()
            .or_else(|_| chrono::NaiveTime::parse_from_str(&cutoff, "%H:%M"))
            .unwrap_or_default(),
    };
    Ok((
        r.get("id"),
        BusinessDate::of(now.at, cfg.tz, cfg.cutoff).to_iso(),
    ))
}

fn row_to_shift(r: &sqlx::sqlite::SqliteRow) -> ShiftView {
    ShiftView {
        id: r.get("id"),
        shift_no: r.get("shift_no"),
        business_date: r.get("business_date"),
        status: r.get("status"),
        opened_at: r.get("opened_at"),
        opened_by: r.get("opened_by"),
        opening_float: r.get("opening_float"),
        closed_at: r.get("closed_at"),
        expected_cash: r.get("expected_cash"),
        counted_cash: r.get("counted_cash"),
        cash_variance: r.get("cash_variance"),
        note: r.get("note"),
    }
}

const SHIFT_COLS: &str = "id, shift_no, business_date, status, opened_at, opened_by, opening_float,
     closed_at, expected_cash, counted_cash, cash_variance, note";

/// 目前開著的班。整台機器同時只會有一班。
pub async fn current_shift(ctx: &Ctx) -> AppResult<Option<ShiftView>> {
    let sql = format!(
        "SELECT {SHIFT_COLS} FROM shifts WHERE status = 'open' ORDER BY opened_at DESC LIMIT 1"
    );
    Ok(sqlx::query(&sql)
        .fetch_optional(ctx.db.reader())
        .await?
        .as_ref()
        .map(row_to_shift))
}

async fn open_shift_row(uow: &mut SqliteUow) -> AppResult<Option<ShiftView>> {
    let sql = format!(
        "SELECT {SHIFT_COLS} FROM shifts WHERE status = 'open' ORDER BY opened_at DESC LIMIT 1"
    );
    Ok(sqlx::query(&sql)
        .fetch_optional(uow.conn())
        .await?
        .as_ref()
        .map(row_to_shift))
}

/// 這一筆交易屬於哪一班。
///
/// 沒有開班時回 None 而不是報錯 —— 「忘了開班」不該讓店家收不了錢。
/// 那些單會落在「無班別」裡，日結時看得到。
pub async fn current_shift_id(uow: &mut SqliteUow) -> AppResult<Option<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT id FROM shifts WHERE status = 'open' ORDER BY opened_at DESC LIMIT 1",
    )
    .fetch_optional(uow.conn())
    .await?)
}

/// 這個營業日還能不能寫入。
///
/// 日結完成之後就不行了。少了這道鎖，「日結」只是一個沒有意義的時間戳 ——
/// 而事後補進來的單會讓已經印出來的 Z 報表對不上。
pub async fn ensure_day_open(uow: &mut SqliteUow, business_date: &str) -> AppResult<()> {
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM business_days WHERE business_date = ?1")
            .bind(business_date)
            .fetch_optional(uow.conn())
            .await?;
    match status.as_deref() {
        Some("closed") | Some("locked") => Err(AppError::Conflict(format!(
            "{business_date} 已經日結，不能再新增或修改這一天的單。\n\
             如果真的需要補單，請開新的營業日並在備註裡註明。"
        ))),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------- 開班

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenShiftReq {
    /// 抽屜裡先放的零錢。
    pub opening_float: i64,
    /// 開班盤點（選填）。有盤點就以盤點金額為準。
    pub counts: Option<Vec<DenomCount>>,
    pub note: Option<String>,
}

pub async fn open_shift(ctx: &Ctx, req: OpenShiftReq) -> AppResult<ShiftView> {
    rbac::require(&ctx.db, &ctx.actor, PERM_OPEN).await?;
    if req.opening_float < 0 {
        return Err(AppError::Validation("準備金不能是負數".into()));
    }

    let now = Stamp::now();
    let (store_id, business_date) = store_id_and_date(ctx, &now).await?;

    let mut uow = ctx.db.begin_write().await?;
    ensure_day_open(&mut uow, &business_date).await?;
    if let Some(open) = open_shift_row(&mut uow).await? {
        return Err(AppError::Conflict(format!(
            "{} 還開著（{}），請先關班再開新的一班。",
            open.shift_no, open.opened_at
        )));
    }

    // 營業日在第一次開班時建立。
    sqlx::query(
        "INSERT INTO business_days (id, store_id, business_date, status, opened_at, opened_by,
                                    created_at, updated_at)
         VALUES (?1, ?2, ?3, 'open', ?4, ?5, ?4, ?4)
         ON CONFLICT (store_id, business_date) DO NOTHING",
    )
    .bind(Id::new().as_str())
    .bind(&store_id)
    .bind(&business_date)
    .bind(now.iso())
    .bind(&ctx.actor.user_id)
    .execute(uow.conn())
    .await?;

    let counted = req
        .counts
        .as_ref()
        .map(|c| c.iter().map(DenomCount::subtotal).sum::<i64>());
    let opening_float = counted.unwrap_or(req.opening_float);

    let shift_no =
        sequence::next_no(&mut uow, &store_id, Scope::Shift, &business_date, &now).await?;
    let id = Id::new().to_string();
    sqlx::query(
        "INSERT INTO shifts (id, store_id, business_date, shift_no, status, opened_by, opened_at,
                             opening_float, note, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6, ?7, ?8, ?6, ?6)",
    )
    .bind(&id)
    .bind(&store_id)
    .bind(&business_date)
    .bind(&shift_no)
    .bind(&ctx.actor.user_id)
    .bind(now.iso())
    .bind(opening_float)
    .bind(&req.note)
    .execute(uow.conn())
    .await?;

    if let Some(counts) = &req.counts {
        save_counts(&mut uow, &id, "open", counts, &now).await?;
    }
    // 準備金也是一筆現金異動：不記的話，抽屜裡的錢從哪裡來就查不出來。
    sqlx::query(
        "INSERT INTO cash_movements (id, shift_id, business_date, kind, amount, actor_id, created_at)
         VALUES (?1, ?2, ?3, 'opening_float', ?4, ?5, ?6)",
    )
    .bind(Id::new().as_str())
    .bind(&id)
    .bind(&business_date)
    .bind(opening_float)
    .bind(&ctx.actor.user_id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    audit::write_in(
        &mut uow,
        AuditEntry::new("shift", &id, AuditAction::Create)
            .amount(opening_float)
            .on(&business_date),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;

    current_shift(ctx)
        .await?
        .ok_or_else(|| AppError::Internal("班別建好了卻讀不回來".into()))
}

async fn save_counts(
    uow: &mut SqliteUow,
    shift_id: &str,
    phase: &str,
    counts: &[DenomCount],
    now: &Stamp,
) -> AppResult<()> {
    for c in counts {
        if c.count < 0 || c.denomination <= 0 {
            return Err(AppError::Validation("盤點的張數與面額不能是負數".into()));
        }
        sqlx::query(
            "INSERT INTO shift_counts (id, shift_id, phase, denomination, count, subtotal, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (shift_id, phase, denomination)
             DO UPDATE SET count = excluded.count, subtotal = excluded.subtotal",
        )
        .bind(Id::new().as_str())
        .bind(shift_id)
        .bind(phase)
        .bind(c.denomination)
        .bind(c.count)
        .bind(c.subtotal())
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------- 現金進出

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashMovementReq {
    /// paid_in（收入）/ paid_out（支出）/ drop（投保險箱）。
    pub kind: String,
    pub amount: i64,
    pub reason_id: Option<String>,
    pub note: Option<String>,
}

pub async fn record_cash_movement(ctx: &Ctx, req: CashMovementReq) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_CLOSE).await?;
    if !["paid_in", "paid_out", "drop", "adjust"].contains(&req.kind.as_str()) {
        return Err(AppError::Validation(format!(
            "不認得的現金異動：{}",
            req.kind
        )));
    }
    if req.amount <= 0 {
        return Err(AppError::Validation("金額必須大於 0".into()));
    }

    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    let shift = open_shift_row(&mut uow)
        .await?
        .ok_or_else(|| AppError::Conflict("現在沒有開著的班，請先開班".into()))?;

    sqlx::query(
        "INSERT INTO cash_movements (id, shift_id, business_date, kind, amount, reason_id, note,
                                     actor_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(Id::new().as_str())
    .bind(&shift.id)
    .bind(&shift.business_date)
    .bind(&req.kind)
    .bind(req.amount)
    .bind(&req.reason_id)
    .bind(&req.note)
    .bind(&ctx.actor.user_id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    // 現金異動是最容易被拿來動手腳的地方，所以稽核一定要留。
    let mut entry = AuditEntry::new("cash_movement", &shift.id, AuditAction::Create)
        .amount(if req.kind == "paid_in" {
            req.amount
        } else {
            -req.amount
        })
        .on(&shift.business_date);
    if let Some(r) = &req.reason_id {
        entry = entry.reason(r);
    }
    audit::write_in(&mut uow, entry, &ctx.actor, &now).await?;
    uow.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------- 統計

async fn sales_totals(uow: &mut SqliteUow, filter: &Filter<'_>) -> AppResult<SalesTotals> {
    let sql = format!(
        "SELECT COUNT(*) AS bills, COALESCE(SUM(subtotal), 0) AS subtotal,
                COALESCE(SUM(discount_total), 0) AS discount,
                COALESCE(SUM(service_charge), 0) AS service_charge,
                COALESCE(SUM(rounding_adjustment), 0) AS rounding,
                COALESCE(SUM(sales_amount), 0) AS sales,
                COALESCE(SUM(tax_amount), 0) AS tax,
                COALESCE(SUM(grand_total), 0) AS total
           FROM bills WHERE status = 'settled' AND {}",
        filter.sql
    );
    let r = filter.bind(sqlx::query(&sql)).fetch_one(uow.conn()).await?;
    Ok(SalesTotals {
        bills: r.get("bills"),
        subtotal: r.get("subtotal"),
        discount: r.get("discount"),
        service_charge: r.get("service_charge"),
        rounding: r.get("rounding"),
        sales: r.get("sales"),
        tax: r.get("tax"),
        total: r.get("total"),
    })
}

async fn payment_totals(uow: &mut SqliteUow, filter: &Filter<'_>) -> AppResult<Vec<PaymentTotal>> {
    let sql = format!(
        "SELECT method_code_snapshot AS code, method_name_snapshot AS name,
                COUNT(*) AS count, COALESCE(SUM(amount), 0) AS amount
           FROM payments WHERE status = 'captured' AND {}
          GROUP BY method_code_snapshot, method_name_snapshot
          ORDER BY amount DESC",
        filter.sql
    );
    let rows = filter.bind(sqlx::query(&sql)).fetch_all(uow.conn()).await?;
    Ok(rows
        .iter()
        .map(|r| PaymentTotal {
            code: r.get("code"),
            name: r.get("name"),
            count: r.get("count"),
            amount: r.get("amount"),
        })
        .collect())
}

async fn void_totals(uow: &mut SqliteUow, business_date: &str) -> AppResult<VoidTotals> {
    let r = sqlx::query(
        "SELECT COUNT(*) AS n, COALESCE(SUM(oi.unit_price * oi.qty_milli / 1000), 0) AS amount
           FROM order_items oi
           JOIN orders o ON o.id = oi.order_id
          WHERE o.business_date = ?1 AND oi.voided_at IS NOT NULL",
    )
    .bind(business_date)
    .fetch_one(uow.conn())
    .await?;
    Ok(VoidTotals {
        voided_lines: r.get("n"),
        voided_amount: r.get("amount"),
    })
}

/// 統計的範圍：一個班，或一整個營業日。
struct Filter<'a> {
    sql: &'static str,
    value: &'a str,
}

impl<'a> Filter<'a> {
    fn shift(id: &'a str) -> Self {
        Self {
            sql: "shift_id = ?1",
            value: id,
        }
    }
    fn day(date: &'a str) -> Self {
        Self {
            sql: "business_date = ?1",
            value: date,
        }
    }
    fn bind<'q>(
        &'a self,
        q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>
    where
        'a: 'q,
    {
        q.bind(self.value)
    }
}

async fn cash_summary(
    uow: &mut SqliteUow,
    shift: &ShiftView,
    payments: &[PaymentTotal],
) -> AppResult<CashSummary> {
    let cash_sales: i64 = payments
        .iter()
        .filter(|p| p.code == "cash")
        .map(|p| p.amount)
        .sum();

    let moves = sqlx::query(
        "SELECT kind, COALESCE(SUM(amount), 0) AS amount FROM cash_movements
          WHERE shift_id = ?1 GROUP BY kind",
    )
    .bind(&shift.id)
    .fetch_all(uow.conn())
    .await?;
    let mut paid_in = 0i64;
    let mut paid_out = 0i64;
    for m in &moves {
        let amount: i64 = m.get("amount");
        match m.get::<String, _>("kind").as_str() {
            "paid_in" => paid_in += amount,
            // drop（投保險箱）也是把錢拿出抽屜，所以算支出。
            "paid_out" | "drop" => paid_out += amount,
            _ => {}
        }
    }

    // 找零已經在 payments.amount 之外（amount 是沖銷金額，不含找回去的錢），
    // 所以這裡不需要再扣一次。
    let expected = shift.opening_float + cash_sales + paid_in - paid_out;
    Ok(CashSummary {
        opening_float: shift.opening_float,
        cash_sales,
        paid_in,
        paid_out,
        expected,
        counted: shift.counted_cash,
        variance: shift.cash_variance,
    })
}

async fn build_shift_report(uow: &mut SqliteUow, shift: &ShiftView) -> AppResult<ShiftReport> {
    let filter = Filter::shift(&shift.id);
    let sales = sales_totals(uow, &filter).await?;
    let payments = payment_totals(uow, &filter).await?;
    let cash = cash_summary(uow, shift, &payments).await?;
    let voids = void_totals(uow, &shift.business_date).await?;
    Ok(ShiftReport {
        shift_no: shift.shift_no.clone(),
        business_date: shift.business_date.clone(),
        opened_at: shift.opened_at.clone(),
        closed_at: shift.closed_at.clone(),
        sales,
        payments,
        cash,
        voids,
    })
}

/// X 報表：不關班，中途看。
///
/// **需要 `report.daily` 權限**，收銀員預設拿不到 —— 那正是盲盤的前提：
/// 能看到應有現金的人，就不是要數錢的那個人。
pub async fn x_report(ctx: &Ctx) -> AppResult<ShiftReport> {
    rbac::require(&ctx.db, &ctx.actor, PERM_REPORT).await?;
    let mut uow = ctx.db.begin_write().await?;
    let shift = open_shift_row(&mut uow)
        .await?
        .ok_or_else(|| AppError::NotFound("現在沒有開著的班".into()))?;
    let report = build_shift_report(&mut uow, &shift).await?;
    uow.rollback().await?;
    Ok(report)
}

// ---------------------------------------------------------------- 關班

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseShiftReq {
    /// 面額盤點。**先數再看差異**，這是盲盤的整個重點。
    pub counts: Vec<DenomCount>,
    pub note: Option<String>,
}

pub async fn close_shift(ctx: &Ctx, req: CloseShiftReq) -> AppResult<ShiftReport> {
    rbac::require(&ctx.db, &ctx.actor, PERM_CLOSE).await?;
    if req.counts.is_empty() {
        return Err(AppError::Validation(
            "關班要先盤點抽屜裡的現金 —— 這是盲盤的重點，不能跳過".into(),
        ));
    }

    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    let shift = open_shift_row(&mut uow)
        .await?
        .ok_or_else(|| AppError::Conflict("現在沒有開著的班".into()))?;

    save_counts(&mut uow, &shift.id, "close", &req.counts, &now).await?;
    let counted: i64 = req.counts.iter().map(DenomCount::subtotal).sum();

    let mut report = build_shift_report(&mut uow, &shift).await?;
    let expected = report.cash.expected;
    let variance = counted - expected;
    report.cash.counted = Some(counted);
    report.cash.variance = Some(variance);
    report.closed_at = Some(now.iso().to_string());

    let summary = serde_json::to_string(&report)
        .map_err(|e| AppError::Internal(format!("班別摘要序列化失敗：{e}")))?;

    sqlx::query(
        "UPDATE shifts SET status = 'closed', closed_by = ?2, closed_at = ?3,
                           expected_cash = ?4, counted_cash = ?5, cash_variance = ?6,
                           summary_json = ?7, note = COALESCE(?8, note), updated_at = ?3
          WHERE id = ?1",
    )
    .bind(&shift.id)
    .bind(&ctx.actor.user_id)
    .bind(now.iso())
    .bind(expected)
    .bind(counted)
    .bind(variance)
    .bind(&summary)
    .bind(&req.note)
    .execute(uow.conn())
    .await?;

    audit::write_in(
        &mut uow,
        AuditEntry::new("shift", &shift.id, AuditAction::Update)
            // 差異是這筆稽核的重點：短少的金額要查得出是哪一班。
            .amount(variance)
            .on(&shift.business_date),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;

    Ok(report)
}

// ---------------------------------------------------------------- 日結

pub async fn close_business_day(ctx: &Ctx) -> AppResult<DayReport> {
    rbac::require(&ctx.db, &ctx.actor, PERM_Z).await?;

    let now = Stamp::now();
    let (store_id, business_date) = store_id_and_date(ctx, &now).await?;

    let mut uow = ctx.db.begin_write().await?;
    ensure_day_open(&mut uow, &business_date).await?;

    if let Some(open) = open_shift_row(&mut uow).await? {
        return Err(AppError::Conflict(format!(
            "{} 還開著，請先關班再日結。",
            open.shift_no
        )));
    }

    let day_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM business_days WHERE store_id = ?1 AND business_date = ?2",
    )
    .bind(&store_id)
    .bind(&business_date)
    .fetch_optional(uow.conn())
    .await?;
    let Some(day_id) = day_id else {
        return Err(AppError::Conflict(
            "這一天還沒有開過班，沒有東西可以日結。".into(),
        ));
    };

    let filter = Filter::day(&business_date);
    let sales = sales_totals(&mut uow, &filter).await?;
    let payments = payment_totals(&mut uow, &filter).await?;
    let voids = void_totals(&mut uow, &business_date).await?;
    let top_items = top_items(&mut uow, &business_date).await?;

    let sql =
        format!("SELECT {SHIFT_COLS} FROM shifts WHERE business_date = ?1 ORDER BY opened_at");
    let shifts: Vec<ShiftView> = sqlx::query(&sql)
        .bind(&business_date)
        .fetch_all(uow.conn())
        .await?
        .iter()
        .map(row_to_shift)
        .collect();

    let z_report_no = sequence::next_no(
        &mut uow,
        &store_id,
        Scope::Shift,
        &format!("Z{business_date}"),
        &now,
    )
    .await?;

    // ★ 聚合寫進 daily_summaries：報表只讀它，不掃 order_items。
    //   三年後 order_items 上看百萬列，全表掃會愈跑愈慢 ——
    //   而愈忙的店愈慢，剛好是最不能慢的那些店。
    let mut metrics: Vec<(&str, String, String, i64, i64)> = vec![
        (
            "sales.total",
            String::new(),
            String::new(),
            sales.bills,
            sales.total,
        ),
        ("sales.net", String::new(), String::new(), 0, sales.sales),
        ("sales.tax", String::new(), String::new(), 0, sales.tax),
        (
            "sales.discount",
            String::new(),
            String::new(),
            0,
            sales.discount,
        ),
        (
            "sales.service_charge",
            String::new(),
            String::new(),
            0,
            sales.service_charge,
        ),
        (
            "sales.rounding",
            String::new(),
            String::new(),
            0,
            sales.rounding,
        ),
        (
            "void.lines",
            String::new(),
            String::new(),
            voids.voided_lines,
            voids.voided_amount,
        ),
    ];
    for p in &payments {
        metrics.push(("payment", p.code.clone(), p.name.clone(), p.count, p.amount));
    }
    for it in &top_items {
        metrics.push((
            "item",
            it.name.clone(),
            it.name.clone(),
            it.qty_milli,
            it.amount,
        ));
    }
    for s in &shifts {
        metrics.push((
            "shift.variance",
            s.shift_no.clone(),
            s.shift_no.clone(),
            0,
            s.cash_variance.unwrap_or(0),
        ));
    }

    for (metric, key, label, qty, amount) in &metrics {
        sqlx::query(
            "INSERT INTO daily_summaries (id, business_day_id, business_date, metric, dim_key,
                                          dim_label, qty, amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (business_day_id, metric, dim_key)
             DO UPDATE SET qty = excluded.qty, amount = excluded.amount",
        )
        .bind(Id::new().as_str())
        .bind(&day_id)
        .bind(&business_date)
        .bind(metric)
        .bind(key)
        .bind(label)
        .bind(qty)
        .bind(amount)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    let report = DayReport {
        business_date: business_date.clone(),
        z_report_no: z_report_no.clone(),
        closed_at: now.iso().to_string(),
        sales,
        payments,
        voids,
        shifts,
        top_items,
    };
    let summary = serde_json::to_string(&report)
        .map_err(|e| AppError::Internal(format!("日結摘要序列化失敗：{e}")))?;

    sqlx::query(
        "UPDATE business_days SET status = 'closed', closed_at = ?2, closed_by = ?3,
                                  z_report_no = ?4, summary_json = ?5, updated_at = ?2
          WHERE id = ?1",
    )
    .bind(&day_id)
    .bind(now.iso())
    .bind(&ctx.actor.user_id)
    .bind(&z_report_no)
    .bind(&summary)
    .execute(uow.conn())
    .await?;

    audit::write_in(
        &mut uow,
        AuditEntry::new("business_day", &day_id, AuditAction::Update)
            .amount(report.sales.total)
            .on(&business_date),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;

    Ok(report)
}

async fn top_items(uow: &mut SqliteUow, business_date: &str) -> AppResult<Vec<ItemLine>> {
    let rows = sqlx::query(
        "SELECT oi.name_snapshot AS name,
                COALESCE(SUM(oi.qty_milli), 0) AS qty,
                COALESCE(SUM(oi.taxable_amount), 0) AS amount
           FROM order_items oi
           JOIN orders o ON o.id = oi.order_id
          WHERE o.business_date = ?1 AND oi.voided_at IS NULL AND o.status = 'settled'
          GROUP BY oi.name_snapshot
          ORDER BY amount DESC, name
          LIMIT 20",
    )
    .bind(business_date)
    .fetch_all(uow.conn())
    .await?;
    Ok(rows
        .iter()
        .map(|r| ItemLine {
            name: r.get("name"),
            qty_milli: r.get("qty"),
            amount: r.get("amount"),
        })
        .collect())
}

/// 今天的營業狀態，給畫面上的班別列用。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStatus {
    pub business_date: String,
    pub status: String,
    pub shift: Option<ShiftView>,
    pub closed_shifts: i64,
}

pub async fn day_status(ctx: &Ctx) -> AppResult<DayStatus> {
    let now = Stamp::now();
    let (_, business_date) = store_id_and_date(ctx, &now).await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM business_days WHERE business_date = ?1")
            .bind(&business_date)
            .fetch_optional(ctx.db.reader())
            .await?;
    let closed_shifts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM shifts WHERE business_date = ?1 AND status <> 'open'",
    )
    .bind(&business_date)
    .fetch_one(ctx.db.reader())
    .await?;

    Ok(DayStatus {
        business_date,
        status: status.unwrap_or_else(|| "not_started".into()),
        shift: current_shift(ctx).await?,
        closed_shifts,
    })
}
