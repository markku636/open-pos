//! 出單機子系統。
//!
//! # 這一層唯一的寫入原語是 `send(bytes, deadline)`
//!
//! 不是 `print(doc)`、不是 `write_line()`。理由是**所有**出單機錯誤最後都長成
//! 同一個樣子：位元組送不出去。把原語收斂成一個，重試、佇列、死信這些真正難的
//! 東西才只需要寫一次。
//!
//! # `deadline` 不是可選的防禦性設計
//!
//! 缺紙的印表機會照常 accept TCP 連線但不 drain buffer，`write()` 於是永久阻塞。
//! 沒有 deadline 的話，那條 worker lane 會**靜默**死鎖 —— 該分區所有後續的單
//! 全部消失，而且不會有任何錯誤訊息。這是 POS 最惡劣的失敗模式。
//!
//! # 為什麼 driver 用 enum 分派而 repo 用 `Arc<dyn>`
//!
//! 資料層有 2 種後端、每支方法都伴隨 DB round trip，vtable 的 2ns 無關痛癢，
//! 而 trait 方法多，手寫 match 成本高。出單機反過來：trait 只有 7 支方法、
//! 其中 3 支有預設實作，而 `Active` 讓「哪些傳輸方式被編進去了」變成編譯期
//! 就看得見的事實（`printer-usb` feature 沒開時，那個 arm 根本不存在）。

pub mod escpos;
pub mod file;
pub mod network;

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::receipt::PaperWidth;

use escpos::{constants as c, CjkEncoding};

/// 各種操作的 deadline。
///
/// 取值的依據是「店員願意站在機器前面等多久」，不是網路 RTT ——
/// 廚房在等這張單，而收銀台前面站著客人。
pub mod deadline {
    use std::time::Duration;
    /// 探測：使用者按下「測試連線」正在看著畫面。
    pub const PROBE: Duration = Duration::from_secs(3);
    /// 文字模式送單：1–2 KB。
    pub const SEND_TEXT: Duration = Duration::from_secs(10);
    /// 點陣圖送單：80mm 40 行約 86 KB，慢機器要 30 秒。
    pub const SEND_RASTER: Duration = Duration::from_secs(30);
    /// 狀態查詢：查不到就當作查不到，不要卡住 UI。
    pub const STATUS: Duration = Duration::from_secs(2);
}

/// 傳輸方式。
///
/// v1 只實作 `Network` 與 `File`，但 enum 一次留好 —— 之後補 USB 不必動
/// 佇列、路由、設定檔的任何一行。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transport {
    /// TCP 9100（RAW / JetDirect）。台灣餐飲最常見的接法。
    Network { host: String, port: u16 },
    /// 寫進檔案。fakeprinter 的另一半、也是「先印到檔案看看」的除錯手段。
    File { path: String, append: bool },
    /// v1 不實作，僅保留形狀。走 nusb（純 Rust）不走 rusb/libusb。
    Usb {
        vid: u16,
        pid: u16,
        serial: Option<String>,
    },
    /// v1 不實作，僅保留形狀。
    Bluetooth { addr: String, channel: u8 },
}

impl Transport {
    /// 給人看的一行描述。錯誤訊息與診斷包都用它。
    pub fn describe(&self) -> String {
        match self {
            Transport::Network { host, port } => format!("網路 {host}:{port}"),
            Transport::File { path, .. } => format!("檔案 {path}"),
            Transport::Usb { vid, pid, .. } => format!("USB {vid:04x}:{pid:04x}"),
            Transport::Bluetooth { addr, .. } => format!("藍牙 {addr}"),
        }
    }
}

/// 印表機能力。
///
/// **不是**「規格表」而是「這台機器上我們敢做什麼」。查不到的一律當成沒有 ——
/// 樂觀假設在出單機上的代價是印出一半的單。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterCaps {
    pub paper: PaperWidth,
    /// 支援 `GS v 0` 點陣圖。幾乎所有機器都支援，但仍要能關掉。
    pub raster: bool,
    /// 有切刀。沒有的話送切紙指令通常無害，但預覽要照實顯示。
    pub cutter: bool,
    /// 有錢箱腳位。
    pub drawer: bool,
    /// 會回應 `DLE EOT` 狀態查詢。**大多數便宜機器不會**，
    /// 所以整套設計不能依賴它。
    pub status_query: bool,
    /// 內建中文字庫的編碼。文字模式才用得到。
    pub encoding: CjkEncoding,
}

impl PrinterCaps {
    /// 保守預設：只保證印得出字，其他一律當作沒有。
    pub fn conservative(paper: PaperWidth) -> Self {
        Self {
            paper,
            raster: true,
            cutter: true,
            drawer: false,
            status_query: false,
            encoding: CjkEncoding::Big5,
        }
    }
}

