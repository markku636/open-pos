//! 把列印佇列送出去的背景工作。
//!
//! # 每台機器一條通道，不是一個全域池
//!
//! 飲料吧那台掛掉時，它會一路重試到退避上限。如果所有工作共用一個池，
//! 這些重試會把收銀台的收據卡在後面 —— 而客人正站在櫃檯前面等著拿發票。
//! 所以按印表機分組，各組之間互不影響。
//!
//! # 交易與外部 I/O 嚴格分開
//!
//! 「認領」與「回報結果」各自是一個短交易，中間那段送位元組的時間**不持有
//! 任何資料庫連線**。寫入池只有一條連線，而一台缺紙的機器可以卡住 10 秒 ——
//! 抓著連線去送資料等於讓全店的寫入排隊。

use std::collections::BTreeMap;
use std::time::Duration;

use sqlx::Row;

use crate::core::clock::Stamp;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::infra::printer::queue::{seed_of, Decision, RetryPolicy};
use crate::infra::printer::{deadline, Active, PrinterDriver, RetryClass};
use crate::receipt::ReceiptDoc;
use crate::services::printer;

/// 一次 tick 做完之後的結果，方便測試與診斷。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickReport {
    /// 從 outbox 展開出來的新工作數。
    pub fanned_out: usize,
    pub printed: usize,
    pub failed: usize,
    pub dead: usize,
}

/// 跑一輪：展開意圖 → 送出到期的工作。
///
/// 這個函式是**可重入且冪等**的：呼叫兩次不會印兩次（第二次認領不到任何工作）。
/// 測試因此可以直接呼叫它，不必等背景迴圈。
pub async fn tick(ctx: &Ctx) -> AppResult<TickReport> {
    let mut report = TickReport {
        fanned_out: printer::fan_out(ctx).await?,
        ..Default::default()
    };

    let now = Stamp::now();
    let due = sqlx::query(
        "SELECT j.id, j.printer_id, j.doc, j.attempts, j.created_at,
                p.name AS printer_name, p.transport, p.caps
           FROM print_jobs j
           JOIN printers p ON p.id = j.printer_id
          WHERE j.status = 'pending' AND j.next_attempt_at <= ?1
            AND p.deleted_at IS NULL AND p.is_active = 1
          ORDER BY j.priority, j.created_at, j.id
          LIMIT 200",
    )
    .bind(now.iso())
    .fetch_all(ctx.db.reader())
    .await?;
    if due.is_empty() {
        return Ok(report);
    }

    // 按印表機分組。BTreeMap 讓順序可重現。
    let mut lanes: BTreeMap<String, Vec<Job>> = BTreeMap::new();
    for r in &due {
        lanes
            .entry(r.get("printer_id"))
            .or_default()
            .push(Job::from_row(r));
    }

    // 各通道之間並行、通道內序列。一台機器同時只吃一份資料 ——
    // 9100 通常只接受一條連線，並行送過去只會互相踢掉。
    //
    // 並行是這裡的重點：飲料吧那台掛掉時會一路撞 10 秒的 timeout，
    // 序列跑的話櫃檯的收據就要排在它後面，而客人正站在那裡等發票。
    let mut set = tokio::task::JoinSet::new();
    for jobs in lanes.into_values() {
        let ctx = ctx.clone();
        set.spawn(async move { run_lane(&ctx, jobs).await });
    }
    while let Some(joined) = set.join_next().await {
        // 通道自己 panic 不該讓整輪失敗 —— 其他機器的單還是要印出去。
        let lane = match joined {
            Ok(r) => r?,
            Err(e) => {
                tracing::error!(error = %e, "某一台印表機的通道異常結束");
                continue;
            }
        };
        report.printed += lane.0;
        report.failed += lane.1;
        report.dead += lane.2;
    }
    Ok(report)
}

struct Job {
    id: String,
    printer_name: String,
    transport: String,
    caps: String,
    doc: String,
    attempts: i64,
    created_at: String,
}

