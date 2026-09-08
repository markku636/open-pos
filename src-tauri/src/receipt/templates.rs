//! 版型：把一張單變成 `ReceiptDoc`。
//!
//! 這裡是**純函式**，吃的是一個與資料庫無關的 `TicketData`。
//! 這樣做的兩個理由：
//!
//! 1. 版型是最需要反覆微調的東西（欄寬、要不要印 SKU、備註放哪），
//!    而每次微調都要能用快照測試一眼驗收。純函式才辦得到。
//! 2. 補印時是拿**存起來的 doc** 重送，不重跑業務邏輯。版型與查詢分離之後，
//!    「產生 doc」與「送去印」就自然是兩件事。

use serde::{Deserialize, Serialize};

use crate::receipt::{Cell, Finish, PaperWidth, ReceiptDoc, TextStyle};

/// 出單原因。**加點與退點必須是獨立原因**，這是餐飲 POS 最常漏的需求：
/// 它們在廚房是完全不同的動作，混在一起印會讓廚師重做整桌。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketReason {
    NewOrder,
    AddItems,
    Void,
    Reprint,
    Settle,
}

impl TicketReason {
    fn banner(self) -> Option<&'static str> {
        match self {
            Self::NewOrder => None,
            Self::AddItems => Some("※ 加點 ※"),
            Self::Void => Some("※ 取消 ※"),
            Self::Reprint => Some("※ 補印 ※"),
            Self::Settle => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TicketLine {
    /// 品名。用 `short_name` 優先（58mm 一行只放得下約 9 個中文字）。
    pub name: String,
    /// 加購與客製（去冰、半糖、不要香菜）。
    pub options: Vec<String>,
    pub note: Option<String>,
    /// 數量 × 1000。
    pub qty_milli: i64,
    /// 這一行的金額（整數元）。廚房單不印金額，收據才印。
    pub amount: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaymentLine {
    pub method: String,
    pub amount: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TicketData {
    pub store_name: String,
    pub order_no: String,
    /// 「內用 A3」「外帶」。
    pub channel_label: String,
    pub table_label: Option<String>,
    /// 已格式化的時間（版型層不碰時鐘）。
    pub printed_at: String,
    pub lines: Vec<TicketLine>,
    pub subtotal: i64,
    pub discount_total: i64,
    pub service_charge: i64,
    pub rounding_adjustment: i64,
    pub grand_total: i64,
    pub sales_amount: i64,
    pub tax_amount: i64,
    pub payments: Vec<PaymentLine>,
    pub change: i64,
    /// 出單分區名稱（廚房單用）。
    pub station: Option<String>,
    pub reason: TicketReason,
    /// 重印次數。> 0 時要印在單上 —— 重印收據可以拿去做假帳，
    /// 所以必須讓拿到單的人看得出這是第幾次印。
    pub reprint_seq: u32,
    /// 分帳時的那一段。整單結帳是 None。
    pub split: Option<SplitLabel>,
}

/// 分帳的收據要多印的東西。
///
/// **「還差多少」是這裡最重要的一個數字。** 分帳最常見的意外是「以為收完了」
/// —— 三個人各付各的，第三個人走掉了而沒有人發現。所以只要還沒收完，
/// 收據上就要印出來，而且要印得很明顯。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitLabel {
    pub index: i64,
    /// 總共幾份。按金額分帳時要收到最後一筆才知道。
    pub count: Option<i64>,
    /// 這一份收多少。
    pub part_total: i64,
    /// 整張單多少。
    pub order_total: i64,
    /// 收完這一份之後還差多少。
    pub remaining: i64,
}

/// 把一模一樣的行合併成一行。
///
/// # 為什麼
///
/// 收銀員在尖峰時間是**連點四下**，不是去改數量欄位。於是四個人的吃到飽會變成
/// 四行一模一樣的「晚餐吃到飽（大人）」，收據長一倍、客人數不清楚自己被算了幾份。
/// Clover 的「Group identical items into one line」預設就是開的，
/// 我們照做（見 `docs/dining-modes.md` 第七節）。
///
/// # 只在版型層做，不動 `order_items`
///
/// 資料庫那邊四行就是四行 —— 那是「誰在幾點點了什麼」的真相，
/// 也是分項分帳挑得動單一品項的前提。合併是**列印時的呈現**，
/// 不是資料的改寫。Clover 把它做成一個列印選項，理由應該也是同一個。
///
/// # 合併的條件嚴格到幾乎吹毛求疵
///
/// 品名、選項、備註三者**全部**相同才合併。差一個字都不併：
/// 「珍奶（去冰）」與「珍奶（正常冰）」是兩杯不同的飲料，
/// 而「不要香菜」是廚房會出事的那一行。寧可多印一行，不可併錯。
fn collapse(lines: &[TicketLine]) -> Vec<TicketLine> {
    let mut out: Vec<TicketLine> = Vec::with_capacity(lines.len());
    for l in lines {
        // 線性搜尋而不是 HashMap：一張單撐死幾十行，而**保留第一次出現的順序**
        // 才是這裡真正要的 —— 廚師是照著單子由上往下做的。
        if let Some(prev) = out
            .iter_mut()
            .find(|p| p.name == l.name && p.options == l.options && p.note == l.note)
        {
            prev.qty_milli += l.qty_milli;
            prev.amount += l.amount;
        } else {
            out.push(l.clone());
        }
    }
    out
}

fn qty_label(qty_milli: i64) -> String {
    if qty_milli % 1000 == 0 {
        (qty_milli / 1000).to_string()
    } else {
        // 半份用小數呈現，廚師才看得懂。
        format!("{:.1}", qty_milli as f64 / 1000.0)
    }
}

fn money(n: i64) -> String {
    // 收據上不加千分位：出單機的字型是等寬點陣，逗號會讓數字欄看起來歪掉，
    // 而且台灣的單價很少超過四位數。
    n.to_string()
}

/// 廚房單。**不印金額** —— 廚師不需要知道，而且印了會讓單變長、變慢。
pub fn kitchen_ticket(data: &TicketData, paper: PaperWidth) -> ReceiptDoc {
    let mut doc = ReceiptDoc::new(paper);

    if let Some(b) = data.reason.banner() {
        doc = doc.banner(b, true);
    }

    // 桌號用大字：廚師是在幾公尺外掃一眼找單的。
    let head = data
        .table_label
        .clone()
        .unwrap_or_else(|| data.channel_label.clone());
    doc = doc.banner(&head, false);

    doc = doc
        .styled(
            format!("#{}  {}", data.order_no, data.printed_at),
            TextStyle::centered(),
        )
        .rule();

    if let Some(station) = &data.station {
        doc = doc.styled(station, TextStyle::bold());
    }

    for l in &collapse(&data.lines) {
        // 數量放前面：廚師先看幾份，再看是什麼。
        doc = doc.columns(
            vec![
                Cell::left(format!("{} {}", qty_label(l.qty_milli), l.name)),
                Cell::right(""),
            ],
            vec![9, 1],
        );
        for o in &l.options {
            doc = doc.text(format!("   - {o}"));
        }
        if let Some(n) = &l.note {
            // 備註反白：那是最容易被漏掉、也最容易出事的一行。
            doc = doc.styled(
                format!("   ★ {n}"),
                TextStyle {
                    invert: true,
                    ..Default::default()
                },
            );
        }
    }

    doc.rule().finish(Finish {
        open_drawer: false,
        ..Finish::default()
    })
}

/// 客人收據。
pub fn customer_receipt(data: &TicketData, paper: PaperWidth) -> ReceiptDoc {
    let mut doc = ReceiptDoc::new(paper)
        .styled(&data.store_name, TextStyle::centered())
        .feed(1)
        .columns(
            vec![
                Cell::left(format!("#{}", data.order_no)),
                Cell::right(&data.channel_label),
            ],
            vec![1, 1],
        )
        .text(&data.printed_at);

    if data.reprint_seq > 0 {
        // 重印必須看得出來 —— 兩張一樣的收據可以拿去做假帳。
        doc = doc.styled(
            format!("※ 補印 第 {} 次 ※", data.reprint_seq),
            TextStyle::centered(),
        );
    }

    if let Some(sp) = &data.split {
        doc = doc.styled(
            match sp.count {
                Some(n) => format!("── 分帳 {}／{} ──", sp.index, n),
                // 還不知道總共幾份的時候不要編一個出來。
                None => format!("── 分帳 第 {} 筆 ──", sp.index),
            },
            TextStyle::centered(),
        );
    }

    doc = doc.rule();

    for l in &collapse(&data.lines) {
        doc = doc.columns(
            vec![
                Cell::left(&l.name),
                Cell::right(qty_label(l.qty_milli)),
                Cell::right(money(l.amount)),
            ],
            vec![3, 1, 1],
        );
        for o in &l.options {
            doc = doc.text(format!("  - {o}"));
        }
    }

    doc = doc.rule();

    let mut row = |label: &str, amount: i64| {
        doc = std::mem::replace(&mut doc, ReceiptDoc::new(paper)).columns(
            vec![Cell::left(label), Cell::right(money(amount))],
            vec![3, 2],
        );
    };
    // 分帳單上「小計」跟「本次應付」是同一個數字，印兩次只是讓人多讀一行。
    if data.split.is_none() {
        row("小計", data.subtotal);
    }
    if data.discount_total != 0 {
        row("折扣", -data.discount_total);
    }
    if data.service_charge != 0 {
        row("服務費", data.service_charge);
    }
    if data.rounding_adjustment != 0 {
        row("進位調整", data.rounding_adjustment);
    }

    if let Some(sp) = &data.split {
        // 分帳的收據上「合計」是**這個人要付的錢**，所以整單金額要另外列出來 ——
        // 少了它，客人無從判斷自己那一份算得對不對。
        row("全單", sp.order_total);
    }

    doc = doc
        .rule()
        .columns(
            vec![
                Cell::left(if data.split.is_some() {
                    "本次應付"
                } else {
                    "合計"
                }),
                Cell::right(money(data.grand_total)),
            ],
            vec![3, 2],
        )
        // 稅額要印出來：客人拿這張去報帳時會用到，而且未稅與稅額分開列
        // 是統一發票的格式要求，先養成習慣。
        .columns(
            vec![
                Cell::left(format!("未稅 {}", money(data.sales_amount))),
                Cell::right(format!("稅 {}", money(data.tax_amount))),
            ],
            vec![1, 1],
        );

    if !data.payments.is_empty() {
        doc = doc.rule();
        for p in &data.payments {
            doc = doc.columns(
                vec![Cell::left(&p.method), Cell::right(money(p.amount))],
                vec![3, 2],
            );
        }
        if data.change > 0 {
            doc = doc.columns(
                vec![Cell::left("找零"), Cell::right(money(data.change))],
                vec![3, 2],
            );
        }
    }

    // ★ 分帳最貴的意外是「以為收完了」：三個人各付各的，第三個人走掉而
    //   沒有人發現。所以只要還沒收完，就把它印在收據最下面 ——
    //   店員撕單的時候一定會看到那一行。
    if let Some(sp) = &data.split {
        if sp.remaining > 0 {
            doc = doc.feed(1).styled(
                format!("※ 這張單還差 {} 元 ※", money(sp.remaining)),
                TextStyle::centered(),
            );
        }
    }

    doc.feed(1)
        .styled("謝謝光臨", TextStyle::centered())
        .finish(Finish {
            open_drawer: true,
            ..Finish::default()
        })
}

/// 報表單（班別交接單、X 報表、Z 報表共用）。
///
/// 做成通用版型而不是三個各寫一份：三種報表的排版需求完全一樣
/// （標題、幾個分區、每區若干「說明 + 金額」兩欄），差別只在內容。
/// 分成三份的話，改一次欄寬要改三個地方，而它們一定會慢慢長歪。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportSection {
    pub title: Option<String>,
    /// (說明, 值)。值已經格式化好 —— 版型層不做數字格式化，
    /// 因為「要不要加千分位」是業務決定不是排版決定。
    pub rows: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportData {
    pub store_name: String,
    pub title: String,
    /// 班別號或 Z 報表號。店家對帳時會報這個號碼。
    pub subtitle: String,
    pub printed_at: String,
    pub sections: Vec<ReportSection>,
    pub footer: Option<String>,
}

/// 報表單。**不開錢箱** —— 印報表不是收錢。
pub fn report_ticket(data: &ReportData, paper: PaperWidth) -> ReceiptDoc {
    let cols = paper.cols();
    let mut doc = ReceiptDoc::new(paper)
        .styled(&data.store_name, TextStyle::centered())
        .banner(&data.title, false)
        .styled(&data.subtitle, TextStyle::centered())
        .styled(&data.printed_at, TextStyle::centered())
        .rule();

    for section in &data.sections {
        if let Some(t) = &section.title {
            doc = doc.styled(t, TextStyle::bold());
        }
        for (label, value) in &section.rows {
            // 說明靠左、金額靠右。這是所有報表單唯一的排版規則，
            // 而它必須在 58mm 與 80mm 上都成立。
            doc = doc.columns(
                vec![Cell::left(label.clone()), Cell::right(value.clone())],
                vec![3, 2],
            );
        }
        doc = doc.rule();
    }

    if let Some(f) = &data.footer {
        for line in crate::receipt::layout::wrap(f, cols) {
            doc = doc.styled(line, TextStyle::centered());
        }
    }
    // 交接單通常要簽名。留白比讓店員自己找空位寫好。
    doc.feed(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::render_text::PlainTextRenderer;

    fn sample() -> TicketData {
        TicketData {
            store_name: "小明快炒".into(),
            order_no: "A-20260906-0042".into(),
            channel_label: "內用".into(),
            table_label: Some("A3".into()),
            printed_at: "2026-09-06 19:24".into(),
            lines: vec![
                TicketLine {
                    name: "珍珠奶茶".into(),
                    options: vec!["半糖".into(), "去冰".into()],
                    note: None,
                    qty_milli: 2000,
                    amount: 120,
                },
                TicketLine {
                    name: "滷肉飯".into(),
                    options: vec![],
                    note: Some("不要香菜".into()),
                    qty_milli: 1000,
                    amount: 55,
                },
                TicketLine {
                    name: "貢丸湯".into(),
                    options: vec![],
                    note: None,
                    qty_milli: 500,
                    amount: 15,
                },
            ],
            subtotal: 190,
            discount_total: 0,
            service_charge: 19,
            rounding_adjustment: 0,
            grand_total: 209,
            sales_amount: 199,
            tax_amount: 10,
            payments: vec![PaymentLine {
                method: "現金".into(),
                amount: 500,
            }],
            change: 291,
            station: Some("熱炒區".into()),
            reason: TicketReason::NewOrder,
            reprint_seq: 0,
            split: None,
        }
    }

    fn render(doc: &ReceiptDoc) -> String {
        PlainTextRenderer::default().render_to_string(doc)
    }

    #[test]
    fn kitchen_ticket_has_no_money_on_it() {
        let out = render(&kitchen_ticket(&sample(), PaperWidth::Mm80));
        assert!(out.contains("珍珠奶茶"));
        assert!(out.contains("半糖"));
        // 廚師不需要知道金額，印了只是讓單變長變慢。
        assert!(!out.contains("120"), "廚房單不該有金額：\n{out}");
        assert!(!out.contains("合計"));
    }

    #[test]
    fn kitchen_ticket_puts_quantity_before_the_name() {
        // 廚師先看幾份，再看是什麼。
        let out = render(&kitchen_ticket(&sample(), PaperWidth::Mm80));
        assert!(out.contains("2 珍珠奶茶"), "\n{out}");
        assert!(out.contains("0.5 貢丸湯"), "半份要看得懂：\n{out}");
    }

    #[test]
    fn kitchen_ticket_marks_add_items_so_the_chef_does_not_redo_the_table() {
        let mut d = sample();
        d.reason = TicketReason::AddItems;
        let out = render(&kitchen_ticket(&d, PaperWidth::Mm80));
        assert!(out.contains("加點"), "\n{out}");
    }

    #[test]
    fn notes_are_highlighted_because_they_are_the_easiest_thing_to_miss() {
        let out = PlainTextRenderer::with_style_marks()
            .render_to_string(&kitchen_ticket(&sample(), PaperWidth::Mm80));
        assert!(out.contains("[   ★ 不要香菜]"), "備註要反白：\n{out}");
    }

    /// ★ 分帳的收據要說清楚三件事：這是第幾份、這一份收多少、整張單還差多少。
    ///
    /// 第三件是最重要的。分帳最貴的意外是「以為收完了」—— 三個人各付各的，
    /// 第三個人走掉而沒有人發現。所以只要還沒收完，就要印在單上。
    #[test]
    fn a_split_receipt_says_how_much_is_still_owed() {
        let mut d = sample();
        d.split = Some(SplitLabel {
            index: 2,
            count: Some(3),
            part_total: 70,
            order_total: 209,
            remaining: 69,
        });
        d.grand_total = 70;
        d.sales_amount = 67;
        d.tax_amount = 3;
        let out = render(&customer_receipt(&d, PaperWidth::Mm80));
        assert!(out.contains("分帳 2／3"), "要看得出是第幾份：\n{out}");
        assert!(
            out.contains("本次應付"),
            "「合計」在分帳單上會被誤讀成整單：\n{out}"
        );
        assert!(
            out.contains("全單"),
            "整單金額也要在，客人才對得起來：\n{out}"
        );
        assert!(
            !out.contains("小計"),
            "分帳單上「小計」跟「本次應付」是同一個數字，印兩次只是多一行要讀的東西：\n{out}"
        );
        assert!(out.contains("209"));
        assert!(out.contains("還差 69"), "★ 沒收完一定要印出來：\n{out}");
    }

    /// 還不知道總共幾份的時候不要編一個數字出來。
    #[test]
    fn an_open_ended_split_does_not_invent_a_denominator() {
        let mut d = sample();
        d.split = Some(SplitLabel {
            index: 1,
            count: None,
            part_total: 100,
            order_total: 209,
            remaining: 109,
        });
        let out = render(&customer_receipt(&d, PaperWidth::Mm80));
        assert!(
            out.contains("分帳 第 1 筆"),
            "
{out}"
        );
        assert!(!out.contains("／"), "沒有分母就不要印分數：\n{out}");
    }

    /// 收完最後一份就不該再嚇人。
    #[test]
    fn the_last_split_receipt_has_no_warning() {
        let mut d = sample();
        d.split = Some(SplitLabel {
            index: 3,
            count: Some(3),
            part_total: 69,
            order_total: 209,
            remaining: 0,
        });
        let out = render(&customer_receipt(&d, PaperWidth::Mm80));
        assert!(
            out.contains("分帳 3／3"),
            "
{out}"
        );
        assert!(!out.contains("還差"), "收完了就不要再印欠款：\n{out}");
    }

    #[test]
    fn receipt_totals_line_up_and_include_tax_breakdown() {
        let out = render(&customer_receipt(&sample(), PaperWidth::Mm80));
        assert!(out.contains("小明快炒"));
        assert!(out.contains("合計"));
        assert!(out.contains("209"));
        assert!(out.contains("未稅 199"));
        assert!(out.contains("稅 10"));
        assert!(out.contains("找零"));
        assert!(out.contains("291"));
    }

    #[test]
    fn reprinted_receipt_says_so() {
        // 兩張一樣的收據可以拿去做假帳，所以重印必須看得出來。
        let mut d = sample();
        d.reprint_seq = 2;
        let out = render(&customer_receipt(&d, PaperWidth::Mm80));
        assert!(out.contains("補印 第 2 次"), "\n{out}");
    }

    #[test]
    fn receipt_opens_the_drawer_but_the_kitchen_ticket_does_not() {
        assert!(
            customer_receipt(&sample(), PaperWidth::Mm80)
                .finish
                .open_drawer
        );
        assert!(
            !kitchen_ticket(&sample(), PaperWidth::Mm80)
                .finish
                .open_drawer
        );
    }

    #[test]
    fn every_line_fits_the_paper_on_both_widths() {
        // 超出紙寬的行會被出單機硬折，破壞所有對齊。
        for paper in [PaperWidth::Mm58, PaperWidth::Mm80] {
            for doc in [
                kitchen_ticket(&sample(), paper),
                customer_receipt(&sample(), paper),
            ] {
                for (i, line) in render(&doc).lines().enumerate() {
                    assert!(
                        crate::receipt::layout::display_width(line) <= paper.cols(),
                        "{paper:?} 第 {i} 行超出 {} 欄：{line:?}",
                        paper.cols()
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod collapse_tests {
    use super::*;

    fn line(name: &str, options: &[&str], note: Option<&str>, qty: i64, amount: i64) -> TicketLine {
        TicketLine {
            name: name.into(),
            options: options.iter().map(|s| (*s).to_string()).collect(),
            note: note.map(str::to_string),
            qty_milli: qty,
            amount,
        }
    }

    /// 收銀員連點四下 → 收據上只印一行 ×4。
    #[test]
    fn identical_lines_become_one_line_with_the_quantities_added_up() {
        let out = collapse(&[
            line("晚餐吃到飽", &[], None, 1000, 599),
            line("晚餐吃到飽", &[], None, 1000, 599),
            line("晚餐吃到飽", &[], None, 1000, 599),
            line("晚餐吃到飽", &[], None, 1000, 599),
        ]);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].qty_milli, 4000);
        assert_eq!(out[0].amount, 2396, "金額也要加總，否則收據對不上總計");
    }

    /// ★ 合併之後金額總和必須不變。
    ///
    /// 這是這支函式唯一絕對不能破的性質：收據上各行加總要等於總計，
    /// 而總計是另外算出來的。差一元的收據會被客人當場抓到。
    #[test]
    fn collapsing_never_changes_the_total() {
        let lines = vec![
            line("珍珠奶茶", &["去冰"], None, 1000, 66),
            line("滷肉飯", &[], Some("不要香菜"), 2000, 110),
            line("珍珠奶茶", &["去冰"], None, 1000, 66),
            line("滷肉飯", &[], None, 1000, 55),
        ];
        let before: i64 = lines.iter().map(|l| l.amount).sum();
        let out = collapse(&lines);
        assert_eq!(out.iter().map(|l| l.amount).sum::<i64>(), before);
        assert_eq!(
            out.iter().map(|l| l.qty_milli).sum::<i64>(),
            lines.iter().map(|l| l.qty_milli).sum::<i64>()
        );
    }

    /// 選項或備註不一樣就**不併**。
    ///
    /// 「去冰」與「正常冰」是兩杯不同的飲料；「不要香菜」是廚房會出事的那一行。
    /// 寧可多印一行，不可併錯。
    #[test]
    fn a_different_option_or_note_keeps_the_lines_apart() {
        let out = collapse(&[
            line("珍珠奶茶", &["去冰"], None, 1000, 60),
            line("珍珠奶茶", &["正常冰"], None, 1000, 60),
            line("珍珠奶茶", &["去冰"], Some("走路喝"), 1000, 60),
            line("珍珠奶茶", &["去冰"], None, 1000, 60),
        ]);

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].options, vec!["去冰".to_string()]);
        assert_eq!(out[0].qty_milli, 2000, "第一與第四行才是同一杯");
        assert_eq!(out[1].options, vec!["正常冰".to_string()]);
        assert_eq!(out[2].note.as_deref(), Some("走路喝"));
    }

    /// 順序照第一次出現的位置，不重排。廚師是照著單子由上往下做的。
    #[test]
    fn the_order_of_first_appearance_is_kept() {
        let out = collapse(&[
            line("滷肉飯", &[], None, 1000, 55),
            line("珍珠奶茶", &[], None, 1000, 60),
            line("滷肉飯", &[], None, 1000, 55),
        ]);
        assert_eq!(
            out.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
            vec!["滷肉飯", "珍珠奶茶"]
        );
    }

    #[test]
    fn an_empty_ticket_stays_empty() {
        assert!(collapse(&[]).is_empty());
    }
}
