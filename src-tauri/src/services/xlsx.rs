//! Excel 匯出。
//!
//! # 為什麼不是只有 CSV
//!
//! CSV 已經有了，而且它在「丟給會計」這件事上完全夠用。但老闆自己看的時候
//! 會遇到三個問題，而三個都不是 CSV 能解的：
//!
//! 1. **金額變成文字。** Excel 開 CSV 時 `1,200` 會被當字串，加總出來是 0。
//!    這裡寫的是**真的數字**，選起來左下角就有總和。
//! 2. **一個檔只能放一張表。** 一份日報表其實是四五張表（銷售、付款方式、
//!    退款、品項排行、各班差異），CSV 只能全部疊在一欄裡用「類型」欄分。
//! 3. **欄寬與凍結。** 一份要捲一百列的表，沒有凍結標題列就沒有人看得完。
//!
//! # 純 Rust
//!
//! `rust_xlsxwriter` 不帶 default features（`zlib` 那個 feature 會拉進
//! `libz-sys`，那是 C 依賴）。全案「絕不引入 C 相依」的原則在這裡一樣適用 ——
//! 一個開源專案最不該做的事，就是讓人 clone 下來之後建不起來。

use rust_xlsxwriter::{Format, FormatAlign, FormatBorder, Workbook, Worksheet};

use crate::error::{AppError, AppResult};
use crate::services::sales::{channel_label, SalesReport};
use crate::services::shift::DayReport;

/// 一整份日報表。回傳寫出去的檔案路徑。
pub fn write_day_report(report: &DayReport, dir: &std::path::Path) -> AppResult<String> {
    let mut book = Workbook::new();
    let s = Styles::new();

    // ── 總覽 ─────────────────────────────────────────────
    {
        let sheet = book.add_worksheet();
        sheet
            .set_name("日報表")
            .map_err(|e| AppError::Internal(format!("Excel 分頁命名失敗：{e}")))?;
        title(sheet, &s, &format!("{} 日結報表", report.business_date))?;
        let mut row = 2;
        kv(sheet, &s, &mut row, "Z 報表編號", &report.z_report_no)?;
        kv(sheet, &s, &mut row, "日結時間", &report.closed_at)?;
        row += 1;

        section(sheet, &s, &mut row, "銷售")?;
        money_row(sheet, &s, &mut row, "帳單數", report.sales.bills)?;
        money_row(sheet, &s, &mut row, "銷售總額", report.sales.total)?;
        money_row(sheet, &s, &mut row, "未稅", report.sales.sales)?;
        money_row(sheet, &s, &mut row, "稅額", report.sales.tax)?;
        money_row(sheet, &s, &mut row, "折扣", -report.sales.discount)?;
        money_row(sheet, &s, &mut row, "服務費", report.sales.service_charge)?;
        row += 1;

        section(sheet, &s, &mut row, "退款")?;
        money_row(sheet, &s, &mut row, "筆數", report.refunds.count)?;
        money_row(sheet, &s, &mut row, "金額", -report.refunds.amount)?;
        money_row(sheet, &s, &mut row, "其中現金", -report.refunds.cash_amount)?;
        row += 1;

        section(sheet, &s, &mut row, "作廢")?;
        money_row(sheet, &s, &mut row, "退掉的品項", report.voids.voided_lines)?;
        money_row(sheet, &s, &mut row, "金額", -report.voids.voided_amount)?;

        sheet.set_column_width(0, 22).ok();
        sheet.set_column_width(1, 18).ok();
    }

    // ── 付款方式 ─────────────────────────────────────────
    sheet_table(
        &mut book,
        &s,
        "付款方式",
        &["方式", "筆數", "金額"],
        report.payments.iter().map(|p| {
            vec![
                Cell::Text(p.name.clone()),
                Cell::Num(p.count),
                Cell::Money(p.amount),
            ]
        }),
    )?;

    // ── 品項排行 ─────────────────────────────────────────
    sheet_table(
        &mut book,
        &s,
        "品項排行",
        &["品項", "數量", "金額"],
        report.top_items.iter().map(|i| {
            vec![
                Cell::Text(i.name.clone()),
                // 數量是千分之一刻度，看報表的人不需要知道內部單位。
                Cell::Float(i.qty_milli as f64 / 1000.0),
                Cell::Money(i.amount),
            ]
        }),
    )?;

    // ── 各班現金差異 ─────────────────────────────────────
    sheet_table(
        &mut book,
        &s,
        "各班現金",
        &["班別", "應有現金", "實際盤點", "差異"],
        report.shifts.iter().map(|sh| {
            vec![
                Cell::Text(sh.shift_no.clone()),
                Cell::Money(sh.expected_cash.unwrap_or(0)),
                Cell::Money(sh.counted_cash.unwrap_or(0)),
                Cell::Money(sh.cash_variance.unwrap_or(0)),
            ]
        }),
    )?;

    save(book, dir, &format!("日報表-{}.xlsx", report.business_date))
}