impl Job {
    fn from_row(r: &sqlx::sqlite::SqliteRow) -> Self {
        Self {
            id: r.get("id"),
            printer_name: r.get("printer_name"),
            transport: r.get("transport"),
            caps: r.get("caps"),
            doc: r.get("doc"),
            attempts: r.get("attempts"),
            created_at: r.get("created_at"),
        }
    }
}

/// 一台機器的通道。回傳 (印出, 失敗待重試, 進死信)。
async fn run_lane(ctx: &Ctx, jobs: Vec<Job>) -> AppResult<(usize, usize, usize)> {
    let Some(first) = jobs.first() else {
        return Ok((0, 0, 0));
    };

    // driver 一台開一次，整條通道共用 —— 每張單重開連線在同一批裡沒有意義，
    // 而 `NetworkPrinter` 本來就是每次 send 自己開一條新連線。
    let driver = match open(&first.transport, &first.caps) {
        Ok(d) => d,
        Err(e) => {
            // 設定壞掉：整條通道的單一次全部標成 permanent，
            // 不要讓它們慢慢重試 20 次才發現是同一個原因。
            let mut dead = 0;
            for job in &jobs {
                finish(ctx, job, Err(&e)).await?;
                dead += 1;
            }
            return Ok((0, 0, dead));
        }
    };
    let caps = driver.caps();

    let (mut ok, mut failed, mut dead) = (0usize, 0usize, 0usize);
    for job in &jobs {
        if !claim(ctx, &job.id).await? {
            continue; // 別人先拿走了（或已被取消）
        }

        let result = match render(&job.doc, &caps) {
            // ★ 送資料的這段時間不持有任何資料庫連線。
            Ok(bytes) => driver.send(&bytes, deadline::SEND_TEXT).await,
            Err(e) => Err(e),
        };

        match &result {
            Ok(()) => ok += 1,
            Err(e) => match classify(ctx, job, e).await? {
                Outcome::Retry => failed += 1,
                Outcome::Dead => dead += 1,
            },
        }
        if result.is_ok() {
            finish(ctx, job, Ok(())).await?;
        }

        // 一台機器已經連不上時，不必把同一批剩下的單全部再試一次 ——
        // 那只是把同一個 10 秒 timeout 乘上 20 遍。
        if matches!(&result, Err(e) if RetryClass::of(e) == RetryClass::Transient) {
            break;
        }
    }
    driver.close().await;
    Ok((ok, failed, dead))
}

fn open(transport: &str, caps: &str) -> AppResult<Active> {
    let transport = serde_json::from_str(transport)
        .map_err(|e| AppError::Validation(format!("印表機的連線設定讀不懂：{e}").into()))?;
    let caps = serde_json::from_str(caps)
        .map_err(|e| AppError::Validation(format!("印表機的能力設定讀不懂：{e}").into()))?;
    Active::open(&transport, caps)
}

fn render(doc_json: &str, caps: &crate::infra::printer::PrinterCaps) -> AppResult<Vec<u8>> {
    let doc: ReceiptDoc = serde_json::from_str(doc_json)
        // 版面壞掉是永久錯誤：重試一萬次也一樣。
        .map_err(|e| AppError::Validation(format!("這張單的版面讀不懂：{e}").into()))?;
    let out =
        crate::infra::printer::escpos::EscPosTextRenderer::new(caps.paper.cols(), caps.encoding)
            .encode(&doc);
    if !out.missing.is_empty() {
        // 缺字**不擋列印**（一份印得出來的單比完美的單有用），但要留下紀錄，
        // 讓店家知道該把這台機器改成點陣圖模式。
        tracing::warn!(
            missing = ?out.missing,
            "這台機器的字庫印不出某些字，建議改用點陣圖模式"
        );
    }
    Ok(out.bytes)
}

enum Outcome {
    Retry,
    Dead,
}

