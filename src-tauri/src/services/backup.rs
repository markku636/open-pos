//! 備份與還原。
//!
//! # 這是 v1.0 的 P0，不是加分項
//!
//! 使用者第一個月掉資料，一個開源專案的聲譽就結束了 —— 而且他不會回來
//! 告訴你原因。所以備份要**預設開啟**、要**自己跑**、要**能還原**，
//! 而不是一個藏在設定裡等人去打開的開關。
//!
//! # 備份必須離開這顆硬碟
//!
//! 跟資料庫放在同一顆硬碟上的備份只防「誤刪」，不防「硬碟壞掉」。
//! 所以設定頁第一個欄位是「第二個位置」，而健康檢查會一直唸到店家設好為止。
//!
//! # 還原走「暫存 + 重開」而不是就地替換
//!
//! 資料庫檔案在程式跑的時候是開著的，Windows 上根本改不動它。
//! 所以還原分兩步：先把選好的備份放到 `restore.pending.db` 並留下標記，
//! 下次開機在開啟資料庫**之前**套用。
//!
//! 這個做法還順便解掉一個更難的問題：套用到一半當機時，標記還在，
//! 下次開機會再套用一次 —— 而就地替換在同樣的情況下會留下一個殘破的資料庫。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::clock::Stamp;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::infra::backup::{self, BackupBucket};
use crate::infra::settings::{self, AppSettings};
use crate::paths::DataLayout;
use crate::services::rbac;

const PERM_SETTINGS: &str = "settings.store";

/// 還原標記的檔名。放在資料目錄根層，因為它要在資料庫打開之前被讀到。
const PENDING_DB: &str = "restore.pending.db";
const PENDING_MARK: &str = "restore.pending.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRestore {
    /// 使用者選的那一份備份（原始位置），只是為了讓訊息說得出來。
    pub source: String,
    pub staged_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub path: String,
    pub name: String,
    pub bucket: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    /// 在外接位置（隨身碟）上的那一份。
    pub external: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRunResult {
    pub local_path: String,
    pub external_path: Option<String>,
    pub size_bytes: u64,
    pub took_ms: u128,
    /// 外接位置設了但寫不進去時的說明。**不是錯誤** ——
    /// 本機那一份已經備好了，隨身碟沒插不該讓整件事失敗。
    pub external_error: Option<String>,
}

pub async fn get_settings(ctx: &Ctx) -> AppResult<AppSettings> {
    Ok(settings::load(&ctx.layout.settings_file()).await)
}

pub async fn save_settings(ctx: &Ctx, s: AppSettings) -> AppResult<AppSettings> {
    rbac::require(&ctx.db, &ctx.actor, PERM_SETTINGS).await?;
    if let Some(dir) = &s.backup.external_dir {
        let dir = dir.trim();
        if !dir.is_empty() {
            let path = Path::new(dir);
            if !path.is_dir() {
                return Err(AppError::Validation(
                    format!(
                        "找不到備份資料夾「{dir}」。\n\
                     如果那是隨身碟，請先插上去再儲存。"
                    )
                    .into(),
                ));
            }
            // 立刻試寫一次。等到半夜自動備份才發現沒有寫入權限，
            // 就是「以為有在備份、其實三個月沒備了」的來源。
            let probe = path.join(".open-pos-write-test");
            tokio::fs::write(&probe, b"ok").await.map_err(|e| {
                AppError::Validation(format!("「{dir}」寫不進去（{e}）—— 請換一個資料夾。").into())
            })?;
            let _ = tokio::fs::remove_file(&probe).await;
        }
    }
    settings::save(&ctx.layout.settings_file(), &s).await?;
    Ok(s)
}

