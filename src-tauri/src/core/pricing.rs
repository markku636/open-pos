//! 定價引擎。
//!
//! **零 I/O、零 async、零 SQL、零時鐘。** 所有外部資料（價格規則、可用時段、
//! 現在幾點）都由呼叫端先解析完再傳進來。這讓 `compute()` 能用假資料在
//! microsecond 內跑幾十萬次 property test —— 而「錢算對了沒」正是最值得
//! 這樣測的東西。
//!
//! # 計算順序（唯一權威定義）
//!
//! ```text
//!  1. 每行 gross    = (單價 + Σ加購) × 數量
//!  2. 每行 單品折扣
//!  3. 每行 net      = gross − 單品折扣
//!  4. subtotal      = Σ net
//!  5. 整單折扣       → 依 net 比例分攤回各行
//!  6. 服務費         = round(服務費基數 × 費率) → 依折後金額分攤回各行
//!  7. total_raw     = subtotal − 整單折扣 + 服務費
//!  8. 抹零
//!  9. grand_total   = total_raw + 抹零調整
//! 10. 內含稅拆算     = split_tax_inclusive(grand_total)
//! 11. 每行 taxable  → Σ 必須嚴格等於 grand_total
//! ```
//!
//! # 三個絕對不能做的事
//!
//! * **不能先算稅再加服務費。** 內用 10% 服務費在稅法上就是銷售額的一部分，
//!   必須開在發票內、必須課 5% 營業稅。先算稅等於短漏營業稅。
//! * **不能各自算 sales 與 tax 再相加。** 那樣兩者之和可能不等於客人實付金額。
//!   正解是算稅額再相減（見 `money::split_tax_inclusive`）。
//! * **不能各處自己寫分攤。** 折扣、服務費、抹零、分帳四個地方必須共用
//!   同一個 `allocate()`，否則各自的尾差處理會不一致，
//!   而發票要求 Σ 品項金額 == 總計，差一元就退件。

use serde::{Deserialize, Serialize};

use crate::core::money::{
    allocate, apply_rounding, div_round_half_up, split_tax_inclusive, Money, RoundingPolicy,
};
use crate::error::{AppError, AppResult};

/// 數量的刻度：千分之一份。支援「半份」這種真實需求（500 = 0.5）。
pub const QTY_SCALE: i64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    #[default]
    DineIn,
    Takeout,
    Delivery,
}

