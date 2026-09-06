//! 備份與還原。
//!
//! # 為什麼不能用檔案複製
//!
//! WAL 模式下，最近的交易還躺在 `pos.db-wal` 裡尚未 checkpoint 回主檔。
//! `fs::copy("pos.db")` 拿到的是**缺了最後幾筆交易**的檔案，而且如果複製途中
//! 有人在寫，還會拿到撕裂的頁面。兩種情況都不會報錯 —— 你會得到一個看起來
//! 正常、實際上壞掉或過期的備份，直到真的需要它的那天才發現。
//!
//! 正解是 `VACUUM INTO`：SQLite 在一個讀取交易裡產出**一致且已壓實**的完整副本，
//! 不需要停機，一句 SQL 就好。
//!
//! （note：db-kit 的 `backup.rs` 是用 `tokio::fs::copy` 的。那對「使用者自己選的
//! 資料庫檔」是合理的，因為它不擁有那個連線；open-pos 擁有自己的資料庫，
//! 沒有理由承擔那個風險。）
//!
//! # 為什麼要備份到第二個實體媒體
//!
//! 整店唯一一份 `pos.db` 躺在一台便宜消費級機器的 SSD 上。硬碟壞掉那天不是
//! 停業幾小時，是當天所有交易紀錄消失。備份到同一顆硬碟等於沒有備份。
//! 預設目標是「一支常插著的 USB 隨身碟」。

use std::path::{Path, PathBuf};
use std::time::Instant;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection, SqliteConnection};

use crate::core::clock;
use crate::error::{AppError, AppResult};
use crate::infra::db::sqlite::SqliteDb;
use crate::paths::DataLayout;

/// SQLite 檔案的魔術字串（前 16 bytes）。
const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";

#[derive(Debug, Clone)]
pub struct BackupResult {
    pub path: PathBuf,
    pub bytes: u64,
    pub took_ms: u128,
}

/// 備份到指定檔案。
///
/// 產出走 `.partial` 再 rename —— 與 `app_settings.json` 的原子寫入同一個理由：
/// 中途斷電時不要留下一個看起來像備份的半成品。
pub async fn backup_to(db: &SqliteDb, dst: &Path) -> AppResult<BackupResult> {
    let started = Instant::now();

    if let Some(parent) = dst.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::Storage(format!("建立備份目錄失敗：{e}")))?;
    }

    let tmp = dst.with_extension("partial");
    let _ = tokio::fs::remove_file(&tmp).await;
    // VACUUM INTO 要求目標檔不存在。
    let _ = tokio::fs::remove_file(dst).await;

    // VACUUM INTO 不接受參數綁定，必須把路徑寫進 SQL 字面值。
    // SQLite 的字串字面值只需要把單引號加倍；反斜線沒有特殊意義，
    // 所以 Windows 路徑可以直接放。
    let literal = tmp.to_string_lossy().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{literal}'"))
        // 走讀取池：備份是長時間的讀操作，不該佔住唯一那條寫入連線。
        .execute(db.reader())
        .await
        .map_err(|e| AppError::Db(format!("備份失敗：{e}")))?;

    // 落地前先驗一次。一個沒驗過的備份等於一個沒有的備份。
    validate_backup_file(&tmp).await?;

    tokio::fs::rename(&tmp, dst)
        .await
        .map_err(|e| AppError::Storage(format!("備份檔改名失敗：{e}")))?;

    let bytes = tokio::fs::metadata(dst).await.map(|m| m.len()).unwrap_or(0);

    tracing::info!(path = %dst.display(), bytes, "備份完成");
    Ok(BackupResult {
        path: dst.to_path_buf(),
        bytes,
        took_ms: started.elapsed().as_millis(),
    })
}

/// 依營業日 / 班別 / 小時分類的備份檔名。
pub fn backup_path(layout: &DataLayout, bucket: BackupBucket, label: &str) -> PathBuf {
    layout
        .backups_dir()
        .join(bucket.dir_name())
        .join(format!("{label}.db"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupBucket {
    Hourly,
    Shift,
    Daily,
}

impl BackupBucket {
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::Shift => "shift",
            Self::Daily => "daily",
        }
    }
    /// 各分類保留幾份。
    pub fn keep(self) -> usize {
        match self {
            Self::Hourly => 24,
            Self::Shift => 30,
            Self::Daily => 90,
        }
    }
}

/// 驗證一個備份檔是否真的可用。
///
/// 兩層：檔頭魔術字串（擋掉「使用者選錯檔案」）+ `integrity_check`（擋掉真的壞掉的檔）。
///
/// db-kit 的同名函式只驗前 16 bytes。那對「開啟使用者指定的資料庫」夠用，
/// 但備份是要在最壞的一天拿出來救命的東西，值得多付一次完整掃描。
pub async fn validate_backup_file(path: &Path) -> AppResult<()> {
    let head = read_head(path, SQLITE_MAGIC.len()).await?;
    if head != SQLITE_MAGIC {
        return Err(AppError::Validation(format!(
            "{} 不是 SQLite 資料庫檔",
            path.display()
        )));
    }

    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .disable_statement_logging()
        .connect()
        .await
        .map_err(|e| AppError::Validation(format!("備份檔開不起來：{e}")))?;

    let result: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut conn)
        .await
        .map_err(|e| AppError::Validation(format!("完整性檢查失敗：{e}")))?;
    let _ = conn.close().await;

    if !result.eq_ignore_ascii_case("ok") {
        return Err(AppError::Validation(format!(
            "備份檔完整性檢查未通過：{result}"
        )));
    }
    Ok(())
}

