//! 店家設定。
//!
//! # 為什麼這一頁必須存在
//!
//! 在這之前 `stores` 只由 seed 建立，之後**沒有任何方式可以改** ——
//! 店名、統編、稅率、服務費率、抹零、營業日切點全部釘死在第一次啟動的那一刻。
//!
//! 而台灣餐廳的 10% 服務費是每一家都要自己決定的東西；統編沒填的話收據上
//! 印不出來；營業日切點不對，日結會把凌晨兩點的單算到隔天。
//! 這些都不是「進階設定」，是開店第一天就要改的東西。
//!
//! # 改了不會回頭重算
//!
//! **已經結帳的單不會變**：它存的是當時算好的金額。那是刻意的 ——
//! 三個月前的帳不該因為今天改了設定而變動，一份會自己變的報表在稅務查核上
//! 完全站不住。
//!
//! 但**還開著沒結的單會用新費率重算**，因為 `order::recompute()` 每次都重讀
//! `stores`，而那張單還沒有「當時算好的金額」這回事。畫面上的說明要把這兩件事
//! 分開講清楚，含糊的講法在稅這個題目上等於沒講。
//! 兩者都有測試釘住：`src-tauri/tests/store_settings.rs`。
//!
//! 這也是為什麼這裡不提供「重算歷史訂單」的按鈕：那個功能唯一的用途
//! 是把帳做平，而那正是它不該存在的理由。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::services::rbac;

const PERM: &str = "settings.store";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreView {
    pub id: String,
    pub code: String,
    pub name: String,
    /// 統一編號。收據要印，開發票更要。
    pub tax_id: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub tz: String,
    /// 營業日切點（`05:00`）。凌晨兩點的單算前一天。
    pub business_day_cutoff: String,
    pub currency: String,
    /// 稅率（basis point）。台灣 5% = 500。
    pub tax_rate_bp: i64,
    /// 內用服務費率（basis point）。10% = 1000。外帶不收。
    pub service_charge_rate_bp: i64,
    /// none / to_five / floor_five / floor_ten
    pub rounding_policy: String,
    /// 每人低消。**只用來提醒，不會自動補一行差額。**
    pub min_charge_per_head: i64,
    /// 開桌費 / お通し 的商品。開檯時自動點上「人數」份。
    ///
    /// 只存商品不存金額 —— 金額是那個商品的售價，而它已經有規格、平日假日價、
    /// 稅別、廚房分區一整套。詳見 `migrations/0012_cover_charge.sql`。
    pub cover_charge_item_id: Option<String>,
    /// 那個商品現在叫什麼、多少錢。畫面要顯示「開桌費 $50 / 人」，
    /// 而讓前端自己再查一次商品是在等兩邊哪天顯示不一樣。
    pub cover_charge_label: Option<String>,
}

