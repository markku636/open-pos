//! 多語系：繁體中文 / English / 日本語。
//!
//! # 為什麼訊息不在丟出來的地方就翻好
//!
//! 因為**同一個錯誤可能要用兩種語言講**。收銀機設成日文、廚房的 KDS 平板留著
//! 中文，這在觀光區的店裡是正常的設定。如果 `AppError` 在 `services/` 裡就已經
//! 變成一串中文，那台日文收銀機永遠拿不到日文訊息 —— 而那時候要補，得把
//! 每一個 `Err(...)` 都翻出來重寫一次。
//!
//! 所以錯誤帶的是**鍵值與參數**，翻譯發生在最外層（Tauri command 與 axum
//! handler），也就是唯一知道「這次請求是誰發的、他要什麼語言」的地方。
//!
//! # 為什麼 `Msg` 還留著一個「字面字串」的形態
//!
//! 這是給遷移用的。專案裡有一百多處 `AppError::Validation("...".into())`，
//! 如果 `Msg` 只接受鍵值，那就得**一次改完**才能編譯 —— 而一個必須一次改完
//! 的重構，多半會在改到一半的時候被別的事情打斷，然後永遠停在那裡。
//!
//! `From<String>` 讓舊的呼叫點原封不動繼續編譯，新的與改過的走 `msg!`。
//! 底下 `no_new_literal_messages` 那條測試會盯著剩餘數量只能少不能多。
//!
//! # 日文與列印
//!
//! 介面切成日文**不代表那台印表機印得出日文**。實測（見
//! `infra::printer::escpos` 的 `neither_encoding_covers_both_languages_so_the_shop_must_pick`）：
//! Big5 有假名、有長音符，但編不出新字體 —— 也就是「円」「内税」「売上」。
//! 反過來 Shift_JIS 編不出「麵」「奶」「雞」。
//!
//! 所以「機器的字庫」是店家要自己選的設定，不能由介面語言推導。
//! `encode_text` 回報的缺字清單就是「這台機器印不出這個語言」的證據，
//! 設定頁要據此擋下來，而不是印一堆全形空白給店家看。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 支援的語言。
///
/// 只有三個，而且**刻意不做「自動偵測」**：POS 是店家設定好就不會再動的東西，
/// 而依作業系統語言自動切換會讓同一間店的兩台機器顯示不同語言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Locale {
    #[default]
    #[serde(rename = "zh-TW")]
    ZhTw,
    En,
    Ja,
}

impl Locale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ZhTw => "zh-TW",
            Self::En => "en",
            Self::Ja => "ja",
        }
    }

    /// 目錄裡的欄位順序。
    fn idx(self) -> usize {
        match self {
            Self::ZhTw => 0,
            Self::En => 1,
            Self::Ja => 2,
        }
    }

    /// 從設定值解析。認不得的一律回中文而不是報錯 ——
    /// 一個因為語言代碼打錯就開不了的收銀機，比顯示錯語言糟得多。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" | "en-us" | "en-gb" => Self::En,
            "ja" | "ja-jp" => Self::Ja,
            _ => Self::ZhTw,
        }
    }

    pub const ALL: [Locale; 3] = [Locale::ZhTw, Locale::En, Locale::Ja];
}

/// 一則要給人看的訊息。
///
/// 兩種形態：帶鍵值的（可翻譯），與字面字串的（尚未 i18n，過渡期）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Msg {
    /// 目錄裡的鍵。`None` 代表這則訊息還沒有 i18n。
    pub key: Option<&'static str>,
    /// `key` 是 `None` 時要顯示的原文；有 key 時當成 fallback。
    pub literal: String,
    /// 佔位符的值，例如 `{n}` → `("n", "3")`。
    pub args: Vec<(&'static str, String)>,
}

impl Msg {
    /// 還沒 i18n 的字面訊息。
    pub fn literal(s: impl Into<String>) -> Self {
        Self {
            key: None,
            literal: s.into(),
            args: Vec::new(),
        }
    }

