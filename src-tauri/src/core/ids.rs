//! 主鍵型別。
//!
//! ULID 存成 26 碼 TEXT。四個理由，每一個都排除了一種候選：
//!
//! * **不用自增整數**：多分店合併時撞號要全表 remap；掃碼點餐的手機端必須先產生
//!   暫存 id 才能送出，客戶端拿不到自增值；`AUTOINCREMENT` 是 SQLite 專有語法
//!   （PG 要換 `GENERATED ALWAYS AS IDENTITY`），是方言分歧點；而且猜得到的 id
//!   對掃碼點餐的 URL 是安全問題。
//! * **不用 UUIDv4**：全隨機導致 B-tree 頁分裂嚴重；人眼也看不出先後，除錯不便。
//! * **不用 ULID BLOB(16)**：省 10 bytes，但用 db-kit 開檔是亂碼、log 不可讀。
//! * **用 ULID TEXT(26)**：前 48 bit 是毫秒時間戳，所以**字典序等於產生時序** ——
//!   插入是 append-only，且 `ORDER BY id` 就等於 `ORDER BY created_at`，省一個索引。
//!
//! 唯一例外是**人看的單號**（`A-20260906-0042`），那個走 `sequences` 表原子配號。

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(String);

/// 全域單調 ULID 產生器。
///
/// ⚠️ **`ulid::Ulid::new()` 不保證同毫秒內單調** —— 它每次都取一組新的隨機低位，
/// 所以同一毫秒產生的兩個 id，字典序是隨機的。這一點會直接推翻整個主鍵設計所依賴的
/// 不變量（append-only 插入、以 `ORDER BY id` 取代 `ORDER BY created_at`）。
///
/// POS 在尖峰時一毫秒內產生多個 id 是常態（一張單的十個明細列），所以必須用
/// `Generator`：它在同毫秒內改為遞增隨機位，保證單調。
///
/// 這個不變量由 `lexical_order_matches_creation_order` 測試守住。
static ULID_GEN: once_cell::sync::Lazy<parking_lot::Mutex<ulid::Generator>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(ulid::Generator::new()));

impl Id {
    /// 產生新 id（同毫秒內保證單調遞增）。
    pub fn new() -> Self {
        let mut g = ULID_GEN.lock();
        // generate() 只在「同一毫秒內隨機位全部用盡」時失敗，機率極低。
        // 真的發生時退回非單調版本 —— 寧可順序略有偏差，也不要在收銀機上 panic。
        let u = g.generate().unwrap_or_else(|_| ulid::Ulid::new());
        Self(u.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// 驗證外部傳入的字串（例如手機端產生的暫存 id）。
    pub fn parse(s: &str) -> AppResult<Self> {
        ulid::Ulid::from_string(s)
            .map(|u| Self(u.to_string()))
            .map_err(|e| AppError::Validation(format!("不是合法的 ULID：{s}（{e}）").into()))
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
    }
}

impl FromStr for Id {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_26_chars() {
        assert_eq!(Id::new().as_str().len(), 26);
    }

    #[test]
    fn lexical_order_matches_creation_order() {
        // 這是選 ULID 的核心理由：字典序 == 時序。
        // 同毫秒內 ULID 的隨機段仍單調遞增，所以連續產生也成立。
        let ids: Vec<Id> = (0..64).map(|_| Id::new()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "ULID 的字典序必須等於產生順序");
    }

    #[test]
    fn rejects_garbage() {
        assert!(Id::parse("not-a-ulid").is_err());
        let good = Id::new();
        assert_eq!(Id::parse(good.as_str()).unwrap(), good);
    }
}
