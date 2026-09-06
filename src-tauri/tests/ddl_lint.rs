//! DDL lint —— 讓 v1 的 SQLite DDL 從第一行起就是 PostgreSQL-portable。
//!
//! # 為什麼需要這支測試
//!
//! v1 只出 SQLite，`migrations/` 是單一目錄的純 SQLite SQL（見
//! `docs/adr/0003-single-source-ddl.md`）。這是刻意的：
//!
//! * 不用 `{{TOKEN}}` 模板 + 自寫 migrator —— 自寫 migrator 是把「不能出錯的東西」
//!   自己重寫一遍，寫錯的後果是弄壞使用者正在營業的資料庫；而且模板讓 SQL 不再是 SQL，
//!   貼不進編輯器、linter 也壞掉，外部貢獻者直接被勸退。
//! * 不在 v1 就開 `migrations/postgres/` —— 那份 DDL 在 v2.0 之前沒有任何人會執行，
//!   一份從不執行的 DDL 就是一份會無聲腐爛的註解。
//!
//! 代價是「v2.0 才一次性手工轉譯」。**這支 lint 就是讓那次轉譯不會變成地獄的支柱**：
//! 它保證 DDL 裡不會累積 SQLite 專有語法，也保證欄位命名與型別的對應是機械的。
//!
//! 沒有它，方案就不成立。

use std::collections::HashMap;
use std::path::PathBuf;

/// SQLite 專有、在 PostgreSQL 上不存在或語意不同的語法。
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "AUTOINCREMENT",
        "PG 要用 GENERATED ALWAYS AS IDENTITY；本專案一律用 ULID TEXT 主鍵",
    ),
    ("WITHOUT ROWID", "SQLite 專有"),
    (") STRICT", "SQLite 專有"),
    (
        "COLLATE NOCASE",
        "SQLite 專有；改用 lower() 函式索引，兩邊都支援",
    ),
    ("IFNULL(", "改用標準的 COALESCE()"),
    (
        "DATETIME('NOW')",
        "時間一律由 Rust 寫入，不用資料庫預設值（見 ADR 0002）",
    ),
    ("DATE('NOW')", "同上"),
    ("STRFTIME(", "SQLite 專有日期函式"),
    ("JULIANDAY(", "SQLite 專有日期函式"),
    ("LAST_INSERT_ROWID", "本專案主鍵由應用層產生（ULID）"),
    ("SQLITE_MASTER", "內省語法兩邊不同，不該出現在 migration 裡"),
    (
        "PRAGMA ",
        "PRAGMA 屬於連線設定，由 Rust 端統一設（見 infra/db/sqlite）",
    ),
    ("GLOB ", "SQLite 專有；改用 LIKE"),
    ("INSERT OR IGNORE", "改用標準的 ON CONFLICT DO NOTHING"),
    ("INSERT OR REPLACE", "改用標準的 ON CONFLICT DO UPDATE"),
    (
        "IF NOT EXISTS",
        "migration 有版本控管；IF NOT EXISTS 只會掩蓋『重跑』這種真正的錯誤",
    ),
];

/// 只允許這兩種欄位型別。
///
/// 刻意不允許 REAL / BLOB / NUMERIC / DECIMAL / VARCHAR(n) / BOOLEAN / DATETIME：
/// 它們要嘛在 PG 上語意不同，要嘛（REAL）根本不該用來存錢。
const ALLOWED_TYPES: &[&str] = &["TEXT", "INTEGER"];

/// 金額欄位的名稱特徵。命中的欄位型別必須是 INTEGER。
///
/// ★ 這條規則不是形式主義：SQLite 的 INTEGER 是動態最大 8 bytes，
/// PostgreSQL 的 INTEGER 是固定 int4（上限 21 億）。金額欄位若在 v2.0 被轉成 int4，
/// 年度彙總超過 21 億元就溢位 —— 而這個 bug 只會在 v2.0 之後、只在有規模的使用者身上、
/// 只在年底出現。在 v1 就把清單釘死，v2.0 的轉譯器直接吃這張表。
const MONEY_SUFFIXES: &[&str] = &[
    "_amount",
    "_price",
    "_total",
    "_cash",
    "_float",
    "_variance",
    "_charge",
    "_delta",
    "_bp",
];

fn migrations_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri 應該有上層目錄")
        .join("migrations")
}

fn read_migrations() -> Vec<(String, String)> {
    let dir = migrations_dir();
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("讀取 {} 失敗：{e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "sql").unwrap_or(false))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let body = std::fs::read_to_string(&p).unwrap();
            (name, body)
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "migrations/ 不該是空的");
    out
}

