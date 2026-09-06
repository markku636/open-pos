//! 時間戳的唯一出口。
//!
//! ADR 0002 規定時間存 ISO-8601 UTC 固定寬度文字（24 字元，含三位毫秒）。
//! 「固定寬度」是重點：補零之後字典序才等於時序，`ORDER BY created_at` 才正確。
//!
//! 也刻意提供 `Clock` 這個小抽象。不是為了單元測試好寫（純函式層本來就不碰時間），
//! 而是為了一條更重要的規則：**同一筆交易的所有寫入必須拿到完全相同的時間戳**。
//! 各處自己呼叫 `Utc::now()` 會讓 `orders.settled_at` 與 `payments.paid_at`
//! 差幾毫秒，報表就對不起來。做法是在 use case 開頭取一次，往下傳。

use chrono::{DateTime, SecondsFormat, Utc};

/// 產生符合 ADR 0002 格式的時間戳。
pub fn to_iso(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// 取現在時間。**一個 use case 只該呼叫一次**，之後往下傳。
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// 便利函式：現在時間的 ISO 文字。只用於不屬於任何交易的場合（log、檔名）。
pub fn now_iso() -> String {
    to_iso(now())
}

/// 一次 use case 內共用的時間戳。
#[derive(Clone, Debug)]
pub struct Stamp {
    pub at: DateTime<Utc>,
    iso: String,
}

impl Stamp {
    pub fn now() -> Self {
        Self::at(now())
    }
    pub fn at(at: DateTime<Utc>) -> Self {
        Self {
            iso: to_iso(at),
            at,
        }
    }
    /// 寫進資料庫的文字。
    pub fn iso(&self) -> &str {
        &self.iso
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn format_is_fixed_width_utc_with_millis() {
        let s = to_iso(Utc.with_ymd_and_hms(2026, 9, 6, 14, 23, 45).unwrap());
        assert_eq!(s, "2026-09-06T14:23:45.000Z");
        assert_eq!(s.len(), 24, "固定寬度是字典序等於時序的前提");
    }

    #[test]
    fn lexical_order_matches_chronological_order() {
        let a = to_iso(Utc.with_ymd_and_hms(2026, 9, 6, 9, 0, 0).unwrap());
        let b = to_iso(Utc.with_ymd_and_hms(2026, 9, 6, 10, 0, 0).unwrap());
        // 若沒有補零（例如 "9:00" vs "10:00"），字串比較會把 10 點排在 9 點前面。
        assert!(a < b, "{a} 應小於 {b}");
    }

    #[test]
    fn stamp_reuses_the_same_instant() {
        let s = Stamp::now();
        // 同一個 Stamp 產生的文字必須完全一致 —— 這正是它存在的理由。
        assert_eq!(s.iso(), s.iso());
        assert_eq!(s.iso(), to_iso(s.at));
    }
}
