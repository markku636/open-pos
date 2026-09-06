//! 出單機設定與列印佇列。
//!
//! # 為什麼是「兩跳」：outbox → print_jobs
//!
//! 交易裡寫的是 **outbox**：一筆「這張單該印出來」的意圖，只知道分區、
//! 不知道印表機。真正的展開（分區 → 哪幾台機器）發生在交易外的 `fan_out`。
//!
//! 拆成兩跳不是為了漂亮，是因為**大多數安裝在第一天沒有設定任何印表機**：
//!
//! * 如果在交易裡就決定印表機，沒設定的店家等於整批單直接消失，
//!   而且事後補設定也救不回來。
//! * 意圖留在 outbox 裡，畫面就能誠實地說「有 3 張單等著印，但還沒設定印表機」，
//!   設定完成之後它們會自己流出去（除非已經超過 30 分鐘 —— 那時印出來也沒意義了）。
//!
//! # 這一層的指令不掛在區網上
//!
//! 改印表機設定、看失敗的單、補印，全部只走 Tauri IPC。就算區網服務有漏洞，
//! 攻擊面也只到「亂送單」，到不了「改設定」。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::infra::printer::escpos::CjkEncoding;
use crate::infra::printer::routing::{self, Binding, BindingMode, Fallback, RoutableLine, Station};
use crate::infra::printer::{deadline, Active, PrinterCaps, PrinterDriver, Transport};
use crate::receipt::templates::TicketReason;
use crate::receipt::{PaperWidth, ReceiptDoc};
use crate::services::rbac;

const PERM_PRINTER: &str = "settings.printer";
const PERM_REPRINT: &str = "print.receipt.reprint";

