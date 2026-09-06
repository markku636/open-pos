//! 金額型別與捨入。
//!
//! # 為什麼是「整數元」而不是「分」
//!
//! 法源：加值型及非加值型營業稅法施行細則 §32-1 ——
//! 「銷項稅額＝當期開立統一發票總額÷(1+徵收率)×徵收率」
//! 「前項銷項稅額，尾數不滿通用貨幣一元者，按四捨五入計算」，銷售額＝總額−稅額。
//!
//! 也就是說**稅額與銷售額在法規層就是整數元**，而且「先算稅額再倒扣銷售額」正是官方
//! 用來避免 1 元尾差的算法。整條下游（發票欄位、金流、現金抽屜、找零）也都只認整數元。
//!
//! 「分」這個刻度同時**多餘又不夠**：多餘，是因為沒有任何下游需要角分；
//! 不夠，是因為真正需要小數的地方（未稅單價、秤重單價）MIG 允許到 7 位小數。
//! 所以改成兩個型別：Money（整數元，所有落地的金額）與 Micros（1/1,000,000 元，
//! 只活在計算中間與單價欄位）。
//!
//! # 為什麼不是 REAL / TEXT
//!
//! REAL：IEEE754 無法精確表示 0.1，2100.0 / 1.05 會得到 1999.9999999999998；
//! 而且 SUM(REAL) 的結果隨加總順序而異，報表無法重現 —— 這在稽核上不可接受。
//! TEXT：SQLite 對 TEXT 做字典序比較（"9" 會大於 "10"），SUM / ORDER BY 全部會錯。

use serde::{Deserialize, Serialize};

/// 金額。單位固定為**整數元**（TWD）。
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Serialize, Deserialize, Hash,
)]
#[serde(transparent)]
pub struct Money(pub i64);

impl Money {
    pub const ZERO: Money = Money(0);
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::Add for Money {
    type Output = Money;
    fn add(self, rhs: Money) -> Money {
        Money(self.0 + rhs.0)
    }
}
impl std::ops::Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Money) -> Money {
        Money(self.0 - rhs.0)
    }
}
impl std::iter::Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        Money(iter.map(|m| m.0).sum())
    }
}
impl std::fmt::Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 單價專用刻度：1/1,000,000 元。
/// 只用於未稅單價與秤重單價的中間計算，進 Money 時四捨五入一次。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Micros(pub i64);

pub const MICROS_PER_UNIT: i64 = 1_000_000;

impl Micros {
    pub fn from_money(m: Money) -> Micros {
        Micros(m.0 * MICROS_PER_UNIT)
    }
    /// 四捨五入回整數元。
    pub fn to_money(self) -> Money {
        Money(div_round_half_up(self.0, MICROS_PER_UNIT))
    }
}

/// 整數四捨五入除法。中間值走 i128 防溢位。
///
/// 台灣稅務是「四捨五入」而**不是**銀行家捨入：2.5 進位成 3（不是 2）。
/// 負數對稱：-2.5 進位成 -3。
pub fn div_round_half_up(num: i64, den: i64) -> i64 {
    debug_assert!(den > 0, "分母必須為正");
    let (n, d) = (num as i128, den as i128);
    let r = if n >= 0 {
        (n * 2 + d) / (d * 2)
    } else {
        -(((-n) * 2 + d) / (d * 2))
    };
    r as i64
}

/// 抹零政策。台灣現金流通最小單位是 1 元，部分店家再抹到 5 / 10 元。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundingPolicy {
    /// 不抹零（金額本來就是整數元）。
    #[default]
    None,
    /// 四捨五入到 5 元。
    ToFive,
    /// 捨去到 5 元（對客人有利）。
    FloorFive,
    /// 捨去到 10 元（夜市 / 小吃常見）。
    FloorTen,
}

