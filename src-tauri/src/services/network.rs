//! 區網連線狀態。
//!
//! # 為什麼這一頁值得存在
//!
//! **裝機第一天的頭號故障就是「平板連不上收銀機」**，而它的三個成因
//! 都不會在任何錯誤訊息裡出現：
//!
//! 1. Windows 防火牆的對話框在第一次啟動時跳出來，而使用者按了「取消」。
//! 2. 路由器跳電重開，主機的 IP 從 .23 變成 .24 —— 全店印好的桌卡 QR 同時作廢。
//! 3. 主機有好幾張網卡（Wi-Fi、有線、Docker、WSL、VPN），而系統挑了錯的那張。
//!
//! 三個都是「東西看起來都正常，就是連不上」。所以這裡把**平板要開的網址**、
//! **所有候選網卡**、**上一次的 IP** 全部攤開來給人看。
//!
//! # 這個檢查證明了什麼、沒證明什麼
//!
//! 從自己的區網 IP 連自己，只能證明「server 真的綁在區網介面上」——
//! 那會抓到「只綁了 127.0.0.1」這一類錯誤。
//!
//! **它證明不了防火牆有沒有放行**：Windows 對「自己連自己」多半在核心裡就
//! 短路了，根本不經過防火牆。唯一能證明防火牆的是**另一台裝置**，所以畫面上
//! 要老實說出這件事，並且給出那台裝置該開的網址 —— 而不是印一個綠燈讓人
//! 以為沒問題。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::ctx::Ctx;
use crate::error::AppResult;

/// 上一次看到的區網 IP 存在這個 key 底下。
const LAST_IP_KEY: &str = "lan.last_ip";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInterface {
    pub name: String,
    pub ip: String,
    /// 這一張是不是我們挑來當區網位址的那一張。
    pub chosen: bool,
    /// 明顯不該拿來用的（虛擬網卡、DHCP 失敗的 169.254）。
    pub usable: bool,
    /// 這張網卡為什麼不能用。`None` = 可以用。
    ///
    /// **回代碼不回句子** —— 後端不知道看的人要哪一種語言。
    /// 廠商名（Docker / WSL / …）是專有名詞，三種語言都一樣，所以直接帶過去。
    pub note: Option<NicNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanStatus {
    pub port: u16,
    pub ip: Option<String>,
    /// 廚房平板要開的網址。
    pub kds_url: Option<String>,
    /// 顧客手機掃 QR 會開的網址（v1.3 才會用到）。
    pub order_url: Option<String>,
    /// server 真的綁在區網介面上。**這不等於防火牆有放行。**
    pub bound: bool,
    /// 探測結果。同樣回代碼，句子在前端組。
    pub detail: LanDetail,
    pub interfaces: Vec<NetInterface>,
    /// 上一次記下來的 IP。跟現在不一樣時，桌卡 QR 全部要重印。
    pub previous_ip: Option<String>,
    /// IP 換過了。
    pub ip_changed: bool,
    /// 複製去用系統管理員身分執行的防火牆指令。
    pub firewall_command: String,
}

pub async fn lan_status(ctx: &Ctx, port: u16) -> AppResult<LanStatus> {
    let interfaces = interfaces();
    let chosen = interfaces.iter().find(|i| i.chosen).map(|i| i.ip.clone());

    let (bound, detail) = match &chosen {
        Some(ip) => probe(ip, port),
        None => (false, LanDetail::NoAddress),
    };

    let previous_ip = get_setting(ctx, LAST_IP_KEY).await?;
    let ip_changed = matches!((&previous_ip, &chosen), (Some(a), Some(b)) if a != b);
    if let Some(ip) = &chosen {
        // 記下來，下次啟動才比得出來。
        set_setting(ctx, LAST_IP_KEY, ip).await?;
    }

    Ok(LanStatus {
        port,
        kds_url: chosen
            .as_ref()
            .map(|ip| format!("http://{ip}:{port}/kds.html")),
        order_url: chosen
            .as_ref()
            .map(|ip| format!("http://{ip}:{port}/order.html")),
        ip: chosen,
        bound,
        detail,
        interfaces,
        previous_ip,
        ip_changed,
        // 不自己偷偷提權改防火牆：開源專案這樣做會被質疑，而且使用者也該
        // 知道自己的機器被改了什麼。給指令讓他自己執行。
        firewall_command: format!(
            "netsh advfirewall firewall add rule name=\"open-pos\" \
             dir=in action=allow protocol=TCP localport={port}"
        ),
    })
}

/// 從區網位址連自己一次。
/// 一張網卡不能用的原因。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NicNote {
    /// loopback / link_local / not_private / virtual
    ///
    /// 用 String 而不是 &'static str：derive 的 Deserialize 需要擁有所有權的資料。
    /// 網卡數量是個位數，這點配置成本無所謂。
    pub code: String,
    /// `virtual` 時是廠商名（Docker、WSL…）。專有名詞，不翻譯。
    pub vendor: Option<String>,
}

/// 區網探測的結果。
///
/// OS 給的錯誤字串（`error`）原樣帶著 —— 它本來就不是我們的文案，
/// 翻譯它只會讓使用者更難把訊息貼到搜尋引擎裡。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum LanDetail {
    /// 一張可用的網卡都找不到。
    NoAddress,
    /// server 確實綁在這張網卡上。
    ///
    /// 注意這**不等於**平板連得進來 —— 從自己連自己證明不了防火牆有放行，
    /// 前端的文案必須把這件事說清楚。
    Bound,
    ResolveFailed {
        addr: String,
        error: String,
    },
    ConnectFailed {
        addr: String,
        error: String,
    },
}