// ---------------------------------------------------------------- 型別

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterView {
    pub id: String,
    pub name: String,
    pub transport: Transport,
    pub caps: PrinterCaps,
    /// raster（點陣圖，換任何機一模一樣）或 text（依賴機器內建字庫，快但可能缺字）。
    pub render_mode: String,
    pub is_active: bool,
    pub last_probe_at: Option<String>,
    pub last_probe_ok: Option<bool>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterInput {
    /// None = 新增。
    pub id: Option<String>,
    pub name: String,
    pub transport: Transport,
    pub paper: PaperWidth,
    pub encoding: Option<CjkEncoding>,
    pub cutter: Option<bool>,
    pub drawer: Option<bool>,
    pub status_query: Option<bool>,
    pub render_mode: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub ok: bool,
    /// 給店員看的一句話。失敗時必須說出下一步該做什麼。
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationView {
    pub id: String,
    pub name: String,
    pub template: String,
    pub split_per_item: bool,
    pub sort_order: i64,
    pub is_active: bool,
    pub printers: Vec<StationPrinter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationPrinter {
    pub printer_id: String,
    pub priority: i64,
    pub mode: BindingMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationInput {
    pub id: Option<String>,
    pub name: String,
    pub template: Option<String>,
    pub split_per_item: Option<bool>,
    pub sort_order: Option<i64>,
    pub is_active: Option<bool>,
    /// None = 不動綁定。Some = 整組取代。
    pub printers: Option<Vec<StationPrinter>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintJobView {
    pub id: String,
    pub printer_id: String,
    pub printer_name: String,
    pub station_name: Option<String>,
    pub order_id: Option<String>,
    pub doc_type: String,
    pub reason: String,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub last_error_class: Option<String>,
    pub created_at: String,
    pub done_at: Option<String>,
}

/// 佇列的一句話摘要，給常駐在畫面上的徽章用。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintQueueStatus {
    pub pending: i64,
    pub dead: i64,
    /// 有意圖但還展不開（通常是還沒設定任何印表機）。
    pub unrouted: i64,
    /// 有沒有需要人去處理的事。true 時 UI 要亮紅點。
    pub needs_attention: bool,
    pub detail: String,
}

// ---------------------------------------------------------------- 讀：設定

pub async fn list_printers(ctx: &Ctx) -> AppResult<Vec<PrinterView>> {
    let rows = sqlx::query(
        "SELECT id, name, transport, caps, render_mode, is_active,
                last_probe_at, last_probe_ok, last_error
           FROM printers WHERE deleted_at IS NULL ORDER BY name",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    rows.iter().map(row_to_printer).collect()
}

fn row_to_printer(r: &sqlx::sqlite::SqliteRow) -> AppResult<PrinterView> {
    let transport: String = r.get("transport");
    let caps: String = r.get("caps");
    Ok(PrinterView {
        id: r.get("id"),
        name: r.get("name"),
        // 設定壞掉時不要讓整頁載不出來 —— 那會讓店家連「刪掉它」都做不到。
        transport: serde_json::from_str(&transport).map_err(|e| {
            AppError::Storage(format!(
                "印表機「{}」的連線設定讀不懂：{e}",
                r.get::<String, _>("name")
            ))
        })?,
        caps: serde_json::from_str(&caps)
            .unwrap_or_else(|_| PrinterCaps::conservative(PaperWidth::Mm80)),
        render_mode: r.get("render_mode"),
        is_active: r.get::<i64, _>("is_active") == 1,
        last_probe_at: r.get("last_probe_at"),
        last_probe_ok: r.get::<Option<i64>, _>("last_probe_ok").map(|v| v == 1),
        last_error: r.get("last_error"),
    })
}

pub async fn list_stations(ctx: &Ctx) -> AppResult<Vec<StationView>> {
    let rows = sqlx::query(
        "SELECT id, name, template, split_per_item, sort_order, is_active
           FROM print_stations WHERE deleted_at IS NULL ORDER BY sort_order, name",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let binds = sqlx::query(
        "SELECT station_id, printer_id, priority, mode FROM station_printers
          ORDER BY station_id, priority, printer_id",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let mut by_station: std::collections::HashMap<String, Vec<StationPrinter>> = Default::default();
    for b in &binds {
        by_station
            .entry(b.get("station_id"))
            .or_default()
            .push(StationPrinter {
                printer_id: b.get("printer_id"),
                priority: b.get("priority"),
                mode: if b.get::<String, _>("mode") == "always" {
                    BindingMode::Always
                } else {
                    BindingMode::Failover
                },
            });
    }

    Ok(rows
        .iter()
        .map(|r| {
            let id: String = r.get("id");
            StationView {
                printers: by_station.remove(&id).unwrap_or_default(),
                id,
                name: r.get("name"),
                template: r.get("template"),
                split_per_item: r.get::<i64, _>("split_per_item") == 1,
                sort_order: r.get("sort_order"),
                is_active: r.get::<i64, _>("is_active") == 1,
            }
        })
        .collect())
}

// ---------------------------------------------------------------- 寫：設定

pub async fn upsert_printer(ctx: &Ctx, input: PrinterInput) -> AppResult<PrinterView> {
    rbac::require(&ctx.db, &ctx.actor, PERM_PRINTER).await?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("印表機名稱不能空白".into()));
    }
    // 裝機第一天的頭號故障是位址打錯，而症狀是「按了結帳沒反應」。
    // 在設定頁就攔下來，比讓它變成一張進死信的單好得多。
    if let Transport::Network { host, .. } = &input.transport {
        if let Some(hint) = crate::infra::printer::network::looks_like_a_typo(host) {
            return Err(AppError::Validation(format!("印表機位址有問題：{hint}")));
        }
    }

    let caps = PrinterCaps {
        paper: input.paper,
        raster: true,
        cutter: input.cutter.unwrap_or(true),
        drawer: input.drawer.unwrap_or(false),
        status_query: input.status_query.unwrap_or(false),
        encoding: input.encoding.unwrap_or_default(),
    };
    let render_mode = input.render_mode.unwrap_or_else(|| "text".into());
    if render_mode != "raster" && render_mode != "text" {
        return Err(AppError::Validation("列印模式只能是 raster 或 text".into()));
    }

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());
    let transport_json = serde_json::to_string(&input.transport)
        .map_err(|e| AppError::Internal(format!("連線設定序列化失敗：{e}")))?;
    let caps_json = serde_json::to_string(&caps)
        .map_err(|e| AppError::Internal(format!("能力設定序列化失敗：{e}")))?;

    let mut uow = ctx.db.begin_write().await?;
    if input.id.is_none() {
        sqlx::query(
            "INSERT INTO printers (id, store_id, name, transport, caps, render_mode, is_active,
                                   created_at, updated_at)
             SELECT ?1, s.id, ?2, ?3, ?4, ?5, ?6, ?7, ?7 FROM stores s ORDER BY s.id LIMIT 1",
        )
        .bind(&id)
        .bind(name)
        .bind(&transport_json)
        .bind(&caps_json)
        .bind(&render_mode)
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE printers SET name = ?2, transport = ?3, caps = ?4, render_mode = ?5,
                                 is_active = ?6, updated_at = ?7
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(name)
        .bind(&transport_json)
        .bind(&caps_json)
        .bind(&render_mode)
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到印表機 {id}")));
        }
    }
    uow.commit().await?;

    list_printers(ctx)
        .await?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::Internal("印表機存好了卻讀不回來".into()))
}

