//! 備份排程。
//!
//! # 為什麼不是單純的「每小時整點」
//!
//! 一台收銀機只在營業時間開著。整點排程在早上十點開機、晚上十點關機的店裡
//! 是會漏掉的，而漏掉的方式很安靜：沒有錯誤、沒有訊息，只是一整天沒有備份。
//!
//! 所以這裡的判斷是「**距離上次備份多久**」，而不是「現在是不是整點」。
//! 開機時先檢查一次，超時就立刻補跑。
//!
//! # 備份失敗不能讓迴圈停掉
//!
//! 停掉之後就再也不會備份了，而且沒有任何人會知道 —— 而那正是
//! 「以為有在備份、其實三個月沒備了」的成因。

use std::time::Duration;

use crate::ctx::Ctx;
use crate::infra::backup::BackupBucket;
use crate::infra::settings;
use crate::services::backup;

/// 每小時備份一次。
const HOURLY: Duration = Duration::from_secs(60 * 60);
/// 多久檢查一次。分鐘級的解析度對「每小時」綽綽有餘，而空轉的成本是一次 stat。
const TICK: Duration = Duration::from_secs(60);

/// 最近一次備份距今多久。看的是檔案的修改時間 ——
/// 資料庫裡的紀錄可能因為還原而回到過去，但檔案不會騙人。
pub fn since_last_backup(ctx: &Ctx) -> Option<Duration> {
    let mut newest: Option<std::time::SystemTime> = None;
    for bucket in [
        BackupBucket::Hourly,
        BackupBucket::Shift,
        BackupBucket::Daily,
    ] {
        let dir = ctx.layout.backups_dir().join(bucket.dir_name());
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) != Some("db") {
                continue;
            }
            if let Ok(t) = entry.metadata().and_then(|m| m.modified()) {
                newest = Some(newest.map_or(t, |n| n.max(t)));
            }
        }
    }
    newest.and_then(|t| t.elapsed().ok())
}

pub fn spawn(ctx: Ctx) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let cfg = settings::load(&ctx.layout.settings_file()).await;
            if cfg.backup.hourly {
                let due = match since_last_backup(&ctx) {
                    Some(age) => age >= HOURLY,
                    // 一份備份都沒有：立刻備一次。第一天就要有東西可以還原。
                    None => true,
                };
                if due {
                    match backup::run_backup(&ctx, BackupBucket::Hourly).await {
                        Ok(r) => {
                            tracing::info!(
                                path = %r.local_path,
                                bytes = r.size_bytes,
                                took_ms = r.took_ms,
                                external = r.external_path.is_some(),
                                "自動備份完成"
                            );
                            if let Some(e) = &r.external_error {
                                // 外接失敗不是致命的，但要說出來 ——
                                // 隨身碟被拔掉是最常見的「備份其實沒在跑」。
                                tracing::warn!("外接備份失敗：{e}");
                            }
                        }
                        // 失敗不能讓迴圈停掉：停掉之後就再也不會備份了，
                        // 而且沒有任何人會知道。
                        Err(e) => tracing::error!(error = %e.message(), "自動備份失敗"),
                    }
                }
            }
            tokio::time::sleep(TICK).await;
        }
    })
}

/// 關班／日結時備一份。這兩個時間點是「一段營業的結束」，
/// 也是還原時最想要的還原點。
pub async fn backup_on_close(ctx: &Ctx, bucket: BackupBucket) {
    let cfg = settings::load(&ctx.layout.settings_file()).await;
    if !cfg.backup.on_close {
        return;
    }
    match backup::run_backup(ctx, bucket).await {
        Ok(r) => tracing::info!(path = %r.local_path, "關帳備份完成"),
        // 備份失敗**不能讓關班失敗** —— 班一定要關得掉，
        // 不然店員會卡在一個關不了的畫面前面，而錢已經數完了。
        Err(e) => tracing::error!(error = %e.message(), "關帳備份失敗（不影響關班）"),
    }
}