/// 重試分類。
///
/// **三類不是兩類。** 店員看到「飲料吧缺紙」會去換紙，看到「連線失敗」會去看
/// 網路線 —— 把兩者混成「印表機錯誤」等於什麼都沒講。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClass {
    /// 網路抖動、暫時忙碌。自動重試就會好。
    Transient,
    /// 缺紙、開蓋、卡刀。**要人去處理**，重試一萬次也沒用。
    NeedsAttention,
    /// 版面產生失敗、設定錯誤。重試永遠不會好，直接進死信。
    Permanent,
}

impl RetryClass {
    /// 從錯誤推斷分類。
    ///
    /// 這裡刻意保守：認不出來的一律當 `Transient`。誤判成暫時性最多多試幾次，
    /// 誤判成永久性會讓一張本來印得出來的單直接消失。
    pub fn of(err: &AppError) -> Self {
        match err {
            AppError::Timeout(_) => RetryClass::Transient,
            AppError::Unsupported(_) | AppError::Validation(_) => RetryClass::Permanent,
            AppError::Printer(msg) => {
                if msg.contains("缺紙") || msg.contains("開蓋") || msg.contains("卡紙") {
                    RetryClass::NeedsAttention
                } else {
                    RetryClass::Transient
                }
            }
            _ => RetryClass::Transient,
        }
    }

    /// 給店員看的一句話 —— 要直接說「去做什麼」，不是說「發生了什麼」。
    pub fn hint(&self) -> &'static str {
        match self {
            RetryClass::Transient => "系統會自動重試",
            RetryClass::NeedsAttention => "需要有人到機器旁邊處理（換紙／關蓋）",
            RetryClass::Permanent => "重試不會好，請檢查印表機設定",
        }
    }
}

/// `DLE EOT` 回來的狀態。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterStatus {
    pub online: bool,
    pub paper_out: bool,
    /// 紙快用完（不是所有機器都有這個感測器）。
    pub paper_near_end: bool,
    pub cover_open: bool,
    pub cutter_error: bool,
}

impl PrinterStatus {
    /// 解析 `DLE EOT 1`（印表機狀態）與 `DLE EOT 4`（紙張狀態）的回應位元組。
    ///
    /// 位元定義來自 ESC/POS 規範；bit 4 恆為 1、bit 0/1 恆為 0 是這個協定的
    /// 「這確實是一個狀態位元組」標記，收到不符的就別亂解讀。
    pub fn from_bytes(printer: Option<u8>, paper: Option<u8>) -> Self {
        let mut s = Self {
            online: true,
            ..Default::default()
        };
        if let Some(b) = printer.filter(|b| b & 0b1001_0011 == 0b0001_0010) {
            s.online = b & 0b0000_1000 == 0;
            s.cover_open = b & 0b0000_0100 != 0;
        }
        if let Some(b) = paper.filter(|b| b & 0b1001_0011 == 0b0001_0010) {
            s.paper_near_end = b & 0b0000_1100 != 0;
            s.paper_out = b & 0b0110_0000 != 0;
        }
        s
    }

    /// 有沒有需要人去處理的事。
    pub fn needs_attention(&self) -> bool {
        !self.online || self.paper_out || self.cover_open || self.cutter_error
    }
}

/// 錢箱腳位。兩個腳位是因為一台機器可以接兩個錢箱（兩個收銀員各一個）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CashDrawerPin {
    Pin2,
    Pin5,
}

/// 出單機驅動。
#[async_trait]
pub trait PrinterDriver: Send + Sync {
    fn describe(&self) -> String;
    fn caps(&self) -> PrinterCaps;

    /// 確認機器真的在。**只證明連得上**，不證明有紙。
    async fn probe(&self) -> AppResult<()>;

    /// 唯一的寫入原語。全有全無，且必須在 deadline 內完成。
    async fn send(&self, bytes: &[u8], deadline: Duration) -> AppResult<()>;

    /// 查狀態。大多數便宜機器不回應，所以預設就是「不支援」——
    /// 呼叫端必須把「查不到」當成正常情況處理，不是錯誤。
    async fn status(&self) -> AppResult<PrinterStatus> {
        Err(AppError::Unsupported("這台機器不支援狀態查詢".into()))
    }