/// 套用抹零。
///
/// 用 div_euclid 而非除號：Rust 的除號對負數是向零截斷（-150/100 得到 -1），
/// div_euclid 才是向下取整（得到 -2）。全額退款會產生負總額，
/// 用除號會把抹零抹錯方向。
pub fn apply_rounding(total: Money, p: RoundingPolicy) -> Money {
    match p {
        RoundingPolicy::None => total,
        RoundingPolicy::ToFive => Money(div_round_half_up(total.0, 5) * 5),
        RoundingPolicy::FloorFive => Money(total.0.div_euclid(5) * 5),
        RoundingPolicy::FloorTen => Money(total.0.div_euclid(10) * 10),
    }
}

/// 台灣 5% 內含稅：從含稅總額拆出未稅銷售額與稅額。
///
/// **稅額由官方公式算出、銷售額用相減得到**，所以 sales + tax == total 恆成立 ——
/// 那是財政部對 F0401 的硬檢核，差 1 元整批退件。
///
/// 絕對不能寫成「先 round(total/1.05) 得 sales，再 round(sales*0.05) 得 tax」：
/// 那樣兩者相加可能不等於客人實付金額，發票金額與收款金額對不上。
pub fn split_tax_inclusive(total: Money, rate_bp: i64) -> (Money, Money) {
    debug_assert!((0..10_000).contains(&rate_bp), "稅率須為 0..100% 的 bp");
    let tax = div_round_half_up(total.0 * rate_bp, 10_000 + rate_bp);
    (Money(total.0 - tax), Money(tax))
}

