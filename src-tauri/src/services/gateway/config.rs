//! 金流設定。
//!
//! # 憑證只進不出
//!
//! 設定頁存得進去，但**讀不回來** —— 回給畫面的永遠只有「有沒有設定」與
//! 末四碼。理由不是加密（那一欄沒有加密，而假裝它有比誠實地說出來更危險），
//! 是**減少它出現的地方**：一個永遠不會被送到前端的秘密，就不會躺在
//! webview 的記憶體裡、不會被截圖截到、不會出現在使用者貼給我的畫面裡。
//!
//! 診斷包與 log 一律不含它（見 `diagnostics.rs` 的斷言測試）。
//! 而備份出去的 `.db` 檔要當成含有金流憑證的檔案來保管 —— 這一句直接寫在
//! 設定頁上給店家看。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::services::rbac;

const PERM_SETTINGS: &str = "settings.store";

/// 一個金流商需要哪些欄位。
///
/// 寫死在程式裡而不是讓使用者自由填 key/value：欄位名稱打錯一個字的症狀是
/// 「設定看起來都填好了，但一刷就失敗」，而那是最難查的一種。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialField {
    pub key: &'static str,
    pub label: &'static str,
    /// 給店家看的一句話：這個值要去哪裡拿。
    pub hint: &'static str,
    /// 已經填過了（畫面上顯示末四碼，不回明文）。
    pub is_set: bool,
    pub tail: Option<String>,
}

