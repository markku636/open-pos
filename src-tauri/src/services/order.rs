//! 點餐與結帳。
//!
//! # 交易邊界
//!
//! 每一個 `pub async fn` 就是一個 use case，也就是一個 `UnitOfWork`。
//! 交易內**只寫資料庫**：要印的單寫進 `outbox`，由背景 worker 去碰印表機。
//! 寫入池只有一條連線，一個卡住的 TCP 連線會讓全店的寫入排隊。
//!
//! # 每次都重算整張單
//!
//! 加一個品項時不是「算這一行然後加進去」，而是把整張單重新丟給定價引擎。
//! 慢一點點，但換來一個很重要的性質：**畫面上的金額與資料庫裡的金額
//! 永遠由同一段程式碼、同一份輸入產生**。增量更新在折扣與服務費分攤的
//! 場景下幾乎必然會漂。
//!
//! # 冪等
//!
//! 所有寫入都吃一個由前端產生的 `client_id` / `idem_key`。
//! 這不是為了「主機掛掉」，是為了每天都在發生的 Wi-Fi 抖動：
//! 廚房的不鏽鋼、2.4GHz 干擾、平板省電 —— 重送是常態，重複收款不能是。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::business_date::{BusinessDate, BusinessDayConfig};
use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::core::money::{Money, RoundingPolicy};
use crate::core::pricing::{self, Channel, LineInput, PricingInput, PricingOutput, QTY_SCALE};
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::infra::db::sqlite::SqliteUow;
use crate::receipt::templates::{self, TicketData, TicketLine, TicketReason};
use crate::receipt::PaperWidth;
use crate::services::audit::{self, AuditAction, AuditEntry};
use crate::services::sequence::{self, Scope};

