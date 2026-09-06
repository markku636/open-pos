//! ESC/POS 位元組產生。
//!
//! # 為什麼不用 escpos crate 當抽象層
//!
//! 兩個實測到的限制：
//!
//! 1. 它的 `Driver` trait 是**同步**的（`fn write(&self, &[u8]) -> Result<()>`），
//!    塞不進 async 的列印佇列 worker。
//! 2. 它的 `PageCode` 有 38 個變體，**完全沒有中文碼頁** —— CJK 只有日文假名。
//!    印繁體中文它幫不上任何忙。
//!
//! 所以這裡自己產生位元組。指令常數集中在 `constants.rs`，
//! 與 `fakeprinter` 的解碼器共用。
//!
//! # 文字模式的中文
//!
//! ESC/POS 的漢字模式送的是 Big5 或 GB18030 的**雙位元組**，不是 UTF-8。
//! 流程是 `FS &` 進漢字模式 → 送編碼後的位元組 → `FS .` 離開。
//! 送純 ASCII 之前必須離開，否則 ASCII 會被當成漢字高位元組。
//!
//! # 缺字怎麼辦
//!
//! Big5 只有 13,053 個漢字。「𩵚魠魚」的「𩵚」、原住民語譯字都不在裡面。
//! `encode_text` 會回報缺字清單，呼叫端據此決定：把整張單改走點陣圖模式
//! （正確但慢），或印替代字並警告店家。
//! **絕不能默默印出問號** —— 廚師看不懂的單就是一份重做的餐。

pub mod constants;
pub mod decode;

use serde::{Deserialize, Serialize};

use crate::receipt::layout::Align;
use crate::receipt::{Block, CutMode, ReceiptDoc, TextStyle};

use constants as c;

/// 出單機內建的中文字庫用哪一種編碼。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CjkEncoding {
    /// 台灣機常見。
    #[default]
    Big5,
    /// 陸製機常見（向下相容 GBK / GB2312）。
    Gb18030,
    /// 少數新款或改過韌體的機器。
    Utf8,
}

/// 編碼一段文字，回傳位元組與**編不出來的字**。
///
/// ⚠️ 不能直接用 `encoding_rs::BIG5.encode()`：它對無法映射的字元會產生
/// HTML numeric reference（`&#12345;`），而印表機會老老實實把
/// `&#12345;` 這幾個字印出來。所以逐字元編碼，缺字另外回報。
pub fn encode_text(s: &str, enc: CjkEncoding) -> (Vec<u8>, Vec<char>) {
    if enc == CjkEncoding::Utf8 {
        return (s.as_bytes().to_vec(), Vec::new());
    }
    let encoding = match enc {
        CjkEncoding::Big5 => encoding_rs::BIG5,
        CjkEncoding::Gb18030 => encoding_rs::GB18030,
        CjkEncoding::Utf8 => unreachable!(),
    };

    let mut out = Vec::with_capacity(s.len());
    let mut missing = Vec::new();
    let mut buf = [0u8; 4];

    for ch in s.chars() {
        let piece = ch.encode_utf8(&mut buf);
        let (bytes, _, had_errors) = encoding.encode(piece);
        if had_errors {
            missing.push(ch);
            // 用全形空白佔位，維持欄寬對齊 —— 印一個看不懂的符號比留白更糟，
            // 而版面歪掉會讓整張單都難讀。
            out.extend_from_slice(&[0xA1, 0x40]);
        } else {
            out.extend_from_slice(&bytes);
        }
    }
    (out, missing)
}

/// 把 `ReceiptDoc` 編成 ESC/POS 文字模式的位元組。
///
/// 這是 v1 的實作。**點陣圖模式（raster）才是最終要的預設** ——
/// 它不依賴印表機內建字庫、換任何機出來都一樣、而且可以做像素級快照測試。
/// 但它需要內嵌一份 CJK 字型，那是 M5 後段的工作。
/// 在那之前文字模式是可用的路徑，缺字會被回報而不是默默印錯。
pub struct EscPosTextRenderer {
    pub cols: usize,
    pub encoding: CjkEncoding,
    /// 切刀與列印頭有物理距離，不走紙會切在內容上。
    pub cut_feed_lines: u8,
    pub has_cutter: bool,
}

#[derive(Debug, Default)]
pub struct EncodeReport {
    pub bytes: Vec<u8>,
    /// 這張單裡編不出來的字。UI 要把它顯示給店家看 ——
    /// 「你的品名有這台機器印不出來的字」是可以改品名解決的問題。
    pub missing: Vec<char>,
}

impl EscPosTextRenderer {
    pub fn new(cols: usize, encoding: CjkEncoding) -> Self {
        Self {
            cols,
            encoding,
            cut_feed_lines: 3,
            has_cutter: true,
        }
    }

