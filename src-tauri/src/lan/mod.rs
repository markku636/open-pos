//! 區網 HTTP server：KDS 與顧客掃碼點餐的後端。
//!
//! # RPC over HTTP，不是 REST
//!
//! 端點只有一個形狀：`POST /api/rpc/{name}`，而 `{name}` 與 Tauri command 的
//! 名稱**一對一**。這不是偷懶 —— 它讓前端的 `shared/api.ts` 只需要一份：
//!
//! ```ts
//! const tauri = { call: (n, a) => invoke(n, a) }
//! const http  = { call: (n, a) => fetch(`/api/rpc/${n}`, {...}) }
//! ```
//!
//! 回應形狀也對齊：成功時直接回 T 的 JSON（**不包 envelope**），
//! 失敗時非 2xx + `{"error": AppError}`。因為 Tauri 的 invoke 成功會 resolve 出 T、
//! 失敗會 reject 出 AppError，HTTP 端對齊之後兩邊的產出完全同形。
//!
//! # 權限邊界
//!
//! **管理功能不存在於這個 server。** 改菜單、看報表、設定印表機、關班只走 Tauri IPC。
//! 這樣就算 LAN server 有漏洞，攻擊面也只到「亂送單」，到不了「看營業額 / 改價格」。

pub mod router;

use std::net::SocketAddr;

use tokio::sync::oneshot;

use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};

pub const DEFAULT_PORT: u16 = 8129;

pub struct LanHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
    pub addr: SocketAddr,
}

impl LanHandle {
    /// 優雅關閉。app 結束前呼叫，讓進行中的請求做完。
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}

/// 啟動 LAN server。
///
/// 先用**同步**的 `std::net::TcpListener` 綁定，才能在啟動流程裡當場知道
/// 「連接埠被佔用」並跳對話框請使用者換一個 —— 而不是丟一個背景 task
/// 讓它默默失敗、使用者一頭霧水地問「為什麼平板連不上」。
pub fn spawn(ctx: Ctx, port: u16, ui_dir: Option<std::path::PathBuf>) -> AppResult<LanHandle> {
    let std_listener = std::net::TcpListener::bind(("0.0.0.0", port)).map_err(|e| {
        AppError::Startup(format!(
            "無法綁定連接埠 {port}：{e}\n\n\
             可能是另一個程式（或另一個 open-pos）已經佔用它。\n\
             請到設定頁改用其他連接埠，或關掉佔用的程式。"
        ))
    })?;
    std_listener
        .set_nonblocking(true)
        .map_err(|e| AppError::Startup(format!("設定 socket 失敗：{e}")))?;
    let addr = std_listener
        .local_addr()
        .map_err(|e| AppError::Startup(format!("取得位址失敗：{e}")))?;

    let listener = tokio::net::TcpListener::from_std(std_listener)
        .map_err(|e| AppError::Startup(format!("轉換 listener 失敗：{e}")))?;

    let app = router::build_with_ui(ctx, ui_dir);
    let (tx, rx) = oneshot::channel::<()>();

    let task = tokio::spawn(async move {
        let served = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
        if let Err(e) = served {
            tracing::error!(error = %e, "LAN server 結束於錯誤");
        }
    });

    tracing::info!(%addr, "LAN server 已啟動");
    Ok(LanHandle {
        shutdown: Some(tx),
        task,
        addr,
    })
}

/// 適合印進桌卡 QR 的區網位址。
///
/// 挑選規則刻意保守 —— 開發機上的虛擬網卡多到爆，不濾掉的話使用者一定選錯，
/// 而且是「QR 都印出去貼在桌上了才發現連不上」這種最貴的錯法。
#[cfg(feature = "server")]
pub fn lan_base_url(port: u16) -> Option<String> {
    let ip = local_ip_address::local_ip().ok()?;
    if !is_usable(&ip) {
        return None;
    }
    Some(format!("http://{ip}:{port}"))
}

#[cfg(feature = "server")]
fn is_usable(ip: &std::net::IpAddr) -> bool {
    match ip {
        // IPv6 在 v1 一律排除。理由不是技術而是現場：店家 AP 的 IPv6 設定千奇百怪，
        // 而且 URL 裡要用方括號，QR 掃出來使用者看到會怕。
        std::net::IpAddr::V6(_) => false,
        std::net::IpAddr::V4(v4) => {
            // link-local（169.254.x）代表 DHCP 失敗，這個位址明天就會變。
            !v4.is_loopback() && !v4.is_link_local() && v4.is_private()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn rejects_loopback_and_apipa() {
        assert!(!is_usable(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        // DHCP 失敗時 Windows 會給 169.254.x.x —— 拿它去印 QR 明天就失效。
        assert!(!is_usable(&IpAddr::V4(Ipv4Addr::new(169, 254, 1, 5))));
        // 公開位址不該出現在區網 QR 上。
        assert!(!is_usable(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    }

    #[test]
    fn accepts_typical_home_router_ranges() {
        assert!(is_usable(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50))));
        assert!(is_usable(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8))));
        assert!(is_usable(&IpAddr::V4(Ipv4Addr::new(172, 16, 3, 9))));
    }
}
