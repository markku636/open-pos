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

#[cfg(feature = "gui")]
pub mod commands;
pub mod core;
pub mod ctx;
pub mod error;
pub mod guard;
pub mod infra;
#[cfg(feature = "server")]
pub mod lan;
pub mod paths;
pub mod receipt;
pub mod services;

use std::path::{Path, PathBuf};

use core::clock::Stamp;

use error::AppResult;
use guard::InstanceLock;
use infra::db::{Db, DbBackend, DbConfig};
use paths::DataLayout;

/// 啟動後的執行環境。
pub struct Runtime {
    pub ctx: ctx::Ctx,
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

    let Db::Sqlite(sqlite) = db;
    let actor = services::seed::default_actor(&sqlite).await?;
    Ok(Runtime {
        ctx: std::sync::Arc::new(ctx::AppCtx {
            db: sqlite,
            layout,
            started_at: now.at,
            actor,
        }),
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

/// GUI 進入點。
///
/// 啟動順序刻意讓 `boot()` 在建立視窗**之前**跑完：開機檢查失敗時要能跳出一個
/// 讀得懂的對話框然後結束，而不是先開一個空白視窗再說。不懂電腦的店家看到
/// 空白視窗只會重開機，而重開機解決不了「資料庫放在網路磁碟上」這種問題。
#[cfg(feature = "gui")]
pub fn run() {
    let rt = match tauri::async_runtime::block_on(boot(BootOptions::default())) {
        Ok(rt) => rt,
        Err(e) => {
            fatal_dialog(&e.message());
            std::process::exit(1);
        }
    };

    let ctx = rt.ctx.clone();

    #[cfg(feature = "server")]
    let lan = match lan::spawn(ctx.clone(), lan::DEFAULT_PORT, default_ui_dir()) {
        Ok(h) => Some(h),
        Err(e) => {
            // 區網服務起不來**不該讓整台收銀機開不了**。
            // 沒有它只是平板與手機連不進來，櫃檯照樣能收錢 —— 而收錢是主線。
            tracing::error!(error = %e.message(), "區網服務啟動失敗，僅以單機模式繼續");
            fatal_dialog(&format!(
                "區網服務無法啟動，平板與手機將連不進來。

{}

收銀功能不受影響，可以繼續使用。",
                e.message()
            ));
            None
        }
    };

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // 擋雙開的第二道（第一道是 guard.rs 的檔案鎖）。
        // 這一道純粹是 UX：把既有視窗喚到前面，而不是讓使用者對著錯誤訊息發呆。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
                let _ = w.unminimize();
            }
        }))
        .manage(ctx)
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::health,
            commands::lan_info,
            commands::menu_tree,
            commands::upsert_category,
            commands::delete_category,
            commands::upsert_item,
            commands::delete_item,
            commands::upsert_variant,
            commands::delete_variant,
            commands::open_order,
            commands::add_lines,
            commands::void_line,
            commands::settle,
            commands::get_order,
            commands::list_open_orders,
            commands::payment_methods,
        ])
        .setup(|app| {
            use tauri::Manager;
            // 視窗在 tauri.conf.json 裡設 visible: false，等前端掛載完才顯示 ——
            // 否則會先閃一下白色再變深色。
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
            Ok(())
        });

    builder
        .run(tauri::generate_context!())
        .expect("Tauri 啟動失敗");

    #[cfg(feature = "server")]
    if let Some(h) = lan {
        tauri::async_runtime::block_on(h.shutdown());
    }
    tauri::async_runtime::block_on(rt.ctx.db.close());
}

/// 開機失敗時把訊息送到眼前。
///
/// 這一段是刻意用原生對話框而不是 log：開機失敗時**還沒有視窗**，
/// 而店家不會去翻 log 檔。
#[cfg(feature = "gui")]
fn fatal_dialog(message: &str) {
    tracing::error!("{message}");
    #[cfg(windows)]
    {
        // 直接用 Win32 MessageBox：此時 Tauri 還沒起來，用不了它的 dialog plugin。
        use std::os::windows::ffi::OsStrExt;
        fn wide(s: &str) -> Vec<u16> {
            std::ffi::OsStr::new(s)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }
        let (text, title) = (wide(message), wide("open-pos"));
        // SAFETY: 兩個字串都是以 NUL 結尾的合法 UTF-16，生命週期涵蓋這次呼叫。
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    eprintln!(
        "open-pos 無法啟動：

{message}"
    );
}

/// 找 dist/：先看執行檔旁邊（安裝後的樣子），再往上找（開發時的樣子）。
#[cfg(feature = "gui")]
fn default_ui_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    for up in [0usize, 1, 2, 3] {
        let mut base = dir.to_path_buf();
        for _ in 0..up {
            base = base.parent()?.to_path_buf();
        }
        let candidate = base.join("dist");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}