/// 去掉 `--` 註解，避免規則誤判說明文字。
fn strip_comments(sql: &str) -> String {
    sql.lines()
        .map(|l| match l.find("--") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn no_sqlite_only_syntax() {
    for (file, raw) in read_migrations() {
        let sql = strip_comments(&raw).to_uppercase();
        for (needle, why) in FORBIDDEN {
            assert!(
                !sql.contains(needle),
                "{file}：出現禁用語法 `{needle}` —— {why}\n\
                 （規則見 docs/adr/0003-single-source-ddl.md）"
            );
        }
    }
}

/// 解析 `CREATE TABLE` 區塊裡的 (欄位名, 型別)。
///
/// 刻意只做到「夠用」的程度：取每個頂層逗號分段的前兩個 token，
/// 並跳過 CONSTRAINT / PRIMARY / FOREIGN / UNIQUE / CHECK 這些表級約束。
fn parse_columns(sql: &str) -> Vec<(String, String, String)> {
    let mut cols = Vec::new();
    let up = sql.to_uppercase();
    let mut search_from = 0usize;

    while let Some(rel) = up[search_from..].find("CREATE TABLE ") {
        let start = search_from + rel;
        let Some(open) = sql[start..].find('(') else {
            break;
        };
        let open = start + open;

        let table: String = sql[start + "CREATE TABLE ".len()..open]
            .trim()
            .trim_matches('"')
            .to_string();

        // 找出對應的右括號。
        let mut depth = 0i32;
        let mut close = open;
        for (i, c) in sql[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &sql[open + 1..close];

        // 依頂層逗號切分。
        let mut depth = 0i32;
        let mut cur = String::new();
        let mut parts = Vec::new();
        for c in body.chars() {
            match c {
                '(' => {
                    depth += 1;
                    cur.push(c);
                }
                ')' => {
                    depth -= 1;
                    cur.push(c);
                }
                ',' if depth == 0 => {
                    parts.push(std::mem::take(&mut cur));
                }
                _ => cur.push(c),
            }
        }
        parts.push(cur);

        for p in parts {
            let p = p.trim();
            if p.is_empty() {
                continue;
            }
            let mut it = p.split_whitespace();
            let (Some(name), Some(ty)) = (it.next(), it.next()) else {
                continue;
            };
            let upper_name = name.to_uppercase();
            if matches!(
                upper_name.as_str(),
                "CONSTRAINT" | "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK"
            ) {
                continue;
            }
            cols.push((
                table.clone(),
                name.trim_matches('"').to_string(),
                ty.to_uppercase(),
            ));
        }
        search_from = close;
    }
    cols
}

#[test]
fn only_portable_column_types_are_used() {
    for (file, raw) in read_migrations() {
        let sql = strip_comments(&raw);
        for (table, col, ty) in parse_columns(&sql) {
            assert!(
                ALLOWED_TYPES.contains(&ty.as_str()),
                "{file}：{table}.{col} 的型別是 {ty}，只允許 {ALLOWED_TYPES:?}。\n\
                 REAL 不可用來存錢（累加漂移會讓發票被退件）；\n\
                 VARCHAR/BOOLEAN/DATETIME 在 PG 上語意不同，改用 TEXT / INTEGER。"
            );
        }
    }
}

#[test]
fn money_columns_are_integers() {
    for (file, raw) in read_migrations() {
        let sql = strip_comments(&raw);
        for (table, col, ty) in parse_columns(&sql) {
            let lower = col.to_lowercase();
            if MONEY_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
                assert_eq!(
                    ty, "INTEGER",
                    "{file}：{table}.{col} 看起來是金額或比率欄位，型別必須是 INTEGER（整數元 / basis point），\n\
                     實際是 {ty}。理由見本檔頂端的 MONEY_SUFFIXES 註解。"
                );
            }
        }
    }
}

#[test]
fn index_names_are_prefixed_and_globally_unique() {
    let mut seen: HashMap<String, String> = HashMap::new();
    for (file, raw) in read_migrations() {
        let sql = strip_comments(&raw);
        for line in sql.lines() {
            let t = line.trim();
            let upper = t.to_uppercase();
            if !upper.starts_with("CREATE INDEX") && !upper.starts_with("CREATE UNIQUE INDEX") {
                continue;
            }
            let name = t
                .split_whitespace()
                .nth(if upper.starts_with("CREATE UNIQUE") {
                    3
                } else {
                    2
                })
                .expect("CREATE INDEX 後應有名稱")
                .to_string();

            let expected_prefix = if upper.starts_with("CREATE UNIQUE") {
                "uq_"
            } else {
                "idx_"
            };
            assert!(
                name.starts_with(expected_prefix),
                "{file}：索引 {name} 應以 {expected_prefix} 開頭"
            );
            // PG 的索引名是 schema-scoped、SQLite 是 db-scoped，
            // 統一要求全域唯一，兩邊才能用同一份名稱。
            if let Some(prev) = seen.insert(name.clone(), file.clone()) {
                panic!("索引名稱重複：{name}（{prev} 與 {file}）");
            }
        }
    }
}

#[test]
fn migration_filenames_are_ordered_and_prefixed() {
    for (file, _) in read_migrations() {
        let stem = file.trim_end_matches(".sql");
        let (num, rest) = stem.split_at(4);
        assert!(
            num.chars().all(|c| c.is_ascii_digit()) && rest.starts_with('_'),
            "migration 檔名須為 NNNN_描述.sql，實際是 {file}"
        );
    }
}
