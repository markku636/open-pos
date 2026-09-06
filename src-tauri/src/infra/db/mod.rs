//! 資料庫後端。
//!
//! v1 只有 SQLite。加 PostgreSQL（v2.0）時**整個專案只有這個檔案需要改**
//! —— `open()` 多一條 match arm、新增一個 `postgres/` 模組。
//! `core/` 與 `services/` 兩層完全不動，那是驗證資料層抽象是否成功的唯一標準。

pub mod sqlite;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DbBackend {
    #[default]
    Sqlite,
    #[cfg(feature = "postgres")]
    Postgres,
}

#[derive(Debug, Clone)]
pub struct DbConfig {
    pub backend: DbBackend,
    /// SQLite：資料庫檔案路徑。PostgreSQL：連線 URL。
    pub location: String,
    pub max_readers: Option<u32>,
}

pub enum Db {
    Sqlite(sqlite::SqliteDb),
}

impl Db {
    pub async fn open(cfg: &DbConfig) -> AppResult<Self> {
        match cfg.backend {
            DbBackend::Sqlite => Ok(Db::Sqlite(
                sqlite::SqliteDb::open(Path::new(&cfg.location), cfg.max_readers).await?,
            )),
            #[cfg(feature = "postgres")]
            DbBackend::Postgres => unimplemented!("PostgreSQL 後端於 v2.0 加入"),
        }
    }

    pub async fn schema_fingerprint(&self) -> AppResult<String> {
        match self {
            Db::Sqlite(d) => d.schema_fingerprint().await,
        }
    }

    pub async fn close(&self) {
        match self {
            Db::Sqlite(d) => d.close().await,
        }
    }
}
