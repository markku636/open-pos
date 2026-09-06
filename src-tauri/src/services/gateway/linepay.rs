//! LINE Pay，走 **Offline API v4**（掃客人手機上的「我的條碼」）。
//!
//! # 為什麼是 Offline v4 而不是大家比較常看到的 Online v3
//!
//! 三個理由，而第一個是決定性的：
//!
//! 1. **Offline v4 的查詢端點吃的是「我方的 orderId」**
//!    （`GET /v4/payments/orders/{orderId}/check`），
//!    而 Online v3 的查詢吃的是 LINE Pay 的 `transactionId`。
//!
//!    差別在超時的那一刻：`transactionId` 是**回應**帶回來的，而超時的定義
//!    就是沒有拿到回應。所以 Online v3 在「送出去了但不知道結果」時
//!    **根本無法查詢** —— 那條路在櫃檯上是結構性不安全的。
//!    Offline v4 查得到，因為我方的 orderId 在送出**之前**就已經寫進資料庫。
//!
//! 2. Offline v4 用 HMAC 標頭，不需要固定對外 IP。一般餐廳用的是浮動 IP 的
//!    消費級網路，v2/v2.4 那種 IP 白名單的做法在店裡根本不成立。
//!
//! 3. v4 是 2025 年 11 月為台灣的電支法規推出的版本。
//!
//! # returnCode 1172 不是失敗
//!
//! 「同一個 orderId 已經存在」代表**上一次那筆其實送到了**。把它當失敗處理
//! 會讓收銀員再刷一次，於是扣兩次。所以它映射到 `Indeterminate`，
//! 接下來一定要去查詢。
//!
//! # 這個檔案裡沒有經過真實驗證的東西
//!
//! 簽章、端點與錯誤碼的**構造**來自官方文件（信心高，見下方測試裡的
//! 註解），但**沒有任何一行跑過真的 LINE Pay 沙箱** —— 那需要商家帳號。
//! 所以 `sign()` 是純函式而且有測試，而網路那一段是隔離的 trait，
//! 拿到憑證之後只要換掉 transport 就能驗。

use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// 沙箱與正式的網址。
///
/// 做成 enum 而不是字串：一個「不小心把沙箱網址設成正式」的設定，
/// 症狀是拿真的信用卡在測試。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseUrl {
    Sandbox,
    Production,
}

impl BaseUrl {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sandbox => "https://sandbox-api-pay.line.me",
            Self::Production => "https://api-pay.line.me",
        }
    }
}

/// 掃客人手機拿到的一次性條碼。
///
/// 台灣是**固定 18 碼**，而且從客人產生起算只有五分鐘 —— 收銀員掃完之後
/// 還去問廚房要不要加蛋，回來就過期了。長度在這裡就擋，不要送出去才被退。
#[derive(Debug, Clone)]
pub struct OneTimeKey(String);