// ---------------------------------------------------------------- DTO

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewLine {
    pub item_id: String,
    pub variant_id: Option<String>,
    /// 數量 × 1000。省略時視為一份。
    pub qty_milli: Option<i64>,
    #[serde(default)]
    pub modifier_ids: Vec<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOrderReq {
    #[serde(default)]
    pub channel: Channel,
    pub table_id: Option<String>,
    pub guest_count: Option<i64>,
    /// 送單端產生的 ULID。重送時回同一張單而不是開第二張。
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddLinesReq {
    pub order_id: String,
    /// 樂觀鎖。與資料庫不符時回 409，前端重讀後再送。
    pub expected_rev: i64,
    pub lines: Vec<NewLine>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentReq {
    pub method_code: String,
    /// 沖銷金額（整數元）。
    pub amount: i64,
    /// 客人給的錢（只有現金會與 amount 不同）。
    pub tendered: Option<i64>,
    pub ref_no: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleReq {
    pub order_id: String,
    pub expected_rev: i64,
    pub payments: Vec<PaymentReq>,
    pub idem_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderLineView {
    pub id: String,
    pub line_no: i64,
    pub name: String,
    pub variant_name: Option<String>,
    pub options: Vec<String>,
    pub note: Option<String>,
    pub qty_milli: i64,
    pub unit_price: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderView {
    pub id: String,
    pub order_no: String,
    pub status: String,
    /// 樂觀鎖版本。寫入時要帶回來。
    pub rev: i64,
    pub channel: String,
    pub table_id: Option<String>,
    pub table_label: Option<String>,
    pub guest_count: i64,
    pub business_date: String,
    pub lines: Vec<OrderLineView>,
    pub subtotal: i64,
    pub service_charge: i64,
    pub rounding_adjustment: i64,
    pub grand_total: i64,
    pub sales_amount: i64,
    pub tax_amount: i64,
    pub paid_total: i64,
    pub change_total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResult {
    pub order: OrderView,
    pub bill_no: String,
    pub change: i64,
}

// ---------------------------------------------------------------- 店家設定

struct StoreConfig {
    id: String,
    name: String,
    tax_rate_bp: i64,
    service_charge_rate_bp: i64,
    rounding: RoundingPolicy,
    day: BusinessDayConfig,
}

async fn load_store(ctx: &Ctx) -> AppResult<StoreConfig> {
    let r = sqlx::query(
        "SELECT id, name, tax_rate_bp, service_charge_rate_bp, rounding_policy, tz, business_day_cutoff
           FROM stores WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_optional(ctx.db.reader())
    .await?
    .ok_or_else(|| AppError::Internal("找不到店家資料 —— 種子資料可能沒跑完".into()))?;

    let tz: String = r.get("tz");
    let cutoff: String = r.get("business_day_cutoff");
    let rounding: String = r.get("rounding_policy");

    Ok(StoreConfig {
        id: r.get("id"),
        name: r.get("name"),
        tax_rate_bp: r.get("tax_rate_bp"),
        service_charge_rate_bp: r.get("service_charge_rate_bp"),
        rounding: match rounding.as_str() {
            "to_five" => RoundingPolicy::ToFive,
            "floor_five" => RoundingPolicy::FloorFive,
            "floor_ten" => RoundingPolicy::FloorTen,
            _ => RoundingPolicy::None,
        },
        day: BusinessDayConfig {
            tz: tz.parse().unwrap_or(chrono_tz::Asia::Taipei),
            cutoff: cutoff
                .parse::<chrono::NaiveTime>()
                .or_else(|_| chrono::NaiveTime::parse_from_str(&cutoff, "%H:%M"))
                .unwrap_or(BusinessDayConfig::default().cutoff),
        },
    })
}

/// 服務費只對內用收 —— 這是台灣慣例，也是消費爭議的常見來源。
fn service_rate_for(store: &StoreConfig, channel: Channel) -> i64 {
    match channel {
        Channel::DineIn => store.service_charge_rate_bp,
        _ => 0,
    }
}

// ---------------------------------------------------------------- 開單

pub async fn open_order(ctx: &Ctx, req: OpenOrderReq) -> AppResult<OrderView> {
    let store = load_store(ctx).await?;
    let now = Stamp::now();
    let business_date = BusinessDate::of(now.at, store.day.tz, store.day.cutoff);

    // 冪等：同一個 client_id 重送時回既有的那張單，不要開第二張。
    if let Some(cid) = &req.client_id {
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM orders WHERE store_id = ?1 AND client_id = ?2")
                .bind(&store.id)
                .bind(cid)
                .fetch_optional(ctx.db.reader())
                .await?;
        if let Some(id) = existing {
            return get_order(ctx, &id).await;
        }
    }

    let mut uow = ctx.db.begin_write().await?;
    // 日結完成的營業日拒絕任何寫入。少了這道鎖，「日結」只是一個時間戳，
    // 而事後補進來的單會讓已經印出來的 Z 報表對不上。
    crate::services::shift::ensure_day_open(&mut uow, &business_date.to_iso()).await?;
    let order_no = sequence::next_no(
        &mut uow,
        &store.id,
        Scope::Order,
        &business_date.to_iso(),
        &now,
    )
    .await?;

    let order_id = Id::new().to_string();
    let table_session_id = match (&req.table_id, req.channel) {
        (Some(table_id), Channel::DineIn) => Some(
            open_table_session(&mut uow, ctx, table_id, &store, &business_date, &req, &now).await?,
        ),
        _ => None,
    };

    sqlx::query(
        "INSERT INTO orders (id, store_id, business_date, order_no, client_id, rev, channel, source,
                             table_id, table_session_id, guest_count, status, opened_at,
                             created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, 'pos', ?7, ?8, ?9, 'draft', ?10, ?11, ?10, ?10)",
    )
    .bind(&order_id)
    .bind(&store.id)
    .bind(business_date.to_iso())
    .bind(&order_no)
    .bind(&req.client_id)
    .bind(channel_str(req.channel))
    .bind(&req.table_id)
    .bind(&table_session_id)
    .bind(req.guest_count.unwrap_or(1))
    .bind(now.iso())
    .bind(&ctx.actor.user_id)
    .execute(uow.conn())
    .await?;

    write_event(
        &mut uow,
        &order_id,
        1,
        "opened",
        None,
        Some("draft"),
        ctx,
        &now,
    )
    .await?;
    uow.commit().await?;

    get_order(ctx, &order_id).await
}

/// 開桌。
///
/// `table_sessions` 上有一個 partial unique index 保證「一桌同時只有一個未關
/// session」。兩個收銀員同時點同一桌是真實會發生的事（一個在櫃檯、一個拿平板
/// 在外場），所以這裡不先查再寫 —— 那中間就是競態窗口。直接寫，撞到唯一索引
/// 就把它翻譯成看得懂的訊息。
async fn open_table_session(
    uow: &mut SqliteUow,
    ctx: &Ctx,
    table_id: &str,
    store: &StoreConfig,
    business_date: &BusinessDate,
    req: &OpenOrderReq,
    now: &Stamp,
) -> AppResult<String> {
    // 已經開著的話沿用它（同一桌加點）。
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM table_sessions WHERE table_id = ?1 AND status <> 'closed'",
    )
    .bind(table_id)
    .fetch_optional(uow.conn())
    .await?;
    if let Some(id) = existing {
        return Ok(id);
    }

    let session_id = Id::new().to_string();
    let r = sqlx::query(
        "INSERT INTO table_sessions (id, store_id, table_id, business_date, guest_count,
                                     status, opened_at, opened_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?6, ?6)",
    )
    .bind(&session_id)
    .bind(&store.id)
    .bind(table_id)
    .bind(business_date.to_iso())
    .bind(req.guest_count.unwrap_or(1))
    .bind(now.iso())
    .bind(&ctx.actor.user_id)
    .execute(uow.conn())
    .await;

    match r {
        Ok(_) => Ok(session_id),
        Err(e) if is_unique_violation(&e) => Err(AppError::Conflict(
            "這一桌剛剛已經被開檯了，請重新整理後再試一次。".into(),
        )),
        Err(e) => Err(e.into()),
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.message().contains("UNIQUE"))
}

// ---------------------------------------------------------------- 加點

pub async fn add_lines(ctx: &Ctx, req: AddLinesReq) -> AppResult<OrderView> {
    if req.lines.is_empty() {
        return Err(AppError::Validation("沒有要加的品項".into()));
    }
    let store = load_store(ctx).await?;
    let now = Stamp::now();

    let mut uow = ctx.db.begin_write().await?;
    let head = load_order_head(&mut uow, &req.order_id).await?;
    crate::services::shift::ensure_day_open(&mut uow, &head.business_date).await?;
    ensure_open(&head)?;
    ensure_rev(&head, req.expected_rev)?;

    let mut line_no = next_line_no(&mut uow, &req.order_id).await?;
    let mut added: std::collections::HashSet<String> = Default::default();
    for l in &req.lines {
        let resolved = resolve_line(&mut uow, l).await?;
        added.insert(insert_line(&mut uow, &req.order_id, line_no, l, &resolved, &now).await?);
        line_no += 1;
    }

    let totals = recompute(&mut uow, &req.order_id, &store, head.channel, &now).await?;
    let new_status = if head.status == "draft" {
        "placed"
    } else {
        &head.status
    };
    bump_order(&mut uow, &req.order_id, head.rev, new_status, &totals, &now).await?;

    let seq = next_event_seq(&mut uow, &req.order_id).await?;
    write_event(
        &mut uow,
        &req.order_id,
        seq,
        "lines_added",
        Some(&head.status),
        Some(new_status),
        ctx,
        &now,
    )
    .await?;

    // 廚房單進 outbox。交易內不碰印表機 —— 一個卡住的 TCP 連線會讓全店寫入排隊。
    //
    // ★ 加點單**只印這一次新增的行**。
    //   把整張單重印一次，廚師會把已經做好的珍珠奶茶再做一杯 ——
    //   而他不會知道那是重複的，因為單上看起來就是要做兩杯。
    let (reason, only) = if head.status == "draft" {
        (TicketReason::NewOrder, None)
    } else {
        (TicketReason::AddItems, Some(&added))
    };
    enqueue_kitchen_ticket(&mut uow, ctx, &store, &req.order_id, reason, only, &now).await?;

    uow.commit().await?;
    get_order(ctx, &req.order_id).await
}

// ---------------------------------------------------------------- 退點

pub async fn void_line(
    ctx: &Ctx,
    order_id: String,
    expected_rev: i64,
    line_id: String,
    reason_id: Option<String>,
) -> AppResult<OrderView> {
    let store = load_store(ctx).await?;
    let now = Stamp::now();

    let mut uow = ctx.db.begin_write().await?;
    let head = load_order_head(&mut uow, &order_id).await?;
    crate::services::shift::ensure_day_open(&mut uow, &head.business_date).await?;
    ensure_open(&head)?;
    ensure_rev(&head, expected_rev)?;

    let amount: Option<i64> = sqlx::query_scalar(
        "SELECT taxable_amount FROM order_items
          WHERE id = ?1 AND order_id = ?2 AND voided_at IS NULL",
    )
    .bind(&line_id)
    .bind(&order_id)
    .fetch_optional(uow.conn())
    .await?;
    let amount =
        amount.ok_or_else(|| AppError::NotFound("找不到這個品項，或它已經被退掉了".into()))?;

    sqlx::query(
        "UPDATE order_items SET voided_at = ?2, void_reason_id = ?3, void_by = ?4, updated_at = ?2
          WHERE id = ?1",
    )
    .bind(&line_id)
    .bind(now.iso())
    .bind(&reason_id)
    .bind(&ctx.actor.user_id)
    .execute(uow.conn())
    .await?;

    let totals = recompute(&mut uow, &order_id, &store, head.channel, &now).await?;
    bump_order(&mut uow, &order_id, head.rev, &head.status, &totals, &now).await?;

    let mut entry = AuditEntry::new("OrderItem", &line_id, AuditAction::Void)
        .amount(-amount)
        .on(&head.business_date);
    if let Some(r) = &reason_id {
        entry = entry.reason(r);
    }
    audit::write_in(&mut uow, entry, &ctx.actor, &now).await?;

    let seq = next_event_seq(&mut uow, &order_id).await?;
    write_event(
        &mut uow,
        &order_id,
        seq,
        "line_voided",
        None,
        None,
        ctx,
        &now,
    )
    .await?;

    // 退點也要出單 —— 廚房已經在做了，不通知的話那份餐會照樣做出來。
    // 同樣只印被退掉的那一行：一張寫著「取消」又列出整桌菜的單，
    // 廚師會不知道到底要取消哪一項。
    let voided: std::collections::HashSet<String> = std::iter::once(line_id.clone()).collect();
    enqueue_kitchen_ticket(
        &mut uow,
        ctx,
        &store,
        &order_id,
        TicketReason::Void,
        Some(&voided),
        &now,
    )
    .await?;

    uow.commit().await?;
    get_order(ctx, &order_id).await
}

// ---------------------------------------------------------------- 結帳

pub async fn settle(ctx: &Ctx, req: SettleReq) -> AppResult<SettleResult> {
    // 冪等：同一個 idem_key 重送直接回上次的結果，不重複收款。
    if let Some(cached) = load_idempotent::<SettleResult>(ctx, &req.idem_key).await? {
        return Ok(cached);
    }

    let store = load_store(ctx).await?;
    let now = Stamp::now();

    let mut uow = ctx.db.begin_write().await?;
    let head = load_order_head(&mut uow, &req.order_id).await?;
    crate::services::shift::ensure_day_open(&mut uow, &head.business_date).await?;
    ensure_rev(&head, req.expected_rev)?;
    if head.status == "settled" {
        return Err(AppError::Conflict("這張單已經結過帳了".into()));
    }
    if head.status == "voided" {
        return Err(AppError::Conflict("這張單已作廢，不能結帳".into()));
    }

    let totals = recompute(&mut uow, &req.order_id, &store, head.channel, &now).await?;
    let grand_total = totals.grand_total.0;

    // 付款驗證。
    //
    // `amount` 與 `tendered` 是兩件事，混在一起是找零算錯的根源：
    // * `amount`   = 這一筆要沖銷掉帳單多少（刷卡就是刷這麼多）
    // * `tendered` = 客人實際遞出來的錢（只有現金會比 amount 大）
    //
    // 收銀機的兩種操作習慣都要支援：
    //   (a) 輸入「應收 175、客人給 500」→ amount=175, tendered=500
    //   (b) 直接輸入「客人給 500」      → amount=500, tendered 省略
    // 所以實際沖銷的是 `min(amount, 尚欠)`，多出來的部分才是找零。
    if req.payments.is_empty() {
        return Err(AppError::Validation("請至少選一種付款方式".into()));
    }
    let offered: i64 = req
        .payments
        .iter()
        .map(|p| p.amount.max(p.tendered.unwrap_or(0)))
        .sum();
    if offered < grand_total {
        return Err(AppError::Validation(format!(
            "收款金額 {offered} 元不足應收的 {grand_total} 元，還差 {} 元",
            grand_total - offered
        )));
    }

    let bill_no =
        sequence::next_no(&mut uow, &store.id, Scope::Bill, &head.business_date, &now).await?;

    // ★ 每一筆交易都要掛上班別。
    //
    //   這是 M1 的硬前提之一：沒有它，過去的 Z 報表永遠重建不出來 ——
    //   而「哪一班收了多少現金」正是關班盤點唯一能對的東西。
    //   沒開班時是 None（忘了開班不該讓店家收不了錢），那些單在日結時
    //   會落在「無班別」裡，看得到。
    let shift_id = crate::services::shift::current_shift_id(&mut uow).await?;
    let bill_id = Id::new().to_string();

    sqlx::query(
        "INSERT INTO bills (id, order_id, store_id, shift_id, business_date, bill_no, split_mode,
                            split_index, split_count, subtotal, discount_total, service_charge,
                            rounding_adjustment, grand_total, sales_amount, tax_amount,
                            paid_total, change_total, status, settled_at, settled_by,
                            created_at, updated_at)
         VALUES (?1, ?2, ?3, ?17, ?4, ?5, 'none', 1, 1, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 ?13, ?14, 'settled', ?15, ?16, ?15, ?15)",
    )
    .bind(&bill_id)
    .bind(&req.order_id)
    .bind(&store.id)
    .bind(&head.business_date)
    .bind(&bill_no)
    .bind(totals.subtotal.0)
    .bind(totals.order_discount_total.0 + totals.line_discount_total.0)
    .bind(totals.service_charge.0)
    .bind(totals.rounding_adjustment.0)
    .bind(grand_total)
    .bind(totals.sales_amount.0)
    .bind(totals.tax_amount.0)
    .bind(0i64) // 實收與找零在跑完付款迴圈之後回填
    .bind(0i64)
    .bind(now.iso())
    .bind(&ctx.actor.user_id)
    .bind(&shift_id)
    .execute(uow.conn())
    .await?;

    let mut change = 0i64;
    let mut paid = 0i64;
    let mut remaining = grand_total;
    for p in &req.payments {
        let method = load_payment_method(&mut uow, &store.id, &p.method_code).await?;
        let tendered = p.tendered.unwrap_or(p.amount);
        // 實際沖銷的金額不可能超過還欠的部分。
        let applied = p.amount.min(remaining).max(0);

        // 找零只可能出現在能找零的方式上（實務上就是現金）。
        // 刷卡「找零」是不存在的東西，所以刷超過就是輸入錯誤，要擋下來 ——
        // 默默把它當找零會讓當天的現金短少。
        let this_change = if method.allows_change {
            (tendered - applied).max(0)
        } else {
            if tendered > applied {
                return Err(AppError::Validation(format!(
                    "「{}」不能找零，金額請改成剛好 {applied} 元",
                    method.name
                )));
            }
            0
        };
        remaining -= applied;
        paid += applied;
        change += this_change;

        sqlx::query(
            "INSERT INTO payments (id, bill_id, order_id, store_id, shift_id, business_date,
                                   payment_method_id, method_code_snapshot, method_name_snapshot,
                                   amount, tendered, change_amount, status, ref_no,
                                   paid_at, created_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?15, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'captured', ?12, ?13, ?14, ?13)",
        )
        .bind(Id::new().as_str())
        .bind(&bill_id)
        .bind(&req.order_id)
        .bind(&store.id)
        .bind(&head.business_date)
        .bind(&method.id)
        .bind(&method.code)
        .bind(&method.name)
        .bind(applied)
        .bind(if method.allows_change { tendered } else { 0 })
        .bind(this_change)
        .bind(&p.ref_no)
        .bind(now.iso())
        .bind(&ctx.actor.user_id)
        .bind(&shift_id)
        .execute(uow.conn())
        .await?;
    }

    if remaining > 0 {
        return Err(AppError::Validation(format!("還差 {remaining} 元沒有付清")));
    }

    sqlx::query("UPDATE bills SET paid_total = ?2, change_total = ?3 WHERE id = ?1")
        .bind(&bill_id)
        .bind(paid)
        .bind(change)
        .execute(uow.conn())
        .await?;

    sqlx::query(
        "UPDATE orders SET status = 'settled', rev = rev + 1, paid_total = ?2, change_total = ?3,
                           settled_at = ?4, updated_at = ?4
          WHERE id = ?1",
    )
    .bind(&req.order_id)
    .bind(paid)
    .bind(change)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    audit::write_in(
        &mut uow,
        AuditEntry::new("Order", &req.order_id, AuditAction::Create)
            .to(&bill_no)
            .on(&head.business_date),
        &ctx.actor,
        &now,
    )
    .await?;

    let seq = next_event_seq(&mut uow, &req.order_id).await?;
    write_event(
        &mut uow,
        &req.order_id,
        seq,
        "settled",
        Some(&head.status),
        Some("settled"),
        ctx,
        &now,
    )
    .await?;

    // 桌位釋放。
    if let Some(sid) = &head.table_session_id {
        sqlx::query(
            "UPDATE table_sessions SET status = 'closed', closed_at = ?2, closed_by = ?3, updated_at = ?2
              WHERE id = ?1",
        )
        .bind(sid)
        .bind(now.iso())
        .bind(&ctx.actor.user_id)
        .execute(uow.conn())
        .await?;
    }

    enqueue_receipt(&mut uow, ctx, &store, &req.order_id, &bill_no, change, &now).await?;

    let order = load_order_view(&mut uow, &req.order_id).await?;
    let result = SettleResult {
        order,
        bill_no,
        change,
    };
    save_idempotent(&mut uow, &req.idem_key, "settle", &result, &now).await?;
    uow.commit().await?;
    Ok(result)
}

// ---------------------------------------------------------------- 讀

pub async fn get_order(ctx: &Ctx, id: &str) -> AppResult<OrderView> {
    let mut uow = ctx.db.begin_write().await?;
    let v = load_order_view(&mut uow, id).await?;
    uow.rollback().await?;
    Ok(v)
}

pub async fn list_open_orders(ctx: &Ctx) -> AppResult<Vec<OrderView>> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM orders WHERE status NOT IN ('settled', 'voided') ORDER BY opened_at",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(get_order(ctx, &id).await?);
    }
    Ok(out)
}

// ---------------------------------------------------------------- 內部：訂單標頭

struct OrderHead {
    rev: i64,
    status: String,
    channel: Channel,
    business_date: String,
    table_session_id: Option<String>,
}

async fn load_order_head(uow: &mut SqliteUow, id: &str) -> AppResult<OrderHead> {
    let r = sqlx::query(
        "SELECT rev, status, channel, business_date, table_session_id FROM orders WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(uow.conn())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("找不到訂單 {id}")))?;

    let channel: String = r.get("channel");
    Ok(OrderHead {
        rev: r.get("rev"),
        status: r.get("status"),
        channel: match channel.as_str() {
            "takeout" => Channel::Takeout,
            "delivery" => Channel::Delivery,
            _ => Channel::DineIn,
        },
        business_date: r.get("business_date"),
        table_session_id: r.get("table_session_id"),
    })
}

fn channel_str(c: Channel) -> &'static str {
    match c {
        Channel::DineIn => "dine_in",
        Channel::Takeout => "takeout",
        Channel::Delivery => "delivery",
    }
}

fn channel_label(c: Channel) -> &'static str {
    match c {
        Channel::DineIn => "內用",
        Channel::Takeout => "外帶",
        Channel::Delivery => "外送",
    }
}

