# ADR 0002 — 時間存 UTC 文字，營業日寫入時算好

狀態：已採用（2026-09-06）

## 決定

* 時間欄位存 **ISO-8601 UTC 固定寬度文字**：`2026-09-06T14:23:45.123Z`（24 字元）。
* **一律由 Rust 寫入**，DDL 裡不放 `DEFAULT (datetime('now'))`。
* 營業日 `business_date` 在**寫入時算好**並落成獨立欄位（`YYYY-MM-DD`），
  永遠不在查詢時現算。

## 理由

**為什麼是固定寬度文字而不是 epoch 整數。**
固定寬度加補零 ⇒ 字典序等於時序，排序不需要轉換；用 db-kit 直接開檔除錯時肉眼可讀；
換 PostgreSQL 只要一行 `ALTER ... USING c::timestamptz`（epoch 要 `to_timestamp(c/1000.0)`）；
而且 sqlx 兩邊都能解成同一個 Rust 型別 `DateTime<Utc>`。
多出來的 16 bytes 對地端 POS 完全不是問題。

**為什麼不用資料庫預設值。**
三個理由：`datetime('now')` 沒有毫秒、與格式不合；PG 的等價寫法是 `now()`，是方言分歧點；
最重要的是**同一筆交易的多張表必須拿到完全相同的時間戳**（`Utc::now()` 取一次傳進整個
use case），資料庫預設值做不到，會讓 `orders.settled_at` 與 `payments.paid_at` 差幾毫秒，
報表對不起來。

**為什麼營業日要落成欄位。**
夜市攤與居酒屋營業到凌晨三點，「9/6 02:30 的訂單」屬於 9/5 的營業日。
若靠報表查詢現算 `datetime(created_at, '-5 hours')`，會有五個災難：
函式包住欄位讓**索引直接失效**；時區換算容易錯；切點設定若中途被改過，歷史資料前後不一致；
日結之後補的單會落到錯誤的營業日；SQLite 與 PG 的日期函式不同。

## 後果

* `stores.tz` 與 `stores.business_day_cutoff`（預設 `Asia/Taipei` 與 `05:00`）是設定項。
* `BusinessDate::of()` 是純函式，用 `chrono-tz`（純 Rust，內含 IANA tzdb）。
* 以下的表都要帶 `business_date` 欄位並建索引：orders、bills、payments、refunds、
  shifts、cash_movements、table_sessions、einvoices、audit_logs、approvals、print_jobs。
* 跨營業日未結的桌（客人坐到凌晨四點）在日結時列為「跨日未結轉入」，
  `orders.business_date` 保持開桌時的值不變 —— 這是餐飲慣例。

## 補做的代價

要對歷史列反推時區與切點（而切點可能中途改過），所有報表查詢與索引全部重寫，
且日結數字會在補做前後對不起來。屬於「極痛」等級，所以列為 M1 硬前提。
