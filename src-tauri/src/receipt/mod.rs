//! 收據版面引擎。
//!
//! # ReceiptDoc 是與裝置無關的中間層
//!
//! 收據、廚房單、出杯單、將來的電子發票證明聯與標籤機貼紙，全部先產生成
//! `ReceiptDoc`，再由不同的 renderer 轉成純文字（預覽 / 測試）、
//! ESC/POS 位元組、點陣圖，或 v2 的 ZPL。
//!
//! **換一個 renderer 就多支援一整類硬體** —— 這是這層間接最大的回報。
//!
//! # 三個刻意的限制（用表達力換可測試性）
//!
//! * **無巢狀。** `blocks` 是平的 `Vec`。收據沒有 CSS box model 的需求，
//!   巢狀只會讓排版引擎與快照測試變複雜。要表格就用 `Block::Columns`。
//! * **無絕對定位。** 一切都是由上而下的流。
//! * **可序列化。** 整份 doc 能存進列印佇列 —— 補印時直接重送這份 doc，
//!   不重跑業務邏輯。這是**資料一致性**的要求，不是效能最佳化：
//!   重跑業務邏輯去補印，印出來的會是「訂單被改過之後」的內容，那是錯的。
//!   廚房單是「當時的指令」，收據才是「當前的事實」。

pub mod layout;
pub mod render_text;
pub mod templates;

use serde::{Deserialize, Serialize};

pub use layout::Align;

/// 紙寬。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaperWidth {
    Mm58,
    #[default]
    Mm80,
}

impl PaperWidth {
    /// 字型 A（12 dot 寬）下每行的半形字元數。全形字佔 2 格。
    ///
    /// 這是**最常見**的規格而不是唯一規格（少數 80mm 機是 42 欄），
    /// 所以 `PrinterCaps` 允許逐台覆寫。
    pub fn cols(self) -> usize {
        match self {
            Self::Mm58 => 32,
            Self::Mm80 => 48,
        }
    }

    /// 203dpi 下的可印點寬（raster 模式用）。
    pub fn dots(self) -> u16 {
        match self {
            Self::Mm58 => 384,
            Self::Mm80 => 576,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CutMode {
    None,
    #[default]
    Partial,
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finish {
    pub cut: CutMode,
    /// 出單後是否彈錢箱。只有收據要，廚房單不要。
    pub open_drawer: bool,
    /// 切紙前先走幾行。切刀與列印頭有物理距離，不走紙會切在內容上。
    pub feed_lines: u8,
}

impl Default for Finish {
    fn default() -> Self {
        Self {
            cut: CutMode::Partial,
            open_drawer: false,
            feed_lines: 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TextStyle {
    pub align: Align,
    pub bold: bool,
    pub underline: bool,
    /// 白字黑底。廚房單標「退單」「加急」很有用。
    pub invert: bool,
    /// 字級倍數 1..=4。
    pub scale: u8,
}

impl TextStyle {
    pub fn centered() -> Self {
        Self {
            align: Align::Center,
            ..Default::default()
        }
    }
    pub fn bold() -> Self {
        Self {
            bold: true,
            ..Default::default()
        }
    }
    pub fn right() -> Self {
        Self {
            align: Align::Right,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub content: String,
    pub align: Align,
}

impl Cell {
    pub fn left(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            align: Align::Left,
        }
    }
    pub fn right(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            align: Align::Right,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        content: String,
        #[serde(default)]
        style: TextStyle,
    },
    /// 左中右多欄。**這覆蓋收據排版 90% 的需求** ——
    /// 有它就不需要一個通用表格引擎。
    Columns {
        cells: Vec<Cell>,
        /// 欄寬權重。`[3, 1, 1]` = 品名 / 數量 / 小計。
        weights: Vec<u8>,
    },
    Rule {
        #[serde(default = "default_rule_char")]
        ch: char,
    },
    Feed {
        lines: u8,
    },
    /// 大字強調（廚房單的「內用 / 外帶」「桌號 A3」「※ 加點 ※」）。
    ///
    /// 獨立成 Block 而不是 TextStyle 的組合，因為兩種 renderer 的實作差很多：
    /// 文字模式是 `GS !` 倍寬倍高，raster 模式是換大字級再畫框。
    Banner {
        content: String,
        #[serde(default)]
        boxed: bool,
    },
    QrCode {
        data: String,
    },
    Barcode {
        data: String,
    },
}

fn default_rule_char() -> char {
    '-'
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptDoc {
    pub paper: PaperWidth,
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub finish: Finish,
}

impl ReceiptDoc {
    pub fn new(paper: PaperWidth) -> Self {
        Self {
            paper,
            blocks: Vec::new(),
            finish: Finish::default(),
        }
    }

    pub fn text(mut self, s: impl Into<String>) -> Self {
        self.blocks.push(Block::Text {
            content: s.into(),
            style: TextStyle::default(),
        });
        self
    }

    pub fn styled(mut self, s: impl Into<String>, style: TextStyle) -> Self {
        self.blocks.push(Block::Text {
            content: s.into(),
            style,
        });
        self
    }

    pub fn banner(mut self, s: impl Into<String>, boxed: bool) -> Self {
        self.blocks.push(Block::Banner {
            content: s.into(),
            boxed,
        });
        self
    }

    pub fn columns(mut self, cells: Vec<Cell>, weights: Vec<u8>) -> Self {
        self.blocks.push(Block::Columns { cells, weights });
        self
    }

    pub fn rule(mut self) -> Self {
        self.blocks.push(Block::Rule { ch: '-' });
        self
    }

    pub fn feed(mut self, lines: u8) -> Self {
        self.blocks.push(Block::Feed { lines });
        self
    }

    pub fn finish(mut self, finish: Finish) -> Self {
        self.finish = finish;
        self
    }
}

/// 把 `ReceiptDoc` 轉成可送給印表機的位元組。
///
/// v1 有兩個實作：`PlainTextRenderer`（預覽、快照測試、fakeprinter 的解碼輸出）
/// 與之後的 ESC/POS raster。v2 會加 `ZplRenderer` 支援標籤機。
pub trait ReceiptRenderer {
    fn render(&self, doc: &ReceiptDoc) -> crate::error::AppResult<Vec<u8>>;
}