fn ensure_open(head: &OrderHead) -> AppResult<()> {
    match head.status.as_str() {
        "settled" => Err(AppError::Conflict("這張單已經結帳了，不能再改".into())),
        "voided" => Err(AppError::Conflict("這張單已作廢".into())),
        _ => Ok(()),
    }
}

/// 樂觀鎖。
///
/// 不符時回 409 而不是直接覆蓋 —— 兩個人同時改同一張單（收銀員在櫃檯加點、
/// 服務生拿平板也在加點）是真實情境，默默覆蓋會讓其中一邊的品項憑空消失。
fn ensure_rev(head: &OrderHead, expected: i64) -> AppResult<()> {
    if head.rev != expected {
        return Err(AppError::Conflict(format!(
            "這張單剛剛被別人改過了（目前版本 {}，你手上的是 {expected}）。請重新整理後再試一次。",
            head.rev
        )));
    }
    Ok(())
}

async fn next_line_no(uow: &mut SqliteUow, order_id: &str) -> AppResult<i64> {
    let n: Option<i64> =
        sqlx::query_scalar("SELECT MAX(line_no) FROM order_items WHERE order_id = ?1")
            .bind(order_id)
            .fetch_one(uow.conn())
            .await?;
    Ok(n.unwrap_or(0) + 1)
}

