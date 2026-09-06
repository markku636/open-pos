//! 應用層的基本查詢：版本、資料位置、健康狀態。
//!
//! `health()` 不是裝飾品。地端 + 離線 + 非技術使用者三件事疊起來，代表你永遠
//! 無法重現任何一個「今天中午印不出來，現在又好了」的回報。所以 UI 上要有一排
//! 常駐的紅綠燈，讓店家在打電話之前就能自己看出是哪一段斷了。

use serde::{Deserialize, Serialize};

use crate::ctx::Ctx;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub schema_version: i64,
    pub started_at: String,
}

pub async fn app_info(ctx: &Ctx) -> AppResult<AppInfo> {
    Ok(AppInfo {
        version: crate::VERSION.to_string(),
        data_dir: ctx.layout.root.to_string_lossy().into_owned(),
        schema_version: crate::infra::db::sqlite::SqliteDb::max_migration_version(),
        started_at: crate::core::clock::to_iso(ctx.started_at),
    })
}

/// 一項健康檢查的結果。`ok = false` 時 `detail` 必須說得出「該怎麼辦」。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthItem {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub ok: bool,
    pub items: Vec<HealthItem>,
}

pub async fn health(ctx: &Ctx) -> AppResult<Health> {
    let mut items = Vec::new();

    // ① 資料庫可寫。只是開一個交易再回捲 —— 不留下任何痕跡，但會真的取得寫入鎖，
    //    所以「磁碟滿了」「檔案被防毒軟體鎖住」這類問題會在這裡現形。
    let db_writable = match ctx.db.begin_write().await {
        Ok(uow) => {
            let ok = uow.rollback().await.is_ok();
            (
                ok,
                if ok {
                    "可寫入".into()
                } else {
                    "回捲失敗".to_string()
                },
            )
        }
        Err(e) => (false, format!("無法取得寫入交易：{}", e.message())),
    };
    items.push(HealthItem {
        name: "資料庫".into(),
        ok: db_writable.0,
        detail: db_writable.1,
    });

    // ② 上次備份距今多久。這一項比前一項更常救人：資料庫幾乎不會壞，
    //    但「以為有在備份、其實 USB 拔掉三個月了」非常常見。
    let backup = last_backup_age_hours(ctx);
    items.push(match backup {
        Some(h) if h <= 26.0 => HealthItem {
            name: "備份".into(),
            ok: true,
            detail: format!("最近一次在 {h:.0} 小時前"),
        },
        Some(h) => HealthItem {
            name: "備份".into(),
            ok: false,
            detail: format!("最近一次在 {h:.0} 小時前 —— 請確認備份用的隨身碟還插著"),
        },
        None => HealthItem {
            name: "備份".into(),
            ok: false,
            detail: "還沒有任何備份 —— 請到設定頁指定備份位置（建議用常插著的隨身碟）".into(),
        },
    });

    Ok(Health {
        ok: items.iter().all(|i| i.ok),
        items,
    })
}

fn last_backup_age_hours(ctx: &Ctx) -> Option<f64> {
    let mut newest: Option<std::time::SystemTime> = None;
    for bucket in ["hourly", "shift", "daily"] {
        let dir = ctx.layout.backups_dir().join(bucket);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            if e.path().extension().map(|x| x != "db").unwrap_or(true) {
                continue;
            }
            if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                newest = Some(match newest {
                    Some(cur) if cur >= m => cur,
                    _ => m,
                });
            }
        }
    }
    newest
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs_f64() / 3600.0)
}
