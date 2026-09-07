//! 營業日。
//!
//! 夜市攤、居酒屋營業到凌晨三點。「9/6 02:30 的訂單」屬於 **9/5 的營業日**。
//!
//! 這件事**必須在寫入時算好並落成獨立欄位**，不能在報表查詢時現算。理由有五個，
//! 每一個都會實際出事：
//!
//! 1. 查詢時現算要包函式（`datetime(created_at, '-5 hours')`），**索引直接失效**
//! 2. 時區換算容易錯，而且切點設定可能中途被改過，歷史資料會前後不一致
//! 3. 日結之後補的單會落到錯誤的營業日
//! 4. SQLite 與 PostgreSQL 的日期函式不同，是方言分歧點
//! 5. 班別（shift）也掛營業日，兩邊算法一旦有落差就對不起來

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 營業日。存成 `YYYY-MM-DD` 文字（PG 端對應 DATE）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BusinessDate(pub NaiveDate);

impl BusinessDate {
    /// 由 UTC 時間、店家時區、換日點決定營業日。
    pub fn of(at: DateTime<Utc>, tz: Tz, cutoff: NaiveTime) -> Self {
        let local = at.with_timezone(&tz).naive_local();
        let d = if local.time() < cutoff {
            local
                .date()
                .pred_opt()
                .expect("NaiveDate 不會在合理年份下溢位")
        } else {
            local.date()
        };
        Self(d)
    }

    pub fn to_iso(self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }

    pub fn parse(s: &str) -> AppResult<Self> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Self)
            .map_err(|e| AppError::Validation(format!("不是合法的營業日 {s}：{e}").into()))
    }
}

impl std::fmt::Display for BusinessDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_iso())
    }
}

/// 店家的營業日設定。
#[derive(Clone, Copy, Debug)]
pub struct BusinessDayConfig {
    pub tz: Tz,
    pub cutoff: NaiveTime,
}

impl Default for BusinessDayConfig {
    fn default() -> Self {
        Self {
            tz: chrono_tz::Asia::Taipei,
            // 預設凌晨五點換日：涵蓋絕大多數台灣餐飲的收攤時間。
            cutoff: NaiveTime::from_hms_opt(5, 0, 0).expect("05:00 是合法時間"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn taipei(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        chrono_tz::Asia::Taipei
            .with_ymd_and_hms(y, m, d, h, mi, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn after_midnight_belongs_to_previous_business_day() {
        let cfg = BusinessDayConfig::default();
        // 9/6 凌晨 2:30 還是 9/5 的生意
        let bd = BusinessDate::of(taipei(2026, 9, 6, 2, 30), cfg.tz, cfg.cutoff);
        assert_eq!(bd.to_iso(), "2026-09-05");
    }

    #[test]
    fn after_cutoff_belongs_to_same_day() {
        let cfg = BusinessDayConfig::default();
        let bd = BusinessDate::of(taipei(2026, 9, 6, 5, 0), cfg.tz, cfg.cutoff);
        assert_eq!(bd.to_iso(), "2026-09-06");
        let bd = BusinessDate::of(taipei(2026, 9, 6, 11, 30), cfg.tz, cfg.cutoff);
        assert_eq!(bd.to_iso(), "2026-09-06");
    }

    #[test]
    fn utc_input_is_converted_not_truncated() {
        // 台北 00:30 = UTC 前一天 16:30。若忘了轉時區會算成前一天，
        // 但正確答案是「台北時間仍在換日點前，所以屬於前一個營業日」。
        let cfg = BusinessDayConfig::default();
        let at = taipei(2026, 1, 1, 0, 30);
        assert_eq!(at.format("%Y-%m-%d %H:%M").to_string(), "2025-12-31 16:30");
        assert_eq!(
            BusinessDate::of(at, cfg.tz, cfg.cutoff).to_iso(),
            "2025-12-31"
        );
    }

    #[test]
    fn round_trips_through_text() {
        let bd = BusinessDate::parse("2026-09-05").unwrap();
        assert_eq!(bd.to_iso(), "2026-09-05");
        assert!(BusinessDate::parse("2026/09/05").is_err());
    }
}
