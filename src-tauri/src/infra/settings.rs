//! 應用程式設定（`app_settings.json`）。
//!
//! # 為什麼不放資料庫
//!
//! 因為有些設定是**在資料庫打不開的時候才要用的** —— 備份位置就是最明顯的
//! 一個：資料庫壞了要還原時，正是最需要知道備份放在哪裡的時候。
//!
//! # 原子寫入
//!
//! 先寫 `.tmp` 再 rename。設定檔寫到一半斷電會變成一個壞掉的 JSON，
//! 而下一次開機讀不到設定的後果是「備份忽然停了，而沒有人知道」。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub backup: BackupSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupSettings {
    /// 第二個實體媒體的位置（建議是常插著的 USB 隨身碟）。
    ///
    /// **這是備份唯一真正有用的部分。** 備份跟資料庫放在同一顆硬碟上，
    /// 硬碟壞掉時兩份一起死；那種備份只防「誤刪」，不防「壞掉」。
    pub external_dir: Option<String>,
    /// 每小時自動備份。
    pub hourly: bool,
    /// 關班與日結時各備份一次。
    pub on_close: bool,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            external_dir: None,
            // 預設全開：會去關掉它的人知道自己在做什麼，
            // 而不知道的人正是最需要它的人。
            hourly: true,
            on_close: true,
        }
    }
}

/// 讀設定。檔案不存在或壞掉都回預設值 ——
/// 一個讀不到設定就開不了店的收銀機，比沒有設定檔更糟。
pub async fn load(path: &Path) -> AppSettings {
    let Ok(text) = tokio::fs::read_to_string(path).await else {
        return AppSettings::default();
    };
    match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "設定檔讀不懂，改用預設值");
            AppSettings::default()
        }
    }
}

/// 原子寫入。
pub async fn save(path: &Path, settings: &AppSettings) -> AppResult<()> {
    let text = serde_json::to_string_pretty(settings)
        .map_err(|e| AppError::Internal(format!("設定序列化失敗：{e}")))?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, text.as_bytes())
        .await
        .map_err(|e| AppError::Storage(format!("寫不進設定檔：{e}")))?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| AppError::Storage(format!("設定檔更名失敗：{e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("open-pos-settings-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("app_settings.json")
    }

    #[tokio::test]
    async fn round_trips() {
        let p = tmp("round");
        let mut s = AppSettings::default();
        s.backup.external_dir = Some(r"E:\pos-backup".into());
        s.backup.hourly = false;
        save(&p, &s).await.unwrap();
        assert_eq!(load(&p).await, s);
    }

    #[tokio::test]
    async fn a_missing_or_broken_file_falls_back_to_defaults() {
        // 讀不到設定就開不了店的收銀機，比沒有設定檔更糟。
        let p = tmp("broken");
        assert_eq!(load(&p).await, AppSettings::default());
        std::fs::write(&p, b"{ this is not json").unwrap();
        assert_eq!(load(&p).await, AppSettings::default());
    }

    #[tokio::test]
    async fn defaults_have_backup_switched_on() {
        // 會去關掉它的人知道自己在做什麼；不知道的人正是最需要它的人。
        let d = BackupSettings::default();
        assert!(d.hourly);
        assert!(d.on_close);
    }

    #[tokio::test]
    async fn unknown_fields_do_not_wipe_the_file() {
        // 舊版程式讀到新版寫的設定時，不該把整份設定當成壞掉。
        let p = tmp("forward");
        std::fs::write(
            &p,
            br#"{"backup":{"hourly":false,"somethingNew":123},"futureSection":{}}"#,
        )
        .unwrap();
        let s = load(&p).await;
        assert!(!s.backup.hourly);
        assert!(s.backup.on_close, "沒給的欄位要用預設值");
    }
}