    pub fn encode(&self, doc: &ReceiptDoc) -> EncodeReport {
        let mut out = Vec::new();
        let mut missing = Vec::new();
        out.extend_from_slice(c::INIT);

        for block in &doc.blocks {
            match block {
                Block::Text { content, style } => {
                    self.write_styled(&mut out, &mut missing, content, style)
                }
                Block::Columns { cells, weights } => {
                    // 分欄在文字模式下就是「補好空白的一整行」——
                    // 印表機沒有 tab stop 的概念，對齊全靠我們自己算。
                    let rendered = crate::receipt::render_text::PlainTextRenderer::default()
                        .render_columns_for(self.cols, cells, weights);
                    for line in rendered {
                        self.write_styled(&mut out, &mut missing, &line, &TextStyle::default());
                    }
                }
                Block::Rule { ch } => {
                    let line: String = std::iter::repeat_n(*ch, self.cols).collect();
                    self.write_styled(&mut out, &mut missing, &line, &TextStyle::default());
                }
                Block::Feed { lines } => {
                    out.extend_from_slice(c::FEED_LINES);
                    out.push(*lines);
                }
                Block::Banner { content, boxed } => {
                    // Banner 用倍寬倍高。廚師是在幾公尺外掃一眼找單的。
                    out.extend_from_slice(c::SIZE);
                    out.push(0x11); // 寬 ×2、高 ×2
                    let style = TextStyle {
                        align: Align::Center,
                        bold: true,
                        invert: *boxed,
                        ..Default::default()
                    };
                    self.write_styled(&mut out, &mut missing, content, &style);
                    out.extend_from_slice(c::SIZE);
                    out.push(0x00);
                }
                // QR 與條碼在 v1 的文字模式下不輸出。
                // 它們要等 raster 模式 —— 內建的 GS ( k 指令尺寸是離散的模組大小，
                // 很難精準命中電子發票規定的「≧1.5 公分」，而且各廠牌行為不一致。
                Block::QrCode { .. } | Block::Barcode { .. } => {}
            }
        }

        if doc.finish.feed_lines > 0 {
            out.extend_from_slice(c::FEED_LINES);
            out.push(doc.finish.feed_lines);
        }
        if doc.finish.open_drawer {
            // ESC p 0 25 250：第 0 號腳位，脈衝 50ms / 500ms。
            out.extend_from_slice(c::DRAWER);
            out.extend_from_slice(&[0x00, 25, 250]);
        }
        match doc.finish.cut {
            CutMode::None => {}
            _ if !self.has_cutter => {
                // 沒有切刀的機器送 GS V 會亂印或卡住，改成多走幾行讓人撕。
                out.extend_from_slice(c::FEED_LINES);
                out.push(self.cut_feed_lines);
            }
            CutMode::Partial => {
                out.extend_from_slice(c::CUT);
                out.push(c::CUT_PARTIAL);
                out.push(self.cut_feed_lines);
            }
            CutMode::Full => {
                out.extend_from_slice(c::CUT);
                out.push(c::CUT_FULL);
                out.push(self.cut_feed_lines);
            }
        }

        EncodeReport {
            bytes: out,
            missing,
        }
    }

    fn write_styled(
        &self,
        out: &mut Vec<u8>,
        missing: &mut Vec<char>,
        text: &str,
        style: &TextStyle,
    ) {
        out.extend_from_slice(&[0x1B, c::ALIGN]);
        out.push(match style.align {
            Align::Left => c::ALIGN_LEFT,
            Align::Center => c::ALIGN_CENTER,
            Align::Right => c::ALIGN_RIGHT,
        });
        if style.bold {
            out.extend_from_slice(&[0x1B, c::EMPHASIS, 1]);
        }
        if style.underline {
            out.extend_from_slice(&[0x1B, c::UNDERLINE, 1]);
        }
        if style.invert {
            out.extend_from_slice(c::REVERSE);
            out.push(1);
        }

        for line in crate::receipt::layout::wrap(text, self.cols) {
            self.write_line(out, missing, &line);
            out.push(b'\n');
        }

        if style.invert {
            out.extend_from_slice(c::REVERSE);
            out.push(0);
        }
        if style.underline {
            out.extend_from_slice(&[0x1B, c::UNDERLINE, 0]);
        }
        if style.bold {
            out.extend_from_slice(&[0x1B, c::EMPHASIS, 0]);
        }
    }

