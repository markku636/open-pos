//! 診斷資訊匯出。
//!
//! # 為什麼這是 v1.0 的功能而不是「以後再說」
//!
//! 你會收到的 issue 長這樣：「今天中午印不出來，現在又好了」。
//!
//! 地端 + 離線 + 非技術使用者三件事疊起來，代表**維護者沒有任何辦法重現**。
//! 一人維護的專案通常不是死在寫程式，是死在無法診斷的回報上。
//!
//! # 為什麼是一個純文字檔而不是 zip
//!
//! 因為它要能被**貼進 GitHub issue**。純文字可以直接看、可以搜尋、
//! reviewer 不必下載附件再解壓縮；而且少一個壓縮相依。
//!
//! # 什麼不會被放進來
//!
//! 品名、客人資訊、帳單明細一律不放。要診斷「印不出來」需要的是設定與
//! 錯誤訊息，不是那家店賣了什麼。**預設不上傳任何東西** —— 檔案存到使用者
//! 選的位置，要不要貼出去是他的決定。

use std::fmt::Write as _;

use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::services::{backup_worker, printer, shift};

/// 最多帶多少行 log。太少查不出東西，太多沒有人會讀完。
const LOG_TAIL_LINES: usize = 400;

/// 產生診斷內容（純文字）。
pub async fn report(ctx: &Ctx) -> AppResult<String> {
    let mut out = String::new();
    let now = crate::core::clock::Stamp::now();

    let _ = writeln!(out, "# open-pos 診斷資訊");
    let _ = writeln!(out, "產生時間　{}", now.iso());
    let _ = writeln!(out, "程式版本　{}", crate::VERSION);
    let _ = writeln!(
        out,
        "schema　　{}",
        crate::infra::db::sqlite::SqliteDb::max_migration_version()
    );
    let _ = writeln!(out, "作業系統　{}", std::env::consts::OS);
    let _ = writeln!(out, "資料目錄　{}", ctx.layout.root.display());
    let _ = writeln!(
        out,
        "啟動時間　{}",
        crate::core::clock::to_iso(ctx.started_at)
    );

    section(&mut out, "健康檢查");
    match crate::services::app::health(ctx).await {
        Ok(h) => {
            for item in &h.items {
                let _ = writeln!(
                    out,
                    "  [{}] {}：{}",
                    if item.ok { "OK" } else { "!!" },
                    item.name,
                    item.detail
                );
            }
        }
        Err(e) => {
            let _ = writeln!(out, "  健康檢查本身失敗：{}", e.message());
        }
    }

    section(&mut out, "出單機");
    match printer::list_printers(ctx).await {
        Ok(list) if list.is_empty() => {
            let _ = writeln!(out, "  （還沒有設定任何出單機）");
        }
        Ok(list) => {
            for p in &list {
                let _ = writeln!(
                    out,
                    "  {} | {} | {:?} | {} | 上次測試 {} {}",
                    p.name,
                    p.transport.describe(),
                    p.caps.paper,
                    p.render_mode,
                    p.last_probe_at.as_deref().unwrap_or("—"),
                    match p.last_probe_ok {
                        Some(true) => "成功".to_string(),
                        Some(false) => format!("失敗：{}", p.last_error.as_deref().unwrap_or("")),
                        None => String::new(),
                    }
                );
            }
        }
        Err(e) => {
            let _ = writeln!(out, "  讀不到：{}", e.message());
        }
    }

    section(&mut out, "出單分區");
    match printer::list_stations(ctx).await {
        Ok(list) if list.is_empty() => {
            let _ = writeln!(out, "  （沒有分區，所有單都印到同一台）");
        }
        Ok(list) => {
            for s in &list {
                let _ = writeln!(
                    out,
                    "  {} | 綁 {} 台 | {}",
                    s.name,
                    s.printers.len(),
                    if s.split_per_item {
                        "一品項一張"
                    } else {
                        "整張一起"
                    }
                );
            }
        }
        Err(e) => {
            let _ = writeln!(out, "  讀不到：{}", e.message());
        }
    }

    section(&mut out, "列印佇列");
    match printer::queue_status(ctx).await {
        Ok(q) => {
            let _ = writeln!(
                out,
                "  排隊 {} / 死信 {} / 未展開 {}　—— {}",
                q.pending, q.dead, q.unrouted, q.detail
            );
        }
        Err(e) => {
            let _ = writeln!(out, "  讀不到：{}", e.message());
        }
    }
    // 只列**失敗**的工作。成功的單沒有診斷價值，只會把檔案撐長。
    match printer::list_print_jobs(ctx, Some(200)).await {
        Ok(jobs) => {
            let failed: Vec<_> = jobs
                .iter()
                .filter(|j| j.status == "dead" || j.status == "failed" || j.attempts > 1)
                .take(20)
                .collect();
            if failed.is_empty() {
                let _ = writeln!(out, "  最近沒有失敗的列印工作。");
            }
            for j in failed {
                let _ = writeln!(
                    out,
                    "  {} | {} | {} | {} | 試 {} 次 | {} | {}",
                    j.created_at,
                    j.printer_name,
                    j.doc_type,
                    j.reason,
                    j.attempts,
                    j.last_error_class.as_deref().unwrap_or("—"),
                    j.last_error.as_deref().unwrap_or("")
                );
            }
        }
        Err(e) => {
            let _ = writeln!(out, "  讀不到列印紀錄：{}", e.message());
        }
    }

    section(&mut out, "備份");
    match backup_worker::since_last_backup(ctx) {
        Some(age) => {
            let _ = writeln!(out, "  上次備份在 {:.1} 小時前", age.as_secs_f64() / 3600.0);
        }
        None => {
            let _ = writeln!(out, "  !! 一份備份都沒有");
        }
    }
    match crate::services::backup::list_backups(ctx).await {
        Ok(files) => {
            for f in files.iter().take(5) {
                let _ = writeln!(
                    out,
                    "  {} | {} | {} bytes | {}",
                    f.bucket,
                    f.name,
                    f.size_bytes,
                    if f.external { "外接" } else { "本機" }
                );
            }
        }
        Err(e) => {
            let _ = writeln!(out, "  讀不到：{}", e.message());
        }
    }

    section(&mut out, "班別與營業日");
    match shift::day_status(ctx).await {
        Ok(d) => {
            let _ = writeln!(
                out,
                "  {} | {} | 已關 {} 班 | 目前 {}",
                d.business_date,
                d.status,
                d.closed_shifts,
                d.shift
                    .as_ref()
                    .map(|s| s.shift_no.clone())
                    .unwrap_or_else(|| "沒有開著的班".into())
            );
        }
        Err(e) => {
            let _ = writeln!(out, "  讀不到：{}", e.message());
        }
    }

    section(&mut out, "資料量（不含任何品名與客人資訊）");
    for (label, sql) in [
        ("訂單", "SELECT COUNT(*) FROM orders"),
        ("明細", "SELECT COUNT(*) FROM order_items"),
        ("帳單", "SELECT COUNT(*) FROM bills"),
        ("收款", "SELECT COUNT(*) FROM payments"),
        (
            "商品",
            "SELECT COUNT(*) FROM items WHERE deleted_at IS NULL",
        ),
        ("列印工作", "SELECT COUNT(*) FROM print_jobs"),
        ("稽核紀錄", "SELECT COUNT(*) FROM audit_logs"),
    ] {
        let n: i64 = sqlx::query_scalar(sql)
            .fetch_one(ctx.db.reader())
            .await
            .unwrap_or(-1);
        let _ = writeln!(out, "  {label}　{n}");
    }

    section(&mut out, &format!("最近 {LOG_TAIL_LINES} 行 log"));
    out.push_str(&log_tail(ctx).await);

    let _ = writeln!(
        out,
        "\n---\n這份檔案不含品名、客人資訊或帳單明細。\n\
         open-pos 不會自動上傳任何東西 —— 要不要貼出去是你的決定。"
    );
    Ok(out)
}