pub async fn delete_printer(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_PRINTER).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    // 軟刪除：歷史的 print_jobs 還指著它，硬刪會讓「昨天那張單印到哪台」查不出來。
    let n = sqlx::query(
        "UPDATE printers SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到印表機 {id}")));
    }
    sqlx::query("DELETE FROM station_printers WHERE printer_id = ?1")
        .bind(&id)
        .execute(uow.conn())
        .await?;
    uow.commit().await?;
    Ok(())
}

pub async fn upsert_station(ctx: &Ctx, input: StationInput) -> AppResult<StationView> {
    rbac::require(&ctx.db, &ctx.actor, PERM_PRINTER).await?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("分區名稱不能空白".into()));
    }
    let template = input.template.unwrap_or_else(|| "kitchen".into());
    if !["kitchen", "drink", "receipt", "label"].contains(&template.as_str()) {
        return Err(AppError::Validation(format!("不認得的單別：{template}")));
    }

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());

    let mut uow = ctx.db.begin_write().await?;
    if input.id.is_none() {
        sqlx::query(
            "INSERT INTO print_stations (id, store_id, name, template, split_per_item,
                                         sort_order, is_active, created_at, updated_at)
             SELECT ?1, s.id, ?2, ?3, ?4, ?5, ?6, ?7, ?7 FROM stores s ORDER BY s.id LIMIT 1",
        )
        .bind(&id)
        .bind(name)
        .bind(&template)
        .bind(i64::from(input.split_per_item.unwrap_or(false)))
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE print_stations SET name = ?2, template = ?3, split_per_item = ?4,
                                       sort_order = ?5, is_active = ?6, updated_at = ?7
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(name)
        .bind(&template)
        .bind(i64::from(input.split_per_item.unwrap_or(false)))
        .bind(input.sort_order.unwrap_or(0))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到出單分區 {id}")));
        }
    }

    if let Some(printers) = &input.printers {
        sqlx::query("DELETE FROM station_printers WHERE station_id = ?1")
            .bind(&id)
            .execute(uow.conn())
            .await?;
        for p in printers {
            sqlx::query(
                "INSERT INTO station_printers (station_id, printer_id, priority, mode, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(&id)
            .bind(&p.printer_id)
            .bind(p.priority)
            .bind(match p.mode {
                BindingMode::Always => "always",
                BindingMode::Failover => "failover",
            })
            .bind(now.iso())
            .execute(uow.conn())
            .await?;
        }
    }
    uow.commit().await?;

    list_stations(ctx)
        .await?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| AppError::Internal("分區存好了卻讀不回來".into()))
}

pub async fn delete_station(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_PRINTER).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    let n = sqlx::query(
        "UPDATE print_stations SET deleted_at = ?2, updated_at = ?2
          WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到出單分區 {id}")));
    }
    sqlx::query("DELETE FROM station_printers WHERE station_id = ?1")
        .bind(&id)
        .execute(uow.conn())
        .await?;
    // 品項與分類上的指向留著不動：分區可能是誤刪，救回來時設定還在。
    // 路由那一層本來就會把「指向不存在的分區」當作沒設，掉到 fallback。
    uow.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------- 探測與測試列印