/// 銷售記錄。兩張表：一張一筆帳單一列，一張一個品項一列。
///
/// 分成兩張是因為它們回答不同的問題：「這一天收了幾筆、每筆多少」跟
/// 「這一天賣了幾份珍奶」。把明細塞進同一張表會讓帳單那一列重複 N 次，
/// 而那樣的表在 Excel 裡加總金額會多算好幾倍。
pub fn write_sales(report: &SalesReport, dir: &std::path::Path) -> AppResult<String> {
    let mut book = Workbook::new();
    let s = Styles::new();

    sheet_table(
        &mut book,
        &s,
        "帳單",
        &[
            "營業日",
            "結帳時間",
            "帳單號",
            "訂單號",
            "通路",
            "桌號",
            "分帳",
            "人數",
            "小計",
            "折扣",
            "服務費",
            "總計",
            "未稅",
            "稅額",
            "已退",
            "付款方式",
            "結帳人",
        ],
        report.sales.iter().map(|x| {
            vec![
                Cell::Text(x.business_date.clone()),
                Cell::Text(hhmm(x.settled_at.as_deref())),
                Cell::Text(x.bill_no.clone()),
                Cell::Text(x.order_no.clone()),
                // `Sale` 現在只帶通路代碼（標籤是畫面的事）。這份活頁簿整張是
                // 中文文件，所以在這裡把代碼轉成中文，而不是印 `dine_in` 給老闆看。
                Cell::Text(channel_label(&x.channel).into()),
                Cell::Text(x.table_label.clone().unwrap_or_default()),
                Cell::Text(x.split_label.clone().unwrap_or_default()),
                Cell::Num(x.guest_count),
                Cell::Money(x.subtotal),
                Cell::Money(-x.discount_total),
                Cell::Money(x.service_charge),
                Cell::Money(x.grand_total),
                Cell::Money(x.sales_amount),
                Cell::Money(x.tax_amount),
                Cell::Money(-x.refunded_total),
                Cell::Text(
                    x.payments
                        .iter()
                        .map(|p| p.method.as_str())
                        .collect::<Vec<_>>()
                        .join("、"),
                ),
                Cell::Text(x.settled_by.clone().unwrap_or_default()),
            ]
        }),
    )?;

    sheet_table(
        &mut book,
        &s,
        "品項明細",
        &[
            "營業日",
            "帳單號",
            "品項",
            "規格",
            "選項",
            "備註",
            "數量",
            "單價",
            "金額",
            "已退掉",
        ],
        report.sales.iter().flat_map(|x| {
            x.lines.iter().map(move |l| {
                vec![
                    Cell::Text(x.business_date.clone()),
                    Cell::Text(x.bill_no.clone()),
                    Cell::Text(l.name.clone()),
                    Cell::Text(l.variant_name.clone().unwrap_or_default()),
                    Cell::Text(l.options.join("、")),
                    Cell::Text(l.note.clone().unwrap_or_default()),
                    Cell::Float(l.qty_milli as f64 / 1000.0),
                    Cell::Money(l.unit_price),
                    Cell::Money(l.amount),
                    Cell::Text(if l.voided {
                        "是".into()
                    } else {
                        String::new()
                    }),
                ]
            })
        }),
    )?;

    sheet_table(
        &mut book,
        &s,
        "付款方式",
        &["方式", "金額", "已退"],
        report.by_method.iter().map(|p| {
            vec![
                Cell::Text(p.method.clone()),
                Cell::Money(p.amount),
                Cell::Money(-p.refunded),
            ]
        }),
    )?;

    save(
        book,
        dir,
        &format!("銷售記錄-{}_{}.xlsx", report.from, report.to),
    )
}

// ---------------------------------------------------------------- 內部

