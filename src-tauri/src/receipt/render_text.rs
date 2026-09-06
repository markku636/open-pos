//! 純文字 renderer。
//!
//! 三個用途，全部都很重要：
//!
//! 1. **前端的列印預覽** —— 讓店員在真的印出來之前就看到版面。
//! 2. **快照測試的人類可讀基準** —— 位元組的 diff 是一堆 `1b 40 1d 76`，
//!    review 時只能盲簽；文字的 diff 一眼就看出「品名欄變窄了」「合計行不見了」。
//! 3. **fakeprinter 的輸出** —— 沒有出單機的貢獻者也能看到自己改了什麼。

use crate::error::AppResult;
use crate::receipt::layout::{self, Align};
use crate::receipt::{Block, ReceiptDoc, ReceiptRenderer, TextStyle};

#[derive(Default)]
pub struct PlainTextRenderer {
    /// 是否把樣式標成可見記號（`**粗體**`、`[反白]`）。
    /// 快照測試開著，前端預覽關著。
    pub mark_styles: bool,
}

impl PlainTextRenderer {
    pub fn with_style_marks() -> Self {
        Self { mark_styles: true }
    }

    pub fn render_to_string(&self, doc: &ReceiptDoc) -> String {
        let cols = doc.paper.cols();
        let mut out: Vec<String> = Vec::new();

        for block in &doc.blocks {
            match block {
                Block::Text { content, style } => {
                    for line in layout::wrap(content, cols) {
                        out.push(self.styled_line(&line, cols, style));
                    }
                }
                Block::Columns { cells, weights } => {
                    out.extend(render_columns(cols, cells, weights));
                }
                Block::Rule { ch } => out.push(ch.to_string().repeat(cols)),
                Block::Feed { lines } => {
                    for _ in 0..*lines {
                        out.push(String::new());
                    }
                }
                Block::Banner { content, boxed } => {
                    out.extend(render_banner(cols, content, *boxed));
                }
                // 純文字模式印不出圖，但**必須留下佔位** ——
                // 否則預覽會比實際短一截，店員以為紙不夠長。
                Block::QrCode { data } => {
                    out.push(layout::pad(&format!("[QR:{data}]"), cols, Align::Center))
                }
                Block::Barcode { data } => {
                    out.push(layout::pad(&format!("[BAR:{data}]"), cols, Align::Center))
                }
            }
        }

        for _ in 0..doc.finish.feed_lines {
            out.push(String::new());
        }
        // 尾端的空白行不 trim —— 走紙長度是版面的一部分，
        // 少了它切紙會切在字上。
        let mut s = out.join("\n");
        s.push('\n');
        s
    }

    fn styled_line(&self, line: &str, cols: usize, style: &TextStyle) -> String {
        let text = if self.mark_styles {
            let mut t = line.to_string();
            if style.bold {
                t = format!("**{t}**");
            }
            if style.invert {
                t = format!("[{t}]");
            }
            if style.underline {
                t = format!("_{t}_");
            }
            t
        } else {
            line.to_string()
        };
        layout::pad(&text, cols, style.align)
    }
}

fn render_columns(cols: usize, cells: &[crate::receipt::Cell], weights: &[u8]) -> Vec<String> {
    if cells.is_empty() {
        return vec![String::new()];
    }
    // 權重數量與欄位數量不一致時，不要 panic —— 那是資料問題，
    // 而收據引擎在營業中崩掉比印歪一行嚴重得多。缺的補 1。
    let mut w: Vec<u8> = weights.to_vec();
    w.resize(cells.len(), 1);
    let widths = layout::split_columns(cols, &w);

    // 每一欄各自斷行，再逐列組起來 —— 品名太長時只有那一欄往下長，
    // 數量與金額留在第一列。
    let wrapped: Vec<Vec<String>> = cells
        .iter()
        .zip(&widths)
        .map(|(c, width)| layout::wrap(&c.content, *width))
        .collect();

    let rows = wrapped.iter().map(|c| c.len()).max().unwrap_or(1);
    (0..rows)
        .map(|r| {
            let mut line = String::new();
            for (i, col) in wrapped.iter().enumerate() {
                let text = col.get(r).map(String::as_str).unwrap_or("");
                line.push_str(&layout::pad(text, widths[i], cells[i].align));
            }
            // 補位造成的行尾空白對出單機沒有意義，但會讓快照 diff 很吵。
            line.trim_end().to_string()
        })
        .collect()
}

fn render_banner(cols: usize, content: &str, boxed: bool) -> Vec<String> {
    // Banner 在真的出單機上是倍寬字，一行只放得下一半的字數。
    // 純文字模式沒有倍寬，但**必須用一半的寬度斷行**，
    // 否則預覽的斷行位置會與實際不同 —— 那正是預覽要避免的事。
    let inner = cols / 2;
    let lines = layout::wrap(content, inner.saturating_sub(if boxed { 2 } else { 0 }));

    let mut out = Vec::new();
    if boxed {
        out.push(layout::pad(&"=".repeat(inner), cols, Align::Center));
    }
    for l in lines {
        let body = if boxed {
            format!(
                "|{}|",
                layout::pad(&l, inner.saturating_sub(2), Align::Center)
            )
        } else {
            l
        };
        out.push(layout::pad(&body, cols, Align::Center));
    }
    if boxed {
        out.push(layout::pad(&"=".repeat(inner), cols, Align::Center));
    }
    out
}

