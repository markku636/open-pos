//! 一段期間的營運分析。
//!
//! # 跟 Z 報表的分工
//!
//! Z 報表是**當天算好、永不重算的快照**，它要回答的是「這一天收了多少、
//! 對不對得起來」—— 那是稅務與交接的問題，數字一旦定了就不能再動。
//!
//! 這一頁不一樣：它是**現算的**，要回答的是老闆的問題 ——
//! 「哪個時段最忙」「這個月折掉多少」「什麼賣得最好」。現算才能任意選區間，
//! 而且這些數字改變主意也不會有人受傷。
//!
//! # 時段是用店家的時區分的
//!
//! `settled_at` 存 UTC，SQLite 沒有時區轉換。所以這裡把當期的帳單撈回 Rust
//! 再用 `chrono-tz` 分桶 —— 一天撐死幾百張帳單，成本可以忽略。
//! 直接拿 UTC 的小時去分，台灣的午餐尖峰會落在「凌晨 4 點」。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::ctx::Ctx;
use crate::error::AppResult;
use crate::services::rbac;

const PERM_REPORT: &str = "report.daily";

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InsightQuery {
    /// 營業日起（含）。省略＝今天。
    pub from: Option<String>,
    /// 營業日迄（含）。省略＝跟 from 一樣。
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourBucket {
    /// 0–23，店家時區。
    pub hour: i64,
    pub bills: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedTotal {
    pub label: String,
    pub count: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub from: String,
    pub to: String,
    pub bills: i64,
    pub total: i64,
    /// 平均客單價。**營業額除以帳單數，不是除以人數** ——
    /// 分帳會把一桌拆成好幾張帳單，所以這個數字在分帳多的店會偏低，
    /// 而那正是它該反映的事實：櫃檯每收一次錢平均收多少。
    pub average_bill: i64,
    /// 每一個小時（店家時區）。沒有生意的小時不列。
    pub hours: Vec<HourBucket>,
    /// 折扣與招待，依名稱分組。金額是正數（折掉多少）。
    pub discounts: Vec<NamedTotal>,
    /// 作廢的品項，依原因分組。「退點」與「整單作廢」分開列 ——
    /// 前者是日常，後者每一次都值得看一眼。
    pub voids: Vec<NamedTotal>,
    /// 品項排行。
    pub items: Vec<NamedTotal>,
    /// 內用 / 外帶 / 外送各佔多少。
    pub channels: Vec<NamedTotal>,
}

/// 帳單狀態：退過款的仍然算營業額（見 `shift::sales_totals` 的說明）。
const COUNTED: &str = "('settled', 'partially_refunded', 'refunded')";

pub async fn insight(ctx: &Ctx, q: InsightQuery) -> AppResult<Insight> {
    rbac::require(&ctx.db, &ctx.actor, PERM_REPORT).await?;
    let now = Stamp::now();
    let from = match q.from {
        Some(d) => d,
        None => crate::services::shift::today(ctx, &now).await?,
    };
    let to = q.to.unwrap_or_else(|| from.clone());

    let tz: String = sqlx::query_scalar("SELECT tz FROM stores WHERE deleted_at IS NULL LIMIT 1")
        .fetch_optional(ctx.db.reader())
        .await?
        .unwrap_or_else(|| "Asia/Taipei".into());
    let tz: chrono_tz::Tz = tz.parse().unwrap_or(chrono_tz::Asia::Taipei);

    // ── 帳單：總額、張數、時段分布 ────────────────────────────
    let bills = sqlx::query(&format!(
        "SELECT settled_at, grand_total FROM bills
          WHERE business_date >= ?1 AND business_date <= ?2 AND status IN {COUNTED}"
    ))
    .bind(&from)
    .bind(&to)
    .fetch_all(ctx.db.reader())
    .await?;

    let mut buckets = [(0i64, 0i64); 24];
    let mut total = 0i64;
    for b in &bills {
        let amount: i64 = b.get("grand_total");
        total += amount;
        let settled: Option<String> = b.get("settled_at");
        if let Some(h) = settled.as_deref().and_then(|s| local_hour(s, tz)) {
            buckets[h as usize].0 += 1;
            buckets[h as usize].1 += amount;
        }
    }
    let hours: Vec<HourBucket> = buckets
        .iter()
        .enumerate()
        .filter(|(_, (n, _))| *n > 0)
        .map(|(h, (n, amount))| HourBucket {
            hour: h as i64,
            bills: *n,
            total: *amount,
        })
        .collect();

    let count = bills.len() as i64;

    // ── 折扣與招待 ───────────────────────────────────────────
    // 招待（comp）與一般折扣分開看：招待是送出去的東西，折扣是少收的錢，
    // 而老闆對這兩件事的容忍度完全不同。
    let discounts = named_totals(
        ctx,
        "SELECT CASE WHEN d.type_snapshot = 'comp' THEN '招待：' ELSE '折扣：' END
                || COALESCE(r.name, d.name_snapshot) AS label,
                COUNT(*) AS n, COALESCE(SUM(d.amount), 0) AS amount
           FROM order_discounts d
           JOIN orders o ON o.id = d.order_id
           LEFT JOIN reason_codes r ON r.id = d.reason_id
          WHERE o.business_date >= ?1 AND o.business_date <= ?2 AND o.status = 'settled'
          GROUP BY label ORDER BY amount DESC",
        &from,
        &to,
    )
    .await?;

    let voids = named_totals(
        ctx,
        // 「點錯退掉一項」與「整張單作廢」是兩件很不一樣的事：前者是日常，
        // 後者每一次都值得看一眼（尤其是結完帳之後作廢的）。分開列。
        "SELECT CASE WHEN o.status = 'voided' THEN '整單作廢：' ELSE '退點：' END
                || COALESCE(r.name, '未填原因') AS label, COUNT(*) AS n,
                COALESCE(SUM(oi.unit_price * oi.qty_milli / 1000), 0) AS amount
           FROM order_items oi
           JOIN orders o ON o.id = oi.order_id
           LEFT JOIN reason_codes r ON r.id = oi.void_reason_id
          WHERE o.business_date >= ?1 AND o.business_date <= ?2 AND oi.voided_at IS NOT NULL
          GROUP BY label ORDER BY amount DESC",
        &from,
        &to,
    )
    .await?;

    let items = named_totals(
        ctx,
        "SELECT oi.name_snapshot AS label, COALESCE(SUM(oi.qty_milli) / 1000, 0) AS n,
                COALESCE(SUM(oi.taxable_amount), 0) AS amount
           FROM order_items oi
           JOIN orders o ON o.id = oi.order_id
          WHERE o.business_date >= ?1 AND o.business_date <= ?2
            AND oi.voided_at IS NULL AND o.status = 'settled'
          GROUP BY label ORDER BY amount DESC LIMIT 30",
        &from,
        &to,
    )
    .await?;

    let channels = named_totals(
        ctx,
        "SELECT CASE o.channel WHEN 'dine_in' THEN '內用' WHEN 'takeout' THEN '外帶'
                               WHEN 'delivery' THEN '外送' ELSE o.channel END AS label,
                COUNT(*) AS n, COALESCE(SUM(o.grand_total), 0) AS amount
           FROM orders o
          WHERE o.business_date >= ?1 AND o.business_date <= ?2 AND o.status = 'settled'
          GROUP BY label ORDER BY amount DESC",
        &from,
        &to,
    )
    .await?;

    Ok(Insight {
        from,
        to,
        bills: count,
        total,
        average_bill: if count > 0 { total / count } else { 0 },
        hours,
        discounts,
        voids,
        items,
        channels,
    })
}

async fn named_totals(ctx: &Ctx, sql: &str, from: &str, to: &str) -> AppResult<Vec<NamedTotal>> {
    let rows = sqlx::query(sql)
        .bind(from)
        .bind(to)
        .fetch_all(ctx.db.reader())
        .await?;
    Ok(rows
        .iter()
        .map(|r| NamedTotal {
            label: r.get("label"),
            count: r.get("n"),
            amount: r.get("amount"),
        })
        .collect())
}

/// UTC 的 ISO 字串在店家時區裡是幾點。
///
/// 直接砍字串取小時會得到 UTC 的小時 —— 台灣的午餐尖峰會落在「凌晨 4 點」，
/// 而看到那張圖的人會以為系統壞了。
fn local_hour(iso: &str, tz: chrono_tz::Tz) -> Option<i64> {
    use chrono::Timelike;
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|t| t.with_timezone(&tz).hour() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_are_bucketed_in_the_store_timezone() {
        // 台北的中午十二點是 UTC 04:00。
        assert_eq!(
            local_hour("2026-09-06T04:00:00.000Z", chrono_tz::Asia::Taipei),
            Some(12)
        );
        // 跨日：台北 01:00 是前一天的 UTC 17:00。
        assert_eq!(
            local_hour("2026-09-05T17:30:00.000Z", chrono_tz::Asia::Taipei),
            Some(1)
        );
    }

    #[test]
    fn a_broken_timestamp_does_not_panic() {
        assert_eq!(local_hour("not a time", chrono_tz::Asia::Taipei), None);
    }
}
