//! 藍新金流（NewebPay）MPG 的密碼學層。
//!
//! # 這個檔案只做「算得出來」的那一半，而且是刻意的
//!
//! 加密、簽章、驗章、參數正規化 —— 這些完全不需要網路、不需要商店代號、
//! 不需要跟藍新簽約，任何人 clone 下來 `cargo test` 就能驗證它對不對。
//! 而且藍新的技術文件**自己附了測試向量**（一組 HashKey/HashIV、一段明文、
//! 一段密文、一個雜湊值），所以「對不對」有客觀答案，不必等到正式環境被退件。
//!
//! 下面每一個 `#[test]` 都是那份文件上印出來的數字。
//!
//! # 為什麼沒有 `impl PaymentGateway`
//!
//! 因為 MPG 不是一支可以用 HTTP client 呼叫的 API，它是**瀏覽器表單轉址**。
//! 藍新的規範白紙黑字寫著：
//!
//! > 禁用以 iframe 或 proxy 或幕後 Http Post 方式等使用 MPG 支付頁
//!
//! 違反會回 `MPG02005 驗證資料錯誤(來源不合法)`。也就是說，就算我們照著
//! `PaymentGateway::pay()` 的形狀送出去，藍新也會擋下來 —— 那不是我們可以
//! 靠寫程式繞過的事。
//!
//! 而「幕後授權」（真正能 server-to-server 刷卡的那支）需要 PCI DSS 的
//! 合規聲明文件（AOC）。一台放在小吃店櫃檯、同時在跑訂單資料庫與廚房出單機的
//! Windows 電腦，不可能拿得到 AOC，也不應該碰到卡號。
//!
//! **所以卡片在這套 POS 裡的正解是 `manual`**（銀行給的那台實體刷卡機，
//! 收銀員抄授權碼），而 MPG 的用途是「客人用自己的手機付」——
//! 掃碼點餐、電話預訂、訂位訂金那一類。
//!
//! 完整的 MPG 流程還需要一個**公開網址**來接收 `NotifyURL` 回呼，而一台在
//! 店裡 NAT 後面的電腦沒有公開網址。要嘛店家自己架一個小中繼站，要嘛不用。
//! 這件事寫在 `docs/payment-gateways.md`，而不是假裝它不存在 ——
//! 一個「設定頁上有藍新、按下去卻永遠停在等待中」的 POS 比沒有藍新更糟。
//!
//! 這一層先把**不會變、可驗證、將來一定用得到**的部分做對。

use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// 藍新的商店金鑰。
///
/// # HashKey / HashIV 沒有任何推導
///
/// 後台給的那兩串字元**直接當成 bytes 用**：32 個字元就是 AES-256 的金鑰，
/// 16 個字元就是 CBC 的 IV。沒有 KDF、沒有先做雜湊、沒有 base64 解碼。
///
/// 這件事值得寫下來，因為「金鑰看起來像 base64 所以應該要先 decode」
/// 是很自然的直覺，而它會讓每一筆交易都被退件。
///
/// # IV 是固定的
///
/// 每一筆交易用同一個 IV。以密碼學來說這是壞習慣（相同明文會產生相同密文），
/// 但那是協定規定的，**不要自作聰明去改**。
pub struct Keys {
    key: [u8; 32],
    iv: [u8; 16],
}

/// **不印出金鑰。**
///
/// `Keys` 會出現在 `unwrap()` 的 panic 訊息、`tracing` 的欄位、以及任何
/// 有人隨手寫 `{:?}` 的地方。derive 一個 Debug 等於在這些地方全都印出
/// 收款權限，而那種洩漏是永遠不會有人發現的 —— 所以這裡手寫一個會遮蔽的。
impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NewebPayKeys(<已遮蔽>)")
    }
}