impl ReceiptRenderer for PlainTextRenderer {
    fn render(&self, doc: &ReceiptDoc) -> AppResult<Vec<u8>> {
        Ok(self.render_to_string(doc).into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::{Cell, PaperWidth};

    fn render(doc: &ReceiptDoc) -> String {
        PlainTextRenderer::default().render_to_string(doc)
    }

    #[test]
    fn columns_line_up_on_the_grid() {
        let doc = ReceiptDoc::new(PaperWidth::Mm58)
            .columns(
                vec![Cell::left("珍珠奶茶"), Cell::right("2"), Cell::right("120")],
                vec![3, 1, 1],
            )
            .columns(
                vec![Cell::left("滷肉飯"), Cell::right("1"), Cell::right("55")],
                vec![3, 1, 1],
            );
        let out = render(&doc);
        let lines: Vec<&str> = out.lines().collect();
        // 58mm = 32 欄，權重 [3,1,1] → [18, 7, 7]
        // 金額欄的右邊界必須對齊 —— 這是收據上最一眼看得出的瑕疵。
        assert_eq!(lines[0], "珍珠奶茶                2    120");
        assert_eq!(lines[1], "滷肉飯                  1     55");
        for l in &lines[..2] {
            assert_eq!(layout::display_width(l), 32);
        }
    }

    #[test]
    fn a_long_name_wraps_within_its_own_column() {
        // 品名太長時只有品名欄往下長，數量與金額留在第一列。
        let doc = ReceiptDoc::new(PaperWidth::Mm58).columns(
            vec![
                Cell::left("特製招牌無敵超級大碗牛肉麵加辣加麵"),
                Cell::right("1"),
                Cell::right("260"),
            ],
            vec![3, 1, 1],
        );
        let lines: Vec<String> = render(&doc).lines().map(String::from).collect();
        assert!(lines.len() >= 2);
        assert!(lines[0].contains("260"), "第一列要有金額：{}", lines[0]);
        assert!(!lines[1].contains("260"), "續行不該重複金額：{}", lines[1]);
    }

    #[test]
    fn rule_fills_the_paper_width() {
        for (paper, n) in [(PaperWidth::Mm58, 32), (PaperWidth::Mm80, 48)] {
            let out = render(&ReceiptDoc::new(paper).rule());
            assert_eq!(out.lines().next().unwrap().len(), n);
        }
    }

    #[test]
    fn banner_wraps_at_half_width_because_it_prints_double_wide() {
        // 真機上 Banner 是倍寬字，一行只放得下一半 ——
        // 預覽必須用同樣的斷行位置，否則預覽就失去意義了。
        let doc = ReceiptDoc::new(PaperWidth::Mm58).banner("內用 A3 桌", false);
        let out = render(&doc);
        let line = out.lines().next().unwrap();
        assert_eq!(layout::display_width(line), 32);
        assert!(line.contains("內用 A3 桌"));
    }

    #[test]
    fn boxed_banner_draws_a_frame() {
        let out = render(&ReceiptDoc::new(PaperWidth::Mm58).banner("加點", true));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].contains("===="));
        assert!(lines[1].contains('|'));
        assert!(lines[2].contains("===="));
    }

    #[test]
    fn qr_and_barcode_leave_a_placeholder_so_the_preview_length_is_honest() {
        // 預覽比實際短一截的話，店員會以為紙夠長。
        let doc = ReceiptDoc::new(PaperWidth::Mm58)
            .text("x")
            .clone_with_qr("AB12345678");
        let out = render(&doc);
        assert!(out.contains("[QR:AB12345678]"));
    }

    #[test]
    fn feed_lines_are_kept_because_cutting_needs_them() {
        // 切刀與列印頭有物理距離。少了走紙，切紙會切在字上。
        let doc = ReceiptDoc::new(PaperWidth::Mm58).text("結束");
        let out = render(&doc);
        assert!(out.ends_with("\n\n\n\n"), "預設 feed 3 行沒有保留：{out:?}");
    }

    #[test]
    fn mismatched_column_weights_do_not_panic() {
        // 資料問題不該讓收據引擎在營業中崩掉 —— 印歪一行遠比當機好。
        let doc = ReceiptDoc::new(PaperWidth::Mm58).columns(
            vec![Cell::left("甲"), Cell::right("乙"), Cell::right("丙")],
            vec![3],
        );
        let out = render(&doc);
        assert_eq!(out.lines().count(), 1 + 3);
    }

    #[test]
    fn doc_round_trips_through_json() {
        // 列印佇列存的是 doc 快照，所以序列化必須是無損的。
        let doc = ReceiptDoc::new(PaperWidth::Mm80)
            .banner("內用", true)
            .rule()
            .columns(vec![Cell::left("珍奶"), Cell::right("60")], vec![3, 1])
            .text("謝謝光臨");
        let json = serde_json::to_string(&doc).unwrap();
        let back: ReceiptDoc = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
        assert_eq!(render(&doc), render(&back));
    }

    impl ReceiptDoc {
        fn clone_with_qr(mut self, data: &str) -> Self {
            self.blocks.push(Block::QrCode {
                data: data.to_string(),
            });
            self
        }
    }
}