async fn claim(ctx: &Ctx, id: &str) -> AppResult<bool> {
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    let n = sqlx::query(
        "UPDATE print_jobs SET status = 'printing', attempts = attempts + 1, updated_at = ?2
          WHERE id = ?1 AND status = 'pending'",
    )
    .bind(id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    uow.commit().await?;
    Ok(n == 1)
}

async fn classify(ctx: &Ctx, job: &Job, err: &AppError) -> AppResult<Outcome> {
    let class = RetryClass::of(err);
    let policy = RetryPolicy::default();
    let age = age_of(&job.created_at);
    let decision = policy.decide(class, job.attempts as u32 + 1, age, seed_of(&job.id));

    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    match &decision {
        Decision::Retry(delay) => {
            let next = now.at + chrono::Duration::from_std(*delay).unwrap_or_default();
            sqlx::query(
                "UPDATE print_jobs SET status = 'pending', next_attempt_at = ?2,
                                       last_error = ?3, last_error_class = ?4, updated_at = ?5
                  WHERE id = ?1",
            )
            .bind(&job.id)
            .bind(crate::core::clock::to_iso(next))
            .bind(err.message())
            .bind(class_str(class))
            .bind(now.iso())
            .execute(uow.conn())
            .await?;
        }
        Decision::DeadLetter(reason) => {
            // 死信必須發出聲音。POS 最常見的客訴是「廚房沒收到單」，
            // 技術根因幾乎都是「系統知道印失敗了但沒告訴任何人」。
            let message = reason.message(&job.printer_name);
            tracing::error!(
                printer = %job.printer_name,
                job = %job.id,
                "{message}"
            );
            sqlx::query(
                "UPDATE print_jobs SET status = 'dead', last_error = ?2, last_error_class = ?3,
                                       updated_at = ?4, done_at = ?4
                  WHERE id = ?1",
            )
            .bind(&job.id)
            .bind(&message)
            .bind(class_str(class))
            .bind(now.iso())
            .execute(uow.conn())
            .await?;
        }
    }
    uow.commit().await?;

    Ok(match decision {
        Decision::Retry(_) => Outcome::Retry,
        Decision::DeadLetter(_) => Outcome::Dead,
    })
}

async fn finish(ctx: &Ctx, job: &Job, result: Result<(), &AppError>) -> AppResult<()> {
    match result {
        Ok(()) => {
            let now = Stamp::now();
            let mut uow = ctx.db.begin_write().await?;
            sqlx::query(
                "UPDATE print_jobs SET status = 'done', last_error = NULL,
                                       updated_at = ?2, done_at = ?2 WHERE id = ?1",
            )
            .bind(&job.id)
            .bind(now.iso())
            .execute(uow.conn())
            .await?;
            uow.commit().await?;
            Ok(())
        }
        Err(e) => {
            classify(ctx, job, e).await?;
            Ok(())
        }
    }
}

fn class_str(c: RetryClass) -> &'static str {
    match c {
        RetryClass::Transient => "transient",
        RetryClass::NeedsAttention => "needs_attention",
        RetryClass::Permanent => "permanent",
    }
}

fn age_of(created_at: &str) -> Duration {
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return Duration::ZERO;
    };
    (chrono::Utc::now() - created.with_timezone(&chrono::Utc))
        .to_std()
        .unwrap_or(Duration::ZERO)
}

/// 背景迴圈。
///
/// 每兩秒醒一次。這個間隔是「客人按下結帳到廚房出單」的體感延遲，
/// 而不是系統負擔 —— 兩秒的空轉查詢在 SQLite 上是微秒級的。
pub fn spawn(ctx: Ctx) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match tick(&ctx).await {
                Ok(r) if r.printed > 0 || r.dead > 0 => {
                    tracing::info!(printed = r.printed, dead = r.dead, "列印佇列");
                }
                Ok(_) => {}
                // 佇列自己壞掉不該讓迴圈停掉 —— 停掉之後就再也沒有單會被印出來，
                // 而且沒有任何人會知道。
                Err(e) => tracing::error!(error = %e, "列印佇列這一輪失敗"),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}