/// 最大餘數法（Hare）分配：把 total 按 weights 比例分成 n 份，
/// 保證各份加總嚴格等於 total。
///
/// 用在四個地方，而且**必須是同一個函式**，不可各寫一套：
/// 整單折扣分攤、服務費分攤、抹零分攤、分帳金額分攤。
///
/// 平手時以「較大 weight 優先、再以 index 小者優先」決定 —— 保證同輸入完全確定，
/// 不受迭代順序影響。可重現是稽核的必要條件。
pub fn allocate(total: i64, weights: &[i64]) -> Vec<i64> {
    if weights.is_empty() {
        return Vec::new();
    }
    let sum: i128 = weights.iter().map(|&w| w as i128).sum();

    if sum == 0 {
        // 全零權重 → 平均分，餘數從前面幾份開始加。
        let n = weights.len() as i64;
        let q = total.div_euclid(n);
        let mut rem = total - q * n; // 0 <= rem < n
        return (0..n)
            .map(|_| {
                if rem > 0 {
                    rem -= 1;
                    q + 1
                } else {
                    q
                }
            })
            .collect();
    }

    let t = total as i128;
    let mut out = Vec::with_capacity(weights.len());
    let mut rems: Vec<(i128, i64, usize)> = Vec::with_capacity(weights.len());
    let mut acc: i128 = 0;

    for (i, &w) in weights.iter().enumerate() {
        let exact = t * w as i128;
        let q = exact.div_euclid(sum);
        let r = exact.rem_euclid(sum);
        out.push(q as i64);
        rems.push((r, w, i));
        acc += q;
    }

    let mut left = t - acc;
    // 餘數大者優先；平手時 weight 大者優先；再平手時 index 小者優先。
    rems.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    for &(_, _, i) in &rems {
        if left == 0 {
            break;
        }
        let step: i128 = if left > 0 { 1 } else { -1 };
        out[i] += step as i64;
        left -= step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_half_up_is_not_bankers_rounding() {
        assert_eq!(div_round_half_up(5, 2), 3); // 2.5 進位成 3（銀行家捨入會給 2）
        assert_eq!(div_round_half_up(7, 2), 4); // 3.5 進位成 4
        assert_eq!(div_round_half_up(-5, 2), -3); // 對稱
        assert_eq!(div_round_half_up(4, 2), 2);
    }

    #[test]
    fn round_half_up_does_not_overflow() {
        // 中間值走 i128，i64::MAX 附近不應 panic。
        assert_eq!(div_round_half_up(i64::MAX, 1), i64::MAX);
    }

    #[test]
    fn tax_split_identity_holds_over_wide_range() {
        // 硬檢核：sales + tax == total，全值域恆成立。
        for total in (0..=200_000).step_by(7) {
            let (s, t) = split_tax_inclusive(Money(total), 500);
            assert_eq!(s.0 + t.0, total, "total={total}");
        }
    }

    #[test]
    fn tax_split_within_mof_tolerance() {
        // 軟檢核：稅額與「銷售額乘稅率再四捨五入」的差不得超過 2 元。
        for total in (1..=200_000).step_by(13) {
            let (s, t) = split_tax_inclusive(Money(total), 500);
            let c = div_round_half_up(s.0 * 500, 10_000);
            assert!((t.0 - c).abs() <= 2, "total={total} tax={} c={c}", t.0);
        }
    }

    #[test]
    fn tax_split_known_values() {
        assert_eq!(split_tax_inclusive(Money(105), 500), (Money(100), Money(5)));
        assert_eq!(split_tax_inclusive(Money(100), 500), (Money(95), Money(5)));
        assert_eq!(
            split_tax_inclusive(Money(1000), 500),
            (Money(952), Money(48))
        );
        assert_eq!(split_tax_inclusive(Money(0), 500), (Money(0), Money(0)));
        // 零稅率 / 免稅
        assert_eq!(split_tax_inclusive(Money(250), 0), (Money(250), Money(0)));
    }

    #[test]
    fn allocate_sum_always_equals_total() {
        let cases: &[(i64, &[i64])] = &[
            (100, &[1, 1, 1]),
            (100, &[3, 1, 1]),
            (-50, &[2, 3, 5]),
            (7, &[1, 1, 1, 1, 1, 1, 1, 1]),
            (0, &[5, 5]),
            (1, &[1]),
            (999, &[0, 0, 0]),
        ];
        for (total, w) in cases {
            let out = allocate(*total, w);
            assert_eq!(out.iter().sum::<i64>(), *total, "total={total} w={w:?}");
            assert_eq!(out.len(), w.len());
        }
    }

    #[test]
    fn allocate_even_split_of_100_by_three() {
        // 100 元三人均分：34/33/33，加總必須剛好 100。
        let out = allocate(100, &[1, 1, 1]);
        assert_eq!(out, vec![34, 33, 33]);
    }

    #[test]
    fn allocate_is_deterministic() {
        let a = allocate(1000, &[7, 7, 7, 3, 3]);
        for _ in 0..50 {
            assert_eq!(allocate(1000, &[7, 7, 7, 3, 3]), a);
        }
    }

    #[test]
    fn allocate_handles_empty_and_single() {
        assert!(allocate(100, &[]).is_empty());
        assert_eq!(allocate(100, &[9]), vec![100]);
    }

    #[test]
    fn rounding_floor_ten_is_correct_on_negatives() {
        // 退款產生負總額時，捨去必須往下（-157 變 -160），不能往零。
        assert_eq!(
            apply_rounding(Money(157), RoundingPolicy::FloorTen),
            Money(150)
        );
        assert_eq!(
            apply_rounding(Money(-157), RoundingPolicy::FloorTen),
            Money(-160)
        );
        assert_eq!(
            apply_rounding(Money(157), RoundingPolicy::ToFive),
            Money(155)
        );
        assert_eq!(
            apply_rounding(Money(158), RoundingPolicy::ToFive),
            Money(160)
        );
        assert_eq!(
            apply_rounding(Money(159), RoundingPolicy::FloorFive),
            Money(155)
        );
    }

    #[test]
    fn micros_round_trip() {
        assert_eq!(Micros::from_money(Money(42)).to_money(), Money(42));
        assert_eq!(Micros(1_500_000).to_money(), Money(2)); // 1.5 進位成 2
        assert_eq!(Micros(1_499_999).to_money(), Money(1));
    }
}