async fn next_event_seq(uow: &mut SqliteUow, order_id: &str) -> AppResult<i64> {
    let n: Option<i64> =
        sqlx::query_scalar("SELECT MAX(seq) FROM order_events WHERE order_id = ?1")
            .bind(order_id)
            .fetch_one(uow.conn())
            .await?;
    Ok(n.unwrap_or(0) + 1)
}

/// 寫一筆訂單事件。**append-only：永不 UPDATE、永不 DELETE。**
/// 訂單本身的欄位會被覆寫（狀態與金額都會變），只有這張表保留完整過程。
#[allow(clippy::too_many_arguments)]
async fn write_event(
    uow: &mut SqliteUow,
    order_id: &str,
    seq: i64,
    event_type: &str,
    from: Option<&str>,
    to: Option<&str>,
    ctx: &Ctx,
    now: &Stamp,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO order_events (id, order_id, seq, event_type, from_status, to_status,
                                   actor_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(Id::new().as_str())
    .bind(order_id)
    .bind(seq)
    .bind(event_type)
    .bind(from)
    .bind(to)
    .bind(&ctx.actor.user_id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    Ok(())
}

// ---------------------------------------------------------------- 內部：品項解析

/// 從主檔解析出「當下」的品名與價格。
///
/// 這些值會被**快照**進 `order_items`。之後老闆改名改價都不會動到已經送出的單 ——
/// 而歷史訂單一旦跟著主檔浮動，帳目就永遠對不起來。
struct ResolvedLine {
    name: String,
    short_name: Option<String>,
    category_id: Option<String>,
    category_name: Option<String>,
    variant_name: Option<String>,
    tax_code: String,
    unit_price: i64,
    modifiers: Vec<(String, String, String, i64)>, // (id, group_name, name, price)
    /// 這一行要出到哪一個分區。三層預設：品項 > 分類 > 沒有。
    ///
    /// **在下單當下就決定並快照**，不是出單時才去查。之後老闆把「珍珠奶茶」
    /// 改到別的分區，已經送進廚房的那張單不該跟著跳到另一台機器上；
    /// 補印時也必須印回原來那一台。
    station_id: Option<String>,
}

async fn resolve_line(uow: &mut SqliteUow, l: &NewLine) -> AppResult<ResolvedLine> {
    let r = sqlx::query(
        "SELECT i.name, i.short_name, i.base_price, i.tax_code, i.category_id, i.sold_out_until,
                c.name AS category_name,
                COALESCE(i.station_id, c.default_station_id) AS station_id
           FROM items i
           LEFT JOIN categories c ON c.id = i.category_id
          WHERE i.id = ?1 AND i.deleted_at IS NULL AND i.is_active = 1",
    )
    .bind(&l.item_id)
    .fetch_optional(uow.conn())
    .await?
    .ok_or_else(|| AppError::NotFound("這個商品已經下架了".into()))?;

    // 售完檢查在服務層而不是 UI —— 掃碼點餐的客人手機上是舊資料，
    // 而客人不會知道「剛剛賣完了」。
    let sold_out: Option<String> = r.get("sold_out_until");
    if sold_out.is_some() {
        let name: String = r.get("name");
        return Err(AppError::Validation(format!("「{name}」已售完")));
    }

    let mut unit_price: i64 = r.get("base_price");
    let mut variant_name = None;

    if let Some(vid) = &l.variant_id {
        let v = sqlx::query(
            "SELECT name, price_mode, price, price_delta FROM item_variants
              WHERE id = ?1 AND item_id = ?2 AND deleted_at IS NULL AND is_active = 1",
        )
        .bind(vid)
        .bind(&l.item_id)
        .fetch_optional(uow.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("這個規格已經停用了".into()))?;

        let mode: String = v.get("price_mode");
        unit_price = if mode == "absolute" {
            v.get::<i64, _>("price")
        } else {
            unit_price + v.get::<i64, _>("price_delta")
        };
        variant_name = Some(v.get::<String, _>("name"));
    }

    let mut modifiers = Vec::new();
    for mid in &l.modifier_ids {
        let m = sqlx::query(
            "SELECT m.id, m.name, m.price, g.name AS group_name
               FROM modifiers m JOIN modifier_groups g ON g.id = m.group_id
              WHERE m.id = ?1 AND m.deleted_at IS NULL AND m.is_active = 1",
        )
        .bind(mid)
        .fetch_optional(uow.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("這個選項已經停用了".into()))?;
        modifiers.push((
            m.get::<String, _>("id"),
            m.get::<String, _>("group_name"),
            m.get::<String, _>("name"),
            m.get::<i64, _>("price"),
        ));
    }

    Ok(ResolvedLine {
        name: r.get("name"),
        short_name: r.get("short_name"),
        category_id: r.get("category_id"),
        category_name: r.get("category_name"),
        variant_name,
        tax_code: r.get("tax_code"),
        unit_price,
        modifiers,
        station_id: r.get("station_id"),
    })
}

/// 回傳新建立的那一行的 id。
///
/// 呼叫端需要它：加點單**只能印這一次新增的行**（見 `enqueue_kitchen_ticket`）。
async fn insert_line(
    uow: &mut SqliteUow,
    order_id: &str,
    line_no: i64,
    l: &NewLine,
    r: &ResolvedLine,
    now: &Stamp,
) -> AppResult<String> {
    let qty = l.qty_milli.unwrap_or(QTY_SCALE);
    if qty <= 0 {
        return Err(AppError::Validation("數量必須大於 0".into()));
    }
    let line_id = Id::new().to_string();

    sqlx::query(
        "INSERT INTO order_items (id, order_id, line_no, item_id, variant_id,
                                  name_snapshot, short_name_snapshot, variant_name_snapshot,
                                  category_id_snapshot, category_name_snapshot,
                                  unit_price_snapshot, tax_code_snapshot,
                                  qty_milli, unit_price, note, station_id, kitchen_status,
                                  created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?11, ?14, ?16,
                 'pending', NULL, ?15, ?15)",
    )
    .bind(&line_id)
    .bind(order_id)
    .bind(line_no)
    .bind(&l.item_id)
    .bind(&l.variant_id)
    .bind(&r.name)
    .bind(&r.short_name)
    .bind(&r.variant_name)
    .bind(&r.category_id)
    .bind(&r.category_name)
    .bind(r.unit_price)
    .bind(&r.tax_code)
    .bind(qty)
    .bind(&l.note)
    .bind(now.iso())
    .bind(&r.station_id)
    .execute(uow.conn())
    .await?;

    for (mid, group_name, name, price) in &r.modifiers {
        sqlx::query(
            "INSERT INTO order_item_modifiers (id, order_item_id, modifier_id,
                                               group_name_snapshot, name_snapshot,
                                               unit_price_snapshot, qty, amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?6, ?7)",
        )
        .bind(Id::new().as_str())
        .bind(&line_id)
        .bind(mid)
        .bind(group_name)
        .bind(name)
        .bind(price)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }
    Ok(line_id)
}