enum Cell {
    Text(String),
    Num(i64),
    /// 金額。寫成數字並套千分位格式 —— 選起來 Excel 左下角就有總和。
    Money(i64),
    Float(f64),
}

struct Styles {
    title: Format,
    header: Format,
    label: Format,
    money: Format,
}

impl Styles {
    fn new() -> Self {
        Self {
            title: Format::new().set_bold().set_font_size(14),
            header: Format::new()
                .set_bold()
                .set_background_color(0xE8EEF7)
                .set_border(FormatBorder::Thin),
            label: Format::new().set_bold(),
            // 負數用括號是會計慣例，而報表上的負數幾乎都是「錢出去了」。
            money: Format::new()
                .set_num_format("#,##0;(#,##0)")
                .set_align(FormatAlign::Right),
        }
    }
}

fn title(sheet: &mut Worksheet, s: &Styles, text: &str) -> AppResult<()> {
    sheet
        .write_string_with_format(0, 0, text, &s.title)
        .map_err(xl)?;
    Ok(())
}

fn section(sheet: &mut Worksheet, s: &Styles, row: &mut u32, text: &str) -> AppResult<()> {
    sheet
        .write_string_with_format(*row, 0, text, &s.header)
        .map_err(xl)?;
    sheet.write_blank(*row, 1, &s.header).map_err(xl)?;
    *row += 1;
    Ok(())
}

fn kv(sheet: &mut Worksheet, s: &Styles, row: &mut u32, k: &str, v: &str) -> AppResult<()> {
    sheet
        .write_string_with_format(*row, 0, k, &s.label)
        .map_err(xl)?;
    sheet.write_string(*row, 1, v).map_err(xl)?;
    *row += 1;
    Ok(())
}

fn money_row(sheet: &mut Worksheet, s: &Styles, row: &mut u32, k: &str, v: i64) -> AppResult<()> {
    sheet.write_string(*row, 0, k).map_err(xl)?;
    sheet
        .write_number_with_format(*row, 1, v as f64, &s.money)
        .map_err(xl)?;
    *row += 1;
    Ok(())
}

fn sheet_table<I>(
    book: &mut Workbook,
    s: &Styles,
    name: &str,
    headers: &[&str],
    rows: I,
) -> AppResult<()>
where
    I: Iterator<Item = Vec<Cell>>,
{
    let sheet = book.add_worksheet();
    sheet.set_name(name).map_err(xl)?;
    for (c, h) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(0, c as u16, *h, &s.header)
            .map_err(xl)?;
        // 中文字在 Excel 裡約佔兩個字元寬，欄寬抓寬一點總比被截斷好。
        sheet
            .set_column_width(c as u16, (h.chars().count() as f64 * 2.4).max(9.0))
            .ok();
    }
    let mut r = 1u32;
    for line in rows {
        for (c, cell) in line.iter().enumerate() {
            let c = c as u16;
            match cell {
                Cell::Text(t) => sheet.write_string(r, c, t).map(|_| ()).map_err(xl)?,
                Cell::Num(n) => sheet
                    .write_number(r, c, *n as f64)
                    .map(|_| ())
                    .map_err(xl)?,
                Cell::Money(n) => sheet
                    .write_number_with_format(r, c, *n as f64, &s.money)
                    .map(|_| ())
                    .map_err(xl)?,
                Cell::Float(f) => sheet.write_number(r, c, *f).map(|_| ()).map_err(xl)?,
            }
        }
        r += 1;
    }
    // 凍結標題列：一份要捲一百列的表，沒有它就沒有人看得完。
    sheet.set_freeze_panes(1, 0).ok();
    if r > 1 {
        sheet.autofilter(0, 0, r - 1, headers.len() as u16 - 1).ok();
    }
    Ok(())
}

fn save(mut book: Workbook, dir: &std::path::Path, filename: &str) -> AppResult<String> {
    let path = dir.join(filename);
    book.save(&path)
        .map_err(|e| AppError::Storage(format!("寫不出 Excel：{e}")))?;
    Ok(path.display().to_string())
}

fn xl(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Internal(format!("Excel 產生失敗：{e}"))
}

/// ISO 時間裡的時分。報表上的時間欄不需要日期（同一欄已經有營業日）。
fn hhmm(iso: Option<&str>) -> String {
    iso.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| {
            t.with_timezone(&chrono_tz::Asia::Taipei)
                .format("%H:%M")
                .to_string()
        })
        .unwrap_or_default()
}