impl Keys {
    pub fn new(hash_key: &str, hash_iv: &str) -> AppResult<Self> {
        let k = hash_key.as_bytes();
        let v = hash_iv.as_bytes();
        // 長度不對就當場說清楚。這個錯誤如果放過去，症狀會變成
        // 「每一筆都被藍新退件」，而那時候完全看不出來是金鑰貼錯。
        if k.len() != 32 {
            return Err(AppError::Validation(crate::msg!(
                "gateway.hash_key_len",
                n = k.len()
            )));
        }
        if v.len() != 16 {
            return Err(AppError::Validation(crate::msg!(
                "gateway.hash_iv_len",
                n = v.len()
            )));
        }
        let mut key = [0u8; 32];
        let mut iv = [0u8; 16];
        key.copy_from_slice(k);
        iv.copy_from_slice(v);
        Ok(Self { key, iv })
    }

    /// `TradeInfo`：AES-256-CBC + PKCS#7，輸出**小寫十六進位**。
    ///
    /// 不是 base64。藍新這支 API 從頭到尾沒有出現過 base64。
    pub fn encrypt(&self, plain: &str) -> String {
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        let ct = Aes256CbcEnc::new(&self.key.into(), &self.iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(plain.as_bytes());
        hex_lower(&ct)
    }

    /// 解回呼的 `TradeInfo`。
    ///
    /// 藍新文件附的 PHP 範例用 `OPENSSL_ZERO_PADDING` 加上一段自己寫的
    /// `strippadding()` 來去 padding，而那段 regex 沒有錨定、會誤砍字串中間的
    /// 位元組。標準的 PKCS#7 去 padding 就是對的（我拿文件自己的回呼密文驗過），
    /// 而且它會在 padding 不合法時**失敗**，那正是我們要的：
    /// 解不開就代表這包不是用我們的金鑰加密的，不能信。
    pub fn decrypt(&self, hex: &str) -> AppResult<String> {
        use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
        let raw = hex_decode(hex)?;
        let pt = Aes256CbcDec::new(&self.key.into(), &self.iv.into())
            .decrypt_padded_vec_mut::<Pkcs7>(&raw)
            .map_err(|_| AppError::Validation("藍新回應解不開，金鑰不對或資料被改過".into()))?;
        String::from_utf8(pt)
            .map_err(|_| AppError::Validation("藍新回應解出來不是合法的文字".into()))
    }

    /// `TradeSha`：對**密文**做雜湊。
    ///
    /// `SHA256("HashKey=" + key + "&" + 密文 + "&HashIV=" + iv)`，大寫。
    ///
    /// 注意順序：**Key 在前、IV 在後**。下面的 `check_code` 剛好相反，
    /// 那不是筆誤（見該函式的說明）。
    pub fn trade_sha(&self, cipher_hex: &str) -> String {
        let key = std::str::from_utf8(&self.key).unwrap_or_default();
        let iv = std::str::from_utf8(&self.iv).unwrap_or_default();
        sha256_upper(&format!("HashKey={key}&{cipher_hex}&HashIV={iv}"))
    }

    /// `CheckCode`：查詢／取消**回應**上的驗證碼。
    ///
    /// # 它跟 `TradeSha` 是三件不同的事，混淆是這個 API 最經典的錯誤
    ///
    /// | | 對什麼做雜湊 | 樣板 |
    /// | --- | --- | --- |
    /// | `TradeSha` | 密文 | `HashKey=…&密文&HashIV=…` |
    /// | `CheckCode` | 明文欄位 | `HashIV=…&欄位&HashKey=…`（**反過來**） |
    /// | `CheckValue` | 明文欄位 | `IV=…&欄位&Key=…`（**沒有 Hash 前綴**） |
    ///
    /// 而且因為 `CheckCode` 是對**明文欄位**做的，欄位的排序與百分比編碼
    /// 必須跟藍新那邊一個位元組不差 —— `TradeSha` 沒有這個問題（雙方都是對
    /// 同一串密文做雜湊）。所以這裡的正規化是嚴格的：欄位名 A→Z 排序、
    /// 用 `&` 串接、值走 RFC1738。
    pub fn check_code(&self, fields: &[(&str, &str)]) -> String {
        let key = std::str::from_utf8(&self.key).unwrap_or_default();
        let iv = std::str::from_utf8(&self.iv).unwrap_or_default();
        let body = sorted_query(fields);
        sha256_upper(&format!("HashIV={iv}&{body}&HashKey={key}"))
    }

    /// `CheckValue`：查詢**請求**上的驗證碼。前綴是 `IV=` / `Key=`，
    /// 沒有 `Hash`。
    pub fn check_value(&self, fields: &[(&str, &str)]) -> String {
        let key = std::str::from_utf8(&self.key).unwrap_or_default();
        let iv = std::str::from_utf8(&self.iv).unwrap_or_default();
        let body = sorted_query(fields);
        sha256_upper(&format!("IV={iv}&{body}&Key={key}"))
    }

    /// 回呼進來時驗章。
    ///
    /// **回傳 bool 而不是 Result 的理由**：呼叫端只有一種正確反應 ——
    /// 不符就整包丟掉。給它一個 Result 會誘使人去看錯誤訊息、分辨情況，
    /// 而在驗章這件事上「分辨情況」本身就是漏洞。
    pub fn verify(&self, cipher_hex: &str, their_sha: &str) -> bool {
        let mine = self.trade_sha(cipher_hex);
        // 常數時間比較。時間差攻擊在區網 POS 上不現實，但這是驗章，
        // 寫成安全的形狀不用多花什麼。
        constant_time_eq(
            mine.as_bytes(),
            their_sha.trim().to_ascii_uppercase().as_bytes(),
        )
    }
}

/// PHP `http_build_query` 的編碼（RFC1738）。
///
/// 空白是 `+` 不是 `%20`，百分比後面的十六進位是**大寫**，
/// 而 `-` `_` `.` 不編碼。這幾條看起來瑣碎，但 `CheckCode` 是對明文做的，
/// 差一個位元組就整筆對不上。
fn urlencode_1738(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `k=v&k=v`，欄位名照 A→Z 排序。
fn sorted_query(fields: &[(&str, &str)]) -> String {
    let mut f: Vec<_> = fields.to_vec();
    f.sort_by(|a, b| a.0.cmp(b.0));
    f.iter()
        .map(|(k, v)| format!("{k}={}", urlencode_1738(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// 送出去的那一串明文（不排序 —— 順序照藍新文件的欄位順序）。
pub fn build_query(fields: &[(&str, String)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencode_1738(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn sha256_upper(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex_lower(&h.finalize()).to_ascii_uppercase()
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn hex_decode(s: &str) -> AppResult<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err(AppError::Validation("藍新回應的長度不對".into()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|_| AppError::Validation("藍新回應不是合法的十六進位".into()))
        })
        .collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    // 藍新技術文件（NDNF-1.2.5）§4.1.1 自己印出來的那一組。
    // 用文件上的數字當測試向量，「對不對」就有客觀答案，
    // 不必等到正式環境被退件才知道。
    const DOC_KEY: &str = "Fs5cX1TGqYM2PpdbE14a9H83YQSQF5jn";
    const DOC_IV: &str = "C6AcmfqJILwgnhIP";

    const DOC_PLAIN: &str = "MerchantID=MS127874575&RespondType=String&TimeStamp=1695795410&Version=2.0&MerchantOrderNo=Vanespl_ec_1695795410&Amt=30&ItemDesc=test&NotifyURL=https%3A%2F%2Fwebhook.site%2Fd4db5ad1-2278-466a-9d66-78585c0dbadb";
    const DOC_CIPHER: &str = "f79eac33c4f3245d58f17b544c5d38b09457a6d77e77bae6f10fcc7236fe153ccef1a80001c0746afc063a7570f80ad970d8a32c72332c9ec5547410188007876bdca2bafa52d07d31b6b183f2204d6e4feee6d245e286ab198cf95422ad5843c7696fc943cbb65979ad207607d4b5d97dac4a90ccd5e7a37adb7d7062e838be09d94e8c5dfa145c048e17feabe58c2e310792f0f50f5af32961ffb07ff6649ae1021ad558242551de5f09316e3182e198775e5d1ad5b66a70be290004de750fa85d86b0c2f087b40005d89e048be2ab6fd83f1c522494c093426a10a1f73fe4";
    const DOC_SHA: &str = "84E4D9F96537E029F8450BE1E759080F9AF6995921B7F6F9AAFDDD2C36E7B287";

    // §4.1.4 的回呼範例。
    const DOC_NOTIFY_CIPHER: &str = "ee11d1501e6dc8433c75988258f2343d11f4d0a423be672e8e02aaf373c53c2363aeffdb4992579693277359b3e449ebe644d2075fdfbc10150b1c40e7d24cb215febefdb85b16a5cde449f6b06c58a5510d31e8d34c95284d459ae4b52afc1509c2800976a5c0b99ef24cfd28a2dfc8004215a0c98a1d3c77707773c2f2132f9a9a4ce3475cb888c2ad372485971876f8e2fec0589927544c3463d30c785c2d3bd947c06c8c33cf43e131f57939e1f7e3b3d8c3f08a84f34ef1a67a08efe177f1e663ecc6bedc7f82640a1ced807b548633cfa72d060864271ec79854ee2f5a170aa902000e7c61d1269165de330fce7d10663d1668c711571776365bfdcd7ddc915dcb90d31a9f27af9b79a443ca8302e508b0dbaac817d44cfc44247ae613075dde4ac960f1bdff4173b915e4344bc4567bd32e86be7d796e6d9b9cf20476e4996e98ccc315f1ed03a34139f936797d971f2a3f90bc18f8a155a290bcbcf04f4277171c305bf554f5cba243154b30082748a81f2e5aa432ef9950cc9668cd4330ef7c37537a6dcb5e6ef01b4eca9705e4b097cf6913ee96e81d0389e5f775";
    const DOC_NOTIFY_SHA: &str = "C80876AEBAC0036268C0E240E5BFF69C0470DE9606EEE083C5C8DD64FDB3347A";

    fn keys() -> Keys {
        Keys::new(DOC_KEY, DOC_IV).unwrap()
    }

    #[test]
    fn our_ciphertext_matches_the_manuals_printed_ciphertext() {
        // 這一條測試的價值在於：它證明我們對 AES-256-CBC / PKCS#7 /
        // 「金鑰直接當 bytes 用」/「輸出小寫 hex」四件事的理解全部正確。
        // 只要有一件想錯，這 448 個字元就不會一模一樣。
        assert_eq!(keys().encrypt(DOC_PLAIN), DOC_CIPHER);
    }

    #[test]
    fn round_trip() {
        let k = keys();
        assert_eq!(
            k.decrypt(&k.encrypt("Amt=250&ItemDesc=滷肉飯")).unwrap(),
            "Amt=250&ItemDesc=滷肉飯"
        );
    }

    #[test]
    fn trade_sha_matches_both_printed_digests() {
        let k = keys();
        // 請求那一份。
        assert_eq!(k.trade_sha(DOC_CIPHER), DOC_SHA);
        // 回呼那一份 —— 兩份都對，才能說樣板是對的而不是湊出來的。
        assert_eq!(k.trade_sha(DOC_NOTIFY_CIPHER), DOC_NOTIFY_SHA);
    }

    #[test]
    fn the_manuals_notify_payload_decrypts_with_standard_pkcs7() {
        // 藍新的 PHP 範例用 ZERO_PADDING 加自己寫的去 padding，
        // 這條測試證明那是不必要的：標準 PKCS#7 就解得開。
        let out = keys().decrypt(DOC_NOTIFY_CIPHER).unwrap();
        assert!(out.contains("MerchantID"), "{out}");
        assert!(out.contains("Status"), "{out}");
    }

    #[test]
    fn a_tampered_payload_fails_verification() {
        let k = keys();
        assert!(k.verify(DOC_CIPHER, DOC_SHA));
        // 改掉密文的最後一個字元，簽章就對不上 —— 這正是驗章要擋的事。
        let mut bad = DOC_CIPHER.to_string();
        bad.pop();
        bad.push('0');
        assert!(!k.verify(&bad, DOC_SHA), "改過的內容必須驗不過");
        // 大小寫不該影響（有些回應是小寫的）。
        assert!(k.verify(DOC_CIPHER, &DOC_SHA.to_ascii_lowercase()));
    }

    #[test]
    fn check_code_is_not_trade_sha() {
        // 這三個雜湊長得像但完全不同，把它們搞混是這個 API 最經典的整合錯誤。
        // 這裡不是驗證某個數字，是**釘住三者互不相同**這件事 ——
        // 哪天有人「順手統一」成同一個 helper，這條會紅。
        let k = keys();
        let fields = [
            ("Amt", "10"),
            ("MerchantID", "MS12345678"),
            ("MerchantOrderNo", "MyCompanyOrder_1638423361"),
            ("TradeNo", "21120214151152468"),
        ];
        let cc = k.check_code(&fields);
        let cv = k.check_value(&fields[..3]);
        assert_ne!(cc, cv);
        assert_ne!(cc, k.trade_sha(DOC_CIPHER));
        assert_eq!(cc.len(), 64);
        assert!(cc
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
    }

    #[test]
    fn fields_are_sorted_and_rfc1738_encoded() {
        // CheckCode 是對**明文**做的，所以排序與編碼必須跟藍新一模一樣。
        // 空白是 + 不是 %20，百分比後面是大寫。
        let s = sorted_query(&[("Z", "a b"), ("A", "x/y"), ("M", "3")]);
        assert_eq!(s, "A=x%2Fy&M=3&Z=a+b");
    }

    #[test]
    fn a_rejected_key_is_reported_in_the_shop_language() {
        // 這條在驗整條 i18n 路徑真的通了：msg! → 目錄 → 依語言 render。
        // 沒有它，「後端訊息可以多語系」只是一個沒有人走過的設計。
        use crate::i18n::Locale;

        let err = Keys::new("too-short", DOC_IV).unwrap_err();
        let AppError::Validation(m) = &err else {
            panic!("應該是 Validation：{err:?}");
        };
        assert!(!m.is_literal(), "應該是帶鍵值的訊息，不是寫死的字串");

        // 三種語言都講得出「32」這個關鍵數字。
        for l in Locale::ALL {
            let s = m.render(l);
            assert!(s.contains("32"), "{l:?} 少了長度：{s}");
        }
        // 而且真的是各自的語言，不是三次中文。
        assert!(
            m.render(Locale::En).contains("characters"),
            "{}",
            m.render(Locale::En)
        );
        assert!(
            m.render(Locale::Ja).contains("文字"),
            "{}",
            m.render(Locale::Ja)
        );
        assert_ne!(m.render(Locale::ZhTw), m.render(Locale::Ja));
    }

    #[test]
    fn a_wrong_length_key_is_rejected_with_a_useful_message() {
        // 貼錯金鑰的症狀本來會是「每一筆都被退件」，
        // 那是最難查的一種。在這裡當場擋下來並且說出正確長度。
        let e = Keys::new("too-short", DOC_IV).unwrap_err();
        assert!(format!("{e}").contains("32"), "{e}");
        let e = Keys::new(DOC_KEY, "nope").unwrap_err();
        assert!(format!("{e}").contains("16"), "{e}");
    }
}