pub async fn get(ctx: &Ctx) -> AppResult<StoreView> {
    let r = sqlx::query(
        "SELECT id, code, name, tax_id, address, phone, tz, business_day_cutoff,
                currency, tax_rate_bp, service_charge_rate_bp, rounding_policy,
                min_charge_per_head, cover_charge_item_id,
                (SELECT i.name || '  $' || i.base_price FROM items i
                  WHERE i.id = stores.cover_charge_item_id) AS cover_charge_label
           FROM stores WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_optional(ctx.db.reader())
    .await?
    .ok_or_else(|| AppError::Internal("找不到店家資料".into()))?;

    Ok(StoreView {
        id: r.get("id"),
        code: r.get("code"),
        name: r.get("name"),
        tax_id: r.get("tax_id"),
        address: r.get("address"),
        phone: r.get("phone"),
        tz: r.get("tz"),
        business_day_cutoff: r.get("business_day_cutoff"),
        currency: r.get("currency"),
        tax_rate_bp: r.get("tax_rate_bp"),
        service_charge_rate_bp: r.get("service_charge_rate_bp"),
        rounding_policy: r.get("rounding_policy"),
        min_charge_per_head: r.get("min_charge_per_head"),
        cover_charge_item_id: r.get("cover_charge_item_id"),
        cover_charge_label: r.get("cover_charge_label"),
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreInput {
    pub name: String,
    pub tax_id: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub business_day_cutoff: String,
    pub tax_rate_bp: i64,
    pub service_charge_rate_bp: i64,
    pub rounding_policy: String,
    pub min_charge_per_head: i64,
    /// 開桌費商品。`None` 或空字串 = 不收開桌費。
    #[serde(default)]
    pub cover_charge_item_id: Option<String>,
}

pub async fn update(ctx: &Ctx, input: StoreInput) -> AppResult<StoreView> {
    rbac::require(&ctx.db, &ctx.actor, PERM).await?;

    if input.name.trim().is_empty() {
        return Err(AppError::Validation("店名不能空白".into()));
    }
    // 稅率上限 100%。打錯一個零（500 → 5000）的症狀是每張單都多收 45%，
    // 而收銀員不會知道為什麼 —— 擋在這裡比擋在客人面前便宜得多。
    if !(0..=10_000).contains(&input.tax_rate_bp) {
        return Err(AppError::Validation(
            "稅率要在 0 到 10000 之間（basis point，5% = 500）".into(),
        ));
    }
    if !(0..=10_000).contains(&input.service_charge_rate_bp) {
        return Err(AppError::Validation(
            "服務費率要在 0 到 10000 之間（basis point，10% = 1000）".into(),
        ));
    }
    if input.min_charge_per_head < 0 {
        return Err(AppError::Validation("低消不能是負數".into()));
    }
    if !matches!(
        input.rounding_policy.as_str(),
        "none" | "to_five" | "floor_five" | "floor_ten"
    ) {
        return Err(AppError::Validation(
            format!("不認得的抹零方式：{}", input.rounding_policy).into(),
        ));
    }
    validate_cutoff(&input.business_day_cutoff)?;

    // 開桌費商品要真的存在而且還在賣。
    //
    // 擋在這裡的理由：這個設定的效果要到「下一次有客人入座」才看得到，
    // 而那時錯的不是設定頁而是收銀機 —— 中間可能隔了好幾個小時，
    // 沒有人會把兩件事連起來。空字串視為「不收」，因為 HTML 的 select
    // 沒有 null，空值就是空字串。
    let cover = input
        .cover_charge_item_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(id) = cover {
        let ok: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM items WHERE id = ?1 AND deleted_at IS NULL AND is_active = 1",
        )
        .bind(id)
        .fetch_optional(ctx.db.reader())
        .await?;
        if ok.is_none() {
            return Err(AppError::Validation(
                "選的開桌費商品不存在或已經下架了".into(),
            ));
        }
    }

    // 先讀舊值 —— 稽核要記「從什麼改成什麼」，只記新值的話
    // 事後看不出來到底動了哪一項。
    let current = get(ctx).await?;

    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    sqlx::query(
        "UPDATE stores
            SET name = ?1, tax_id = ?2, address = ?3, phone = ?4,
                business_day_cutoff = ?5, tax_rate_bp = ?6,
                service_charge_rate_bp = ?7, rounding_policy = ?8,
                min_charge_per_head = ?9, cover_charge_item_id = ?11, updated_at = ?10
          WHERE id = (SELECT id FROM stores WHERE deleted_at IS NULL ORDER BY id LIMIT 1)",
    )
    .bind(input.name.trim())
    .bind(
        input
            .tax_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .bind(
        input
            .address
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .bind(
        input
            .phone
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .bind(&input.business_day_cutoff)
    .bind(input.tax_rate_bp)
    .bind(input.service_charge_rate_bp)
    .bind(&input.rounding_policy)
    .bind(input.min_charge_per_head)
    .bind(now.iso())
    .bind(cover)
    .execute(uow.conn())
    .await?;
    // 改設定是會被追究的事：三個月後有人問「為什麼六月的服務費是 5%」，
    // 稽核紀錄是唯一答得出來的地方。
    //
    // ★ 寫在**同一個交易裡**：稽核寫失敗就整筆退回。
    //   這與 kanban 的做法刻意相反（那邊用 try/catch 吞掉稽核失敗）——
    //   會動到金額規則的設定，防弊優先於可用性。
    // 開桌費也要記：它會自動在每一桌加上一行金額，是這一頁裡最容易被
    // 「誰改的？什麼時候改的？」問到的一項。
    let describe = |tax: i64, service: i64, rounding: &str, min: i64, cover: Option<&str>| {
        format!(
            "稅率 {tax} / 服務費 {service} / 抹零 {rounding} / 低消 {min} / 開桌費 {}",
            cover.unwrap_or("無")
        )
    };
    let before = describe(
        current.tax_rate_bp,
        current.service_charge_rate_bp,
        &current.rounding_policy,
        current.min_charge_per_head,
        current.cover_charge_item_id.as_deref(),
    );
    let after = describe(
        input.tax_rate_bp,
        input.service_charge_rate_bp,
        &input.rounding_policy,
        input.min_charge_per_head,
        cover,
    );
    crate::services::audit::write_in(
        &mut uow,
        crate::services::audit::AuditEntry {
            entity_type: "store",
            entity_id: &current.id,
            action: crate::services::audit::AuditAction::SettingsChange,
            old_value: Some(before),
            new_value: Some(after),
            amount_delta: None,
            reason_id: None,
            approved_by: None,
            terminal_id: None,
            shift_id: None,
            business_date: None,
        },
        &ctx.actor,
        &now,
    )
    .await?;

    uow.commit().await?;
    get(ctx).await
}

/// 營業日切點必須是 `HH:MM`。
///
/// 這個欄位錯了，症狀是**日結把凌晨的單算到隔天** —— 而那要等到月底對帳
/// 才會被發現，那時已經有三十天的數字是錯的。所以格式擋在這裡。
fn validate_cutoff(s: &str) -> AppResult<()> {
    let bad = || AppError::Validation("營業日切點要像 05:00 這樣的 24 小時制時間".into());
    let (h, m) = s.split_once(':').ok_or_else(bad)?;
    if h.len() != 2 || m.len() != 2 {
        return Err(bad());
    }
    let (h, m) = (
        h.parse::<u32>().map_err(|_| bad())?,
        m.parse::<u32>().map_err(|_| bad())?,
    );
    if h > 23 || m > 59 {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cutoff_must_look_like_a_time() {
        assert!(validate_cutoff("05:00").is_ok());
        assert!(validate_cutoff("00:00").is_ok());
        assert!(validate_cutoff("23:59").is_ok());

        // 這些錯誤的症狀都是「日結把凌晨的單算到隔天」，
        // 而那要等到月底對帳才發現 —— 所以一個都不能放過。
        assert!(validate_cutoff("5:00").is_err(), "要補零");
        assert!(validate_cutoff("24:00").is_err(), "沒有 24 點");
        assert!(validate_cutoff("05:60").is_err(), "沒有 60 分");
        assert!(validate_cutoff("0500").is_err(), "少了冒號");
        assert!(validate_cutoff("").is_err());
        assert!(validate_cutoff("上午五點").is_err());
    }
}
