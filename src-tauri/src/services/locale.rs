//! 介面語言設定。
//!
//! # 為什麼存在後端而不是瀏覽器
//!
//! 收銀機是**共用的機器**，交接班不會換語言。存 localStorage 的話，同一間店的
//! 兩台機器會顯示不同語言，而且換一台電腦、重灌一次瀏覽器就要重設 ——
//! 那是店家自己完全排除不了的一種問題。
//!
//! 放在 `app_settings` 而不是 `stores`：它是「這台機器怎麼顯示」，不是
//! 「這間店是什麼」。多分店時每一間店的稅率會不一樣，但語言是裝機時設一次
//! 的東西。
//!
//! # 它不決定收據印得出什麼
//!
//! 介面切日文**不代表印表機印得出日文**。那是出單機那一頁的
//! `CjkEncoding` 在管的事，兩者刻意分開 —— 台灣買的 Big5 機器就算 POS 設成
//! 日文，也印不出「円」。詳見 `infra::printer::escpos`。

use crate::ctx::Ctx;
use crate::error::AppResult;
use crate::i18n::Locale;

const KEY: &str = "ui.locale";

/// 目前的介面語言。沒設定過就是繁體中文。
pub async fn get(ctx: &Ctx) -> AppResult<Locale> {
    use sqlx::Row;
    let raw: Option<String> = sqlx::query("SELECT value_json FROM app_settings WHERE key = ?1")
        .bind(KEY)
        .fetch_optional(ctx.db.reader())
        .await?
        .map(|r| r.get::<String, _>("value_json"));

    // 存的是 JSON 字串（"ja"），但手動改過設定檔的人可能會寫成裸字串。
    // 兩種都收 —— 一個因為引號沒打就開不了的收銀機不值得。
    Ok(match raw {
        None => Locale::default(),
        Some(s) => {
            let cleaned = serde_json::from_str::<String>(&s).unwrap_or(s);
            Locale::parse(&cleaned)
        }
    })
}

/// 換語言。
pub async fn set(ctx: &Ctx, locale: Locale) -> AppResult<()> {
    let now = crate::core::clock::Stamp::now();
    let value = serde_json::to_string(locale.as_str())
        .expect("語言代碼是固定的字串字面值，序列化不會失敗");
    let mut uow = ctx.db.begin_write().await?;
    sqlx::query(
        "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE
             SET value_json = excluded.value_json, updated_at = excluded.updated_at",
    )
    .bind(KEY)
    .bind(&value)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    uow.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::i18n::Locale;

    #[test]
    fn a_hand_edited_setting_still_parses() {
        // 有人直接改資料庫、把 "ja" 寫成 ja（少了引號）是會發生的。
        // 兩種都要收 —— 開不了機比顯示錯語言嚴重得多。
        assert_eq!(
            Locale::parse(&serde_json::from_str::<String>("\"ja\"").unwrap_or("ja".into())),
            Locale::Ja
        );
        assert_eq!(
            Locale::parse(&serde_json::from_str::<String>("ja").unwrap_or("ja".into())),
            Locale::Ja
        );
    }
}
