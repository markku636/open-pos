//! 電子金流。
//!
//! # 這一層真正要解的問題不是「怎麼呼叫 API」
//!
//! 是**「請求送出去了，但回應沒收到」**。
//!
//! 刷卡失敗不可怕 —— 收銀員當場就看得到，重刷一次就好。真正可怕的是網路在
//! 送出之後、回應之前斷掉：錢在金流商那邊已經扣了，POS 這邊什麼都不知道。
//! 客人走了，而店家要到隔月對帳單才發現多收了一筆，那時已經找不到人。
//!
//! 所以這裡的每一次對外請求都是三段：
//!
//! 1. **送出之前**先寫一列 `gateway_transactions`（`pending`），並 commit。
//! 2. 呼叫 API。
//! 3. 回應回來才改狀態。**超時或連不上時改成 `unknown`，不是 `failed`。**
//!
//! `unknown` 是這一層最重要的一個狀態：它的意思是「要去對帳」，
//! 而不是「沒有發生」。把它併進 `failed` 會讓錢無聲地消失。
//!
//! # 交易編號在重試時不換
//!
//! 金流商認的是我方送過去的訂單號。重試時**沿用同一個**，才是「不要扣兩次」
//! 唯一的保證 —— 換一個新的等於明確地要求對方再扣一次。
//!
//! # 為什麼付款走在結帳之前
//!
//! 因為「錢收到了但單沒結」可以補結；「單結了但錢沒收到」是把商品送出去。
//! 前者店員看得到（單還在未結清單上），後者沒有人看得到。
//!
//! # 憑證
//!
//! 存在 `payment_gateways.credentials_json`，而**那一欄跟資料庫一樣敏感**。
//! 診斷包與 log 一律不含它（見 `diagnostics.rs`），而備份出去的 `.db`
//! 要當成含有金流憑證的檔案來保管。這一點在設定頁上直接寫給店家看 ——
//! 假裝它被加密了比誠實地說出來更危險。

pub mod config;
pub mod linepay;
pub mod manual;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// 一次付款請求。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayRequest {
    /// 我方交易編號。**重試時沿用同一個。**
    pub merchant_trade_no: String,
    /// 整數元。台幣沒有小數。
    pub amount: i64,
    /// 給客人看的商品說明（金流商多半會顯示在他們的付款頁上）。
    pub description: String,
    /// 客人出示的一次性條碼／QR（掃客人手機那一種流程才有）。
    pub one_time_key: Option<String>,
}

/// 一次付款的結果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayOutcome {
    pub status: TxStatus,
    /// 金流商的交易號。對帳時要用。
    pub gateway_trade_no: Option<String>,
    /// 需要客人動作時，要開給他掃的網址／QR 內容。
    pub redirect_url: Option<String>,
    /// 原始回應。**存起來是為了對帳，不是為了顯示。**
    pub raw: Option<String>,
    /// 給收銀員看的一句話。
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxStatus {
    /// 已經送出，還在等客人／等金流商。
    Pending,
    /// 授權了但還沒請款。
    Authorized,
    /// 錢確定收到了。
    Captured,
    /// 明確失敗（餘額不足、卡片被拒）。**可以放心重來。**
    Failed,
    Cancelled,
    /// ★ 送出去了，但不知道結果。**不可以當成沒發生。**
    Unknown,
    /// 不明的那一筆後來被我們主動取消掉了。
    Reversed,
    /// 店長手動記帳（實體刷卡機那一種）。日結時要拿紙本簽單對。
    ManuallyRecorded,
}

impl TxStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Authorized => "authorized",
            Self::Captured => "captured",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
            Self::Reversed => "reversed",
            Self::ManuallyRecorded => "manually_recorded",
        }
    }

    /// 這個狀態需要人去跟金流商後台對帳。
    pub fn needs_reconciliation(self) -> bool {
        matches!(self, Self::Unknown | Self::Pending | Self::Authorized)
    }

    /// 錢確定進來了。
    ///
    /// `ManuallyRecorded` 也算 —— 錢確實在另一台刷卡機上收了 ——
    /// 但它在日結時要另外拿紙本簽單對，所以狀態是分開的。
    pub fn is_money_in(self) -> bool {
        matches!(self, Self::Captured | Self::ManuallyRecorded)
    }
}

/// 一條金流線。
///
/// # 為什麼 `manual` 也是一個實作
///
/// 因為多數台灣小店的信用卡是走**另一台實體刷卡機**的：收銀員刷完，把終端機
/// 吐出來的授權碼抄進 POS。那不是「還沒串接」的暫時狀態，那是一種正式的、
/// 大概永遠都會存在的收款方式。把它做成 trait 的一個實作，而不是 if/else 的
/// 另一條分支，其餘所有程式碼就只需要認識一種形狀。
#[async_trait::async_trait]
pub trait PaymentGateway: Send + Sync {
    /// 這條線叫什麼（顯示與 log 用）。
    fn provider(&self) -> &'static str;

    /// 需不需要真的對外連線。`false` 的話整條非同步路徑都可以跳過。
    fn is_online(&self) -> bool {
        true
    }

    /// 送出一次付款。
    async fn pay(&self, req: &PayRequest) -> AppResult<PayOutcome>;

    /// 查一筆交易現在的狀態。
    ///
    /// **這是 `unknown` 的解藥。** 沒有這一支，「送出去了但不知道結果」
    /// 就只能靠人去金流商後台一筆一筆看。
    async fn query(&self, merchant_trade_no: &str) -> AppResult<PayOutcome>;

    /// 退款。
    async fn refund(&self, gateway_trade_no: &str, amount: i64) -> AppResult<PayOutcome>;
}

/// 我方交易編號。
///
/// 格式 `{店代碼}{yyyymmdd}{6 碼}`，全部大寫英數 —— 金流商對這個欄位的字元
/// 與長度限制都很嚴（多數只收 20~30 碼的英數），而一個在正式環境才被退件的
/// 編號格式是最難查的那種錯。
pub fn merchant_trade_no(prefix: &str, business_date: &str, seq: &str) -> String {
    let date = business_date.replace('-', "");
    let clean: String = prefix
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect::<String>()
        .to_ascii_uppercase();
    let clean = if clean.is_empty() {
        "POS".to_string()
    } else {
        clean
    };
    let tail: String = seq
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>()
        .to_ascii_uppercase();
    format!("{clean}{date}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trade_number_is_alphanumeric_and_short_enough() {
        let no = merchant_trade_no("開心小吃", "2026-09-07", "01M1VR8Y017G30X2VF1TB6C2RF");
        // 中文被濾掉之後 prefix 是空的，回退到 POS。
        assert!(no.starts_with("POS20260907"), "{no}");
        assert!(no.chars().all(|c| c.is_ascii_alphanumeric()), "{no}");
        assert!(no.len() <= 20, "太長了會被金流商退件：{no}");

        let no = merchant_trade_no("MyShop-1", "2026-09-07", "abcdef123456");
        assert_eq!(no, "MYSH20260907123456");
    }

    #[test]
    fn unknown_is_not_the_same_as_failed() {
        // 這一條看起來像廢話，但它是整層設計的核心：
        // 「送出去了但不知道結果」必須進對帳清單，而「明確失敗」不必。
        assert!(TxStatus::Unknown.needs_reconciliation());
        assert!(!TxStatus::Failed.needs_reconciliation());
        assert!(!TxStatus::Unknown.is_money_in());
        assert!(TxStatus::Captured.is_money_in());
    }
}