impl OneTimeKey {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let s = raw.trim();
        if s.is_empty() {
            return Err("請先掃客人手機上的 LINE Pay 條碼".into());
        }
        if !s.chars().all(|c| c.is_ascii_digit()) {
            return Err("這不是 LINE Pay 的條碼（應該是一串數字）".into());
        }
        if s.len() != 18 {
            return Err(format!(
                "LINE Pay 條碼應該是 18 碼，掃到的是 {} 碼。請客人重新打開條碼再掃一次。",
                s.len()
            ));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 簽章。
///
/// ```text
/// key = channel_secret
/// msg = channel_secret ++ api_path ++ (body | query_string) ++ nonce
/// ```
///
/// ★ **channel secret 出現兩次**：一次當金鑰，一次當訊息的開頭。
/// 這很反直覺，但官方範例就是這樣寫的，而少了前面那一次會得到一個
/// 「看起來完全正常、就是永遠驗不過」的簽章。
///
/// 四個部分之間**沒有分隔字元**，全部是 UTF-8 位元組，輸出是標準
/// base64（含 padding）。
pub fn sign(channel_secret: &str, api_path: &str, body_or_query: &[u8], nonce: &str) -> String {
    let mut mac =
        <Hmac<Sha256>>::new_from_slice(channel_secret.as_bytes()).expect("HMAC 接受任何長度的金鑰");
    mac.update(channel_secret.as_bytes());
    mac.update(api_path.as_bytes());
    mac.update(body_or_query);
    mac.update(nonce.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// 一個簽好名、可以直接送出去的請求。
///
/// # 為什麼 body 是私有的而且沒有 setter
///
/// 因為官方 FAQ 對錯誤碼 1106 的說明是：
/// 「請求內容裡多餘的空白、或 JSON 序列化之後的欄位順序不同，
/// 都會產生不一樣的 MAC」。
///
/// 也就是說**簽的位元組必須跟送出去的位元組一模一樣**。所以這裡序列化一次、
/// 簽那一份、然後把**同一個** `Vec` 搬進請求裡 —— 結構上不存在
/// 「簽 A 送 B」的路徑。
#[derive(Debug, Clone)]
pub struct SignedRequest {
    url: String,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl SignedRequest {
    pub fn post_json<T: Serialize>(
        base: BaseUrl,
        channel_id: &str,
        channel_secret: &str,
        api_path: &str,
        payload: &T,
        nonce: &str,
    ) -> Result<Self, String> {
        let body = serde_json::to_vec(payload).map_err(|e| format!("請求序列化失敗：{e}"))?;
        let sig = sign(channel_secret, api_path, &body, nonce);
        Ok(Self {
            url: format!("{}{}", base.as_str(), api_path),
            headers: vec![
                ("Content-Type", "application/json".into()),
                ("X-LINE-ChannelId", channel_id.to_string()),
                ("X-LINE-Authorization", sig),
                ("X-LINE-Authorization-Nonce", nonce.to_string()),
            ],
            body,
        })
    }

    pub fn get(
        base: BaseUrl,
        channel_id: &str,
        channel_secret: &str,
        api_path: &str,
        query: &str,
        nonce: &str,
    ) -> Self {
        let sig = sign(channel_secret, api_path, query.as_bytes(), nonce);
        let url = if query.is_empty() {
            format!("{}{}", base.as_str(), api_path)
        } else {
            format!("{}{}?{}", base.as_str(), api_path, query)
        };
        Self {
            url,
            headers: vec![
                ("X-LINE-ChannelId", channel_id.to_string()),
                ("X-LINE-Authorization", sig),
                ("X-LINE-Authorization-Nonce", nonce.to_string()),
            ],
            body: Vec::new(),
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn headers(&self) -> &[(&'static str, String)] {
        &self.headers
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

// ---------------------------------------------------------------- 端點

pub const PAY_PATH: &str = "/v4/payments/oneTimeKeys/pay";

/// 查一筆交易。**吃的是我方的 orderId** —— 這正是選 Offline v4 的理由。
pub fn check_path(order_id: &str) -> String {
    format!("/v4/payments/orders/{order_id}/check")
}

// ---------------------------------------------------------------- 線上格式

#[derive(Debug, Clone, Serialize)]
pub struct PayBody {
    /// 我方交易編號。重試時沿用同一個。
    #[serde(rename = "orderId")]
    pub order_id: String,
    /// 掃到的一次性條碼。
    #[serde(rename = "oneTimeKey")]
    pub one_time_key: String,
    /// 台幣是整數，沒有小數。
    pub amount: i64,
    pub currency: String,
    #[serde(rename = "productName")]
    pub product_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiResponse {
    #[serde(rename = "returnCode")]
    pub return_code: String,
    #[serde(rename = "returnMessage")]
    pub return_message: Option<String>,
    pub info: Option<serde_json::Value>,
}

/// 回應碼的分類。
///
/// **拒絕預設**：不認得的碼一律當成「不知道」，不是「失敗」。
/// 把不認得的碼當失敗，等於在每一次 LINE Pay 改版時偷偷把錢送掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// 成功。
    Success,
    /// 對方明確拒絕（餘額不足、條碼過期）。可以放心重來。
    Declined,
    /// ★ 送到了但結果不明 —— 一定要去查。
    Indeterminate,
    /// 我方請求有問題（簽章錯、參數錯）。改好之後可以用**同一個**編號重送。
    Rejected,
}

pub fn classify(return_code: &str) -> Classification {
    match return_code {
        "0000" => Classification::Success,

        // ★ 1172 =「同一個 orderId 已經存在」。
        //    它的意思是**上一次那筆其實送到了** —— 把它當失敗會讓收銀員
        //    再刷一次，於是扣兩次。所以它是 Indeterminate，接下來要去查。
        "1172" => Classification::Indeterminate,

        // 明確的拒絕。
        "1104" // 商家不存在
        | "1105" // 商家無法使用此 API
        | "1124" // 金額資訊有誤
        | "1141" // 付款帳號狀態異常
        | "1142" // 餘額不足
        | "1145" // 付款處理中
        | "1152" // 該交易已付款完成
        | "1155" // 找不到該筆交易
        | "1159" // 找不到付款請求資訊
        | "1164" // 一次性條碼無效或已過期
        | "1170" // 使用者帳戶餘額變動中
        | "1183" // 付款金額低於下限
        | "1184" // 付款金額超過上限
        => Classification::Declined,

        // 我方的錯。
        "1101" | "1102" | "1106" | "1198" => Classification::Rejected,

        // 對方系統忙碌／內部錯誤 —— 這些是「不知道」。
        "1150" | "1169" | "9000" => Classification::Indeterminate,

        // ★ 拒絕預設。不認得的一律當「不知道」，去查一次才知道。
        _ => Classification::Indeterminate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 簽章的四個組成部分都要進去，而且順序不能換。
    ///
    /// 沒有官方的測試向量可以對（LINE Pay 沒有公開），所以這裡驗的是
    /// **構造**：換掉任何一個部分，簽章就要跟著變。這擋得住實作上最常見的
    /// 那幾種錯 —— 少接一段、順序寫反、用了錯的金鑰。
    #[test]
    fn every_part_of_the_message_changes_the_signature() {
        let base = sign(
            "secret",
            "/v4/payments/oneTimeKeys/pay",
            b"{\"a\":1}",
            "nonce-1",
        );

        assert_ne!(
            base,
            sign(
                "other",
                "/v4/payments/oneTimeKeys/pay",
                b"{\"a\":1}",
                "nonce-1"
            )
        );
        assert_ne!(
            base,
            sign("secret", "/v4/payments/other", b"{\"a\":1}", "nonce-1")
        );
        assert_ne!(
            base,
            sign(
                "secret",
                "/v4/payments/oneTimeKeys/pay",
                b"{\"a\":2}",
                "nonce-1"
            )
        );
        assert_ne!(
            base,
            sign(
                "secret",
                "/v4/payments/oneTimeKeys/pay",
                b"{\"a\":1}",
                "nonce-2"
            )
        );

        // 同樣的輸入一定得到同樣的輸出（沒有隨機成分）。
        assert_eq!(
            base,
            sign(
                "secret",
                "/v4/payments/oneTimeKeys/pay",
                b"{\"a\":1}",
                "nonce-1"
            )
        );
        // 標準 base64 含 padding，HMAC-SHA256 是 32 bytes → 44 個字元。
        assert_eq!(base.len(), 44, "{base}");
        assert!(base.ends_with('='), "標準 base64 要有 padding：{base}");
    }

    /// ★ channel secret 出現兩次：一次當金鑰，一次當訊息開頭。
    ///
    /// 這一條很反直覺，所以用一個獨立的實作對照 —— 少了訊息開頭那一次，
    /// 會得到一個「看起來完全正常、就是永遠驗不過」的簽章。
    #[test]
    fn the_secret_is_both_the_key_and_the_start_of_the_message() {
        let secret = "channel-secret";
        let path = "/v4/payments/oneTimeKeys/pay";
        let body = b"{}";
        let nonce = "n";

        let mut without_prefix = <Hmac<Sha256>>::new_from_slice(secret.as_bytes()).unwrap();
        without_prefix.update(path.as_bytes());
        without_prefix.update(body);
        without_prefix.update(nonce.as_bytes());
        let wrong = base64::engine::general_purpose::STANDARD
            .encode(without_prefix.finalize().into_bytes());

        assert_ne!(
            sign(secret, path, body, nonce),
            wrong,
            "少簽了開頭的 secret"
        );
    }

    /// 簽的位元組必須就是送出去的位元組。
    #[test]
    fn the_signed_bytes_are_the_transmitted_bytes() {
        let body = PayBody {
            order_id: "POS20260907000001".into(),
            one_time_key: "123456789012345678".into(),
            amount: 125,
            currency: "TWD".into(),
            product_name: "餐點".into(),
        };
        let req =
            SignedRequest::post_json(BaseUrl::Sandbox, "chan", "sec", PAY_PATH, &body, "nonce-1")
                .unwrap();

        let expected = sign("sec", PAY_PATH, req.body(), "nonce-1");
        let got = req
            .headers()
            .iter()
            .find(|(k, _)| *k == "X-LINE-Authorization")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(got, expected, "簽的跟送的不是同一份位元組");
        assert!(req.url().starts_with("https://sandbox-api-pay.line.me"));
    }

    /// ★ 1172 是「不知道」，不是「失敗」。
    ///
    /// 這一條是整個檔案裡最重要的測試：把它當失敗會讓收銀員再刷一次，
    /// 而客人會被扣兩次錢。
    #[test]
    fn a_duplicate_order_id_means_the_first_one_probably_landed() {
        assert_eq!(classify("1172"), Classification::Indeterminate);
        assert_eq!(classify("0000"), Classification::Success);
        assert_eq!(classify("1142"), Classification::Declined);
        assert_eq!(classify("1106"), Classification::Rejected);
    }

    /// 不認得的回應碼一律當「不知道」。
    ///
    /// 拒絕預設：把不認得的碼當失敗，等於在對方每一次改版時偷偷把錢送掉。
    #[test]
    fn an_unknown_return_code_is_never_treated_as_a_clean_failure() {
        for code in ["9999", "", "1234", "abc"] {
            assert_eq!(
                classify(code),
                Classification::Indeterminate,
                "不認得的碼 {code:?} 不可以當成失敗"
            );
        }
    }

    /// 條碼長度在這裡就擋，不要送出去才被退。
    #[test]
    fn a_bad_barcode_is_rejected_with_something_the_cashier_can_act_on() {
        assert!(OneTimeKey::parse("123456789012345678").is_ok());

        let err = OneTimeKey::parse("12345").unwrap_err();
        assert!(err.contains("18 碼"), "{err}");
        assert!(err.contains("重新打開"), "要告訴收銀員下一步做什麼：{err}");

        assert!(OneTimeKey::parse("").unwrap_err().contains("先掃"));
        assert!(OneTimeKey::parse("abcdefghijklmnopqr")
            .unwrap_err()
            .contains("數字"));
    }

    #[test]
    fn sandbox_and_production_are_different_hosts() {
        // 一個「不小心把沙箱設成正式」的設定，症狀是拿真的信用卡在測試。
        assert_ne!(BaseUrl::Sandbox.as_str(), BaseUrl::Production.as_str());
        assert!(BaseUrl::Sandbox.as_str().contains("sandbox"));
        assert!(!BaseUrl::Production.as_str().contains("sandbox"));
    }

    /// 查詢端點吃的是**我方的** orderId —— 這是選 Offline v4 的全部理由。
    #[test]
    fn the_check_endpoint_is_keyed_by_our_own_order_id() {
        let path = check_path("POS20260907000001");
        assert_eq!(path, "/v4/payments/orders/POS20260907000001/check");
    }
}