/// 這個金流商要哪幾個欄位。
fn fields_for(provider: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    match provider {
        "linepay" => &[
            (
                "channel_id",
                "Channel ID",
                "LINE Pay 商家後台 → 管理付款連結 → 線上技術串接資訊",
            ),
            (
                "channel_secret",
                "Channel Secret",
                "LINE Pay 商家後台，跟 Channel ID 同一頁。這一組等於你的收款權限，不要外流",
            ),
        ],
        "newebpay" => &[
            (
                "merchant_id",
                "商店代號 MerchantID",
                "藍新後台 → 商店資料設定",
            ),
            (
                "hash_key",
                "HashKey",
                "藍新後台 → 商店資料設定 → 串接程式設定",
            ),
            (
                "hash_iv",
                "HashIV",
                "藍新後台 → 商店資料設定 → 串接程式設定，跟 HashKey 一起給的",
            ),
        ],
        // manual 不需要任何憑證 —— 錢是在另一台實體刷卡機上收的。
        _ => &[],
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayView {
    pub id: String,
    /// manual / linepay / newebpay
    pub provider: String,
    pub provider_label: String,
    pub display_name: String,
    pub payment_method_id: Option<String>,
    pub payment_method_name: Option<String>,
    pub is_sandbox: bool,
    pub is_active: bool,
    /// 需要哪些憑證、填了沒有。**不含明文。**
    pub fields: Vec<CredentialField>,
    /// 還缺哪些必填欄位。有東西就代表這條線還不能開。
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayInput {
    pub id: Option<String>,
    pub provider: String,
    pub display_name: String,
    pub payment_method_id: Option<String>,
    pub is_sandbox: Option<bool>,
    pub is_active: Option<bool>,
    /// 只送有改動的欄位。沒送的沿用原本存好的值 ——
    /// 不然每次改個「測試環境」的勾勾都要把 secret 重打一次。
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
}

pub async fn list(ctx: &Ctx) -> AppResult<Vec<GatewayView>> {
    rbac::require(&ctx.db, &ctx.actor, PERM_SETTINGS).await?;
    let rows = sqlx::query(
        "SELECT g.id, g.provider, g.display_name, g.payment_method_id, g.is_sandbox,
                g.is_active, g.credentials_json, m.name AS method_name
           FROM payment_gateways g
           LEFT JOIN payment_methods m ON m.id = g.payment_method_id
          WHERE g.deleted_at IS NULL
          ORDER BY g.display_name",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    Ok(rows.iter().map(view_of).collect())
}

fn view_of(r: &sqlx::sqlite::SqliteRow) -> GatewayView {
    let provider: String = r.get("provider");
    let stored: BTreeMap<String, String> = r
        .get::<Option<String>, _>("credentials_json")
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();

    let fields: Vec<CredentialField> = fields_for(&provider)
        .iter()
        .map(|(key, label, hint)| {
            let value = stored.get(*key).filter(|v| !v.is_empty());
            CredentialField {
                key,
                label,
                hint,
                is_set: value.is_some(),
                // 只回末四碼。夠讓人確認「是不是我貼的那一組」，
                // 又不足以拿去用。
                tail: value.map(|v| {
                    let n = v.chars().count();
                    v.chars().skip(n.saturating_sub(4)).collect()
                }),
            }
        })
        .collect();

    let missing = fields
        .iter()
        .filter(|f| !f.is_set)
        .map(|f| f.label.to_string())
        .collect();

    GatewayView {
        id: r.get("id"),
        provider_label: provider_label(&provider).into(),
        provider,
        display_name: r.get("display_name"),
        payment_method_id: r.get("payment_method_id"),
        payment_method_name: r.get("method_name"),
        is_sandbox: r.get::<i64, _>("is_sandbox") == 1,
        is_active: r.get::<i64, _>("is_active") == 1,
        fields,
        missing,
    }
}

pub fn provider_label(code: &str) -> &'static str {
    match code {
        "manual" => "不串接（自己抄授權碼）",
        "linepay" => "LINE Pay",
        "newebpay" => "藍新金流",
        _ => "未知",
    }
}

pub async fn upsert(ctx: &Ctx, input: GatewayInput) -> AppResult<GatewayView> {
    rbac::require(&ctx.db, &ctx.actor, PERM_SETTINGS).await?;
    if input.display_name.trim().is_empty() {
        return Err(AppError::Validation(crate::msg!("gateway.name_required")));
    }
    if !matches!(input.provider.as_str(), "manual" | "linepay" | "newebpay") {
        return Err(AppError::Validation(crate::msg!(
            "gateway.unknown_provider",
            provider = input.provider
        )));
    }

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());
    let mut uow = ctx.db.begin_write().await?;

    // 沒送的欄位沿用原本的 —— 不然改個勾勾都要把 secret 重打一次。
    let mut creds: BTreeMap<String, String> = if input.id.is_some() {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT credentials_json FROM payment_gateways WHERE id = ?1",
        )
        .bind(&id)
        .fetch_optional(uow.conn())
        .await?
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    for (k, v) in &input.credentials {
        let v = v.trim();
        if v.is_empty() {
            // 空字串＝清掉這一個欄位。畫面上要有辦法把打錯的 secret 拿掉。
            creds.remove(k);
        } else {
            creds.insert(k.clone(), v.to_string());
        }
    }

    // ★ 憑證沒填齊就不准啟用。
    //
    //   一條「開著但缺 secret」的金流線，症狀是每一次刷卡都在客人面前失敗 ——
    //   而設定頁上看起來是綠的。擋在這裡比擋在收銀台前便宜得多。
    let want_active = input.is_active.unwrap_or(false);
    let missing: Vec<&str> = fields_for(&input.provider)
        .iter()
        .filter(|(k, _, _)| !creds.contains_key(*k))
        .map(|(_, label, _)| *label)
        .collect();
    if want_active && !missing.is_empty() {
        return Err(AppError::Validation(
            format!("還缺 {} 才能啟用。", missing.join("、")).into(),
        ));
    }

    let creds_json = serde_json::to_string(&creds).unwrap_or_else(|_| "{}".into());
    if input.id.is_none() {
        sqlx::query(
            "INSERT INTO payment_gateways (id, store_id, provider, display_name,
                                           payment_method_id, is_sandbox, is_active,
                                           credentials_json, created_at, updated_at)
             SELECT ?1, s.id, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8 FROM stores s ORDER BY s.id LIMIT 1",
        )
        .bind(&id)
        .bind(&input.provider)
        .bind(input.display_name.trim())
        .bind(&input.payment_method_id)
        .bind(i64::from(input.is_sandbox.unwrap_or(true)))
        .bind(i64::from(want_active))
        .bind(&creds_json)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE payment_gateways SET provider = ?2, display_name = ?3,
                                         payment_method_id = ?4, is_sandbox = ?5,
                                         is_active = ?6, credentials_json = ?7, updated_at = ?8
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(&input.provider)
        .bind(input.display_name.trim())
        .bind(&input.payment_method_id)
        .bind(i64::from(input.is_sandbox.unwrap_or(true)))
        .bind(i64::from(want_active))
        .bind(&creds_json)
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到金流設定 {id}")));
        }
    }

    // 憑證被改動時留一筆稽核 —— **但不記內容**，只記「有人動過」。
    if !input.credentials.is_empty() {
        crate::services::audit::write_in(
            &mut uow,
            crate::services::audit::AuditEntry::new(
                "PaymentGateway",
                &id,
                crate::services::audit::AuditAction::SettingsChange,
            )
            .to(format!("{} 憑證已更新", input.display_name.trim())),
            &ctx.actor,
            &now,
        )
        .await?;
    }
    uow.commit().await?;

    list(ctx)
        .await?
        .into_iter()
        .find(|g| g.id == id)
        .ok_or_else(|| AppError::Internal("金流設定存好了卻讀不回來".into()))
}