/// 跑一次備份。
///
/// 本機一份、外接一份。外接失敗**不算失敗** —— 本機那一份已經好了，
/// 隨身碟沒插不該讓整件事變成錯誤（但要說出來，健康檢查也會亮）。
pub async fn run_backup(ctx: &Ctx, bucket: BackupBucket) -> AppResult<BackupRunResult> {
    let now = Stamp::now();
    let label = format!("{}-{}", bucket.dir_name(), now.iso().replace(':', "-"));
    let dst = backup::backup_path(&ctx.layout, bucket, &label);

    let result = backup::backup_to(&ctx.db, &dst).await?;
    let _ = backup::rotate(
        &ctx.layout.backups_dir().join(bucket.dir_name()),
        bucket.keep(),
    );

    let cfg = settings::load(&ctx.layout.settings_file()).await;
    let mut external_path = None;
    let mut external_error = None;
    if let Some(dir) = cfg.backup.external_dir.as_deref().filter(|d| !d.is_empty()) {
        let target_dir = Path::new(dir).join("open-pos");
        match copy_out(&dst, &target_dir, &label).await {
            Ok(p) => {
                external_path = Some(p.to_string_lossy().into_owned());
                // 外接那一份也要輪替，不然隨身碟遲早會滿 ——
                // 而「隨身碟滿了所以備份停了」不會有人發現。
                let _ = backup::rotate(&target_dir, bucket.keep());
            }
            Err(e) => {
                tracing::warn!(error = %e.message(), dir, "外接備份失敗");
                external_error = Some(e.message());
            }
        }
    }

    Ok(BackupRunResult {
        local_path: dst.to_string_lossy().into_owned(),
        external_path,
        size_bytes: result.bytes,
        took_ms: result.took_ms,
        external_error,
    })
}

async fn copy_out(src: &Path, dir: &Path, label: &str) -> AppResult<PathBuf> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| AppError::Storage(format!("建不出資料夾：{e}")))?;
    let dst = dir.join(format!("{label}.db"));
    // 走 .partial 再改名：隨身碟被拔掉時不會留下一個看起來完好的半份備份。
    let tmp = dst.with_extension("partial");
    tokio::fs::copy(src, &tmp)
        .await
        .map_err(|e| AppError::Storage(format!("複製到外接位置失敗：{e}")))?;
    tokio::fs::rename(&tmp, &dst)
        .await
        .map_err(|e| AppError::Storage(format!("外接備份更名失敗：{e}")))?;
    Ok(dst)
}

/// 列出所有備份，新的在前。
pub async fn list_backups(ctx: &Ctx) -> AppResult<Vec<BackupFile>> {
    let mut out = Vec::new();
    for bucket in [
        BackupBucket::Hourly,
        BackupBucket::Shift,
        BackupBucket::Daily,
    ] {
        collect(
            &ctx.layout.backups_dir().join(bucket.dir_name()),
            bucket.dir_name(),
            false,
            &mut out,
        )
        .await;
    }

    let cfg = settings::load(&ctx.layout.settings_file()).await;
    if let Some(dir) = cfg.backup.external_dir.as_deref().filter(|d| !d.is_empty()) {
        collect(&Path::new(dir).join("open-pos"), "外接", true, &mut out).await;
    }

    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

async fn collect(dir: &Path, bucket: &str, external: bool, out: &mut Vec<BackupFile>) {
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("db") {
            continue;
        }
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        out.push(BackupFile {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.to_string_lossy().into_owned(),
            bucket: bucket.to_string(),
            size_bytes: meta.len(),
            modified_at: meta.modified().ok().map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                crate::core::clock::to_iso(dt)
            }),
            external,
        });
    }
}

