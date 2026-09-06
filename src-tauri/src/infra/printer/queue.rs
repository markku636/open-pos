//! 列印佇列的重試策略。
//!
//! # 為什麼封頂是 30 秒而不是幾分鐘
//!
//! 一般的背景工作用指數退避退到幾分鐘是對的 —— 那些工作沒有人在等。
//! 出單機不是：**廚房在等這張單**，客人坐在位子上。退避到三分鐘，
//! 等於在網路恢復之後又讓那一桌多等三分鐘。
//!
//! # 為什麼死信條件有三個
//!
//! 次數（試了 20 次還是不行）、年齡（超過 30 分鐘的單印出來也沒意義了，
//! 客人早就走了）、以及分類（版面產生失敗這種錯，試一萬次也一樣）。
//! 少了年齡那一條，一台整晚不通的機器會在隔天早上一次吐出昨晚所有的單。
//!
//! # 死信必須發出聲音
//!
//! POS 最常見的客訴是「廚房沒收到單」，而技術根因幾乎都是
//! 「系統知道印失敗了，但沒有告訴任何人」。所以 `Decision::DeadLetter`
//! 帶著一句給店員看的話，呼叫端有義務把它顯示出來。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::RetryClass;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// 第一次重試等多久，之後每次加倍。
    pub base: Duration,
    /// 退避上限。
    pub cap: Duration,
    /// 試到第幾次就放棄。
    pub max_attempts: u32,
    /// 這張單超過多久就沒有意義了。
    pub max_age: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(1),
            cap: Duration::from_secs(30),
            max_attempts: 20,
            max_age: Duration::from_secs(30 * 60),
        }
    }
}

