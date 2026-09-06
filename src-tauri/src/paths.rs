//! 資料目錄解析。
//!
//! 對標 db-kit `store.rs` 的 `headless_config_dir()` 雙軌設計：GUI 走 Tauri API、
//! headless 走 `dirs`，兩者指向同一路徑，所以 GUI 與 headless 共用同一份資料。
//!
//! 與 db-kit 的差異：用 **app_data_dir 而非 app_config_dir**。
//! db-kit 存的是連線設定（config），open-pos 存的是營運資料（data）。
//! Linux 上兩者差很多：config = ~/.config、data = ~/.local/share ——
//! 放錯會讓使用者的 ~/.config 被塞進一個持續長大的 GB 級檔案。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

pub const APP_IDENTIFIER: &str = "app.openpos.pos";
pub const ENV_DATA_DIR: &str = "OPEN_POS_DATA_DIR";

/// headless（CLI / Docker）用的資料根目錄。
pub fn headless_data_dir() -> AppResult<PathBuf> {
    let base =
        dirs::data_dir().ok_or_else(|| AppError::Storage("無法取得使用者資料目錄".into()))?;
    Ok(base.join(APP_IDENTIFIER))
}

/// 依優先序解析資料根目錄：CLI 參數 > 環境變數 > 平台預設。
pub fn resolve_data_dir(cli_override: Option<&Path>) -> AppResult<PathBuf> {
    if let Some(p) = cli_override {
        return Ok(p.to_path_buf());
    }
    if let Some(v) = std::env::var_os(ENV_DATA_DIR) {
        let p = PathBuf::from(v);
        if !p.as_os_str().is_empty() {
            return Ok(p);
        }
    }
    headless_data_dir()
}

/// 資料根目錄下的固定佈局。
pub struct DataLayout {
    pub root: PathBuf,
}

impl DataLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn db_dir(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn db_file(&self) -> PathBuf {
        self.db_dir().join("pos.db")
    }
    /// 單實例鎖。刻意與 pos.db 同目錄 —— 要守的是「這份資料只有一個 leader」，
    /// 不是「這台機器只跑一個 process」。
    pub fn lock_file(&self) -> PathBuf {
        self.db_dir().join("pos.lock")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }
    pub fn journal_dir(&self) -> PathBuf {
        self.root.join("journal")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("app_settings.json")
    }

    /// 建立所有必要目錄。
    pub fn ensure(&self) -> AppResult<()> {
        for d in [
            self.db_dir(),
            self.backups_dir().join("hourly"),
            self.backups_dir().join("shift"),
            self.backups_dir().join("daily"),
            self.journal_dir(),
            self.logs_dir(),
        ] {
            std::fs::create_dir_all(&d)
                .map_err(|e| AppError::Storage(format!("建立目錄失敗 {}：{e}", d.display())))?;
        }
        Ok(())
    }
}
