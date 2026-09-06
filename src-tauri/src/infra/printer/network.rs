//! 網路型 ESC/POS（TCP 9100，俗稱 RAW / JetDirect）。
//!
//! v1 唯一真正會碰到硬體的 driver。選它當第一個的理由不是它最好接，
//! 而是**它是唯一不需要驅動程式的接法** —— 廠商 SDK 常常只有 Windows DLL，
//! Linux 根本沒有驅動；而 9100 就是一條 TCP 連線送位元組，任何 OS 都一樣。
//!
//! # 為什麼每次送單都重開連線
//!
//! 三個實務理由，缺一不可：
//!
//! 1. **9100 通常只接受一條連線。** 長連線會把機器佔住，店家的其他軟體
//!    （或我們自己的 probe）就連不上了。
//! 2. **半開連線是隱形的。** 印表機重開機或 AP 換頻道之後，我們這端的 socket
//!    看起來還活著，直到某次 write 才失敗 —— 而那時單已經「送出去」了。
//! 3. 每天的量是幾百張單，重開連線的成本（一次 TCP 三向交握）完全不重要。

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::{AppError, AppResult};

use super::escpos::constants as c;
use super::{deadline, PrinterCaps, PrinterDriver, PrinterStatus};

pub struct NetworkPrinter {
    host: String,
    port: u16,
    caps: PrinterCaps,
}

impl NetworkPrinter {
    pub fn new(host: String, port: u16, caps: PrinterCaps) -> Self {
        Self { host, port, caps }
    }

    async fn connect(&self, budget: Duration) -> AppResult<TcpStream> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream = tokio::time::timeout(budget, TcpStream::connect(&addr))
            .await
            .map_err(|_| AppError::Timeout(budget.as_secs()))?
            .map_err(|e| AppError::Printer(format!("連不上 {addr}：{e}")))?;
        // 收據是一次一小包位元組。Nagle 會為了湊滿 MTU 而多等 40ms，
        // 對「按下結帳到吐紙」的體感是實打實的延遲。
        let _ = stream.set_nodelay(true);
        Ok(stream)
    }
}