/// 折扣的算法。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscountKind {
    /// basis point：8500 = 85 折（扣掉 15%）。
    Percent(i64),
    /// 固定金額（整數元）。
    Amount(i64),
    /// 招待：全額折抵。
    ///
    /// 刻意與 Amount 分開：報表上它要進「招待統計」而不是「折扣統計」。
    /// 老闆看折扣是看行銷成效，看招待是看有沒有人在送人情。
    Comp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LineDiscount {
    pub kind: DiscountKind,
    /// 折扣上限（整數元）。None = 無上限。
    pub max_amount: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderDiscount {
    pub kind: DiscountKind,
    pub max_amount: Option<i64>,
    /// 這筆折扣是否在服務費**之前**扣（服務費基數因此變小）。
    ///
    /// 台灣餐飲兩種做法都存在（打折後收 10%、或按原價收 10%），
    /// 所以逐筆決定而不是一個全域開關。
    pub before_service_charge: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LineInput {
    /// 單價（整數元）。價格規則已由呼叫端套用完。
    pub unit_price: Money,
    /// 數量 × 1000。
    pub qty_milli: i64,
    /// 加購項：(單價, 數量)。
    pub modifiers: Vec<(Money, i64)>,
    pub discounts: Vec<LineDiscount>,
}

impl LineInput {
    pub fn simple(unit_price: i64, qty: i64) -> Self {
        Self {
            unit_price: Money(unit_price),
            qty_milli: qty * QTY_SCALE,
            modifiers: Vec::new(),
            discounts: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PricingInput {
    pub channel: Channel,
    /// 服務費費率（basis point）。內用 10% = 1000。不適用的通路傳 0。
    pub service_charge_rate_bp: i64,
    pub rounding: RoundingPolicy,
    /// 營業稅率（basis point）。台灣 5% = 500。
    pub tax_rate_bp: i64,
    pub lines: Vec<LineInput>,
    pub order_discounts: Vec<OrderDiscount>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineOutput {
    pub gross: Money,
    pub line_discount: Money,
    pub net: Money,
    pub allocated_order_discount: Money,
    pub allocated_service_charge: Money,
    /// 這一行最終佔總額的多少。**Σ 必須嚴格等於 grand_total** ——
    /// 發票的品項金額加總要對得上總計，差一元就被退件。
    pub taxable_amount: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingOutput {
    pub lines: Vec<LineOutput>,
    pub subtotal: Money,
    pub line_discount_total: Money,
    pub order_discount_total: Money,
    /// 招待金額另計，不混進折扣統計。
    pub comp_total: Money,
    pub service_charge: Money,
    pub rounding_adjustment: Money,
    pub grand_total: Money,
    pub sales_amount: Money,
    pub tax_amount: Money,
}

/// 依 §計算順序算出一張單的所有金額。
pub fn compute(input: &PricingInput) -> AppResult<PricingOutput> {
    validate(input)?;

    // ── 1~3. 每一行 ───────────────────────────────────────────────
    let mut lines: Vec<LineOutput> = Vec::with_capacity(input.lines.len());
    let mut comp_total = 0i64;

    for l in &input.lines {
        let unit_with_mods: i64 = l.unit_price.0
            + l.modifiers
                .iter()
                .map(|(price, qty)| price.0 * qty)
                .sum::<i64>();
        // 數量是千分之一刻度，乘完要捨回整數元。
        // 全程走 i128：先轉 i64 再除會在極端輸入下溢位，而那是靜默的錯誤 ——
        // 金額突然變成負數，而且只在某一筆離譜的資料上發生。
        let gross = round_div_i128(
            unit_with_mods as i128 * l.qty_milli as i128,
            QTY_SCALE as i128,
        );

        let mut discount = 0i64;
        for d in &l.discounts {
            let amount = apply_discount(gross - discount, &d.kind, d.max_amount);
            if matches!(d.kind, DiscountKind::Comp) {
                comp_total += amount;
            }
            discount += amount;
        }
        // 折扣不可能超過原價 —— 否則會出現負數的一行，發票開不出來。
        discount = discount.min(gross);

        lines.push(LineOutput {
            gross: Money(gross),
            line_discount: Money(discount),
            net: Money(gross - discount),
            allocated_order_discount: Money::ZERO,
            allocated_service_charge: Money::ZERO,
            taxable_amount: Money::ZERO,
        });
    }

    // ── 4. 小計 ──────────────────────────────────────────────────
    let subtotal: i64 = lines.iter().map(|l| l.net.0).sum();
    let line_discount_total: i64 = lines.iter().map(|l| l.line_discount.0).sum();

    // ── 5. 整單折扣 ───────────────────────────────────────────────
    let mut order_discount_total = 0i64;
    let mut discount_before_service = 0i64;
    for d in &input.order_discounts {
        let amount = apply_discount(subtotal - order_discount_total, &d.kind, d.max_amount);
        if matches!(d.kind, DiscountKind::Comp) {
            comp_total += amount;
        }
        if d.before_service_charge {
            discount_before_service += amount;
        }
        order_discount_total += amount;
    }
    order_discount_total = order_discount_total.min(subtotal);
    discount_before_service = discount_before_service.min(order_discount_total);

    let net_weights: Vec<i64> = lines.iter().map(|l| l.net.0).collect();
    for (l, share) in lines
        .iter_mut()
        .zip(allocate(order_discount_total, &net_weights))
    {
        l.allocated_order_discount = Money(share);
    }

    // ── 6. 服務費 ─────────────────────────────────────────────────
    //
    // 服務費**必須**計入稅基：內用 10% 在稅法上就是銷售額的一部分，
    // 要開在統一發票內並課 5% 營業稅。它不是代收轉付。
    let service_base = subtotal - discount_before_service;
    let service_charge = if input.service_charge_rate_bp > 0 {
        div_round_half_up(service_base * input.service_charge_rate_bp, 10_000)
    } else {
        0
    };

    let after_discount_weights: Vec<i64> = lines
        .iter()
        .map(|l| l.net.0 - l.allocated_order_discount.0)
        .collect();
    for (l, share) in lines
        .iter_mut()
        .zip(allocate(service_charge, &after_discount_weights))
    {
        l.allocated_service_charge = Money(share);
    }

    // ── 7~9. 總額與抹零 ────────────────────────────────────────────
    let total_raw = subtotal - order_discount_total + service_charge;
    let grand_total = apply_rounding(Money(total_raw), input.rounding).0;
    let rounding_adjustment = grand_total - total_raw;

    // ── 10. 內含稅拆算 ─────────────────────────────────────────────
    let (sales_amount, tax_amount) = split_tax_inclusive(Money(grand_total), input.tax_rate_bp);

    // ── 11. 每行的可稅金額 ──────────────────────────────────────────
    //
    // 抹零也要分攤下去，否則 Σ taxable 會與 grand_total 差幾元。
    let pre_rounding: Vec<i64> = lines
        .iter()
        .map(|l| l.net.0 - l.allocated_order_discount.0 + l.allocated_service_charge.0)
        .collect();
    let rounding_shares = allocate(rounding_adjustment, &pre_rounding);
    for (i, l) in lines.iter_mut().enumerate() {
        l.taxable_amount = Money(pre_rounding[i] + rounding_shares[i]);
    }

    let out = PricingOutput {
        lines,
        subtotal: Money(subtotal),
        line_discount_total: Money(line_discount_total),
        order_discount_total: Money(order_discount_total),
        comp_total: Money(comp_total),
        service_charge: Money(service_charge),
        rounding_adjustment: Money(rounding_adjustment),
        grand_total: Money(grand_total),
        sales_amount,
        tax_amount,
    };

    debug_assert_invariants(&out);
    Ok(out)
}

/// i128 版的四捨五入除法。
///
/// `money::div_round_half_up` 收 i64；這裡的中間值是「單價 × 數量千分位」，
/// 在極端輸入下會超出 i64，所以整條路徑留在 i128 直到最後才落地。
fn round_div_i128(num: i128, den: i128) -> i64 {
    debug_assert!(den > 0);
    let r = if num >= 0 {
        (num * 2 + den) / (den * 2)
    } else {
        -((-num * 2 + den) / (den * 2))
    };
    r.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// 算一筆折扣的金額。回傳值恆為非負，且不超過 `base`。
fn apply_discount(base: i64, kind: &DiscountKind, max_amount: Option<i64>) -> i64 {
    if base <= 0 {
        return 0;
    }
    let raw = match kind {
        // 8500 表示「打 85 折」，也就是折掉 15% —— 這是台灣講折扣的方式。
        DiscountKind::Percent(bp) => div_round_half_up(base * (10_000 - bp), 10_000),
        DiscountKind::Amount(a) => *a,
        DiscountKind::Comp => base,
    };
    raw.clamp(0, max_amount.unwrap_or(i64::MAX)).min(base)
}

fn validate(input: &PricingInput) -> AppResult<()> {
    if !(0..10_000).contains(&input.tax_rate_bp) {
        return Err(AppError::Validation(format!(
            "稅率 {} bp 不在合理範圍（0 ~ 9999）",
            input.tax_rate_bp
        )));
    }
    if !(0..100_000).contains(&input.service_charge_rate_bp) {
        return Err(AppError::Validation(format!(
            "服務費費率 {} bp 不在合理範圍",
            input.service_charge_rate_bp
        )));
    }
    for (i, l) in input.lines.iter().enumerate() {
        if l.qty_milli <= 0 {
            return Err(AppError::Validation(format!(
                "第 {} 行的數量必須大於 0",
                i + 1
            )));
        }
        if l.unit_price.0 < 0 {
            return Err(AppError::Validation(format!(
                "第 {} 行的單價不能是負數",
                i + 1
            )));
        }
        for d in &l.discounts {
            if let DiscountKind::Percent(bp) = d.kind {
                if !(0..=10_000).contains(&bp) {
                    return Err(AppError::Validation(format!(
                        "第 {} 行的折扣 {bp} bp 不在 0 ~ 10000 之間",
                        i + 1
                    )));
                }
            }
        }
    }
    Ok(())
}

/// 三條不變量。debug build 才檢查 —— 它們是設計的一部分，不是防禦性程式碼。
fn debug_assert_invariants(out: &PricingOutput) {
    debug_assert_eq!(
        out.sales_amount.0 + out.tax_amount.0,
        out.grand_total.0,
        "銷售額 + 稅額必須等於總計 —— 這是財政部對 F0401 的硬檢核"
    );
    let sum: i64 = out.lines.iter().map(|l| l.taxable_amount.0).sum();
    debug_assert_eq!(
        sum, out.grand_total.0,
        "各行金額加總必須等於總計 —— 發票的品項要對得上總額"
    );
    debug_assert!(out.grand_total.0 >= 0, "總額不該是負數；退款走另外的流程");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(lines: Vec<LineInput>) -> PricingInput {
        PricingInput {
            channel: Channel::DineIn,
            service_charge_rate_bp: 0,
            rounding: RoundingPolicy::None,
            tax_rate_bp: 500,
            lines,
            order_discounts: Vec::new(),
        }
    }

    #[test]
    fn simple_order_splits_tax_correctly() {
        let out = compute(&input(vec![LineInput::simple(105, 1)])).unwrap();
        assert_eq!(out.grand_total, Money(105));
        assert_eq!(out.sales_amount, Money(100));
        assert_eq!(out.tax_amount, Money(5));
        assert_eq!(out.lines[0].taxable_amount, Money(105));
    }

    #[test]
    fn quantity_multiplies() {
        let out = compute(&input(vec![LineInput::simple(60, 3)])).unwrap();
        assert_eq!(out.subtotal, Money(180));
    }

    #[test]
    fn half_portion_is_supported() {
        // 半份滷肉飯：55 元的一半 = 27.5，四捨五入成 28。
        let mut l = LineInput::simple(55, 1);
        l.qty_milli = 500;
        let out = compute(&input(vec![l])).unwrap();
        assert_eq!(out.subtotal, Money(28));
    }

    #[test]
    fn modifiers_add_per_unit_and_multiply_by_quantity() {
        // 珍奶 60 + 加珍珠 10 ×2 = 80，點兩杯 = 160。
        let l = LineInput {
            unit_price: Money(60),
            qty_milli: 2 * QTY_SCALE,
            modifiers: vec![(Money(10), 2)],
            discounts: Vec::new(),
        };
        let out = compute(&input(vec![l])).unwrap();
        assert_eq!(out.lines[0].gross, Money(160));
    }

    #[test]
    fn percent_discount_uses_taiwanese_convention() {
        // 8500 bp 是「打 85 折」= 折掉 15%，不是「折掉 85%」。
        let l = LineInput {
            unit_price: Money(200),
            qty_milli: QTY_SCALE,
            modifiers: Vec::new(),
            discounts: vec![LineDiscount {
                kind: DiscountKind::Percent(8500),
                max_amount: None,
            }],
        };
        let out = compute(&input(vec![l])).unwrap();
        assert_eq!(out.lines[0].line_discount, Money(30));
        assert_eq!(out.lines[0].net, Money(170));
    }

    #[test]
    fn max_amount_caps_a_percent_discount() {
        let l = LineInput {
            unit_price: Money(1000),
            qty_milli: QTY_SCALE,
            modifiers: Vec::new(),
            discounts: vec![LineDiscount {
                kind: DiscountKind::Percent(5000), // 五折 = 折 500
                max_amount: Some(100),
            }],
        };
        let out = compute(&input(vec![l])).unwrap();
        assert_eq!(out.lines[0].line_discount, Money(100));
    }

    #[test]
    fn comp_is_counted_separately_from_discounts() {
        // 招待要進「招待統計」而不是「折扣統計」——
        // 老闆看折扣是看行銷成效，看招待是看有沒有人在送人情。
        let l = LineInput {
            unit_price: Money(120),
            qty_milli: QTY_SCALE,
            modifiers: Vec::new(),
            discounts: vec![LineDiscount {
                kind: DiscountKind::Comp,
                max_amount: None,
            }],
        };
        let out = compute(&input(vec![l])).unwrap();
        assert_eq!(out.comp_total, Money(120));
        assert_eq!(out.grand_total, Money(0));
        // 金額為 0 的單仍然要開得出發票。
        assert_eq!(out.sales_amount, Money(0));
        assert_eq!(out.tax_amount, Money(0));
    }

    #[test]
    fn service_charge_is_inside_the_tax_base() {
        // 內用 3 道菜共 1000 元 + 10% 服務費 = 1100，稅從 1100 拆。
        // **絕不能**先對 1000 算稅再加服務費 —— 那是短漏營業稅。
        let mut i = input(vec![
            LineInput::simple(400, 1),
            LineInput::simple(350, 1),
            LineInput::simple(250, 1),
        ]);
        i.service_charge_rate_bp = 1000;
        let out = compute(&i).unwrap();

        assert_eq!(out.subtotal, Money(1000));
        assert_eq!(out.service_charge, Money(100));
        assert_eq!(out.grand_total, Money(1100));
        let (s, t) = split_tax_inclusive(Money(1100), 500);
        assert_eq!((out.sales_amount, out.tax_amount), (s, t));
        assert_eq!(out.sales_amount.0 + out.tax_amount.0, 1100);
    }

    #[test]
    fn discount_before_service_charge_shrinks_the_base() {
        let mut i = input(vec![LineInput::simple(1000, 1)]);
        i.service_charge_rate_bp = 1000;
        i.order_discounts = vec![OrderDiscount {
            kind: DiscountKind::Amount(200),
            max_amount: None,
            before_service_charge: true,
        }];
        let out = compute(&i).unwrap();
        // 折後 800 收 10% = 80
        assert_eq!(out.service_charge, Money(80));
        assert_eq!(out.grand_total, Money(880));
    }

    #[test]
    fn discount_after_service_charge_keeps_the_base() {
        let mut i = input(vec![LineInput::simple(1000, 1)]);
        i.service_charge_rate_bp = 1000;
        i.order_discounts = vec![OrderDiscount {
            kind: DiscountKind::Amount(200),
            max_amount: None,
            before_service_charge: false,
        }];
        let out = compute(&i).unwrap();
        // 按原價 1000 收 10% = 100
        assert_eq!(out.service_charge, Money(100));
        assert_eq!(out.grand_total, Money(900));
    }

    #[test]
    fn order_discount_is_allocated_across_lines_without_remainder() {
        // 100 元折扣分到三行 —— 加總必須剛好 100，不能是 99 或 101。
        let mut i = input(vec![
            LineInput::simple(100, 1),
            LineInput::simple(100, 1),
            LineInput::simple(100, 1),
        ]);
        i.order_discounts = vec![OrderDiscount {
            kind: DiscountKind::Amount(100),
            max_amount: None,
            before_service_charge: true,
        }];
        let out = compute(&i).unwrap();
        let sum: i64 = out.lines.iter().map(|l| l.allocated_order_discount.0).sum();
        assert_eq!(sum, 100);
        assert_eq!(
            out.lines
                .iter()
                .map(|l| l.allocated_order_discount.0)
                .collect::<Vec<_>>(),
            vec![34, 33, 33]
        );
    }

    #[test]
    fn rounding_is_shared_out_so_lines_still_sum_to_the_total() {
        let mut i = input(vec![
            LineInput::simple(37, 1),
            LineInput::simple(41, 1),
            LineInput::simple(53, 1),
        ]);
        i.rounding = RoundingPolicy::FloorTen; // 131 → 130
        let out = compute(&i).unwrap();
        assert_eq!(out.grand_total, Money(130));
        assert_eq!(out.rounding_adjustment, Money(-1));
        let sum: i64 = out.lines.iter().map(|l| l.taxable_amount.0).sum();
        assert_eq!(sum, 130, "抹零也要分攤下去，否則發票品項對不上總計");
    }

    #[test]
    fn takeout_has_no_service_charge() {
        // 外帶不收服務費是台灣慣例；呼叫端傳 0 進來。
        let mut i = input(vec![LineInput::simple(100, 1)]);
        i.channel = Channel::Takeout;
        i.service_charge_rate_bp = 0;
        let out = compute(&i).unwrap();
        assert_eq!(out.service_charge, Money::ZERO);
        assert_eq!(out.grand_total, Money(100));
    }

    #[test]
    fn discount_cannot_exceed_the_line_total() {
        let l = LineInput {
            unit_price: Money(50),
            qty_milli: QTY_SCALE,
            modifiers: Vec::new(),
            discounts: vec![LineDiscount {
                kind: DiscountKind::Amount(999),
                max_amount: None,
            }],
        };
        let out = compute(&input(vec![l])).unwrap();
        assert_eq!(out.lines[0].line_discount, Money(50));
        assert_eq!(out.lines[0].net, Money(0));
        assert!(out.grand_total.0 >= 0, "不能算出負數的總額");
    }

    #[test]
    fn invalid_input_is_rejected_with_a_useful_message() {
        let mut i = input(vec![LineInput::simple(10, 1)]);
        i.lines[0].qty_milli = 0;
        assert!(compute(&i)
            .unwrap_err()
            .message()
            .contains("數量必須大於 0"));

        let mut i = input(vec![LineInput::simple(10, 1)]);
        i.tax_rate_bp = 20_000;
        assert!(compute(&i).unwrap_err().message().contains("稅率"));
    }

    #[test]
    fn empty_order_is_valid_and_zero() {
        let out = compute(&input(vec![])).unwrap();
        assert_eq!(out.grand_total, Money::ZERO);
        assert_eq!(out.sales_amount, Money::ZERO);
        assert!(out.lines.is_empty());
    }

    /// ★ 三條不變量在隨機輸入下都必須成立。
    ///
    /// 這支測試比上面所有具體案例加起來更有價值：它涵蓋的是「我沒想到的組合」，
    /// 而金額 bug 幾乎都藏在那裡。
    #[test]
    fn invariants_hold_over_pseudo_random_orders() {
        // 用一個確定性的 LCG，不引入 proptest 相依也能掃過幾萬種組合，
        // 而且失敗時完全可重現（種子是常數）。
        let mut seed: u64 = 0x5EED_1234_ABCD_0001;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as i64
        };

        for case in 0..20_000 {
            let n = (next() % 6) as usize + 1;
            let lines: Vec<LineInput> = (0..n)
                .map(|_| LineInput {
                    unit_price: Money(next() % 3000),
                    qty_milli: (next() % 5 + 1) * 250, // 0.25 ~ 1.25 份
                    modifiers: if next() % 3 == 0 {
                        vec![(Money(next() % 50), next() % 3 + 1)]
                    } else {
                        Vec::new()
                    },
                    discounts: if next() % 4 == 0 {
                        vec![LineDiscount {
                            kind: DiscountKind::Percent(next() % 10_001),
                            max_amount: None,
                        }]
                    } else {
                        Vec::new()
                    },
                })
                .collect();

            let rounding = match next() % 4 {
                0 => RoundingPolicy::None,
                1 => RoundingPolicy::ToFive,
                2 => RoundingPolicy::FloorFive,
                _ => RoundingPolicy::FloorTen,
            };

            let order_discounts = if next() % 3 == 0 {
                vec![OrderDiscount {
                    kind: DiscountKind::Amount(next() % 500),
                    max_amount: None,
                    before_service_charge: next() % 2 == 0,
                }]
            } else {
                Vec::new()
            };

            let input = PricingInput {
                channel: Channel::DineIn,
                service_charge_rate_bp: if next() % 2 == 0 { 1000 } else { 0 },
                rounding,
                tax_rate_bp: 500,
                lines,
                order_discounts,
            };

            let out = compute(&input).unwrap_or_else(|e| panic!("case {case}: {}", e.message()));

            assert_eq!(
                out.sales_amount.0 + out.tax_amount.0,
                out.grand_total.0,
                "case {case}: 銷售額 + 稅額 != 總計"
            );
            let sum: i64 = out.lines.iter().map(|l| l.taxable_amount.0).sum();
            assert_eq!(sum, out.grand_total.0, "case {case}: 各行加總 != 總計");
            assert!(out.grand_total.0 >= 0, "case {case}: 總額為負");

            // 財政部的軟檢核：|稅額 − round(銷售額 × 稅率)| <= 2
            let c = div_round_half_up(out.sales_amount.0 * 500, 10_000);
            assert!(
                (out.tax_amount.0 - c).abs() <= 2,
                "case {case}: 稅額超出容許誤差"
            );
        }
    }
}