    /// 彈錢箱。走印表機的腳位，不是獨立裝置。
    async fn open_cash_drawer(&self, pin: CashDrawerPin) -> AppResult<()> {
        let m = match pin {
            CashDrawerPin::Pin2 => 0,
            CashDrawerPin::Pin5 => 1,
        };
        // t1=25(×2ms 通電)、t2=250(×2ms 斷電)：這組值幾乎所有錢箱都吃得動。
        // 通電太短彈不開，太長會讓線圈發燙。
        let mut bytes = c::DRAWER.to_vec();
        bytes.extend_from_slice(&[m, 25, 250]);
        self.send(&bytes, deadline::PROBE).await
    }

    async fn close(&self) {}
}

/// 內建 driver 的靜態分派。
pub enum Active {
    Network(network::NetworkPrinter),
    File(file::FilePrinter),
}

impl Active {
    /// 依 `Transport` 開一個 driver。
    ///
    /// 尚未實作的傳輸方式回 `Unsupported` 而不是 panic —— 設定檔可能來自
    /// 較新版本的 app，而使用者不該因為一台印表機設錯就開不了店。
    pub fn open(transport: &Transport, caps: PrinterCaps) -> AppResult<Self> {
        match transport {
            Transport::Network { host, port } => Ok(Active::Network(network::NetworkPrinter::new(
                host.clone(),
                *port,
                caps,
            ))),
            Transport::File { path, append } => Ok(Active::File(file::FilePrinter::new(
                path.into(),
                *append,
                caps,
            ))),
            Transport::Usb { .. } => Err(AppError::Unsupported(
                "USB 出單機還沒支援（v1 只做網路型），請改用網路連線".into(),
            )),
            Transport::Bluetooth { .. } => Err(AppError::Unsupported(
                "藍牙出單機還沒支援（v1 只做網路型），請改用網路連線".into(),
            )),
        }
    }

    fn as_driver(&self) -> &dyn PrinterDriver {
        match self {
            Active::Network(d) => d,
            Active::File(d) => d,
        }
    }
}

#[async_trait]
impl PrinterDriver for Active {
    fn describe(&self) -> String {
        self.as_driver().describe()
    }
    fn caps(&self) -> PrinterCaps {
        self.as_driver().caps()
    }
    async fn probe(&self) -> AppResult<()> {
        self.as_driver().probe().await
    }
    async fn send(&self, bytes: &[u8], deadline: Duration) -> AppResult<()> {
        self.as_driver().send(bytes, deadline).await
    }
    async fn status(&self) -> AppResult<PrinterStatus> {
        self.as_driver().status().await
    }
    async fn close(&self) {
        self.as_driver().close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unimplemented_transports_are_refused_not_panicked() {
        // 設定檔可能來自較新版本的 app。一台印表機設錯不該讓店家開不了店。
        let Err(e) = Active::open(
            &Transport::Usb {
                vid: 0x04b8,
                pid: 0x0202,
                serial: None,
            },
            PrinterCaps::conservative(PaperWidth::Mm80),
        ) else {
            panic!("USB 應該還沒支援");
        };
        assert_eq!(e.code(), "ERR_UNSUPPORTED");
        // 錯誤訊息要給出正確做法，不能只說「不支援」。
        assert!(e.message().contains("網路"), "{}", e.message());
    }

    #[test]
    fn unknown_failures_are_retried_not_dropped() {
        // 誤判成暫時性最多多試幾次；誤判成永久性會讓一張印得出來的單消失。
        assert_eq!(
            RetryClass::of(&AppError::Printer("connection reset".into())),
            RetryClass::Transient
        );
        assert_eq!(
            RetryClass::of(&AppError::Printer("缺紙".into())),
            RetryClass::NeedsAttention
        );
        assert_eq!(
            RetryClass::of(&AppError::Timeout(10)),
            RetryClass::Transient
        );
        assert_eq!(
            RetryClass::of(&AppError::Unsupported("x".into())),
            RetryClass::Permanent
        );
    }

    #[test]
    fn status_bytes_decode_paper_out_and_cover_open() {
        // bit4 恆 1、bit0/1 恆 0 是「這確實是狀態位元組」的標記。
        let s = PrinterStatus::from_bytes(Some(0b0001_0110), Some(0b0111_0010));
        assert!(s.cover_open);
        assert!(s.paper_out);
        assert!(s.needs_attention());

        let ok = PrinterStatus::from_bytes(Some(0b0001_0010), Some(0b0001_0010));
        assert!(!ok.needs_attention());
    }

    #[test]
    fn garbage_status_bytes_are_ignored_rather_than_misread() {
        // 半吊子機器會回一堆雜訊。把雜訊解讀成「缺紙」會讓店員白跑一趟。
        let s = PrinterStatus::from_bytes(Some(0xFF), Some(0xFF));
        assert!(!s.needs_attention());
    }
}