#[async_trait]
impl PrinterDriver for NetworkPrinter {
    fn describe(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn caps(&self) -> PrinterCaps {
        self.caps.clone()
    }

    async fn probe(&self) -> AppResult<()> {
        let mut s = self.connect(deadline::PROBE).await?;
        // 連得上就算過。**刻意不送任何位元組** —— probe 是使用者按「測試連線」
        // 時跑的，不該在客人面前吐出一張空白紙。
        let _ = s.shutdown().await;
        Ok(())
    }

    async fn send(&self, bytes: &[u8], budget: Duration) -> AppResult<()> {
        // ★ 整段（連線 + 寫入 + flush）共用同一個 deadline。
        //
        // 缺紙的印表機會照常 accept TCP 但不 drain buffer，於是 write 永久阻塞。
        // 少了這個 timeout，該分區的 worker 會靜默死鎖 —— 後續所有單消失且
        // 沒有任何錯誤訊息。這是 POS 最惡劣的失敗模式。
        let addr = format!("{}:{}", self.host, self.port);
        let n = bytes.len();
        tokio::time::timeout(budget, async {
            let mut s = self.connect(budget).await?;
            s.write_all(bytes).await.map_err(|e| {
                AppError::Printer(format!("送到 {addr} 失敗（已送 {n} bytes）：{e}"))
            })?;
            s.flush()
                .await
                .map_err(|e| AppError::Printer(format!("送到 {addr} 未完成：{e}")))?;
            // shutdown 讓對端知道這一單送完了。有些機器要看到 FIN 才開始切紙。
            let _ = s.shutdown().await;
            Ok::<(), AppError>(())
        })
        .await
        .map_err(|_| AppError::Timeout(budget.as_secs()))?
    }

    async fn status(&self) -> AppResult<PrinterStatus> {
        if !self.caps.status_query {
            return Err(AppError::Unsupported("這台機器不支援狀態查詢".into()));
        }
        let budget = deadline::STATUS;
        tokio::time::timeout(budget, async {
            let mut s = self.connect(budget).await?;
            let mut query = c::STATUS_QUERY.to_vec();
            query.push(1); // DLE EOT 1：印表機狀態
            query.extend_from_slice(c::STATUS_QUERY);
            query.push(4); // DLE EOT 4：紙張狀態
            s.write_all(&query)
                .await
                .map_err(|e| AppError::Printer(format!("查詢狀態失敗：{e}")))?;

            let mut buf = [0u8; 2];
            // 讀不滿兩個位元組就當作這台機器不回應 —— 這比等到 timeout 誠實，
            // 而且大多數便宜機器確實不回應。
            let got = s.read_exact(&mut buf).await.map(|_| true).unwrap_or(false);
            if !got {
                return Err(AppError::Unsupported("這台機器沒有回應狀態查詢".into()));
            }
            Ok(PrinterStatus::from_bytes(Some(buf[0]), Some(buf[1])))
        })
        .await
        .map_err(|_| AppError::Timeout(budget.as_secs()))?
    }
}

/// 主機名稱看起來像不像「使用者其實想輸入 IP 卻打錯了」。
///
/// 裝機第一天的頭號故障是打錯 IP，而症狀是「按了結帳沒反應」。
/// 在設定頁就攔下來，比讓它變成一張進死信的單好得多。
pub fn looks_like_a_typo(host: &str) -> Option<&'static str> {
    if host.trim().is_empty() {
        return Some("沒有填印表機位址");
    }
    if host.contains(' ') {
        return Some("位址裡有空白");
    }
    if host.starts_with("http") {
        return Some("這裡要填 IP 或主機名稱，不是網址");
    }
    if Path::new(host).is_absolute() {
        return Some("這裡要填 IP 或主機名稱，不是檔案路徑");
    }
    // 四段數字但其中一段 > 255：使用者打錯位數的典型樣子（192.168.1.2555）。
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() == 4
        && parts
            .iter()
            .all(|p| p.chars().all(|ch| ch.is_ascii_digit()))
        && parts.iter().any(|p| p.parse::<u32>().unwrap_or(999) > 255)
    {
        return Some("IP 位址的每一段都必須在 0–255 之間");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::PaperWidth;

    fn printer(port: u16) -> NetworkPrinter {
        NetworkPrinter::new(
            "127.0.0.1".into(),
            port,
            PrinterCaps::conservative(PaperWidth::Mm80),
        )
    }

    #[tokio::test]
    async fn probe_fails_fast_on_a_closed_port() {
        // 埠沒開時 connect 會立刻 refused，不該等到 deadline。
        let e = printer(1).probe().await.unwrap_err();
        assert_eq!(e.code(), "ERR_PRINTER");
        assert!(e.message().contains("127.0.0.1:1"), "{}", e.message());
    }

    /// 一台 accept 了連線卻永遠不回話的機器。缺紙的出單機就長這樣。
    async fn black_hole() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut kept = Vec::new();
            while let Ok((s, _)) = listener.accept().await {
                kept.push(s); // 收下連線，什麼都不做 —— 正是缺紙印表機的行為。
            }
        });
        port
    }

    #[tokio::test]
    async fn a_silent_printer_cannot_hang_the_worker() {
        // ★ 這條測試守的是整個列印子系統最惡劣的失敗模式：
        // 機器 accept 了連線卻不回話，worker 就永久卡在那裡 ——
        // 該分區後續所有的單靜默消失，而且不會有任何錯誤訊息。
        let port = black_hole().await;
        let p = NetworkPrinter::new(
            "127.0.0.1".into(),
            port,
            PrinterCaps {
                status_query: true,
                ..PrinterCaps::conservative(PaperWidth::Mm80)
            },
        );
        let started = std::time::Instant::now();
        let e = p.status().await.unwrap_err();
        assert_eq!(e.code(), "ERR_TIMEOUT");
        assert!(
            started.elapsed() < deadline::STATUS * 3,
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn send_never_outlives_its_deadline() {
        // 送到一個永遠不會有回應的位址（RFC 5737 TEST-NET-1，保證不被路由）。
        // 結果是 timeout 還是「網路不可達」取決於這台機器有沒有預設路由，
        // 兩種都可以接受 —— 這條測試要守的是**有界**，不是特定的錯誤碼。
        //
        // 註：無法用「灌爆 socket 緩衝區」來重現 write 阻塞。Windows 對一條全新
        // 連線的第一次 write 幾乎照單全收（實測 256MB 仍不阻塞），所以那條路徑
        // 在這個平台上測不出來。deadline 的包覆是同一段程式碼，由上面那條
        // status 測試證明它確實會觸發。
        let p = NetworkPrinter::new(
            "192.0.2.1".into(),
            9100,
            PrinterCaps::conservative(PaperWidth::Mm58),
        );
        let started = std::time::Instant::now();
        let e = p
            .send(b"hello", Duration::from_millis(300))
            .await
            .unwrap_err();
        assert!(
            matches!(e.code(), "ERR_TIMEOUT" | "ERR_PRINTER"),
            "非預期的錯誤：{}",
            e.message()
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "send 花了 {:?}，遠超過 300ms 的 deadline",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_small_job_goes_through() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).await.unwrap();
            buf
        });

        printer(port)
            .send(b"HELLO", deadline::SEND_TEXT)
            .await
            .unwrap();
        assert_eq!(server.await.unwrap(), b"HELLO");
    }

    #[test]
    fn common_address_typos_are_caught_before_they_become_a_dead_letter() {
        assert!(looks_like_a_typo("192.168.1.23").is_none());
        assert!(looks_like_a_typo("printer-kitchen").is_none());
        assert!(looks_like_a_typo("192.168.1.2555").is_some());
        assert!(looks_like_a_typo("http://192.168.1.23").is_some());
        assert!(looks_like_a_typo("  ").is_some());
    }
}
