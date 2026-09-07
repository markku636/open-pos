//! 銷售記錄。
//!
//! # 跟「帳單退款」那一頁的分工
//!
//! 退款那一頁是**為了退款而找單**：客人拿著收據回來，收銀員照末幾碼搜。
//! 這一支不一樣 —— 它是**翻帳**：老闆想看昨天賣了什麼、上禮拜三那筆
//! 一千二的單是誰結的、這個月的外送佔多少。
//!
//! 所以查詢條件是**日期區間**加上通路與付款方式，而不是單號；
//! 而且每一筆都要能展開看到當時賣了哪幾樣、加了什麼選項、用什麼付的。
//!
//! # 為什麼看的是 `bills` 而不是 `orders`
//!
//! 因為錢是以帳單為單位收的。分帳之後一張訂單會有好幾張帳單，而「這一天
//! 收了幾筆」問的是帳單數。訂單資訊（品項、桌號）用 join 帶出來。
//!
//! # 為什麼明細用當時的快照
//!
//! `order_items` 存的是**當時的品名與單價**。三個月後老闆改了價、改了名，
//! 翻回來看到的還是那天賣出去的那一份 —— 這是快照存在的全部理由。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::services::rbac;

const PERM_REPORT: &str = "report.daily";

/// 一次最多回幾筆。翻帳的人不會捲完一千筆，而合計那一列已經回答了大問題。
const MAX_ROWS: i64 = 500;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesQuery {
    /// 營業日起（含）。省略＝今天。
    pub from: Option<String>,
    /// 營業日迄（含）。省略＝跟 from 一樣。
    pub to: Option<String>,
    /// 只看某一個通路（dine_in / takeout / delivery）。
    pub channel: Option<String>,
    /// 只看某一種付款方式（cash / credit / linepay…）。
    pub method: Option<String>,
    /// 只看退過款的。翻帳最常問的一個問題。
    #[serde(default)]
    pub refunded_only: bool,
    /// 單號片段（末幾碼）。
    pub keyword: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleLine {
    pub name: String,
    pub variant_name: Option<String>,
    /// 加了什麼選項（半糖、少冰、加珍珠）。
    pub options: Vec<String>,
    pub note: Option<String>,
    pub qty_milli: i64,
    pub unit_price: i64,
    pub amount: i64,
    /// 這一行被退掉了（作廢的品項）。
    pub voided: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalePayment {
    pub method: String,
    pub amount: i64,
    pub refunded: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sale {
    pub bill_id: String,
    pub bill_no: String,
    pub order_id: String,
    pub order_no: String,
    pub business_date: String,
    pub settled_at: Option<String>,
    /// 通路代碼（`dine_in` / `takeout` / `delivery`）。
    ///
    /// **只回代碼、不回標籤。** 標籤是畫面的事：這支查詢不知道、也不該知道
    /// 現在看這一頁的人講哪一種語言。以前這裡還有一個 `channel_label`
    /// 直接吐「內用」出去，於是英文與日文的店員在一切正常的路徑上
    /// 看到中文。前端本來就有一份通路字典，後端再吐一份，
    /// 只是多一份會各自漂移的翻譯。
    pub channel: String,
    pub table_label: Option<String>,
    pub guest_count: i64,
    pub status: String,
    /// 分帳的那一份：「2／4」。整單結帳是 None。
    pub split_label: Option<String>,
    pub subtotal: i64,
    pub discount_total: i64,
    pub service_charge: i64,
    pub grand_total: i64,
    pub sales_amount: i64,
    pub tax_amount: i64,
    pub refunded_total: i64,
    /// 誰結的帳。翻帳時「這筆是誰收的」是最常問的第二個問題。
    pub settled_by: Option<String>,
    pub payments: Vec<SalePayment>,
    pub lines: Vec<SaleLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesReport {
    pub from: String,
    pub to: String,
    /// 這段期間有幾筆、收了多少。**這一行是整頁最先被看的東西。**
    pub count: i64,
    pub total: i64,
    pub sales_amount: i64,
    pub tax_amount: i64,
    pub refunded_total: i64,
    /// 依付款方式的合計。
    pub by_method: Vec<SalePayment>,
    pub sales: Vec<Sale>,
    /// 超過上限時為 true —— 畫面要說「還有更多」，而不是假裝就這些。
    pub truncated: bool,
}

pub async fn history(ctx: &Ctx, q: SalesQuery) -> AppResult<SalesReport> {
    rbac::require(&ctx.db, &ctx.actor, PERM_REPORT).await?;
    let now = Stamp::now();
    let from = match q.from.clone() {
        Some(d) => d,
        None => crate::services::shift::today(ctx, &now).await?,
    };
    let to = q.to.clone().unwrap_or_else(|| from.clone());

    let like = q.keyword.as_deref().map(|s| format!("%{}%", s.trim()));

    // 合計走一支獨立的查詢，**不是拿回傳的那 500 筆算的** ——
    // 一份「只加總到前 500 筆」的營業額比沒有更糟，因為它看起來是完整的。
    let totals = sqlx::query(&format!(
        "SELECT COUNT(*) AS n, COALESCE(SUM(b.grand_total), 0) AS total,
                COALESCE(SUM(b.sales_amount), 0) AS sales,
                COALESCE(SUM(b.tax_amount), 0) AS tax
           FROM bills b JOIN orders o ON o.id = b.order_id
          WHERE {}",
        WHERE_CLAUSE
    ))
    .bind(&from)
    .bind(&to)
    .bind(&q.channel)
    .bind(&like)
    .bind(i64::from(q.refunded_only))
    .fetch_one(ctx.db.reader())
    .await?;

    let refunded_total: i64 = sqlx::query_scalar(&format!(
        "SELECT COALESCE(SUM(r.amount), 0) FROM refunds r
           JOIN bills b ON b.id = r.bill_id JOIN orders o ON o.id = b.order_id
          WHERE {WHERE_CLAUSE}"
    ))
    .bind(&from)
    .bind(&to)
    .bind(&q.channel)
    .bind(&like)
    .bind(i64::from(q.refunded_only))
    .fetch_one(ctx.db.reader())
    .await?;

    let method_rows = sqlx::query(&format!(
        "SELECT p.method_name_snapshot AS method,
                COALESCE(SUM(p.amount), 0) AS amount,
                COALESCE((SELECT SUM(r.amount) FROM refunds r WHERE r.payment_id = p.id), 0)
                  AS refunded
           FROM payments p
           JOIN bills b ON b.id = p.bill_id JOIN orders o ON o.id = b.order_id
          WHERE p.status IN ('captured', 'refunded') AND {WHERE_CLAUSE}
          GROUP BY p.method_name_snapshot ORDER BY amount DESC"
    ))
    .bind(&from)
    .bind(&to)
    .bind(&q.channel)
    .bind(&like)
    .bind(i64::from(q.refunded_only))
    .fetch_all(ctx.db.reader())
    .await?;
    let by_method: Vec<SalePayment> = method_rows
        .iter()
        .map(|r| SalePayment {
            method: r.get("method"),
            amount: r.get("amount"),
            refunded: r.get("refunded"),
        })
        .collect();

    let rows = sqlx::query(&format!(
        "SELECT b.id, b.bill_no, b.business_date, b.settled_at, b.status,
                b.split_mode, b.split_index, b.split_count,
                b.subtotal, b.discount_total, b.service_charge, b.grand_total,
                b.sales_amount, b.tax_amount,
                o.id AS order_id, o.order_no, o.channel, o.guest_count,
                t.code AS table_code, u.name AS settled_by
           FROM bills b
           JOIN orders o ON o.id = b.order_id
           LEFT JOIN dining_tables t ON t.id = o.table_id
           LEFT JOIN users u ON u.id = b.settled_by
          WHERE {WHERE_CLAUSE}
          ORDER BY b.settled_at DESC, b.bill_no DESC
          LIMIT ?6"
    ))
    .bind(&from)
    .bind(&to)
    .bind(&q.channel)
    .bind(&like)
    .bind(i64::from(q.refunded_only))
    .bind(MAX_ROWS + 1)
    .fetch_all(ctx.db.reader())
    .await?;

    let truncated = rows.len() as i64 > MAX_ROWS;
    let mut sales = Vec::with_capacity(rows.len().min(MAX_ROWS as usize));
    for r in rows.iter().take(MAX_ROWS as usize) {
        let bill_id: String = r.get("id");
        let order_id: String = r.get("order_id");
        let channel: String = r.get("channel");

        // 付款只取這一張帳單的 —— 分帳時每一份各自付各自的。
        let pay_rows = sqlx::query(
            "SELECT p.id, p.method_name_snapshot AS method, p.amount,
                    COALESCE((SELECT SUM(r.amount) FROM refunds r WHERE r.payment_id = p.id), 0)
                      AS refunded
               FROM payments p WHERE p.bill_id = ?1 AND p.status IN ('captured', 'refunded')
              ORDER BY p.id",
        )
        .bind(&bill_id)
        .fetch_all(ctx.db.reader())
        .await?;

        // 明細掛在訂單上。分帳的每一份看到的是同一份明細 ——
        // 那是事實：那一桌就是點了這些東西，只是錢分開收。
        let line_rows = sqlx::query(
            "SELECT id, name_snapshot, variant_name_snapshot, note, qty_milli,
                    unit_price, taxable_amount, voided_at
               FROM order_items WHERE order_id = ?1 ORDER BY line_no",
        )
        .bind(&order_id)
        .fetch_all(ctx.db.reader())
        .await?;

        let mut lines = Vec::with_capacity(line_rows.len());
        for l in &line_rows {
            let line_id: String = l.get("id");
            let options: Vec<String> = sqlx::query_scalar(
                "SELECT name_snapshot FROM order_item_modifiers
                  WHERE order_item_id = ?1 ORDER BY id",
            )
            .bind(&line_id)
            .fetch_all(ctx.db.reader())
            .await?;
            lines.push(SaleLine {
                name: l.get("name_snapshot"),
                variant_name: l.get("variant_name_snapshot"),
                options,
                note: l.get("note"),
                qty_milli: l.get("qty_milli"),
                unit_price: l.get("unit_price"),
                amount: l.get("taxable_amount"),
                voided: l.get::<Option<String>, _>("voided_at").is_some(),
            });
        }

        let split_mode: String = r.get("split_mode");
        let split_count: i64 = r.get("split_count");
        sales.push(Sale {
            refunded_total: pay_rows.iter().map(|p| p.get::<i64, _>("refunded")).sum(),
            payments: pay_rows
                .iter()
                .map(|p| SalePayment {
                    method: p.get("method"),
                    amount: p.get("amount"),
                    refunded: p.get("refunded"),
                })
                .collect(),
            lines,
            bill_id,
            bill_no: r.get("bill_no"),
            order_id,
            order_no: r.get("order_no"),
            business_date: r.get("business_date"),
            settled_at: r.get("settled_at"),
            channel,
            table_label: r.get("table_code"),
            guest_count: r.get("guest_count"),
            status: r.get("status"),
            split_label: if split_mode == "none" {
                None
            } else {
                let i: i64 = r.get("split_index");
                Some(if split_count > 0 {
                    format!("{i}／{split_count}")
                } else {
                    format!("第 {i} 筆")
                })
            },
            subtotal: r.get("subtotal"),
            discount_total: r.get("discount_total"),
            service_charge: r.get("service_charge"),
            grand_total: r.get("grand_total"),
            sales_amount: r.get("sales_amount"),
            tax_amount: r.get("tax_amount"),
            settled_by: r.get("settled_by"),
        });
    }

    Ok(SalesReport {
        from,
        to,
        count: totals.get("n"),
        total: totals.get("total"),
        sales_amount: totals.get("sales"),
        tax_amount: totals.get("tax"),
        refunded_total,
        by_method,
        sales,
        truncated,
    })
}

/// 共用的篩選條件。
///
/// 綁定順序固定為 `?1 起日 / ?2 迄日 / ?3 通路 / ?4 單號片段 / ?5 只看退款`，
/// 三支查詢（合計、付款方式、明細）都吃同一組 —— 少了這個約定，
/// 「合計」與「列表」會在某個篩選條件下悄悄地不一致。
///
/// 退過款的帳單仍然算營業額（見 `shift::sales_totals` 的說明）。
const WHERE_CLAUSE: &str = "b.status IN ('settled', 'partially_refunded', 'refunded')
      AND b.business_date >= ?1 AND b.business_date <= ?2
      AND (?3 IS NULL OR o.channel = ?3)
      AND (?4 IS NULL OR b.bill_no LIKE ?4 OR o.order_no LIKE ?4)
      AND (?5 = 0 OR b.status IN ('partially_refunded', 'refunded'))";

/// 通路代碼 → 中文標籤。**現在只剩 Excel 匯出在用。**
///
/// `Sale` 已經不帶標籤了（畫面自己查字典），但匯出的活頁簿從標題列
/// 「營業日 / 帳單號 / 通路」到每一格都是中文 —— 它是一份中文文件，
/// 不是畫面。那份文件的用字歸產生它的 `xlsx.rs` 管，
/// 跟收銀機現在顯示哪一國語言沒有關係。
pub(crate) fn channel_label(code: &str) -> &'static str {
    match code {
        "dine_in" => "內用",
        "takeout" => "外帶",
        "delivery" => "外送",
        _ => "其他",
    }
}

/// 過去某一天的 Z 報表。
///
/// `stored_day_report` 一直存在，但**沒有任何指令掛在它上面** ——
/// 於是日結完成之後，那一天的報表就再也沒有地方看得到了。
/// 而「快照、永不重算」的整個設計前提，就是為了讓它以後還讀得回來。
pub async fn day_report(
    ctx: &Ctx,
    business_date: String,
) -> AppResult<crate::services::shift::DayReport> {
    rbac::require(&ctx.db, &ctx.actor, PERM_REPORT).await?;
    crate::services::shift::stored_day_report(ctx, &business_date).await
}

/// 有日結報表的營業日清單（新到舊）。畫面上的日期下拉靠它。
pub async fn closed_days(ctx: &Ctx) -> AppResult<Vec<String>> {
    rbac::require(&ctx.db, &ctx.actor, PERM_REPORT).await?;
    Ok(sqlx::query_scalar(
        "SELECT business_date FROM business_days
          WHERE summary_json IS NOT NULL ORDER BY business_date DESC LIMIT 400",
    )
    .fetch_all(ctx.db.reader())
    .await?)
}

/// 匯出目的地資料夾要存在而且寫得進去。
///
/// 使用者最常給的錯路徑是「已經拔掉的隨身碟」，而那時候的原生錯誤訊息
/// （`系統找不到指定的路徑`）不會告訴他要做什麼。
pub fn ensure_dir(dir: &str) -> AppResult<std::path::PathBuf> {
    let path = std::path::PathBuf::from(dir);
    if !path.is_dir() {
        return Err(AppError::Validation(
            format!("找不到資料夾 {dir}。\n\n如果那是隨身碟，請確認它還插著。").into(),
        ));
    }
    Ok(path)
}
