//! 執行期共用狀態。
//!
//! 刻意**不依賴 Tauri**：headless 的 `open-posd` 與 GUI 的 `open-pos` 共用同一份。
//! Tauri 那邊只是把它塞進 `State`，axum 那邊塞進 `Extension` —— 兩個 transport
//! 看到的是同一個東西，`services/` 那一層因此完全不知道自己被誰呼叫。

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::infra::db::sqlite::SqliteDb;
use crate::paths::DataLayout;

pub struct AppCtx {
    pub db: SqliteDb,
    pub layout: DataLayout,
    pub started_at: DateTime<Utc>,
}

pub type Ctx = Arc<AppCtx>;