async fn read_head(path: &Path, n: usize) -> AppResult<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut f = tokio::fs::File::open(path)
        .await
        .map_err(|e| AppError::Storage(format!("開啟 {} 失敗：{e}", path.display())))?;
    let mut buf = vec![0u8; n];
    f.read_exact(&mut buf)
        .await
        .map_err(|e| AppError::Validation(format!("檔案太小或無法讀取：{e}")))?;
    Ok(buf)
}

/// 只保留最新的 `keep` 份，其餘刪除。回傳刪掉幾份。
pub fn rotate(dir: &Path, keep: usize) -> AppResult<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| AppError::Storage(format!("讀取 {} 失敗：{e}", dir.display())))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "db").unwrap_or(false))
        .collect();

    // 檔名帶時間戳，所以字典序就是時序 —— 不必去讀 mtime（那在複製到 USB 之後不可靠）。
    files.sort();
    if files.len() <= keep {
        return Ok(0);
    }
    let mut removed = 0;
    for p in &files[..files.len() - keep] {
        if std::fs::remove_file(p).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// 從備份還原。
///
/// **呼叫前必須先關閉資料庫連線池。** 這個函式只碰檔案，不碰連線 ——
/// 讓「誰負責停掉服務」這件事留在呼叫端，而不是藏在這裡。
///
/// 流程刻意保守：
/// 1. 驗證來源檔（檔頭 + 完整性）
/// 2. 檢查它的 migration 版本不比目前的程式新 —— 用新版備份還原到舊版程式，
///    會在某個查詢時噴 `no such column`，而那時已經開了半天的單
/// 3. 把現有的三個檔（db / -wal / -shm）移到 `pre-restore.<時間戳>`，不是刪除
/// 4. 複製新檔進來；失敗就把舊檔搬回去
///
/// 回傳搬走的舊檔路徑，讓 UI 能告訴使用者「你原本的資料在這裡」。
pub async fn restore_from(
    layout: &DataLayout,
    src: &Path,
    current_max_migration: i64,
) -> AppResult<PathBuf> {
    validate_backup_file(src).await?;

    let backup_version = max_migration_version(src).await?;
    if backup_version > current_max_migration {
        return Err(AppError::Validation(format!(
            "這份備份來自較新的版本（schema {backup_version}，目前程式只支援到 {current_max_migration}）。\n\
             請先把 open-pos 更新到較新的版本再還原。"
        )));
    }

    let db = layout.db_file();
    let stamp = clock::now_iso().replace(':', "-");
    let aside = db.with_extension(format!("pre-restore.{stamp}.db"));

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", db.display()));
        if !from.exists() {
            continue;
        }
        let to = PathBuf::from(format!("{}{suffix}", aside.display()));
        tokio::fs::rename(&from, &to)
            .await
            .map_err(|e| AppError::Storage(format!("搬移現有資料失敗：{e}")))?;
        moved.push((from, to));
    }

    if let Err(e) = tokio::fs::copy(src, &db).await {
        // 復原：把剛剛搬走的搬回來，讓店家至少回到還原前的狀態。
        for (from, to) in &moved {
            let _ = tokio::fs::rename(to, from).await;
        }
        return Err(AppError::Storage(format!("複製備份檔失敗：{e}")));
    }

    tracing::warn!(
        src = %src.display(),
        aside = %aside.display(),
        "已從備份還原；還原前的資料保留在 aside 路徑"
    );
    Ok(aside)
}

/// 讀一個資料庫檔的 `_sqlx_migrations` 最高版本。
async fn max_migration_version(path: &Path) -> AppResult<i64> {
    let mut conn: SqliteConnection = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .disable_statement_logging()
        .connect()
        .await
        .map_err(|e| AppError::Validation(format!("備份檔開不起來：{e}")))?;

    // 沒有 _sqlx_migrations 表就當成 0（例如空白或極早期的檔）。
    let v: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_optional(&mut conn)
        .await
        .unwrap_or(None)
        .flatten();
    let _ = conn.close().await;
    Ok(v.unwrap_or(0))
}