/// 店家的時區。讀不到就用台北 —— 這個專案的使用者在台灣，
/// 而「猜錯時區」的代價（單上的時間差 8 小時）遠大於「猜」的代價。
async fn store_timezone(ctx: &Ctx) -> chrono_tz::Tz {
    sqlx::query_scalar::<_, String>(
        "SELECT tz FROM stores WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_optional(ctx.db.reader())
    .await
    .ok()
    .flatten()
    .and_then(|tz| tz.parse().ok())
    .unwrap_or(chrono_tz::Asia::Taipei)
}

async fn open_driver(ctx: &Ctx, printer_id: &str) -> AppResult<(String, Active)> {
    let p = list_printers(ctx)
        .await?
        .into_iter()
        .find(|p| p.id == printer_id)
        .ok_or_else(|| AppError::NotFound(format!("找不到印表機 {printer_id}")))?;
    let driver = Active::open(&p.transport, p.caps)?;
    Ok((p.name, driver))
}

/// 測試連線。**刻意不送任何位元組** —— 使用者按這顆按鈕時客人可能就在旁邊，
/// 不該吐出一張空白紙。
pub async fn probe_printer(ctx: &Ctx, id: String) -> AppResult<ProbeResult> {
    rbac::require(&ctx.db, &ctx.actor, PERM_PRINTER).await?;
    let (name, driver) = open_driver(ctx, &id).await?;
    let result = driver.probe().await;
    driver.close().await;

    let (ok, detail) = match &result {
        Ok(()) => (true, format!("{name} 連得上")),
        Err(e) => (false, e.message()),
    };

    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    sqlx::query(
        "UPDATE printers SET last_probe_at = ?2, last_probe_ok = ?3, last_error = ?4,
                             updated_at = ?2 WHERE id = ?1",
    )
    .bind(&id)
    .bind(now.iso())
    .bind(i64::from(ok))
    .bind(if ok { None } else { Some(detail.clone()) })
    .execute(uow.conn())
    .await?;
    uow.commit().await?;

    Ok(ProbeResult { ok, detail })
}

/// 測試列印。**不進佇列，直接送。**
///
/// 使用者正站在機器前面等著看紙出來 —— 把它排進佇列再等 worker 醒來，
/// 會讓人以為壞掉又按一次，然後印出三張。
pub async fn test_print(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_PRINTER).await?;
    let (name, driver) = open_driver(ctx, &id).await?;
    let caps = driver.caps();
    let now = Stamp::now();
    let tz = store_timezone(ctx).await;

    let doc = ReceiptDoc::new(caps.paper)
        .banner("測試列印", true)
        .rule()
        .text(format!("印表機：{name}"))
        .text(format!(
            "時間：{}",
            crate::core::clock::for_humans(now.at, tz)
        ))
        .text(format!("紙寬：{} 欄", caps.paper.cols()))
        .rule()
        // 這一行是給人看的驗收基準：中文有沒有變成問號、數字欄有沒有對齊。
        .text("繁體中文測試：珍珠奶茶 滷肉飯 燙青菜")
        .text("ABCDEFGHIJ 0123456789")
        .rule()
        .text("看得懂這一張就代表設定正確。");

    let bytes =
        crate::infra::printer::escpos::EscPosTextRenderer::new(caps.paper.cols(), caps.encoding)
            .encode(&doc);

    let result = driver.send(&bytes.bytes, deadline::SEND_TEXT).await;
    driver.close().await;
    result
}

// ---------------------------------------------------------------- 佇列