// ---------------------------------------------------------------- 內部：重算

/// 把整張單重新丟給定價引擎，並把結果寫回每一行。
///
/// 每次都全部重算而不是增量更新：慢一點點，但**畫面上的金額與資料庫裡的金額
/// 永遠由同一段程式碼產生**。折扣與服務費要分攤到每一行，增量更新幾乎必然會漂，
/// 而金額漂掉在 POS 上是最貴的一種 bug。
async fn recompute(
    uow: &mut SqliteUow,
    order_id: &str,
    store: &StoreConfig,
    channel: Channel,
    now: &Stamp,
) -> AppResult<PricingOutput> {
    let rows = sqlx::query(
        "SELECT id, qty_milli, unit_price FROM order_items
          WHERE order_id = ?1 AND voided_at IS NULL ORDER BY line_no",
    )
    .bind(order_id)
    .fetch_all(uow.conn())
    .await?;

    let mut ids: Vec<String> = Vec::with_capacity(rows.len());
    let mut lines: Vec<LineInput> = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: String = r.get("id");
        let mods = sqlx::query(
            "SELECT unit_price_snapshot, qty FROM order_item_modifiers WHERE order_item_id = ?1",
        )
        .bind(&id)
        .fetch_all(uow.conn())
        .await?;

        lines.push(LineInput {
            unit_price: Money(r.get::<i64, _>("unit_price")),
            qty_milli: r.get("qty_milli"),
            modifiers: mods
                .iter()
                .map(|m| {
                    (
                        Money(m.get::<i64, _>("unit_price_snapshot")),
                        m.get::<i64, _>("qty"),
                    )
                })
                .collect(),
            discounts: Vec::new(),
        });
        ids.push(id);
    }

    let out = pricing::compute(&PricingInput {
        channel,
        service_charge_rate_bp: service_rate_for(store, channel),
        rounding: store.rounding,
        tax_rate_bp: store.tax_rate_bp,
        lines,
        order_discounts: Vec::new(),
    })?;

    for (id, l) in ids.iter().zip(&out.lines) {
        sqlx::query(
            "UPDATE order_items SET modifier_amount = ?2, gross_amount = ?3, line_discount = ?4,
                                    net_amount = ?5, allocated_order_discount = ?6,
                                    allocated_service_charge = ?7, taxable_amount = ?8,
                                    updated_at = ?9
              WHERE id = ?1",
        )
        .bind(id)
        .bind(l.gross.0 - l.net.0)
        .bind(l.gross.0)
        .bind(l.line_discount.0)
        .bind(l.net.0)
        .bind(l.allocated_order_discount.0)
        .bind(l.allocated_service_charge.0)
        .bind(l.taxable_amount.0)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }
    Ok(out)
}