    /// 一行文字。ASCII 與漢字要分段送，中間切換漢字模式。
    fn write_line(&self, out: &mut Vec<u8>, missing: &mut Vec<char>, line: &str) {
        if self.encoding == CjkEncoding::Utf8 {
            out.extend_from_slice(line.as_bytes());
            return;
        }

        let mut in_kanji = false;
        let mut chunk = String::new();

        let flush =
            |out: &mut Vec<u8>, missing: &mut Vec<char>, chunk: &mut String, kanji: bool| {
                if chunk.is_empty() {
                    return;
                }
                if kanji {
                    let (bytes, miss) = encode_text(chunk, self.encoding);
                    out.extend_from_slice(&bytes);
                    missing.extend(miss);
                } else {
                    out.extend_from_slice(chunk.as_bytes());
                }
                chunk.clear();
            };

        for ch in line.chars() {
            let wide = !ch.is_ascii();
            if wide != in_kanji {
                flush(out, missing, &mut chunk, in_kanji);
                out.extend_from_slice(if wide { c::KANJI_ON } else { c::KANJI_OFF });
                in_kanji = wide;
            }
            chunk.push(ch);
        }
        flush(out, missing, &mut chunk, in_kanji);
        if in_kanji {
            // 行尾一定要退出漢字模式，否則下一行的 ASCII 會被當成高位元組。
            out.extend_from_slice(c::KANJI_OFF);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::{Cell, PaperWidth};

    #[test]
    fn big5_encodes_common_chinese() {
        let (bytes, missing) = encode_text("珍珠奶茶", CjkEncoding::Big5);
        assert!(missing.is_empty());
        assert_eq!(bytes.len(), 8, "四個中文字應該是 8 個位元組");
    }

    /// ★ 缺字必須被回報，不能默默印出問號或 `&#12345;`。
    ///
    /// 「𩵚魠魚」的「𩵚」是 CJK Ext-B，Big5 沒有。廚師看不懂的單
    /// 就是一份重做的餐，所以這件事必須讓店家知道（去改品名）。
    #[test]
    fn missing_characters_are_reported_not_silently_mangled() {
        let (bytes, missing) = encode_text("𩵚魠魚", CjkEncoding::Big5);
        assert_eq!(missing, vec!['𩵚']);
        // 不能出現 encoding_rs 的 HTML numeric reference。
        assert!(!bytes.windows(2).any(|w| w == b"&#"));
        // 用全形空白佔位，欄寬才不會歪。
        assert_eq!(&bytes[..2], &[0xA1, 0x40]);
    }

    #[test]
    fn ascii_and_chinese_switch_modes_and_always_exit() {
        let doc = ReceiptDoc::new(PaperWidth::Mm58).text("A珍B");
        let out = EscPosTextRenderer::new(32, CjkEncoding::Big5).encode(&doc);
        let b = &out.bytes;

        // 必須進出漢字模式，而且行尾一定退出 ——
        // 沒退出的話下一行的 ASCII 會被當成漢字高位元組，整片亂碼。
        assert!(contains(b, c::KANJI_ON), "應該進過漢字模式");
        let last_off = find_last(b, c::KANJI_OFF).expect("行尾應該退出漢字模式");
        let last_on = find_last(b, c::KANJI_ON).unwrap();
        assert!(last_off > last_on, "最後一個動作必須是離開漢字模式");
    }

    #[test]
    fn cut_is_replaced_by_extra_feed_when_the_printer_has_no_cutter() {
        // 沒有切刀的機器送 GS V 會亂印或卡住。
        let doc = ReceiptDoc::new(PaperWidth::Mm58).text("x");
        let mut r = EscPosTextRenderer::new(32, CjkEncoding::Big5);
        r.has_cutter = false;
        let out = r.encode(&doc);
        assert!(!contains(&out.bytes, c::CUT), "不該送切紙指令");
        assert!(contains(&out.bytes, c::FEED_LINES));
    }

    #[test]
    fn drawer_pulse_is_only_sent_when_asked() {
        let receipt = ReceiptDoc::new(PaperWidth::Mm58)
            .text("x")
            .finish(crate::receipt::Finish {
                open_drawer: true,
                ..Default::default()
            });
        let kitchen = ReceiptDoc::new(PaperWidth::Mm58).text("x");
        let r = EscPosTextRenderer::new(32, CjkEncoding::Big5);
        assert!(contains(&r.encode(&receipt).bytes, c::DRAWER));
        assert!(!contains(&r.encode(&kitchen).bytes, c::DRAWER));
    }

    #[test]
    fn every_document_starts_with_an_init() {
        // 上一張單留下的樣式（粗體、反白、字級）必須被清掉，
        // 否則第一行會繼承前一張單的狀態。
        let doc = ReceiptDoc::new(PaperWidth::Mm80)
            .columns(vec![Cell::left("甲"), Cell::right("1")], vec![3, 1]);
        let out = EscPosTextRenderer::new(48, CjkEncoding::Big5).encode(&doc);
        assert_eq!(&out.bytes[..2], c::INIT);
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
    fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        (0..haystack.len().saturating_sub(needle.len() - 1))
            .rev()
            .find(|&i| &haystack[i..i + needle.len()] == needle)
    }
}