/// 把 outbox 裡的列印意圖展開成一張張指定機器的工作。
///
/// 展不開（還沒設定印表機、分區沒綁機器）時**不動那一列** ——
/// 它留在 pending，設定完成之後會自己流出去。
pub async fn fan_out(ctx: &Ctx) -> AppResult<usize> {
    let pending = sqlx::query(
        "SELECT id, kind, payload_json, business_date, created_at
           FROM outbox
          WHERE status = 'pending' AND kind LIKE 'print.%'
          ORDER BY created_at, id
          LIMIT 200",
    )
    .fetch_all(ctx.db.reader())
    .await?;
    if pending.is_empty() {
        return Ok(0);
    }

    let stations: Vec<Station> = list_stations(ctx)
        .await?
        .into_iter()
        .filter(|s| s.is_active)
        .map(|s| Station {
            id: s.id,
            name: s.name,
            template: s.template,
            split_per_item: s.split_per_item,
            sort_order: s.sort_order,
        })
        .collect();
    let bindings: Vec<Binding> = list_stations(ctx)
        .await?
        .into_iter()
        .flat_map(|s| {
            s.printers.into_iter().map(move |p| Binding {
                station_id: s.id.clone(),
                printer_id: p.printer_id,
                priority: p.priority,
                mode: p.mode,
            })
        })
        .collect();

    // fallback 是「櫃檯那台」。沒有明確的櫃檯時用第一台啟用中的機器 ——
    // 印到某一台總比消失好，店員看到「代印」就知道設定漏了。
    let printers = list_printers(ctx).await?;
    let fallback_printer = printers.iter().find(|p| p.is_active).map(|p| p.id.clone());

    let now = Stamp::now();
    let mut made = 0usize;

    for row in &pending {
        let outbox_id: String = row.get("id");
        let payload: serde_json::Value =
            serde_json::from_str(&row.get::<String, _>("payload_json")).unwrap_or_default();
        let station_id = payload
            .get("stationId")
            .and_then(|v| v.as_str())
            .map(String::from);
        let doc_json = match payload.get("doc") {
            Some(d) => d.to_string(),
            None => {
                mark_outbox_failed(ctx, &outbox_id, "這一列沒有可印的內容", &now).await?;
                continue;
            }
        };

        let Some(fallback_id) = fallback_printer.clone() else {
            // 還沒有任何印表機。**留在 pending** —— 意圖不能丟，
            // 店家可能十分鐘後才把機器接上。
            continue;
        };

        let jobs = routing::plan_jobs(
            &[RoutableLine {
                line_id: outbox_id.clone(),
                station_id: station_id.clone(),
                category_station_id: None,
            }],
            &stations,
            &bindings,
            reason_of(&payload),
            Fallback::Printer(&fallback_id),
        );
        if jobs.is_empty() {
            continue;
        }

        let kind: String = row.get("kind");
        let doc_type = match kind.as_str() {
            // 退款單走收據那條路：它印在櫃檯那台、優先度跟收據一樣，
            // 而且客人站在旁邊等著簽名。
            "print.receipt" | "print.refund" => "receipt",
            "print.shift_report" => "shift_report",
            "print.day_report" => "day_report",
            _ => "kitchen",
        };
        let business_date: Option<String> = row.get("business_date");
        let order_id = payload.get("orderId").and_then(|v| v.as_str());
        let bill_id = payload.get("billId").and_then(|v| v.as_str());

        let mut uow = ctx.db.begin_write().await?;
        for (seq, job) in jobs.iter().enumerate() {
            sqlx::query(
                "INSERT INTO print_jobs (id, store_id, printer_id, station_id, business_date,
                                         order_id, bill_id, doc_type, reason, doc, doc_sha256,
                                         idempotency_key, priority, status, attempts,
                                         next_attempt_at, created_at, updated_at)
                 SELECT ?1, s.id, ?2, ?3, ?4, ?5, ?13, ?6, ?7, ?8, ?9, ?10, ?11,
                        'pending', 0, ?12, ?12, ?12
                   FROM stores s ORDER BY s.id LIMIT 1
                 ON CONFLICT (idempotency_key) DO NOTHING",
            )
            .bind(Id::new().as_str())
            .bind(&job.printer_id)
            .bind(&job.station_id)
            .bind(
                business_date
                    .clone()
                    .unwrap_or_else(|| now.iso()[..10].to_string()),
            )
            .bind(order_id)
            .bind(doc_type)
            .bind(reason_str(&payload))
            .bind(&doc_json)
            .bind(digest(&doc_json))
            // 冪等鍵防的是「同一列 outbox 被展開兩次」——
            // worker 可能在 crash 之後重跑，而重複的單廚房是看得出來的、
            // 漏掉的單沒有人看得出來。
            .bind(format!("{outbox_id}:{seq}"))
            // 收據與報表優先：客人站在櫃檯等發票，店員站在機器旁邊等交接單。
            // 廚房單雖然更急，但它印到的是另一台機器，不會互相排隊。
            .bind(if doc_type == "kitchen" { 10 } else { 0 })
            .bind(now.iso())
            .bind(bill_id)
            .execute(uow.conn())
            .await?;
            made += 1;
        }
        sqlx::query(
            "UPDATE outbox SET status = 'done', done_at = ?2, updated_at = ?2 WHERE id = ?1",
        )
        .bind(&outbox_id)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
        uow.commit().await?;
    }

    Ok(made)
}