/// 準備還原：驗證備份、放到暫存位置、留下標記。**不會就地替換。**
///
/// 回傳給使用者看的一句話。真正的替換發生在下一次啟動，
/// 因為資料庫檔案在程式跑的時候是開著的。
pub async fn stage_restore(ctx: &Ctx, path: String) -> AppResult<String> {
    rbac::require(&ctx.db, &ctx.actor, PERM_SETTINGS).await?;

    let src = PathBuf::from(&path);
    backup::validate_backup_file(&src).await?;

    // 還原到比程式舊的 schema 沒問題（migration 會往前跑），
    // 但反過來不行：用新版備份配舊版程式，會在某個查詢時噴 no such column，
    // 而那時已經開了半天的單。
    let current = crate::infra::db::sqlite::SqliteDb::max_migration_version();
    let backup_version = backup::max_migration_version(&src).await?;
    if backup_version > current {
        return Err(AppError::Validation(
            format!(
                "這份備份來自較新的版本（schema {backup_version}，目前程式只支援到 {current}）。\n\
             請先把 open-pos 更新到較新的版本再還原。"
            )
            .into(),
        ));
    }

    let staged = ctx.layout.root.join(PENDING_DB);
    let tmp = staged.with_extension("partial");
    tokio::fs::copy(&src, &tmp)
        .await
        .map_err(|e| AppError::Storage(format!("複製備份失敗：{e}")))?;
    tokio::fs::rename(&tmp, &staged)
        .await
        .map_err(|e| AppError::Storage(format!("備份更名失敗：{e}")))?;

    let mark = PendingRestore {
        source: path.clone(),
        staged_at: Stamp::now().iso().to_string(),
    };
    let text = serde_json::to_string_pretty(&mark)
        .map_err(|e| AppError::Internal(format!("標記序列化失敗：{e}")))?;
    tokio::fs::write(ctx.layout.root.join(PENDING_MARK), text.as_bytes())
        .await
        .map_err(|e| AppError::Storage(format!("寫不出還原標記：{e}")))?;

    Ok(
        "備份已經準備好了。請關掉 open-pos 再重新開啟，還原會在啟動時完成。\n\
        （現在的資料不會被刪除，會改名成 pre-restore 留在資料夾裡。）"
            .into(),
    )
}

/// 取消還原。
pub async fn cancel_restore(ctx: &Ctx) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_SETTINGS).await?;
    let _ = tokio::fs::remove_file(ctx.layout.root.join(PENDING_MARK)).await;
    let _ = tokio::fs::remove_file(ctx.layout.root.join(PENDING_DB)).await;
    Ok(())
}

pub async fn pending_restore(ctx: &Ctx) -> AppResult<Option<PendingRestore>> {
    Ok(read_pending(&ctx.layout).await)
}

async fn read_pending(layout: &DataLayout) -> Option<PendingRestore> {
    let text = tokio::fs::read_to_string(layout.root.join(PENDING_MARK))
        .await
        .ok()?;
    serde_json::from_str(&text).ok()
}

/// 開機時套用待處理的還原。**必須在開啟資料庫之前呼叫。**
///
/// 回傳被搬到一旁的舊資料位置，讓啟動訊息能告訴使用者「你原本的資料在這裡」。
/// 沒有待處理的還原時回 `None`。
pub async fn apply_pending_restore(layout: &DataLayout) -> AppResult<Option<PathBuf>> {
    let Some(mark) = read_pending(layout).await else {
        return Ok(None);
    };
    let staged = layout.root.join(PENDING_DB);
    if !staged.exists() {
        // 標記在但檔案不在：清掉標記，不要每次開機都失敗。
        let _ = tokio::fs::remove_file(layout.root.join(PENDING_MARK)).await;
        return Err(AppError::Startup(format!(
            "上次準備的還原檔不見了（來源：{}）。沒有做任何變更。",
            mark.source
        )));
    }

    let db = layout.db_file();
    let stamp = crate::core::clock::now_iso().replace(':', "-");
    let aside = db.with_extension(format!("pre-restore.{stamp}.db"));

    // 舊資料**改名保留**而不是刪除。還原是人在慌張的時候做的事，
    // 而「按下去之後原本的資料就沒了」不能是這種操作的行為。
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", db.display()));
        if !from.exists() {
            continue;
        }
        let to = PathBuf::from(format!("{}{suffix}", aside.display()));
        tokio::fs::rename(&from, &to)
            .await
            .map_err(|e| AppError::Startup(format!("搬移現有資料失敗：{e}")))?;
    }

    tokio::fs::rename(&staged, &db)
        .await
        .map_err(|e| AppError::Startup(format!("套用還原失敗：{e}")))?;
    let _ = tokio::fs::remove_file(layout.root.join(PENDING_MARK)).await;

    tracing::info!(
        source = %mark.source,
        aside = %aside.display(),
        "已套用還原"
    );
    Ok(Some(aside))
}
