//! open-pos —— 地端開源餐飲 POS。
//!
//! 分層：
//! ```text
//! 前端三 entry ── src/shared/api.ts（唯一橋接層）
//!      ┌──────────┴──────────┐
//! Tauri commands      axum POST /api/rpc/{name}
//!      └──────────┬──────────┘
//!           services/     ← 兩個 transport 唯一入口
//!         ┌───────┴───────┐
//!       core/           infra/
//! ```
//! `core/` 是純業務（零 I/O），`infra/` 是所有會碰到外界的東西。
//! 交易邊界在 `services/`（一次 use case 一個 UnitOfWork），不在 repo 方法裡。

pub mod core;
pub mod error;
pub mod guard;
pub mod infra;
pub mod paths;
pub mod services;

use std::path::{Path, PathBuf};

use core::clock::Stamp;

use error::AppResult;
use guard::InstanceLock;
use infra::db::{Db, DbBackend, DbConfig};
use paths::DataLayout;

/// 啟動後的執行環境。
pub struct Runtime {
    pub layout: DataLayout,
    pub db: Db,
    /// 單實例鎖。**必須留著** —— 它一被 drop，別的實例就能同時開這份資料。
    _lock: InstanceLock,
}

/// 啟動選項。GUI 與 headless 共用同一條啟動路徑，差別只在誰來呼叫。
#[derive(Debug, Default, Clone)]
pub struct BootOptions {
    pub data_dir: Option<PathBuf>,
    /// 使用者明確同意把資料放在雲端同步資料夾（預設拒絕）。
    pub allow_cloud_sync: bool,
    pub max_readers: Option<u32>,
}

/// 啟動序列。順序是刻意的，每一步都可能拒絕啟動：
///
/// 1. 解析資料目錄
/// 2. **路徑安全檢查**（網路磁碟 / 雲端同步資料夾）—— 在建立任何檔案之前
/// 3. 建立目錄
/// 4. **取單實例鎖** —— 在開資料庫之前，避免兩個實例同時跑 migration
/// 5. 開資料庫（含 migration 與開機自檢：WAL 模式、完整性、外鍵）
pub async fn boot(opts: BootOptions) -> AppResult<Runtime> {
    let root = paths::resolve_data_dir(opts.data_dir.as_deref())?;
    guard::check_data_dir_safety(&root, opts.allow_cloud_sync)?;

    let layout = DataLayout::new(root);
    layout.ensure()?;

    let lock = guard::acquire_instance_lock(&layout.lock_file())?;

    let db = Db::open(&DbConfig {
        backend: DbBackend::Sqlite,
        location: layout.db_file().to_string_lossy().into_owned(),
        max_readers: opts.max_readers,
    })
    .await?;

    // 6. 同步種子資料。權限碼與稅別每次啟動 upsert（升級後新增的會自動出現），
    //    店家資料只在完全空的資料庫上建立一次。
    let Db::Sqlite(sqlite) = &db;
    let now = Stamp::now();
    let mut uow = sqlite.begin_write().await?;
    let created_store = services::seed::apply(&mut uow, &now).await?;
    uow.commit().await?;

    tracing::info!(
        data_dir = %layout.root.display(),
        created_store,
        "open-pos 啟動完成"
    );
    Ok(Runtime {
        layout,
        db,
        _lock: lock,
    })
}

/// 初始化 log。診斷包要靠它 —— 地端 + 離線 + 非技術使用者，
/// 沒有 log 就無法重現任何一個「今天中午印不出來，現在又好了」的回報。
pub fn init_tracing(logs_dir: Option<&Path>) {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_env("OPEN_POS_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false));

    if let Some(dir) = logs_dir {
        let appender = tracing_appender::rolling::daily(dir, "open-pos.log");
        registry
            .with(fmt::layer().with_ansi(false).with_writer(appender))
            .init();
    } else {
        registry.init();
    }
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