fn section(out: &mut String, title: &str) {
    let _ = write!(out, "\n## {title}\n");
}

async fn log_tail(ctx: &Ctx) -> String {
    let dir = ctx.layout.logs_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return format!("  （讀不到 log 目錄 {}）\n", dir.display());
    };
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(t) = entry.metadata().and_then(|m| m.modified()) {
            if newest.as_ref().is_none_or(|(n, _)| t > *n) {
                newest = Some((t, path));
            }
        }
    }
    let Some((_, path)) = newest else {
        return "  （還沒有 log 檔）\n".into();
    };
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => {
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(LOG_TAIL_LINES);
            let mut out = format!("  （來源：{}）\n", path.display());
            for l in &lines[start..] {
                out.push_str("  ");
                out.push_str(l);
                out.push('\n');
            }
            out
        }
        Err(e) => format!("  （讀不到 {}：{e}）\n", path.display()),
    }
}

/// 產生診斷檔並寫到指定資料夾，回傳完整路徑。
pub async fn export(ctx: &Ctx, dir: String) -> AppResult<String> {
    let text = report(ctx).await?;
    let dir = std::path::PathBuf::from(dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Storage(format!("建不出資料夾：{e}")))?;
    let name = format!(
        "open-pos-診斷-{}.txt",
        crate::core::clock::now_iso().replace(':', "-")
    );
    let path = dir.join(name);
    // 加 BOM：Windows 的記事本在沒有 BOM 時會把 UTF-8 中文顯示成亂碼，
    // 而回報問題的人最常用的就是記事本。
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(text.as_bytes());
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| AppError::Storage(format!("寫不出診斷檔：{e}")))?;
    Ok(path.to_string_lossy().into_owned())
}