fn probe(ip: &str, port: u16) -> (bool, LanDetail) {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let addr = match format!("{ip}:{port}").to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => {
                return (
                    false,
                    LanDetail::ResolveFailed {
                        addr: format!("{ip}:{port}"),
                        error: String::new(),
                    },
                )
            }
        },
        Err(e) => {
            return (
                false,
                LanDetail::ResolveFailed {
                    addr: format!("{ip}:{port}"),
                    error: e.to_string(),
                },
            )
        }
    };
    match TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
        Ok(_) => (true, LanDetail::Bound),
        Err(e) => (
            false,
            LanDetail::ConnectFailed {
                addr: format!("{ip}:{port}"),
                error: e.to_string(),
            },
        ),
    }
}

/// 所有 IPv4 網卡，附上「這張能不能用」的判斷。
fn interfaces() -> Vec<NetInterface> {
    let chosen = local_ip_address::local_ip().ok().map(|ip| ip.to_string());
    let mut out: Vec<NetInterface> = local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(name, ip)| {
            let v4 = match ip {
                // IPv6 一律不列：店家 AP 的設定千奇百怪，而 URL 要用方括號，
                // 使用者看到會怕。
                std::net::IpAddr::V6(_) => return None,
                std::net::IpAddr::V4(v4) => v4,
            };
            let note = disqualify(&name, &v4);
            Some(NetInterface {
                chosen: chosen.as_deref() == Some(&v4.to_string()),
                usable: note.is_none(),
                note,
                ip: v4.to_string(),
                name,
            })
        })
        .collect();
    // 能用的排前面，挑中的排最前面。
    out.sort_by_key(|i| (!i.chosen, !i.usable, i.ip.clone()));
    out
}

/// 這張網卡為什麼不該用。`None` = 可以用。
///
/// 開發機上的虛擬網卡多到爆（Docker、WSL、VirtualBox、VPN），不標出來的話
/// 使用者一定選錯 —— 而且是「QR 都印出去貼在桌上了才發現連不上」這種最貴的錯法。
fn disqualify(name: &str, ip: &std::net::Ipv4Addr) -> Option<NicNote> {
    let note = |code: &str| {
        Some(NicNote {
            code: code.to_string(),
            vendor: None,
        })
    };
    if ip.is_loopback() {
        return note("loopback");
    }
    if ip.is_link_local() {
        return note("link_local");
    }
    if !ip.is_private() {
        return note("not_private");
    }
    let lower = name.to_ascii_lowercase();
    // 廠商名是專有名詞，三種語言都一樣，所以帶著代碼一起回去，
    // 句子（「…的虛擬網卡，不是店裡的網路」）由前端組。
    for (needle, vendor) in [
        ("docker", "Docker"),
        ("wsl", "WSL"),
        ("vethernet", "Hyper-V"),
        ("virtualbox", "VirtualBox"),
        ("vmware", "VMware"),
        ("tailscale", "Tailscale"),
        ("zerotier", "ZeroTier"),
    ] {
        if lower.contains(needle) {
            return Some(NicNote {
                code: "virtual".to_string(),
                vendor: Some(vendor.to_string()),
            });
        }
    }
    None
}

async fn get_setting(ctx: &Ctx, key: &str) -> AppResult<Option<String>> {
    Ok(
        sqlx::query("SELECT value_json FROM app_settings WHERE key = ?1")
            .bind(key)
            .fetch_optional(ctx.db.reader())
            .await?
            .map(|r| r.get::<String, _>("value_json")),
    )
}

async fn set_setting(ctx: &Ctx, key: &str, value: &str) -> AppResult<()> {
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    sqlx::query(
        "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE
             SET value_json = excluded.value_json, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    uow.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn virtual_adapters_are_called_out() {
        assert!(disqualify("vEthernet (WSL)", &Ipv4Addr::new(172, 20, 0, 1)).is_some());
        assert!(disqualify("docker0", &Ipv4Addr::new(172, 17, 0, 1)).is_some());
        // 一般的家用／店用網段可以用。
        assert!(disqualify("Wi-Fi", &Ipv4Addr::new(192, 168, 1, 23)).is_none());
        assert!(disqualify("乙太網路", &Ipv4Addr::new(10, 0, 0, 5)).is_none());
    }

    #[test]
    fn dhcp_failure_is_explained_not_just_rejected() {
        // 169.254.x.x 是 DHCP 失敗，而「這個位址明天就會變」正是使用者
        // 需要知道的那一句 —— 只說「不可用」他會以為是程式的問題。
        //
        // 現在斷言的是**分類**而不是文案：句子在前端的字典裡（三種語言各一份），
        // 這裡只要保證這個 IP 被歸到 link_local 而不是被籠統地拒絕。
        let why = disqualify("Wi-Fi", &Ipv4Addr::new(169, 254, 3, 9)).unwrap();
        assert_eq!(why.code, "link_local");
        assert!(why.vendor.is_none());
    }

    #[test]
    fn a_virtual_adapter_reports_which_vendor_it_is() {
        // 廠商名是專有名詞，三種語言都一樣，所以後端帶它過去。
        // 少了它，畫面上只會說「虛擬網卡」，而店裡可能同時有三張。
        let why = disqualify("vEthernet (WSL)", &Ipv4Addr::new(172, 20, 0, 1)).unwrap();
        assert_eq!(why.code, "virtual");
        assert_eq!(why.vendor.as_deref(), Some("WSL"));

        let why = disqualify("docker0", &Ipv4Addr::new(172, 17, 0, 1)).unwrap();
        assert_eq!(why.vendor.as_deref(), Some("Docker"));
    }
}
