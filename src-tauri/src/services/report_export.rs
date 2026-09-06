//! 報表匯出（CSV）。
//!
//! # 為什麼是 CSV 而不是 xlsx
//!
//! 因為收到這個檔案的人是**記帳的那一位**，而她要的是「打開、看一眼、
//! 貼進自己的表」。CSV 到處都打得開，也不必為了它引入一個 Excel 產生器。
//!
//! # 兩個非做不可的細節
//!
//! 1. **UTF-8 BOM**。沒有 BOM 的話，Windows 的 Excel 會用系統 ANSI 碼頁解讀，
//!    中文全部變亂碼 —— 而這是最常見的「你們的匯出壞掉了」。
//! 2. **CRLF 換行**。舊版 Excel 對純 LF 的處理不一致，有時整份擠成一行。
//!
//! # 金額不加千分位
//!
//! `1,234` 在 Excel 裡是**文字**，不是數字 —— 貼進去之後不能加總，
//! 而她要做的第一件事就是加總。

use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::services::rbac;
use crate::services::shift::DayReport;

const PERM_EXPORT: &str = "report.export";

/// CSV 的一格。逗號、引號、換行都要處理，否則品名裡的一個逗號會讓整份錯位。
fn cell(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn row(cells: &[String]) -> String {
    // CRLF：舊版 Excel 對純 LF 的處理不一致，有時整份擠成一行。
    format!("{}\r\n", cells.join(","))
}

/// 把日結報表變成一份 CSV。
pub fn day_report_csv(report: &DayReport) -> String {
    let mut out = String::new();
    let r = |cells: Vec<String>| row(&cells);

    out.push_str(&r(vec![
        cell("項目"),
        cell("說明"),
        cell("數量"),
        cell("金額"),
    ]));

    let s = &report.sales;
    for (label, qty, amount) in [
        ("帳單數", s.bills, s.total),
        ("未稅銷售額", 0, s.sales),
        ("稅額", 0, s.tax),
        ("折扣", 0, -s.discount),
        ("服務費", 0, s.service_charge),
        ("進位調整", 0, s.rounding),
    ] {
        out.push_str(&r(vec![
            cell("銷售"),
            cell(label),
            qty.to_string(),
            amount.to_string(),
        ]));
    }

    for p in &report.payments {
        out.push_str(&r(vec![
            cell("收款"),
            cell(&p.name),
            p.count.to_string(),
            p.amount.to_string(),
        ]));
    }

    out.push_str(&r(vec![
        cell("作廢"),
        cell("退掉的品項"),
        report.voids.voided_lines.to_string(),
        (-report.voids.voided_amount).to_string(),
    ]));

    for sh in &report.shifts {
        out.push_str(&r(vec![
            cell("班別現金差異"),
            cell(&sh.shift_no),
            "0".into(),
            sh.cash_variance.unwrap_or(0).to_string(),
        ]));
    }

    for i in &report.top_items {
        out.push_str(&r(vec![
            cell("品項"),
            cell(&i.name),
            // 數量用「份」而不是千分之一：看報表的人不需要知道內部刻度。
            format!("{:.3}", i.qty_milli as f64 / 1000.0)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string(),
            i.amount.to_string(),
        ]));
    }

    out
}

/// 匯出到檔案，回傳完整路徑。
pub async fn export_day_csv(ctx: &Ctx, report: &DayReport, dir: String) -> AppResult<String> {
    rbac::require(&ctx.db, &ctx.actor, PERM_EXPORT).await?;

    let dir = std::path::PathBuf::from(dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Storage(format!("建不出資料夾：{e}")))?;
    let path = dir.join(format!("open-pos-日結-{}.csv", report.business_date));

    // ★ UTF-8 BOM。沒有它，Windows 的 Excel 會用系統 ANSI 碼頁解讀，
    //   中文全部變亂碼 —— 這是最常見的「你們的匯出壞掉了」。
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(day_report_csv(report).as_bytes());
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| AppError::Storage(format!("寫不出 CSV：{e}")))?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::shift::{ItemLine, PaymentTotal, SalesTotals, VoidTotals};

    fn sample() -> DayReport {
        DayReport {
            business_date: "2026-09-06".into(),
            z_report_no: "Z-20260906-0001".into(),
            closed_at: "2026-09-06T14:00:00.000Z".into(),
            sales: SalesTotals {
                bills: 3,
                subtotal: 215,
                discount: 0,
                service_charge: 0,
                rounding: 0,
                sales: 204,
                tax: 11,
                total: 215,
            },
            payments: vec![PaymentTotal {
                code: "cash".into(),
                name: "現金".into(),
                count: 3,
                amount: 215,
            }],
            voids: VoidTotals::default(),
            shifts: vec![],
            top_items: vec![ItemLine {
                // 品名裡的逗號是真實情況（「A餐, 附湯」），不處理會讓整份錯位。
                name: "招牌套餐, 附湯".into(),
                qty_milli: 2500,
                amount: 180,
            }],
        }
    }

    #[test]
    fn amounts_are_plain_numbers_so_excel_can_sum_them() {
        let csv = day_report_csv(&sample());
        // 千分位會讓 Excel 把它當成文字，而看報表的人第一件事就是加總。
        assert!(!csv.contains("1,234"));
        assert!(csv.contains(",215\r\n"), "{csv}");
        assert!(!csv.contains('$'), "不要在數字欄放貨幣符號");
    }

    #[test]
    fn a_comma_in_an_item_name_does_not_shift_the_columns() {
        let csv = day_report_csv(&sample());
        assert!(csv.contains("\"招牌套餐, 附湯\""), "{csv}");
        // 每一行的欄位數都要一樣，否則 Excel 會把後面的欄位往左推。
        for line in csv.lines().filter(|l| !l.is_empty()) {
            let mut fields = 1;
            let mut in_quotes = false;
            for c in line.chars() {
                match c {
                    '"' => in_quotes = !in_quotes,
                    ',' if !in_quotes => fields += 1,
                    _ => {}
                }
            }
            assert_eq!(fields, 4, "欄位數不對：{line}");
        }
    }

    #[test]
    fn lines_end_with_crlf() {
        // 舊版 Excel 對純 LF 的處理不一致，有時整份擠成一行。
        let csv = day_report_csv(&sample());
        assert!(csv.contains("\r\n"));
        assert!(!csv.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn quantities_are_in_servings_not_thousandths() {
        // 看報表的人不需要知道內部刻度。
        let csv = day_report_csv(&sample());
        assert!(csv.contains(",2.5,"), "{csv}");
        assert!(!csv.contains("2500"), "{csv}");
    }
}