async fn bump_order(
    uow: &mut SqliteUow,
    order_id: &str,
    expected_rev: i64,
    status: &str,
    t: &PricingOutput,
    now: &Stamp,
) -> AppResult<()> {
    // WHERE 帶 rev：即使兩個交易同時走到這裡（理論上不會，寫入池只有一條連線），
    // 也只有一個會成功。這是樂觀鎖的最後一道保險。
    let n = sqlx::query(
        "UPDATE orders SET rev = rev + 1, status = ?3, subtotal = ?4, line_discount_total = ?5,
                           order_discount_total = ?6, service_charge = ?7,
                           rounding_adjustment = ?8, grand_total = ?9, sales_amount = ?10,
                           tax_amount = ?11, placed_at = COALESCE(placed_at, ?12), updated_at = ?12
          WHERE id = ?1 AND rev = ?2",
    )
    .bind(order_id)
    .bind(expected_rev)
    .bind(status)
    .bind(t.subtotal.0)
    .bind(t.line_discount_total.0)
    .bind(t.order_discount_total.0)
    .bind(t.service_charge.0)
    .bind(t.rounding_adjustment.0)
    .bind(t.grand_total.0)
    .bind(t.sales_amount.0)
    .bind(t.tax_amount.0)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();

    if n == 0 {
        return Err(AppError::Conflict(
            "這張單剛剛被別人改過了，請重新整理後再試一次。".into(),
        ));
    }
    Ok(())
}

