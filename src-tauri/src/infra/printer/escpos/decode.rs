//! ESC/POS 位元組流的反向解碼。
//!
//! 兩個用途，而且刻意共用同一份程式碼：
//!
//! 1. **fakeprinter** —— 讓沒有出單機的人也能看到自己改了什麼。
//!    這對開源專案的存活是決定性的：一個要有特定硬體才能驗證的專案，
//!    外部貢獻者是進不來的。
//! 2. **快照測試的 hex 註解** —— 位元組的 diff 是一堆 `1b 40 1d 76`，
//!    review 時只能盲簽；加上解碼註解就能一眼看出改了什麼。
//!
//! 解碼器與編碼器住在同一個 crate、共用 `constants`，所以改指令時
//! 編譯器會逼你兩邊一起改 —— 不會出現「模擬器說 OK 但真機印不出來」。

use super::constants as c;
use super::CjkEncoding;

/// 一段被解出來的動作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Init,
    Text(String),
    Align(&'static str),
    Bold(bool),
    Underline(bool),
    Reverse(bool),
    Size {
        w: u8,
        h: u8,
    },
    KanjiMode(bool),
    Feed(u8),
    Cut {
        full: bool,
    },
    Drawer,
    Raster {
        width: u16,
        height: u16,
    },
    /// 認不得的位元組。**不要安靜地跳過** —— 那正是「模擬器說沒問題、
    /// 真機印出亂碼」的來源。
    Unknown(Vec<u8>),
}

/// 把位元組流解成一連串動作。
pub fn decode(bytes: &[u8], encoding: CjkEncoding) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut i = 0usize;
    let mut kanji = false;
    let mut text: Vec<u8> = Vec::new();

    macro_rules! flush_text {
        () => {
            if !text.is_empty() {
                ops.push(Op::Text(decode_text(&text, encoding, kanji)));
                text.clear();
            }
        };
    }

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0x1B if i + 1 < bytes.len() => {
                flush_text!();
                let cmd = bytes[i + 1];
                let arg = bytes.get(i + 2).copied().unwrap_or(0);
                match cmd {
                    b'@' => {
                        ops.push(Op::Init);
                        i += 2;
                    }
                    c::ALIGN => {
                        ops.push(Op::Align(match arg {
                            1 => "center",
                            2 => "right",
                            _ => "left",
                        }));
                        i += 3;
                    }
                    c::EMPHASIS => {
                        ops.push(Op::Bold(arg != 0));
                        i += 3;
                    }
                    c::UNDERLINE => {
                        ops.push(Op::Underline(arg != 0));
                        i += 3;
                    }
                    b'd' => {
                        ops.push(Op::Feed(arg));
                        i += 3;
                    }
                    b'p' => {
                        ops.push(Op::Drawer);
                        i += 5;
                    }
                    _ => {
                        ops.push(Op::Unknown(bytes[i..(i + 2).min(bytes.len())].to_vec()));
                        i += 2;
                    }
                }
            }
            0x1D if i + 1 < bytes.len() => {
                flush_text!();
                match bytes[i + 1] {
                    b'!' => {
                        let n = bytes.get(i + 2).copied().unwrap_or(0);
                        ops.push(Op::Size {
                            w: (n >> 4) + 1,
                            h: (n & 0x0F) + 1,
                        });
                        i += 3;
                    }
                    b'B' => {
                        ops.push(Op::Reverse(bytes.get(i + 2).copied().unwrap_or(0) != 0));
                        i += 3;
                    }
                    b'V' => {
                        let m = bytes.get(i + 2).copied().unwrap_or(0);
                        ops.push(Op::Cut {
                            full: m == c::CUT_FULL,
                        });
                        i += if m == 65 || m == 66 { 4 } else { 3 };
                    }
                    b'v' => {
                        // GS v 0 m xL xH yL yH data...
                        let xl = bytes.get(i + 4).copied().unwrap_or(0) as u16;
                        let xh = bytes.get(i + 5).copied().unwrap_or(0) as u16;
                        let yl = bytes.get(i + 6).copied().unwrap_or(0) as u16;
                        let yh = bytes.get(i + 7).copied().unwrap_or(0) as u16;
                        let width_bytes = xl | (xh << 8);
                        let height = yl | (yh << 8);
                        ops.push(Op::Raster {
                            width: width_bytes.saturating_mul(8),
                            height,
                        });
                        i += 8 + (width_bytes as usize * height as usize);
                    }
                    _ => {
                        ops.push(Op::Unknown(bytes[i..(i + 2).min(bytes.len())].to_vec()));
                        i += 2;
                    }
                }
            }
            0x1C if i + 1 < bytes.len() => {
                flush_text!();
                match bytes[i + 1] {
                    0x26 => {
                        kanji = true;
                        ops.push(Op::KanjiMode(true));
                    }
                    0x2E => {
                        kanji = false;
                        ops.push(Op::KanjiMode(false));
                    }
                    other => ops.push(Op::Unknown(vec![0x1C, other])),
                }
                i += 2;
            }
            _ => {
                text.push(b);
                i += 1;
            }
        }
    }
    flush_text!();
    ops
}