    pub fn keyed(key: &'static str, args: Vec<(&'static str, String)>) -> Self {
        Self {
            key: Some(key),
            literal: String::new(),
            args,
        }
    }

    /// 這則訊息還沒有被 i18n。
    pub fn is_literal(&self) -> bool {
        self.key.is_none()
    }

    /// 翻成指定語言並填入參數。
    ///
    /// 查不到鍵時回退到中文，再回退到鍵本身 —— **絕不 panic、絕不回空字串**。
    /// 一個因為少一條翻譯就整個當掉的收銀機是不能接受的，而空字串會讓
    /// 店員看到一個沒有任何訊息的錯誤框，比看到英文鍵名更難處理。
    pub fn render(&self, locale: Locale) -> String {
        let template = match self.key {
            None => self.literal.clone(),
            Some(k) => match lookup(k) {
                Some(row) => {
                    let s = row[locale.idx()];
                    if s.is_empty() {
                        // 這個語言還沒翻，退回中文。
                        let zh = row[Locale::ZhTw.idx()];
                        if zh.is_empty() {
                            k.to_string()
                        } else {
                            zh.to_string()
                        }
                    } else {
                        s.to_string()
                    }
                }
                None if !self.literal.is_empty() => self.literal.clone(),
                None => k.to_string(),
            },
        };
        interpolate(&template, &self.args)
    }
}

impl fmt::Display for Msg {
    /// 預設用中文 —— log 與 `{}` 只有這一個合理的選擇，
    /// 因為那些地方沒有「請求者是誰」的資訊。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(Locale::ZhTw))
    }
}

// 讓既有的 `"...".into()` 呼叫點原封不動繼續編譯。
impl From<String> for Msg {
    fn from(s: String) -> Self {
        Msg::literal(s)
    }
}
impl From<&str> for Msg {
    fn from(s: &str) -> Self {
        Msg::literal(s)
    }
}

