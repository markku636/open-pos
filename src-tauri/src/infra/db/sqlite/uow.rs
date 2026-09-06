//! UnitOfWork：一次 use case = 一個交易。
//!
//! # 為什麼不用 `sqlx::Transaction`
//!
//! `sqlx::Transaction<'c, DB>` 帶著具體的 DB 型別參數，**不可 dyn 化**。
//! 如果讓每個 repo 方法自己 `pool.begin()`，就會出現一個結構性缺陷：
//! 一次結帳要同時動 orders / bills / payments / shifts，四個 repo 各開各的交易，
//! 中間任何一步失敗都會留下半套資料 —— 而 POS 的半套資料就是帳目對不起來。
//!
//! 所以交易邊界上移到 use case：`services::*` 開一個 UoW，把它傳給每個 repo 方法。
//! repo trait 的方法簽章長這樣：
//!
//! ```ignore
//! async fn settle_in(&self, uow: &mut SqliteUow, cmd: Settle) -> AppResult<Settlement>;
//! ```
//!
//! # 為什麼是 BEGIN IMMEDIATE
//!
//! SQLite 預設的 `BEGIN`（deferred）在第一次讀取時只取 SHARED 鎖，等到要寫入才升級成
//! RESERVED。升級失敗時回 `SQLITE_BUSY`，而且**整筆交易必須重來**（不能等，因為它已經
//! 讀過可能已被改動的資料）。`BEGIN IMMEDIATE` 一開始就取 RESERVED，把等待挪到
//! 交易開頭 —— 那裡等是安全的。
//!
//! sqlx 沒有暴露「開 IMMEDIATE 交易」的 API，所以這裡自己拿連線、自己下 SQL、自己管
//! commit / rollback。

use sqlx::pool::PoolConnection;
use sqlx::{Executor, Sqlite, SqlitePool};

use crate::error::{AppError, AppResult};

pub struct SqliteUow {
    conn: PoolConnection<Sqlite>,
    finished: bool,
}

impl SqliteUow {
    pub(super) async fn begin(pool: &SqlitePool) -> AppResult<Self> {
        let mut conn = pool.acquire().await.map_err(|e| {
            // 寫入池只有一條連線。取不到的最可能原因不是「太忙」，而是
            // 「已經在一個寫入交易裡了又想再開一個」—— 明講出來，省下數小時除錯。
            AppError::Db(format!(
                "取得寫入連線逾時：{e}\n\
                 最可能的原因是在一個寫入交易進行中又開了另一個交易（自我死鎖）。\n\
                 repo 方法應該接收 &mut SqliteUow，而不是自己開交易。"
            ))
        })?;
        conn.execute("BEGIN IMMEDIATE")
            .await
            .map_err(|e| AppError::Db(format!("開始交易失敗：{e}")))?;
        Ok(Self {
            conn,
            finished: false,
        })
    }

    /// 交易內執行 SQL 用的 executor。
    pub fn conn(&mut self) -> &mut sqlx::SqliteConnection {
        &mut self.conn
    }

    pub async fn commit(mut self) -> AppResult<()> {
        self.conn
            .execute("COMMIT")
            .await
            .map_err(|e| AppError::Db(format!("提交交易失敗：{e}")))?;
        self.finished = true;
        Ok(())
    }

    pub async fn rollback(mut self) -> AppResult<()> {
        self.conn
            .execute("ROLLBACK")
            .await
            .map_err(|e| AppError::Db(format!("回捲交易失敗：{e}")))?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for SqliteUow {
    fn drop(&mut self) {
        // Drop 不能 await，所以這裡只留下警告。真正的回捲由寫入池的 after_release
        // hook 負責（見 sqlite/mod.rs）—— 那是唯一能保證「連線回到池子時是乾淨的」的地方。
        if !self.finished {
            tracing::warn!("UnitOfWork 未經 commit/rollback 就被丟棄，交易將由連線池回捲");
        }
    }
}
