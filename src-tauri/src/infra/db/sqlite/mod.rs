//! SQLite 後端：雙連線池 + UnitOfWork 交易邊界。
//!
//! # 為什麼是雙池
//!
//! WAL 模式下讀者不阻塞寫者、寫者不阻塞讀者。把讀寫分成兩個池，換到兩個好處：
//!
//! 1. **寫入池 `max_connections = 1`** —— 同 process 內所有寫入交易被序列化，
//!    效果等同自己寫一個 single-writer actor，但保留 `BEGIN`/`COMMIT` 的人體工學，
//!    而且 sqlx pool 天然提供 FIFO 背壓與 acquire timeout。
//! 2. **`synchronous` 可以分開設** —— 只有寫入那一條需要 FULL。
//!    NORMAL + WAL 在「app crash」下安全，但「斷電 / OS crash」可能丟掉最後幾筆
//!    已 commit 的交易。對一般 app 可接受，對「已經收了現金」的 POS 不可接受。
//!    而 fsync 成本只發生在每秒個位數的寫入交易上（收銀機的真實負載），完全付得起。
//!
//! # pool-of-1 不等於 actor：兩條必須遵守的規則
//!
//! * **交易入口只有 `begin_write()` 一個**。在一個寫入交易進行中再去 acquire 寫入池，
//!   會等到 acquire timeout 為止 —— 這就是自我死鎖。所以 repo 方法一律吃
//!   `&mut SqliteUow`，不自己拿連線。
//! * **交易內嚴禁任何外部 I/O**（印表機 TCP、檔案投遞、HTTP）。寫入池只有一條連線，
//!   交易一慢，全店的寫入都排隊。需要外部 I/O 的動作一律寫進 outbox 表，
//!   由背景 worker 執行並重試。
//!
//! # 測試絕不用 `:memory:`
//!
//! `sqlite::memory:` 的每一條連線都是**各自獨立的資料庫**。雙池設計下 reader 會看到
//! 一個空的 DB。整合測試一律用暫存**檔案**。

pub mod uow;

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::{Executor, Row};

use crate::error::{AppError, AppResult};

pub use uow::SqliteUow;

/// migration 放在 repo 根的 `migrations/`（相對於 crate root `src-tauri/`）。
/// 刻意不埋在 src-tauri 底下 —— SQL 是產品的一部分，不是 Rust 的實作細節。
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../migrations");

/// 讀取池大小。POS 的讀取來源：收銀 UI、KDS 輪詢、報表、掃碼點餐。
const DEFAULT_MAX_READERS: u32 = 6;
/// 取得寫入連線的等待上限。超時的最可能原因是「在交易裡又開了一個交易」。
const WRITER_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(15);

pub struct SqliteDb {
    writer: SqlitePool,
    reader: SqlitePool,
}

fn base_options(path: &Path) -> AppResult<SqliteConnectOptions> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .map_err(|e| AppError::Db(format!("解析資料庫路徑失敗：{e}")))?
        .filename(path)
        .create_if_missing(true)
        // WAL：讀寫並行的前提，也是雙池設計的基礎。
        .journal_mode(SqliteJournalMode::Wal)
        // sqlx 預設已開，明寫避免版本行為變動。POS 的參照完整性不能靠應用層自律。
        .foreign_keys(true)
        // 保險絲而非主要機制：正常路徑靠 writer pool-of-1 序列化，
        // 這個是為了擋備份 VACUUM INTO / checkpoint 期間的短暫獨佔。
        .busy_timeout(Duration::from_secs(5))
        // 報表的 GROUP BY 會用到臨時表。
        .pragma("temp_store", "memory")
        // 64MB mmap，只影響讀取，不影響耐久性。
        .pragma("mmap_size", "67108864");
    Ok(opts)
}

