# ADR 0003 — v1 用單一 SQLite migration 目錄，v2.0 才分岔

狀態：已採用（2026-09-06）

## 背景

v1 只支援 SQLite，但 PostgreSQL 是明確的路線圖項目。問題是：**現在要為它付多少成本？**

## 考慮過的三個方案

| | A：`{{TOKEN}}` 模板 + 自寫 migrator | B：v1 就開雙目錄 + 指紋對拍 | **C：延後分岔 + Baseline** |
| --- | --- | --- | --- |
| v1 的 migration | 帶模板的 SQL | 寫兩份 | **單一目錄、純 SQLite SQL** |
| Migrator | 自寫約 120 行 | `sqlx::migrate!` | `sqlx::migrate!` |
| v1 成本 | 高 | 高 | **零** |
| 貢獻者心智負擔 | 高 | 中 | **低** |

## 決定：方案 C

v1 用單一 `migrations/` 目錄寫**純 SQLite SQL**，但從第一行 DDL 起就用
`src-tauri/tests/ddl_lint.rs` 強制 PostgreSQL 可攜性。v2.0 時一次性手工產生
`migrations/postgres/0001_baseline.sql`，之後雙目錄雙寫，以 schema 指紋與
golden-day 兩支測試防漂移。

## 理由

**否決 A。** 自寫 migrator 是把「不能出錯的東西」自己重寫一遍 —— `sqlx::migrate!`
處理版本序、checksum、交易包裹、`_sqlx_migrations` 記錄，寫錯的後果是**弄壞使用者
正在營業的資料庫**。而且 `{{MONEY}}` 讓 SQL 不再是 SQL：貼不進 db-kit 的編輯器跑、
linter 壞掉、無法從網路複製範例 —— 對一個要外部貢獻的 MIT 專案，這是直接勸退。
更糟的是「忘記加 token」是靜默錯誤，模板不但沒防住它，還讓人以為有 token 就安全了。

**否決 B。** M1 到 M7 之間沒有任何開發者、CI job 或使用者會執行 `migrations/postgres/`。
**一份從不執行的 DDL 就是一份註解**，會無聲腐爛。而且實作期一定會改 schema，
等於在最不穩定的階段付雙倍成本。

**C 的代價**是 v2.0 那次手工轉譯。但那時轉譯的是**已被打磨穩定的最終形態**，
而不是七輪演進，而且有 lint 保證過程中沒有累積 SQLite 專有語法。

## `ddl_lint` 守的四件事

1. **禁用 SQLite 專有語法**：`AUTOINCREMENT`、`WITHOUT ROWID`、`STRICT`、
   `COLLATE NOCASE`、`IFNULL(`、`datetime('now')`、`strftime(`、`PRAGMA`、`GLOB`、
   `INSERT OR IGNORE/REPLACE`、`IF NOT EXISTS`
2. **只允許 `TEXT` 與 `INTEGER`** 兩種欄位型別
3. **金額欄位必須是 `INTEGER`**（依 `MONEY_SUFFIXES` 命名判定）
4. **索引名稱有前綴且全域唯一**（PG 的索引名是 schema-scoped、SQLite 是 db-scoped）

第 3 條不是形式主義：SQLite 的 `INTEGER` 是動態最大 8 bytes，PG 的 `INTEGER` 是
固定 int4（上限 21 億）。金額欄位若在 v2.0 被轉成 int4，年度彙總超過 21 億元就溢位 ——
而這個 bug 只會在 v2.0 之後、只在有規模的使用者身上、只在年底出現。

## 廢除的機制

原本設計裡的 `@dialect` 逃生艙不再需要 —— v2.0 之後已經是兩份獨立的檔案，
方言差異直接寫在各自的檔裡就好。

## 其他規則

* migration 只做 additive，永不 `DROP COLUMN`（改用 `deprecated_` 前綴 + 停用）
* 已套用的 migration 不可修改（sqlx 會算 checksum，改了會在啟動時報 `VersionMismatch`）
* 種子資料不放 migration，放 `services::seed::apply_if_empty()` ——
  否則使用者刪掉 demo 菜單後永遠回不來，而升級時的 checksum 又逼你不能改