fn reason_of(payload: &serde_json::Value) -> TicketReason {
    match payload.get("reason").and_then(|v| v.as_str()) {
        Some("add_items") => TicketReason::AddItems,
        Some("void") => TicketReason::Void,
        Some("reprint") => TicketReason::Reprint,
        Some("settle") => TicketReason::Settle,
        _ => TicketReason::NewOrder,
    }
}

fn reason_str(payload: &serde_json::Value) -> &'static str {
    match reason_of(payload) {
        TicketReason::NewOrder => "new_order",
        TicketReason::AddItems => "add_items",
        TicketReason::Void => "void",
        TicketReason::Reprint => "reprint",
        TicketReason::Settle => "settle",
    }
}

/// 內容指紋。用途是「這兩張單是不是同一份」，不是安全性。
fn digest(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

async fn mark_outbox_failed(ctx: &Ctx, id: &str, why: &str, now: &Stamp) -> AppResult<()> {
    let mut uow = ctx.db.begin_write().await?;
    sqlx::query(
        "UPDATE outbox SET status = 'dead', last_error = ?2, updated_at = ?3 WHERE id = ?1",
    )
    .bind(id)
    .bind(why)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    uow.commit().await?;
    Ok(())
}

pub async fn list_print_jobs(ctx: &Ctx, limit: Option<i64>) -> AppResult<Vec<PrintJobView>> {
    let rows = sqlx::query(
        "SELECT j.id, j.printer_id, p.name AS printer_name, ps.name AS station_name,
                j.order_id, j.doc_type, j.reason, j.status, j.attempts,
                j.last_error, j.last_error_class, j.created_at, j.done_at
           FROM print_jobs j
           JOIN printers p ON p.id = j.printer_id
           LEFT JOIN print_stations ps ON ps.id = j.station_id
          ORDER BY j.created_at DESC, j.id DESC
          LIMIT ?1",
    )
    .bind(limit.unwrap_or(100).clamp(1, 500))
    .fetch_all(ctx.db.reader())
    .await?;

    Ok(rows
        .iter()
        .map(|r| PrintJobView {
            id: r.get("id"),
            printer_id: r.get("printer_id"),
            printer_name: r.get("printer_name"),
            station_name: r.get("station_name"),
            order_id: r.get("order_id"),
            doc_type: r.get("doc_type"),
            reason: r.get("reason"),
            status: r.get("status"),
            attempts: r.get("attempts"),
            last_error: r.get("last_error"),
            last_error_class: r.get("last_error_class"),
            created_at: r.get("created_at"),
            done_at: r.get("done_at"),
        })
        .collect())
}

/// 佇列摘要。**這是常駐在畫面上的紅點的資料來源。**
///
/// POS 最常見的客訴是「廚房沒收到單」，而技術根因幾乎都是
/// 「系統知道印失敗了但沒告訴任何人」。
pub async fn queue_status(ctx: &Ctx) -> AppResult<PrintQueueStatus> {
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM print_jobs WHERE status IN ('pending', 'printing')",
    )
    .fetch_one(ctx.db.reader())
    .await?;
    let dead: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM print_jobs WHERE status = 'dead'")
        .fetch_one(ctx.db.reader())
        .await?;
    let unrouted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM outbox WHERE status = 'pending' AND kind LIKE 'print.%'",
    )
    .fetch_one(ctx.db.reader())
    .await?;

    let printers: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM printers WHERE deleted_at IS NULL AND is_active = 1",
    )
    .fetch_one(ctx.db.reader())
    .await?;

    let detail = if dead > 0 {
        format!("有 {dead} 張單印不出來 —— 請到出單機設定看原因")
    } else if unrouted > 0 && printers == 0 {
        format!("有 {unrouted} 張單等著印，但還沒有設定任何出單機")
    } else if pending > 0 {
        format!("{pending} 張單排隊中")
    } else {
        "出單正常".into()
    };

    Ok(PrintQueueStatus {
        needs_attention: dead > 0 || (unrouted > 0 && printers == 0),
        pending,
        dead,
        unrouted,
        detail,
    })
}