pub async fn delete(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_SETTINGS).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;

    // 有交易紀錄的不能真的刪掉 —— 那些紀錄要留著對帳。軟刪除。
    let n = sqlx::query(
        "UPDATE payment_gateways SET deleted_at = ?2, is_active = 0, updated_at = ?2
          WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到金流設定 {id}")));
    }
    uow.commit().await?;
    Ok(())
}

/// 有哪些金流商可以選、各自要哪些憑證欄位。
///
/// # 為什麼這要是一支指令，而不是前端自己抄一份
///
/// 前端新增一筆設定時，畫面上就得知道「藍新要三個欄位」—— 而在存檔之前
/// 後端還沒有任何一列可以回。最省事的做法是在 TS 裡再寫一份同樣的表。
///
/// 但那份表會腐爛：改了 `fields_for()` 卻忘了改 TS，症狀是
/// 「畫面上都填好了，存進去卻說缺欄位」，而且只在新增時發生、改既有的那筆
/// 不會發生 —— 是最難重現的那一種。所以這裡多開一支指令，讓那張表
/// 從頭到尾只有一份。
pub fn providers() -> Vec<ProviderDef> {
    ["manual", "linepay", "newebpay"]
        .iter()
        .map(|code| ProviderDef {
            code,
            label: provider_label(code),
            note: provider_note(code),
            fields: fields_for(code)
                .iter()
                .map(|(key, label, hint)| CredentialField {
                    key,
                    label,
                    hint,
                    // 還沒有這一筆，所以一定沒設定過。
                    is_set: false,
                    tail: None,
                })
                .collect(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDef {
    pub code: &'static str,
    pub label: &'static str,
    /// 給店家看的一段話：這條線適合誰、需要先去辦什麼。
    pub note: &'static str,
    pub fields: Vec<CredentialField>,
}

fn provider_note(code: &str) -> &'static str {
    match code {
        "manual" => {
            "刷卡機是銀行給的那一台。收銀員刷完把授權碼抄進 POS，這裡只記帳。             不需要網路，也不需要任何憑證。"
        }
        "linepay" => "掃客人手機出示的付款碼。需要 LINE Pay 商家帳號。",
        "newebpay" => "信用卡。需要跟藍新簽約後拿到的商店代號與兩把金鑰。",
        _ => "",
    }
}

/// 讀出憑證。**只給服務層內部用**，不經過任何指令回到前端。
pub async fn credentials(ctx: &Ctx, gateway_id: &str) -> AppResult<BTreeMap<String, String>> {
    Ok(sqlx::query_scalar::<_, Option<String>>(
        "SELECT credentials_json FROM payment_gateways WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(gateway_id)
    .fetch_optional(ctx.db.reader())
    .await?
    .flatten()
    .and_then(|j| serde_json::from_str(&j).ok())
    .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_says_where_to_get_its_credentials() {
        // 一個只寫著「Channel Secret」的欄位，對沒串接過的店家等於沒有寫。
        for p in ["linepay", "newebpay"] {
            let fields = fields_for(p);
            assert!(!fields.is_empty(), "{p} 沒有定義欄位");
            for (key, label, hint) in fields {
                assert!(!key.is_empty() && !label.is_empty());
                assert!(hint.contains("後台"), "{p}/{key} 沒說去哪裡拿：{hint}");
            }
        }
        // manual 不需要憑證：錢是在另一台實體刷卡機上收的。
        assert!(fields_for("manual").is_empty());
    }
}
