//! ESC/POS 指令常數。
//!
//! **這個檔案是編碼器與解碼器共用的單一事實來源。**
//! `fakeprinter` 的解析器也 import 它 —— 改指令時編譯器會逼你兩邊一起改，
//! 不會出現「模擬器一直說 OK 但真機印不出來」。

/// ESC @ —— 初始化。清掉上一張單留下的樣式狀態。
pub const INIT: &[u8] = &[0x1B, 0x40];

/// ESC a n —— 對齊。0=左 1=中 2=右
pub const ALIGN: u8 = b'a';
pub const ALIGN_LEFT: u8 = 0;
pub const ALIGN_CENTER: u8 = 1;
pub const ALIGN_RIGHT: u8 = 2;

/// ESC E n —— 粗體。
pub const EMPHASIS: u8 = b'E';
/// ESC - n —— 底線。
pub const UNDERLINE: u8 = b'-';
/// GS B n —— 反白（白字黑底）。
pub const REVERSE: &[u8] = &[0x1D, 0x42];
/// GS ! n —— 字級。高 4 bit 是寬度倍數、低 4 bit 是高度倍數，各 0..=7。
pub const SIZE: &[u8] = &[0x1D, 0x21];

/// FS & —— 進入漢字模式。
pub const KANJI_ON: &[u8] = &[0x1C, 0x26];
/// FS . —— 離開漢字模式。
///
/// ⚠️ 送純 ASCII 之前**必須**離開，否則 ASCII 會被當成漢字的高位元組，
/// 印出來是一片亂碼。
pub const KANJI_OFF: &[u8] = &[0x1C, 0x2E];

/// GS V m —— 切紙。66 = 走紙後半切。
pub const CUT: &[u8] = &[0x1D, 0x56];
pub const CUT_PARTIAL: u8 = 66;
pub const CUT_FULL: u8 = 65;

/// ESC d n —— 走 n 行。
pub const FEED_LINES: &[u8] = &[0x1B, 0x64];

/// ESC p m t1 t2 —— 開錢箱。
pub const DRAWER: &[u8] = &[0x1B, 0x70];

/// GS v 0 —— 光柵點陣圖。
pub const RASTER: &[u8] = &[0x1D, 0x76, 0x30];

/// DLE EOT n —— 即時狀態查詢。
///
/// ⚠️ 很多便宜機不回應這個指令，所以查詢一定要有 timeout，
/// 而且拿不到狀態時要退回「送出即算成功」的樂觀模式，不能卡住整條佇列。
pub const STATUS_QUERY: &[u8] = &[0x10, 0x04];