/// 補印收據。
///
/// # 為什麼重送的是快照，而不是重新排版
///
/// 分帳之後一張訂單會有好幾張帳單（$150 分四份就是四張），而客人手上是其中
/// **一份** —— 從訂單重建根本重建不出來。快照同時也更誠實：補印本來就該是
/// 「再給你一張一模一樣的」，而不是一張反映了之後所有改動的新單。
///
/// # 補印一定要看得出來
///
/// 兩張一樣的收據可以拿去做假帳，所以單上會印「※ 補印 第 N 次 ※」，
/// 而且每一次都寫進稽核紀錄。次數是從稽核紀錄數出來的 —— 那是唯一一份
/// 不會被前端漏掉的計數。
pub async fn reprint_receipt(ctx: &Ctx, bill_id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_REPRINT).await?;
    let now = Stamp::now();

    let row = sqlx::query(
        "SELECT receipt_doc, business_date, bill_no, order_id FROM bills WHERE id = ?1",
    )
    .bind(&bill_id)
    .fetch_optional(ctx.db.reader())
    .await?
    .ok_or_else(|| AppError::NotFound("找不到這張帳單".into()))?;

    let doc_json: Option<String> = row.get("receipt_doc");
    let doc_json = doc_json.ok_or_else(|| {
        AppError::NotFound("這張帳單沒有留下收據原稿（升級之前結的帳），補不了。".into())
    })?;
    let business_date: String = row.get("business_date");
    let bill_no: String = row.get("bill_no");
    let order_id: Option<String> = row.get("order_id");

    let seq: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE entity_type = 'Bill' AND entity_id = ?1
            AND action = 'reprint'",
    )
    .bind(&bill_id)
    .fetch_one(ctx.db.reader())
    .await?;

    let mut doc: crate::receipt::ReceiptDoc = serde_json::from_str(&doc_json)
        .map_err(|e| AppError::Internal(format!("收據原稿讀不回來：{e}")))?;
    mark_reprint(&mut doc, seq + 1);

    let mut uow = ctx.db.begin_write().await?;
    let payload = serde_json::json!({
        "orderId": order_id,
        "billId": bill_id,
        "billNo": bill_no,
        "doc": doc,
        "reason": "reprint",
    });
    crate::services::order::enqueue_print(
        &mut uow,
        "print.receipt",
        &business_date,
        &payload,
        &now,
    )
    .await?;

    crate::services::audit::write_in(
        &mut uow,
        crate::services::audit::AuditEntry::new(
            "Bill",
            &bill_id,
            crate::services::audit::AuditAction::Reprint,
        )
        .to(&bill_no)
        .on(&business_date),
        &ctx.actor,
        &now,
    )
    .await?;
    uow.commit().await?;
    Ok(())
}

/// 在收據最上面加一行「※ 補印 第 N 次 ※」。
///
/// 加在最前面而不是原本版型裡的位置，是因為快照已經排好了 ——
/// 而放在最上面反而更難忽略，那正是這一行存在的理由。
fn mark_reprint(doc: &mut crate::receipt::ReceiptDoc, seq: i64) {
    use crate::receipt::{Block, TextStyle};
    doc.blocks.insert(
        0,
        Block::Text {
            content: format!("※ 補印 第 {seq} 次 ※"),
            style: TextStyle::centered(),
        },
    );
    // 補印不開錢箱：沒有人在付錢。
    doc.finish.open_drawer = false;
}

/// 把一張死掉的單放回佇列。
pub async fn retry_print_job(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_REPRINT).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    let n = sqlx::query(
        "UPDATE print_jobs
            SET status = 'pending', attempts = 0, next_attempt_at = ?2,
                last_error = NULL, last_error_class = NULL, updated_at = ?2
          WHERE id = ?1 AND status IN ('dead', 'failed', 'cancelled')",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(
            "找不到這張單，或它不在可以重試的狀態".into(),
        ));
    }
    uow.commit().await?;
    Ok(())
}

pub async fn cancel_print_job(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_REPRINT).await?;
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;
    sqlx::query(
        "UPDATE print_jobs SET status = 'cancelled', updated_at = ?2, done_at = ?2
          WHERE id = ?1 AND status IN ('pending', 'failed', 'dead')",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    uow.commit().await?;
    Ok(())
}