impl SqliteDb {
    /// 開啟資料庫：建立雙池 → 跑 migration → 開機自檢。
    pub async fn open(path: &Path, max_readers: Option<u32>) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Storage(format!("建立資料庫目錄失敗：{e}")))?;
        }

        let writer = SqlitePoolOptions::new()
            .max_connections(1) // ★ 序列化寫入的關鍵
            .min_connections(1) // 常駐，免每次重連付 PRAGMA 成本
            .acquire_timeout(WRITER_ACQUIRE_TIMEOUT)
            .idle_timeout(None) // 永不回收，就這一條
            .max_lifetime(None)
            // 連線歸還池子前，把可能殘留的交易回捲。
            // 沒有這道保險，一個忘了 commit/rollback 的 UoW 會讓唯一那條寫入連線
            // 帶著開啟的交易回到池子，之後每一筆寫入都會看到不一致的狀態。
            .after_release(|conn, _meta| {
                Box::pin(async move {
                    // 沒有進行中的交易時 SQLite 會回錯，直接忽略。
                    let _ = conn.execute("ROLLBACK").await;
                    Ok(true)
                })
            })
            .connect_with(base_options(path)?.synchronous(SqliteSynchronous::Full))
            .await
            .map_err(|e| AppError::Db(format!("開啟寫入連線失敗：{e}")))?;

        // migration 必須在 writer 上、且在 reader 建立之前跑完。
        MIGRATOR
            .run(&writer)
            .await
            .map_err(|e| AppError::Db(format!("資料庫 migration 失敗：{e}")))?;

        let reader = SqlitePoolOptions::new()
            .max_connections(max_readers.unwrap_or(DEFAULT_MAX_READERS).clamp(2, 16))
            .acquire_timeout(Duration::from_secs(10))
            .idle_timeout(Some(Duration::from_secs(300)))
            .connect_with(base_options(path)?.synchronous(SqliteSynchronous::Normal))
            .await
            .map_err(|e| AppError::Db(format!("開啟讀取連線失敗：{e}")))?;

        let db = Self { writer, reader };
        db.verify_boot_invariants().await?;
        Ok(db)
    }

    /// 開機自檢。**任何一項失敗都拒絕啟動**，不降級執行。
    async fn verify_boot_invariants(&self) -> AppResult<()> {
        // ① journal_mode 必須真的是 WAL。
        //    這是偵測「資料庫被放在網路磁碟」最可靠的一點：SQLite 在不支援共享記憶體的
        //    檔案系統上會**靜默退回** delete mode，而不是報錯。guard.rs 的路徑檢查是
        //    第一道，這裡是第二道（擋掉路徑看起來正常、實際上是網路掛載的情形）。
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&self.writer)
            .await
            .map_err(|e| AppError::Db(format!("讀取 journal_mode 失敗：{e}")))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(AppError::Startup(format!(
                "資料庫未能啟用 WAL 模式（目前為 {mode}）。\n\
                 這通常代表資料庫被放在網路磁碟或不支援共享記憶體的檔案系統上，\n\
                 繼續執行會有資料損毀風險。請把資料改放到本機磁碟。"
            )));
        }

        // ② 快速完整性檢查。比 integrity_check 便宜，適合每次開機跑。
        let check: String = sqlx::query_scalar("PRAGMA quick_check")
            .fetch_one(&self.reader)
            .await
            .map_err(|e| AppError::Db(format!("完整性檢查失敗：{e}")))?;
        if !check.eq_ignore_ascii_case("ok") {
            return Err(AppError::Startup(format!(
                "資料庫完整性檢查未通過：{check}\n請從備份還原。"
            )));
        }

        // ③ 外鍵必須是開的。
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&self.writer)
            .await
            .map_err(|e| AppError::Db(format!("讀取 foreign_keys 失敗：{e}")))?;
        if fk != 1 {
            return Err(AppError::Startup("外鍵約束未啟用，拒絕啟動".into()));
        }
        Ok(())
    }

    /// 開始一個寫入交易。**這是全案唯一的交易入口。**
    pub async fn begin_write(&self) -> AppResult<SqliteUow> {
        SqliteUow::begin(&self.writer).await
    }

    /// 讀取池。只用於不需要一致性快照的查詢。
    pub fn reader(&self) -> &SqlitePool {
        &self.reader
    }

    /// 方言中立的 schema 指紋。v2.0 要拿它與 PostgreSQL 對拍；
    /// v1 先用來當「schema 有沒有被意外改動」的 golden test 與診斷面板欄位。
    pub async fn schema_fingerprint(&self) -> AppResult<String> {
        let rows = sqlx::query(
            "SELECT type, name, COALESCE(tbl_name, '') AS tbl_name, COALESCE(sql, '') AS sql
               FROM sqlite_master
              WHERE name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx_%'
              ORDER BY type, name",
        )
        .fetch_all(&self.reader)
        .await
        .map_err(|e| AppError::Db(e.to_string()))?;

        let mut lines: Vec<String> = rows
            .iter()
            .map(|r| {
                let sql: String = r.get("sql");
                format!(
                    "{}|{}|{}|{}",
                    r.get::<String, _>("type"),
                    r.get::<String, _>("name"),
                    r.get::<String, _>("tbl_name"),
                    normalize_sql(&sql)
                )
            })
            .collect();
        lines.sort();
        Ok(lines.join("\n"))
    }

    /// 這個 binary 內建的最高 migration 版本。
    /// 還原時用它擋下「用新版備份還原到舊版程式」—— SQLite 不會攔你，
    /// 只會在某個查詢時噴 no such column，而那時已經開了半天的單。
    pub fn max_migration_version() -> i64 {
        MIGRATOR
            .migrations
            .iter()
            .map(|m| m.version)
            .max()
            .unwrap_or(0)
    }

    pub async fn close(&self) {
        self.writer.close().await;
        self.reader.close().await;
    }
}

/// 壓掉空白差異，讓指紋只反映結構而非格式。
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}