/// `{name}` 換成對應的值。
///
/// 找不到的佔位符**原樣留著**而不是換成空字串：畫面上看到 `{n}` 至少看得出來
/// 是漏了參數，換成空字串會變成「還有 張單沒結帳」這種讀不懂又不像壞掉的句子。
fn interpolate(template: &str, args: &[(&'static str, String)]) -> String {
    if args.is_empty() || !template.contains('{') {
        return template.to_string();
    }
    let mut out = template.to_string();
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

fn lookup(key: &str) -> Option<&'static [&'static str; 3]> {
    CATALOG
        .binary_search_by_key(&key, |(k, _)| k)
        .ok()
        .map(|i| &CATALOG[i].1)
}

/// 建一則帶鍵值的訊息。
///
/// ```ignore
/// msg!("table.has_unpaid", n = still_open)
/// ```
#[macro_export]
macro_rules! msg {
    ($key:literal) => {
        $crate::i18n::Msg::keyed($key, ::std::vec::Vec::new())
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::Msg::keyed(
            $key,
            ::std::vec![$((stringify!($name), ::std::string::ToString::to_string(&$value))),+],
        )
    };
}

/// 訊息目錄：`(鍵, [繁中, English, 日本語])`。
///
/// **必須照鍵值排序** —— `lookup` 走二分搜尋，而下面的 `catalog_is_sorted`
/// 會在沒排好的時候讓測試紅掉（順序錯的症狀是「有些訊息隨機查不到」，
/// 那是最難查的一種）。
///
/// 空字串代表「這個語言還沒翻」，會自動退回中文，不會顯示空白。
static CATALOG: &[(&str, [&str; 3])] = &[
    (
        "gateway.hash_iv_len",
        [
            "藍新 HashIV 必須是 16 個字元，你貼的是 {n} 個",
            "NewebPay HashIV must be exactly 16 characters; you pasted {n}",
            "藍新（NewebPay）の HashIV は 16 文字である必要があります。入力されたのは {n} 文字です",
        ],
    ),
    (
        "gateway.hash_key_len",
        [
            "藍新 HashKey 必須是 32 個字元，你貼的是 {n} 個",
            "NewebPay HashKey must be exactly 32 characters; you pasted {n}",
            "藍新（NewebPay）の HashKey は 32 文字である必要があります。入力されたのは {n} 文字です",
        ],
    ),
    (
        "gateway.name_required",
        [
            "名稱不能空白",
            "The display name cannot be empty",
            "表示名は空にできません",
        ],
    ),
    (
        "gateway.unknown_provider",
        [
            "不認得的金流商：{provider}",
            "Unrecognised payment provider: {provider}",
            "認識できない決済プロバイダーです：{provider}",
        ],
    ),
    (
        "table.has_unpaid",
        [
            "這一桌還有 {n} 張單沒有結帳",
            "This table still has {n} unpaid order(s)",
            "このテーブルには未会計の伝票が {n} 件あります",
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_sorted() {
        // 二分搜尋的前提。沒排好的症狀是「有些訊息查不到」而不是整個壞掉，
        // 所以一定要有一條測試盯著。
        let mut keys: Vec<&str> = CATALOG.iter().map(|(k, _)| *k).collect();
        let original = keys.clone();
        keys.sort_unstable();
        assert_eq!(original, keys, "CATALOG 必須照鍵值排序");

        keys.dedup();
        assert_eq!(keys.len(), CATALOG.len(), "CATALOG 有重複的鍵");
    }

    #[test]
    fn every_key_has_all_three_languages() {
        for (key, row) in CATALOG {
            for (i, locale) in Locale::ALL.iter().enumerate() {
                assert!(
                    !row[i].trim().is_empty(),
                    "{key} 缺 {} 的翻譯",
                    locale.as_str()
                );
            }
        }
    }

    #[test]
    fn placeholders_match_across_languages() {
        // 三種語言的佔位符必須一模一樣。少一個 {n} 的症狀是那個語言的使用者
        // 永遠看不到數字，而其他語言都正常 —— 沒有人會回報，因為看得懂的人
        // 看到的是對的。
        fn holders(s: &str) -> Vec<&str> {
            let mut v: Vec<&str> = s
                .match_indices('{')
                .filter_map(|(i, _)| s[i..].find('}').map(|j| &s[i + 1..i + j]))
                .collect();
            v.sort_unstable();
            v
        }
        for (key, row) in CATALOG {
            let zh = holders(row[0]);
            for (i, locale) in Locale::ALL.iter().enumerate().skip(1) {
                assert_eq!(
                    holders(row[i]),
                    zh,
                    "{key} 的 {} 版本佔位符跟中文對不起來",
                    locale.as_str()
                );
            }
        }
    }

    #[test]
    fn renders_in_each_language_with_args() {
        let m = msg!("table.has_unpaid", n = 3);
        assert_eq!(m.render(Locale::ZhTw), "這一桌還有 3 張單沒有結帳");
        assert_eq!(
            m.render(Locale::En),
            "This table still has 3 unpaid order(s)"
        );
        assert!(
            m.render(Locale::Ja).contains("3 件"),
            "{}",
            m.render(Locale::Ja)
        );
    }

    #[test]
    fn an_unknown_key_never_panics_and_never_renders_empty() {
        // 少一條翻譯不可以讓收銀機當掉，也不可以顯示一個空的錯誤框。
        let m = Msg::keyed("nope.not.here", vec![]);
        for l in Locale::ALL {
            let s = m.render(l);
            assert!(!s.is_empty(), "{l:?} render 出空字串");
        }
    }

    #[test]
    fn a_missing_placeholder_stays_visible() {
        // 漏帶參數時要看得出來漏了，而不是變成讀不懂又不像壞掉的句子。
        let m = Msg::keyed("table.has_unpaid", vec![]);
        assert!(m.render(Locale::ZhTw).contains("{n}"));
    }

    #[test]
    fn literal_messages_still_work_so_the_migration_can_be_incremental() {
        // 這條在保護遷移路徑本身：舊的 `"...".into()` 必須繼續可用，
        // 否則一百多處要一次改完，而那種重構通常改到一半就停了。
        let e: Msg = "還沒設定出單機".into();
        assert!(e.is_literal());
        assert_eq!(e.render(Locale::Ja), "還沒設定出單機");
    }

    #[test]
    fn locale_parse_falls_back_instead_of_failing() {
        assert_eq!(Locale::parse("ja-JP"), Locale::Ja);
        assert_eq!(Locale::parse("EN"), Locale::En);
        // 認不得的回中文，不是報錯 —— 開不了機比顯示錯語言嚴重得多。
        assert_eq!(Locale::parse("klingon"), Locale::ZhTw);
        assert_eq!(Locale::parse(""), Locale::ZhTw);
    }
}