async fn load_order_view(uow: &mut SqliteUow, id: &str) -> AppResult<OrderView> {
    let r = sqlx::query(
        "SELECT o.id, o.order_no, o.status, o.rev, o.channel, o.table_id, o.guest_count,
                o.business_date, o.subtotal, o.service_charge, o.rounding_adjustment,
                o.grand_total, o.sales_amount, o.tax_amount, o.paid_total, o.change_total,
                t.code AS table_code
           FROM orders o
           LEFT JOIN dining_tables t ON t.id = o.table_id
          WHERE o.id = ?1",
    )
    .bind(id)
    .fetch_optional(uow.conn())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("找不到訂單 {id}")))?;

    let items = sqlx::query(
        "SELECT id, line_no, name_snapshot, variant_name_snapshot, note, qty_milli,
                unit_price, taxable_amount
           FROM order_items
          WHERE order_id = ?1 AND voided_at IS NULL ORDER BY line_no",
    )
    .bind(id)
    .fetch_all(uow.conn())
    .await?;

    let mut lines = Vec::with_capacity(items.len());
    for it in &items {
        let line_id: String = it.get("id");
        let options: Vec<String> = sqlx::query_scalar(
            "SELECT name_snapshot FROM order_item_modifiers WHERE order_item_id = ?1 ORDER BY id",
        )
        .bind(&line_id)
        .fetch_all(uow.conn())
        .await?;

        lines.push(OrderLineView {
            id: line_id,
            line_no: it.get("line_no"),
            name: it.get("name_snapshot"),
            variant_name: it.get("variant_name_snapshot"),
            options,
            note: it.get("note"),
            qty_milli: it.get("qty_milli"),
            unit_price: it.get("unit_price"),
            amount: it.get("taxable_amount"),
        });
    }

    Ok(OrderView {
        id: r.get("id"),
        order_no: r.get("order_no"),
        status: r.get("status"),
        rev: r.get("rev"),
        channel: r.get("channel"),
        table_id: r.get("table_id"),
        table_label: r.get("table_code"),
        guest_count: r.get("guest_count"),
        business_date: r.get("business_date"),
        lines,
        subtotal: r.get("subtotal"),
        service_charge: r.get("service_charge"),
        rounding_adjustment: r.get("rounding_adjustment"),
        grand_total: r.get("grand_total"),
        sales_amount: r.get("sales_amount"),
        tax_amount: r.get("tax_amount"),
        paid_total: r.get("paid_total"),
        change_total: r.get("change_total"),
    })
}

// ---------------------------------------------------------------- 內部：付款方式

struct PaymentMethod {
    id: String,
    code: String,
    name: String,
    allows_change: bool,
}

async fn load_payment_method(
    uow: &mut SqliteUow,
    store_id: &str,
    code: &str,
) -> AppResult<PaymentMethod> {
    let r = sqlx::query(
        "SELECT id, code, name, allows_change FROM payment_methods
          WHERE store_id = ?1 AND code = ?2 AND deleted_at IS NULL AND is_active = 1",
    )
    .bind(store_id)
    .bind(code)
    .fetch_optional(uow.conn())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("找不到付款方式「{code}」")))?;

    Ok(PaymentMethod {
        id: r.get("id"),
        code: r.get("code"),
        name: r.get("name"),
        allows_change: r.get::<i64, _>("allows_change") == 1,
    })
}

// ---------------------------------------------------------------- 內部：出單

/// 把一張要印的單寫進 outbox。
///
/// **交易內不碰印表機。** 寫入池只有一條連線，一個卡住的 TCP 連線
/// （缺紙的機器會 accept 連線但不 drain buffer）會讓全店的寫入排隊。
/// 背景 worker 讀 outbox、負責重試與死信。
async fn enqueue_print(
    uow: &mut SqliteUow,
    kind: &str,
    business_date: &str,
    payload: &serde_json::Value,
    now: &Stamp,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO outbox (id, kind, payload_json, status, attempts, next_attempt_at,
                             business_date, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'pending', 0, ?4, ?5, ?4, ?4)",
    )
    .bind(Id::new().as_str())
    .bind(kind)
    .bind(payload.to_string())
    .bind(now.iso())
    .bind(business_date)
    .execute(uow.conn())
    .await?;
    Ok(())
}

async fn build_ticket_data(
    uow: &mut SqliteUow,
    store: &StoreConfig,
    order_id: &str,
    reason: TicketReason,
    now: &Stamp,
    only_lines: Option<&std::collections::HashSet<String>>,
) -> AppResult<TicketData> {
    let v = load_order_view(uow, order_id).await?;

    // ★ 退點單要印的那一行**已經標了作廢**，所以不能只用 `load_order_view`
    //   （它只回未作廢的行）—— 否則「取消珍珠奶茶」會印出一張空白的單。
    //   指定了行 id 時直接從 order_items 撈，作廢與否都撈得到。
    let ticket_lines: Vec<OrderLineView> = match only_lines {
        None => v.lines.clone(),
        Some(keep) => {
            let mut out = Vec::new();
            for line_id in keep {
                let Some(it) = sqlx::query(
                    "SELECT id, line_no, name_snapshot, variant_name_snapshot, note, qty_milli,
                            unit_price, taxable_amount
                       FROM order_items WHERE id = ?1 AND order_id = ?2",
                )
                .bind(line_id)
                .bind(order_id)
                .fetch_optional(uow.conn())
                .await?
                else {
                    continue;
                };
                let options: Vec<String> = sqlx::query_scalar(
                    "SELECT name_snapshot FROM order_item_modifiers
                      WHERE order_item_id = ?1 ORDER BY id",
                )
                .bind(line_id)
                .fetch_all(uow.conn())
                .await?;
                out.push(OrderLineView {
                    id: it.get("id"),
                    line_no: it.get("line_no"),
                    name: it.get("name_snapshot"),
                    variant_name: it.get("variant_name_snapshot"),
                    options,
                    note: it.get("note"),
                    qty_milli: it.get("qty_milli"),
                    unit_price: it.get("unit_price"),
                    amount: it.get("taxable_amount"),
                });
            }
            // 行號排序：廚房單上的順序要跟客人點的順序一樣。
            out.sort_by_key(|l| l.line_no);
            out
        }
    };

    let channel = match v.channel.as_str() {
        "takeout" => Channel::Takeout,
        "delivery" => Channel::Delivery,
        _ => Channel::DineIn,
    };

    Ok(TicketData {
        store_name: store.name.clone(),
        order_no: v.order_no.clone(),
        channel_label: channel_label(channel).to_string(),
        table_label: v.table_label.clone(),
        // ★ 用店家的時區，不是 UTC。
        //
        // 資料庫裡一律存 UTC（字典序即時序、換 PG 免轉換），但**印在紙上的
        // 時間是給人看的**：一位台灣店員在晚上九點半拿到一張寫著 13:35 的單，
        // 只會以為系統壞了。時區轉換就發生在這一行、只發生在這一行。
        printed_at: crate::core::clock::for_humans(now.at, store.day.tz),
        lines: ticket_lines
            .iter()
            .map(|l| TicketLine {
                // 廚房單優先用短名 —— 58mm 一行只放得下約 9 個中文字。
                name: match &l.variant_name {
                    Some(vn) => format!("{}（{vn}）", l.name),
                    None => l.name.clone(),
                },
                options: l.options.clone(),
                note: l.note.clone(),
                qty_milli: l.qty_milli,
                amount: l.amount,
            })
            .collect(),
        subtotal: v.subtotal,
        discount_total: 0,
        service_charge: v.service_charge,
        rounding_adjustment: v.rounding_adjustment,
        grand_total: v.grand_total,
        sales_amount: v.sales_amount,
        tax_amount: v.tax_amount,
        payments: Vec::new(),
        change: 0,
        station: None,
        reason,
        reprint_seq: 0,
    })
}