fn decode_text(bytes: &[u8], encoding: CjkEncoding, kanji: bool) -> String {
    if !kanji || encoding == CjkEncoding::Utf8 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let enc = match encoding {
        CjkEncoding::Big5 => encoding_rs::BIG5,
        CjkEncoding::Gb18030 => encoding_rs::GB18030,
        CjkEncoding::Utf8 => unreachable!(),
    };
    enc.decode(bytes).0.into_owned()
}

/// 把解碼結果整理成人看的文字（fakeprinter 的預設輸出）。
pub fn render_human(ops: &[Op]) -> String {
    let mut out = String::new();
    for op in ops {
        match op {
            Op::Text(t) => out.push_str(t),
            Op::Feed(n) => out.push_str(&"\n".repeat(*n as usize)),
            Op::Cut { full } => {
                let bar = if *full {
                    "──── 切紙（全切）────"
                } else {
                    "┈┈┈┈ 切紙 ┈┈┈┈"
                };
                out.push('\n');
                out.push_str(bar);
                out.push('\n');
            }
            Op::Drawer => out.push_str("\n[錢箱彈開]\n"),
            Op::Raster { width, height } => out.push_str(&format!("\n[點陣圖 {width}×{height}]\n")),
            Op::Unknown(b) => out.push_str(&format!("[?{}]", hex(b))),
            _ => {}
        }
    }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::printer::escpos::EscPosTextRenderer;
    use crate::receipt::{PaperWidth, ReceiptDoc};

    /// ★ 編碼器與解碼器必須互為反函式。
    ///
    /// 這條性質是 fakeprinter 有意義的前提：模擬器看到的東西，
    /// 必須就是真機會看到的東西。
    #[test]
    fn round_trips_through_the_encoder() {
        let doc = ReceiptDoc::new(PaperWidth::Mm58)
            .text("珍珠奶茶 x2")
            .rule()
            .text("Total 120");
        let enc = EscPosTextRenderer::new(32, CjkEncoding::Big5).encode(&doc);
        let ops = decode(&enc.bytes, CjkEncoding::Big5);

        let text = render_human(&ops);
        assert!(text.contains("珍珠奶茶 x2"), "\n{text}");
        assert!(text.contains("Total 120"), "\n{text}");
        assert!(text.contains("切紙"));

        // 認不得的位元組要被標出來，不能安靜跳過 ——
        // 那正是「模擬器說沒問題、真機印出亂碼」的來源。
        assert!(
            !ops.iter().any(|o| matches!(o, Op::Unknown(_))),
            "自家編碼器產生的位元組不該有解不出來的：{ops:?}"
        );
    }

    #[test]
    fn decodes_styles_and_cut() {
        let doc = ReceiptDoc::new(PaperWidth::Mm58)
            .banner("內用", false)
            .styled("粗體", crate::receipt::TextStyle::bold());
        let enc = EscPosTextRenderer::new(32, CjkEncoding::Big5).encode(&doc);
        let ops = decode(&enc.bytes, CjkEncoding::Big5);

        assert!(ops.contains(&Op::Init));
        assert!(ops.iter().any(|o| matches!(o, Op::Size { w: 2, h: 2 })));
        assert!(ops.contains(&Op::Bold(true)));
        assert!(ops.contains(&Op::Bold(false)));
        assert!(ops.iter().any(|o| matches!(o, Op::Cut { .. })));
    }

    #[test]
    fn unknown_bytes_are_surfaced() {
        // 未來加新指令而忘了更新解碼器時，這條性質會讓 fakeprinter 直接顯示 [?..]
        // 而不是假裝一切正常。
        let ops = decode(&[0x1B, 0x7A, 0x01], CjkEncoding::Big5);
        assert!(ops.iter().any(|o| matches!(o, Op::Unknown(_))));
    }
}