/// 這一張單接下來怎麼辦。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 等這麼久之後再試。
    Retry(Duration),
    /// 放棄。字串是**給店員看的一句話**，不是給工程師看的。
    DeadLetter(DeadLetterReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeadLetterReason {
    /// 重試永遠不會好（版面產生失敗、設定錯誤）。
    Permanent,
    /// 試太多次了。
    TooManyAttempts,
    /// 這張單已經太舊，印出來也沒意義。
    TooOld,
}

impl DeadLetterReason {
    /// 給店員看的一句話。要說「發生了什麼、現在該做什麼」。
    pub fn message(&self, printer: &str) -> String {
        match self {
            Self::Permanent => {
                format!("{printer} 印不出來，設定可能有問題 —— 請到設定頁檢查這台機器")
            }
            Self::TooManyAttempts => {
                format!("{printer} 一直連不上，已經停止重試 —— 請檢查電源與網路線，然後按補印")
            }
            Self::TooOld => {
                format!("{printer} 超過 30 分鐘沒印出來，這張單已作廢 —— 需要的話請按補印")
            }
        }
    }
}

impl RetryPolicy {
    /// 退避時間：base × 2^(attempts-1)，封頂，再加上 ±20% 的抖動。
    ///
    /// 抖動是必要的：一台機器斷線時，同一批工作會同時失敗，
    /// 沒有抖動它們就會同時醒來、同時重試，把剛恢復的機器再打掛一次。
    ///
    /// `seed` 由呼叫端提供（通常是工作 id 的雜湊）——
    /// 這一層刻意不碰亂數，才能在測試裡完全決定性地驗證。
    pub fn delay_for(&self, attempts: u32, seed: u64) -> Duration {
        let exp = attempts.saturating_sub(1).min(16);
        let base_ms = self.base.as_millis() as u64;
        let raw = base_ms.saturating_mul(1u64 << exp);
        let capped = raw.min(self.cap.as_millis() as u64);
        // ±20%：seed 落在 [0, 40) 之後平移成 [-20, +20)。
        let pct = (seed % 41) as i64 - 20;
        let jittered = capped as i64 + capped as i64 * pct / 100;
        Duration::from_millis(jittered.max(1) as u64)
    }

    /// 一次失敗之後要怎麼辦。
    pub fn decide(&self, class: RetryClass, attempts: u32, age: Duration, seed: u64) -> Decision {
        if class == RetryClass::Permanent {
            return Decision::DeadLetter(DeadLetterReason::Permanent);
        }
        // 年齡先於次數：一台從頭到尾不通的機器，最後會是「太舊」而不是
        // 「試太多次」—— 那才是店員需要知道的事實。
        if age >= self.max_age {
            return Decision::DeadLetter(DeadLetterReason::TooOld);
        }
        if attempts >= self.max_attempts {
            return Decision::DeadLetter(DeadLetterReason::TooManyAttempts);
        }
        // 缺紙、開蓋這種要人去處理的，直接用上限等待。
        // 每秒去戳一台缺紙的機器不會讓紙自己裝回去，只會把 log 洗掉。
        if class == RetryClass::NeedsAttention {
            return Decision::Retry(self.delay_for(u32::MAX, seed));
        }
        Decision::Retry(self.delay_for(attempts, seed))
    }
}

/// 把字串壓成一個穩定的種子。
///
/// 用工作 id 當種子，同一張單每次算出來的抖動都一樣 ——
/// 「同樣的輸入產生同樣的行為」在稽核與重現 bug 時值得這一點成本。
pub fn seed_of(id: &str) -> u64 {
    // FNV-1a，零相依。
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    // ★ 一定要再過一次雪崩混合。
    //
    // FNV-1a 的低位元幾乎只受最後一個位元組影響，而 ULID 這種
    // 「前面全都一樣、只有結尾不同」的 id 正好踩在它最弱的地方 ——
    // 直接拿去 % 41 會讓一整批工作抖出同一個延遲，抖動就白做了。
    // 這是 splitmix64 的 finalizer。
    h ^= h >> 30;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_JITTER: u64 = 20; // (20 % 41) - 20 == 0

    #[test]
    fn backoff_doubles_then_stops_at_the_cap() {
        let p = RetryPolicy::default();
        let secs = |n| p.delay_for(n, NO_JITTER).as_millis();
        assert_eq!(secs(1), 1_000);
        assert_eq!(secs(2), 2_000);
        assert_eq!(secs(3), 4_000);
        assert_eq!(secs(4), 8_000);
        assert_eq!(secs(5), 16_000);
        // ★ 封頂 30 秒。廚房在等這張單 —— 退避到幾分鐘等於讓那一桌再多等幾分鐘。
        assert_eq!(secs(6), 30_000);
        assert_eq!(secs(50), 30_000);
    }

    #[test]
    fn jitter_stays_within_twenty_percent() {
        let p = RetryPolicy::default();
        for seed in 0..500u64 {
            let d = p.delay_for(6, seed).as_millis() as i64;
            assert!(
                (24_000..=36_000).contains(&d),
                "seed {seed} 的抖動跑出範圍：{d}ms"
            );
        }
    }

    #[test]
    fn the_same_job_always_jitters_the_same_way() {
        // 可重現是稽核與重現 bug 的必要條件。
        let p = RetryPolicy::default();
        let seed = seed_of("01M1VD1JB363W7TYH1K9Y8WJMV");
        let first = p.delay_for(3, seed);
        for _ in 0..50 {
            assert_eq!(p.delay_for(3, seed), first);
        }
    }

    #[test]
    fn different_jobs_do_not_all_wake_up_together() {
        // 一台機器斷線時整批工作同時失敗。沒有抖動它們會同時重試，
        // 把剛恢復的機器再打掛一次。
        let p = RetryPolicy::default();
        let delays: std::collections::BTreeSet<u128> = (0..20)
            .map(|i| p.delay_for(6, seed_of(&format!("job-{i}"))).as_millis())
            .collect();
        assert!(delays.len() > 10, "20 張單只散出 {} 種延遲", delays.len());
    }

    #[test]
    fn a_permanent_failure_never_retries() {
        let p = RetryPolicy::default();
        assert_eq!(
            p.decide(RetryClass::Permanent, 1, Duration::ZERO, 0),
            Decision::DeadLetter(DeadLetterReason::Permanent)
        );
    }

    #[test]
    fn a_stale_ticket_is_dropped_rather_than_printed_tomorrow_morning() {
        // ★ 少了年齡這一條，一台整晚不通的機器會在隔天早上一次吐出昨晚所有的單。
        let p = RetryPolicy::default();
        assert_eq!(
            p.decide(
                RetryClass::Transient,
                2,
                Duration::from_secs(31 * 60),
                NO_JITTER
            ),
            Decision::DeadLetter(DeadLetterReason::TooOld)
        );
    }

    #[test]
    fn age_wins_over_attempt_count() {
        // 一台從頭到尾不通的機器，店員需要知道的事實是「這張單太舊了」，
        // 不是「系統試了 20 次」。
        let p = RetryPolicy::default();
        assert_eq!(
            p.decide(
                RetryClass::Transient,
                999,
                Duration::from_secs(31 * 60),
                NO_JITTER
            ),
            Decision::DeadLetter(DeadLetterReason::TooOld)
        );
    }

    #[test]
    fn too_many_attempts_gives_up() {
        let p = RetryPolicy::default();
        assert_eq!(
            p.decide(
                RetryClass::Transient,
                20,
                Duration::from_secs(60),
                NO_JITTER
            ),
            Decision::DeadLetter(DeadLetterReason::TooManyAttempts)
        );
    }

    #[test]
    fn a_printer_out_of_paper_is_not_poked_every_second() {
        // 每秒去戳一台缺紙的機器不會讓紙自己裝回去，只會把 log 洗掉。
        let p = RetryPolicy::default();
        assert_eq!(
            p.decide(
                RetryClass::NeedsAttention,
                1,
                Duration::from_secs(5),
                NO_JITTER
            ),
            Decision::Retry(Duration::from_secs(30))
        );
    }

    #[test]
    fn dead_letter_messages_tell_the_shop_what_to_do() {
        // 「系統知道印失敗了但沒告訴任何人」是最常見客訴的技術根因，
        // 所以訊息裡必須有機器名稱與下一步動作。
        for reason in [
            DeadLetterReason::Permanent,
            DeadLetterReason::TooManyAttempts,
            DeadLetterReason::TooOld,
        ] {
            let msg = reason.message("飲料吧");
            assert!(msg.contains("飲料吧"), "{msg}");
            assert!(
                msg.contains("請") || msg.contains("需要"),
                "訊息沒有告訴店員該做什麼：{msg}"
            );
        }
    }
}