/// 廚房單。**一個出單分區一張**，不是整單一張。
///
/// 飲料吧不需要看到熱炒的品項，熱炒區也不需要看到飲料 —— 印給他們只會讓
/// 廚師在一張長長的單上找自己那幾行，尖峰時間那就是漏做的來源。
///
/// 分區來自 `order_items.station_id`，那是**下單當下就快照好的**。
/// 這裡刻意只做到「分區」而不是「印表機」：哪一台機器負責哪一區是設定，
/// 而設定可能在單已經排隊之後才被改（或才第一次被設定）。
async fn enqueue_kitchen_ticket(
    uow: &mut SqliteUow,
    _ctx: &Ctx,
    store: &StoreConfig,
    order_id: &str,
    reason: TicketReason,
    // None = 整張單（新單）。Some = 只印這幾行（加點、退點）。
    only_lines: Option<&std::collections::HashSet<String>>,
    now: &Stamp,
) -> AppResult<()> {
    use std::collections::{BTreeMap, HashSet};

    let business_date = data_business_date(uow, order_id).await?;

    // 退點時那一行已經標了 voided_at，所以不能只看未作廢的行 ——
    // 否則「取消珍珠奶茶」的單上會一個字都沒有。
    let rows = sqlx::query(
        "SELECT oi.id, oi.station_id, ps.name AS station_name
           FROM order_items oi
           LEFT JOIN print_stations ps ON ps.id = oi.station_id AND ps.deleted_at IS NULL
          WHERE oi.order_id = ?1
          ORDER BY oi.line_no",
    )
    .bind(order_id)
    .fetch_all(uow.conn())
    .await?;

    // BTreeMap：輸出順序必須可重現（稽核與快照測試都靠它）。
    let mut groups: BTreeMap<Option<String>, (Option<String>, HashSet<String>)> = BTreeMap::new();
    for r in &rows {
        let line_id: String = r.get("id");
        if only_lines.is_some_and(|keep| !keep.contains(&line_id)) {
            continue;
        }
        let station_id: Option<String> = r.get("station_id");
        let station_name: Option<String> = r.get("station_name");
        let entry = groups
            .entry(station_id)
            .or_insert((station_name, HashSet::new()));
        entry.1.insert(line_id);
    }

    for (station_id, (station_name, line_ids)) in groups {
        if line_ids.is_empty() {
            continue;
        }
        let mut data =
            build_ticket_data(uow, store, order_id, reason, now, Some(&line_ids)).await?;
        data.station = station_name;
        // 存的是**已排版的 doc**，不是 order_id。補印時直接重送這份 doc ——
        // 重跑業務邏輯會印出「訂單被改過之後」的內容，而廚房單是「當時的指令」。
        let doc = templates::kitchen_ticket(&data, PaperWidth::Mm80);
        let payload = serde_json::json!({
            "orderId": order_id,
            "stationId": station_id,
            "reason": reason,
            "doc": doc,
        });
        enqueue_print(uow, "print.kitchen", &business_date, &payload, now).await?;
    }
    Ok(())
}

async fn enqueue_receipt(
    uow: &mut SqliteUow,
    _ctx: &Ctx,
    store: &StoreConfig,
    order_id: &str,
    bill_no: &str,
    change: i64,
    now: &Stamp,
) -> AppResult<()> {
    let mut data = build_ticket_data(uow, store, order_id, TicketReason::Settle, now, None).await?;
    data.change = change;
    let payments = sqlx::query(
        "SELECT method_name_snapshot, amount, tendered
           FROM payments WHERE order_id = ?1 ORDER BY id",
    )
    .bind(order_id)
    .fetch_all(uow.conn())
    .await?;
    data.payments = payments
        .iter()
        .map(|p| {
            let amount: i64 = p.get("amount");
            let tendered: i64 = p.get("tendered");
            crate::receipt::templates::PaymentLine {
                method: p.get("method_name_snapshot"),
                // ★ 印客人**給了多少**，不是沖銷了多少。
                //
                //   「現金 95 / 找零 5」在算術上是錯的：客人給的是 100。
                //   收據上這三個數字必須自己對得起來（合計 95、現金 100、找零 5），
                //   否則客人會當場問，而店員也解釋不出來。
                amount: if tendered > amount { tendered } else { amount },
            }
        })
        .collect();

    let business_date = data_business_date(uow, order_id).await?;
    // 收據**重新排版**（不像廚房單用快照）：收據是「當前的事實」，
    // 客人手上那張要反映最後的結帳結果。
    let doc = templates::customer_receipt(&data, PaperWidth::Mm80);
    let payload = serde_json::json!({ "orderId": order_id, "billNo": bill_no, "doc": doc });
    enqueue_print(uow, "print.receipt", &business_date, &payload, now).await
}

async fn data_business_date(uow: &mut SqliteUow, order_id: &str) -> AppResult<String> {
    Ok(
        sqlx::query_scalar("SELECT business_date FROM orders WHERE id = ?1")
            .bind(order_id)
            .fetch_one(uow.conn())
            .await?,
    )
}

// ---------------------------------------------------------------- 內部：冪等

async fn load_idempotent<T: for<'de> Deserialize<'de>>(
    ctx: &Ctx,
    key: &str,
) -> AppResult<Option<T>> {
    let json: Option<String> =
        sqlx::query_scalar("SELECT response_json FROM idempotency_keys WHERE key = ?1")
            .bind(key)
            .fetch_optional(ctx.db.reader())
            .await?;
    match json {
        Some(j) => Ok(serde_json::from_str(&j).ok()),
        None => Ok(None),
    }
}

async fn save_idempotent<T: Serialize>(
    uow: &mut SqliteUow,
    key: &str,
    operation: &str,
    value: &T,
    now: &Stamp,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO idempotency_keys (key, operation, response_json, created_at)
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT(key) DO NOTHING",
    )
    .bind(key)
    .bind(operation)
    .bind(serde_json::to_string(value).unwrap_or_default())
    .bind(now.iso())
    .execute(uow.conn())
    .await?;
    Ok(())
}
