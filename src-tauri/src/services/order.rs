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
use crate::i18n::Msg;
use crate::infra::db::sqlite::SqliteUow;
use crate::msg;
use crate::receipt::templates::{self, TicketData, TicketLine, TicketReason};
use crate::receipt::PaperWidth;
use crate::services::audit::{self, AuditAction, AuditEntry};
use crate::services::rbac;
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
    /// 這一次只結一部分。省略＝把還沒結的全部結掉。
    #[serde(default)]
    pub split: Option<SplitReq>,
}

/// 分帳：一張訂單拆成好幾張帳單，各自結清。
///
/// 三個模式對應櫃檯真的會聽到的三句話：
///
/// * `Even`   —— 「我們四個平分」
/// * `Amount` —— 「我先出 500，剩下他付」
/// * `Items`  —— 「我的只有那碗麵」
///
/// # 為什麼不是「拆成很多張訂單」
///
/// 因為廚房已經照那張單做菜了。拆訂單會讓廚房單、桌位、加點全部要跟著重新
/// 對應，而分帳其實只是**收錢的方式**不同 —— 賣出去的東西一件都沒變。
/// 所以拆的是 `bills`，不是 `orders`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum SplitReq {
    /// 均分成 `parts` 份。這一次結的是還沒結的第一份。
    Even { parts: i64 },
    /// 這一次收 `amount` 元。總共會有幾份，要收到最後一筆才知道。
    Amount { amount: i64 },
    /// 這一次結這幾行。
    #[serde(rename_all = "camelCase")]
    Items { line_ids: Vec<String> },
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
    /// 分項分帳時，這一行是不是已經被誰結掉了。
    pub paid: bool,
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
    /// 已經開出去的帳單收了多少（分帳用）。整單結帳時等於 grand_total。
    pub billed_total: i64,
    /// 已經結了幾份。
    pub bill_count: i64,
    /// 每人低消還差多少。0 = 沒設低消、不是內用、或已經達到。
    ///
    /// **它只是提醒，系統不會自動補一行差額。** 查過的市售產品幾乎都是這樣做的：
    /// 差額該不該收、收多少、要不要通融，是店長當下的判斷，不是軟體的。
    /// 自動補一行的後果是客人在收據上看到一筆他沒點過的東西，
    /// 而收銀員解釋不出來那是什麼。
    pub min_charge_shortfall: i64,
    /// 這張單用哪一種分法（even / by_item / by_amount）。還沒分過是 None。
    pub split_mode: Option<String>,
    /// 平分時說好要分幾份。畫面要靠它把份數鎖住 —— 讓收銀員重選一次再被
    /// 伺服器擋下來，是把系統已經知道的事丟給人記。
    pub split_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResult {
    pub order: OrderView,
    pub bill_no: String,
    pub change: i64,
    /// 這張訂單還有多少沒結。分帳時 > 0，代表**還不能讓客人走**。
    pub remaining: i64,
    /// 這是第幾份。沒分帳時是 1。
    pub split_index: i64,
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
    // 這桌套用哪個吃到飽方案（沒有就是 None，一切照舊）。
    //
    // ★ 點下去方案商品，這一桌就自動變成吃到飽 —— 不需要另外一個「切換模式」的按鈕。
    //
    //   這是 Airレジ 的做法（方案靠點那個商品綁到桌上）。比一顆切換鈕好的地方在於：
    //   店員本來就要點「晚餐吃到飽 ×4」，那一步同時就是宣告，少一個要記的步驟；
    //   而且不會出現「忘了切換模式、飲料全部照原價收」這種在客人面前算錯錢的情況。
    let mut plan = crate::services::dining::active_plan_for_order(&mut uow, &req.order_id).await?;
    if plan.is_none() {
        if let Some(p) = crate::services::dining::plan_for_any_item(&mut uow, &req.lines).await? {
            crate::services::dining::bind_to_order_session(&mut uow, &req.order_id, &p.id, &now)
                .await?;
            plan = Some(p);
        }
    }
    let plan = plan;

    let mut added: std::collections::HashSet<String> = Default::default();
    for l in &req.lines {
        let resolved = resolve_line(&mut uow, l, plan.as_ref()).await?;
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

// ---------------------------------------------------------------- 折扣

const PERM_LINE_DISCOUNT: &str = "discount.line";
const PERM_ORDER_DISCOUNT: &str = "discount.order";
const PERM_COMP: &str = "discount.comp";
const PERM_VOID_ORDER: &str = "order.void";
const PERM_VOID_AFTER_SETTLE: &str = "order.void.after_settle";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscountReq {
    pub order_id: String,
    pub expected_rev: i64,
    /// None = 整單折扣。
    pub line_id: Option<String>,
    /// percent（basis point，8500 = 85 折）/ amount（整數元）/ comp（招待）。
    pub kind: String,
    pub value: i64,
    pub reason_id: Option<String>,
    pub note: Option<String>,
    /// 主管授權（收銀員權限不足時）。
    pub approver_id: Option<String>,
}

/// 打折。
///
/// **招待與折扣分開兩個權限**：老闆看折扣是看行銷成效，看招待是看有沒有人
/// 在送人情。把它們混成一個權限，等於把後者藏進前者裡。
pub async fn apply_discount(ctx: &Ctx, req: DiscountReq) -> AppResult<OrderView> {
    let (perm, label) = match req.kind.as_str() {
        "comp" => (PERM_COMP, "招待"),
        _ if req.line_id.is_some() => (PERM_LINE_DISCOUNT, "單品折扣"),
        _ => (PERM_ORDER_DISCOUNT, "整單折扣"),
    };
    if !["percent", "amount", "comp"].contains(&req.kind.as_str()) {
        return Err(AppError::Validation(
            format!("不認得的折扣種類：{}", req.kind).into(),
        ));
    }
    if req.kind == "percent" && !(1..=10_000).contains(&req.value) {
        return Err(AppError::Validation(
            "折數要用 basis point：8500 = 85 折，1 到 10000 之間".into(),
        ));
    }
    if req.kind == "amount" && req.value <= 0 {
        return Err(AppError::Validation("折抵金額要大於 0".into()));
    }

    let approver = load_actor(ctx, req.approver_id.as_deref()).await?;
    let authorized =
        rbac::require_with_approval(&ctx.db, &ctx.actor, approver.as_ref(), perm).await?;

    let store = load_store(ctx).await?;
    let now = Stamp::now();

    let mut uow = ctx.db.begin_write().await?;
    let head = load_order_head(&mut uow, &req.order_id).await?;
    crate::services::shift::ensure_day_open(&mut uow, &head.business_date).await?;
    ensure_open(&head)?;
    ensure_rev(&head, req.expected_rev)?;

    if let Some(line_id) = &req.line_id {
        let belongs: Option<String> = sqlx::query_scalar(
            "SELECT id FROM order_items WHERE id = ?1 AND order_id = ?2 AND voided_at IS NULL",
        )
        .bind(line_id)
        .bind(&req.order_id)
        .fetch_optional(uow.conn())
        .await?;
        if belongs.is_none() {
            return Err(AppError::NotFound("找不到這一個品項".into()));
        }
    }

    sqlx::query(
        "INSERT INTO order_discounts (id, order_id, order_item_id, name_snapshot, type_snapshot,
                                      value_snapshot, amount, reason_id, note, approved_by,
                                      created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(Id::new().as_str())
    .bind(&req.order_id)
    .bind(&req.line_id)
    .bind(label)
    .bind(&req.kind)
    .bind(req.value)
    .bind(&req.reason_id)
    .bind(&req.note)
    .bind(approver.as_ref().map(|a| a.user_id.clone()))
    .bind(&ctx.actor.user_id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    let before = head.grand_total;
    let totals = recompute(&mut uow, &req.order_id, &store, head.channel, &now).await?;
    bump_order(
        &mut uow,
        &req.order_id,
        head.rev,
        &head.status,
        &totals,
        &now,
    )
    .await?;

    // 折扣是動到錢的操作，稽核一定要留，而且要留「差了多少」。
    let mut entry = AuditEntry::new("Order", &req.order_id, AuditAction::Discount)
        .amount(totals.grand_total.0 - before)
        .on(&head.business_date);
    if let Some(r) = &req.reason_id {
        entry = entry.reason(r);
    }
    if authorized.user_id != ctx.actor.user_id {
        entry = entry.approved_by(&authorized.user_id);
    }
    audit::write_in(&mut uow, entry, &ctx.actor, &now).await?;

    let seq = next_event_seq(&mut uow, &req.order_id).await?;
    write_event(
        &mut uow,
        &req.order_id,
        seq,
        "discount_applied",
        None,
        None,
        ctx,
        &now,
    )
    .await?;

    uow.commit().await?;
    get_order(ctx, &req.order_id).await
}

/// 讀主管。退款與作廢都要用，所以留在 crate 內共用。
pub(crate) async fn load_actor(ctx: &Ctx, user_id: Option<&str>) -> AppResult<Option<rbac::Actor>> {
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let r = sqlx::query("SELECT id, code, name FROM users WHERE id = ?1 AND deleted_at IS NULL")
        .bind(user_id)
        .fetch_optional(ctx.db.reader())
        .await?
        .ok_or_else(|| AppError::NotFound("找不到這位主管".into()))?;
    Ok(Some(rbac::Actor {
        user_id: r.get("id"),
        code: r.get("code"),
        name: r.get("name"),
    }))
}

// ---------------------------------------------------------------- 作廢整單

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoidOrderReq {
    pub order_id: String,
    pub expected_rev: i64,
    /// 作廢一定要有原因。沒有原因的作廢是查不動的。
    pub reason_id: Option<String>,
    pub note: Option<String>,
    pub approver_id: Option<String>,
}

/// 作廢整張單。
///
/// ★ **結帳後作廢是餐飲業最大的防弊點**：結完帳再作廢等於私吞現金。
///   所以它走一個獨立的權限碼（`order.void.after_settle`），而且會寫進
///   `approvals`，日結時看得到。
pub async fn void_order(ctx: &Ctx, req: VoidOrderReq) -> AppResult<OrderView> {
    let now = Stamp::now();
    let store = load_store(ctx).await?;

    let mut uow = ctx.db.begin_write().await?;
    let head = load_order_head(&mut uow, &req.order_id).await?;
    crate::services::shift::ensure_day_open(&mut uow, &head.business_date).await?;
    ensure_rev(&head, req.expected_rev)?;
    if head.status == "voided" {
        return Err(AppError::Conflict("這張單已經作廢了".into()));
    }

    let after_settle = head.status == "settled";
    let perm = if after_settle {
        PERM_VOID_AFTER_SETTLE
    } else {
        PERM_VOID_ORDER
    };
    if after_settle && req.reason_id.is_none() {
        // 結帳後作廢一定要有原因。這是防弊的一半 ——
        // 另一半是有人要簽名。
        return Err(AppError::Validation("結帳後作廢必須選一個原因。".into()));
    }
    uow.rollback().await?;

    let approver = load_actor(ctx, req.approver_id.as_deref()).await?;
    let authorized =
        rbac::require_with_approval(&ctx.db, &ctx.actor, approver.as_ref(), perm).await?;

    let mut uow = ctx.db.begin_write().await?;
    let head = load_order_head(&mut uow, &req.order_id).await?;
    ensure_rev(&head, req.expected_rev)?;

    sqlx::query(
        "UPDATE orders SET status = 'voided', voided_at = ?2, void_reason_id = ?3,
                           void_by = ?4, rev = rev + 1, updated_at = ?2
          WHERE id = ?1",
    )
    .bind(&req.order_id)
    .bind(now.iso())
    .bind(&req.reason_id)
    .bind(&ctx.actor.user_id)
    .execute(uow.conn())
    .await?;
    sqlx::query(
        "UPDATE order_items SET voided_at = ?2, void_reason_id = ?3, void_by = ?4, updated_at = ?2
          WHERE order_id = ?1 AND voided_at IS NULL",
    )
    .bind(&req.order_id)
    .bind(now.iso())
    .bind(&req.reason_id)
    .bind(&ctx.actor.user_id)
    .execute(uow.conn())
    .await?;
    sqlx::query("UPDATE bills SET status = 'voided', updated_at = ?2 WHERE order_id = ?1")
        .bind(&req.order_id)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;

    // ★ 收款也要一起作廢。
    //
    //   作廢的語意是「這筆交易沒有發生過」，錢已經退還給客人了。
    //   少了這一步，關班時那筆現金還會被算進「應有現金」——
    //   於是每一次結帳後作廢都會變成一筆假的短少，而收銀員會被冤枉。
    // payments 是 append-only 的紀錄（沒有 updated_at）：改的只有狀態。
    sqlx::query(
        "UPDATE payments SET status = 'voided' WHERE order_id = ?1 AND status = 'captured'",
    )
    .bind(&req.order_id)
    .execute(uow.conn())
    .await?;

    // 結帳後作廢要留一筆簽核紀錄，日結時看得到。
    if after_settle {
        sqlx::query(
            "INSERT INTO approvals (id, action_code, ref_type, ref_id, amount, reason_id, note,
                                    requested_by, approved_by, auth_method, business_date,
                                    approved_at, created_at)
             VALUES (?1, ?2, 'order', ?3, ?4, ?5, ?6, ?7, ?8, 'pin', ?9, ?10, ?10)",
        )
        .bind(Id::new().as_str())
        .bind(PERM_VOID_AFTER_SETTLE)
        .bind(&req.order_id)
        .bind(head.grand_total)
        .bind(&req.reason_id)
        .bind(&req.note)
        .bind(&ctx.actor.user_id)
        .bind(&authorized.user_id)
        .bind(&head.business_date)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    // 結帳後作廢用獨立的動作碼。日結時要能單獨統計它 ——
    // 「結完帳再作廢」是餐飲業最大的防弊點，混在一般作廢裡就看不出來了。
    let action = if after_settle {
        AuditAction::VoidAfterSettle
    } else {
        AuditAction::Void
    };
    let mut entry = AuditEntry::new("Order", &req.order_id, action)
        // 金額用負的：作廢是把已經計入的錢拿掉。
        .amount(-head.grand_total)
        .on(&head.business_date);
    if let Some(r) = &req.reason_id {
        entry = entry.reason(r);
    }
    if authorized.user_id != ctx.actor.user_id {
        entry = entry.approved_by(&authorized.user_id);
    }
    audit::write_in(&mut uow, entry, &ctx.actor, &now).await?;

    let seq = next_event_seq(&mut uow, &req.order_id).await?;
    write_event(
        &mut uow,
        &req.order_id,
        seq,
        "order_voided",
        Some(&head.status),
        Some("voided"),
        ctx,
        &now,
    )
    .await?;

    // 廚房要知道整桌都不做了。
    enqueue_kitchen_ticket(
        &mut uow,
        ctx,
        &store,
        &req.order_id,
        TicketReason::Void,
        None,
        &now,
    )
    .await?;

    uow.commit().await?;
    get_order(ctx, &req.order_id).await
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
    // 這一次要收多少。沒分帳的話就是「還沒結的全部」。
    let plan = plan_split(&mut uow, &req.order_id, grand_total, req.split.as_ref()).await?;
    let due = plan.due;

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
    if offered < due {
        return Err(AppError::Validation(
            format!(
                "收款金額 {offered} 元不足應收的 {due} 元，還差 {} 元",
                due - offered
            )
            .into(),
        ));
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

    // 稅額是**這一張帳單自己**拆的，不是整單稅額的幾分之幾。
    //
    // 每一張帳單就是一張發票，而發票上的 `銷售額 + 稅額 = 總額` 是財政部的
    // 硬檢核 —— 分攤整單稅額會讓某一份差一元而整批退件。各拆各的，
    // 加總起來與整單稅額差個一兩元是正常且正確的。
    let (part_sales, part_tax) = if plan.mode == "none" {
        (totals.sales_amount.0, totals.tax_amount.0)
    } else {
        let (s, t) = crate::core::money::split_tax_inclusive(Money(due), store.tax_rate_bp);
        (s.0, t.0)
    };
    let (part_subtotal, part_discount, part_service, part_rounding) = if plan.mode == "none" {
        (
            totals.subtotal.0,
            totals.order_discount_total.0 + totals.line_discount_total.0,
            totals.service_charge.0,
            totals.rounding_adjustment.0,
        )
    } else {
        // 分帳的一份沒有自己的「小計 / 折扣 / 服務費」—— 那些是整張單的事。
        // 硬要分攤只會造出一堆對不起來的數字。
        (due, 0, 0, 0)
    };

    sqlx::query(
        "INSERT INTO bills (id, order_id, store_id, shift_id, business_date, bill_no, split_mode,
                            split_index, split_count, subtotal, discount_total, service_charge,
                            rounding_adjustment, grand_total, sales_amount, tax_amount,
                            paid_total, change_total, status, settled_at, settled_by,
                            created_at, updated_at)
         VALUES (?1, ?2, ?3, ?17, ?4, ?5, ?18, ?19, ?20, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 ?13, ?14, 'settled', ?15, ?16, ?15, ?15)",
    )
    .bind(&bill_id)
    .bind(&req.order_id)
    .bind(&store.id)
    .bind(&head.business_date)
    .bind(&bill_no)
    .bind(part_subtotal)
    .bind(part_discount)
    .bind(part_service)
    .bind(part_rounding)
    .bind(due)
    .bind(part_sales)
    .bind(part_tax)
    .bind(0i64) // 實收與找零在跑完付款迴圈之後回填
    .bind(0i64)
    .bind(now.iso())
    .bind(&ctx.actor.user_id)
    .bind(&shift_id)
    .bind(plan.mode)
    .bind(plan.index)
    .bind(plan.count.unwrap_or(0))
    .execute(uow.conn())
    .await?;

    // 分項分帳要記下這一份含哪幾行 —— 否則下一份無從知道哪些已經結過。
    for line_id in &plan.line_ids {
        sqlx::query(
            "INSERT INTO bill_lines (id, bill_id, order_item_id, qty_milli, amount, created_at)
             SELECT ?1, ?2, oi.id, oi.qty_milli, oi.taxable_amount, ?4
               FROM order_items oi WHERE oi.id = ?3",
        )
        .bind(Id::new().as_str())
        .bind(&bill_id)
        .bind(line_id)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    let mut change = 0i64;
    let mut paid = 0i64;
    let mut remaining = due;
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
                return Err(AppError::Validation(
                    format!("「{}」不能找零，金額請改成剛好 {applied} 元", method.name).into(),
                ));
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
        return Err(AppError::Validation(
            format!("還差 {remaining} 元沒有付清").into(),
        ));
    }

    sqlx::query("UPDATE bills SET paid_total = ?2, change_total = ?3 WHERE id = ?1")
        .bind(&bill_id)
        .bind(paid)
        .bind(change)
        .execute(uow.conn())
        .await?;

    // ★ 只有付清了整張單才算 settled。
    //
    // 分帳到一半就把訂單標成已結，是這個功能最容易犯、也最貴的錯：那張單會
    // 從「未結」清單與桌位圖上消失，而剩下的錢還沒收。所以這裡把兩件事分開：
    // 每一份各自入帳（累加 paid_total），整張單的狀態只在最後一份時才動。
    if plan.is_last {
        sqlx::query(
            "UPDATE orders SET status = 'settled', rev = rev + 1,
                               paid_total = paid_total + ?2, change_total = change_total + ?3,
                               settled_at = ?4, updated_at = ?4
              WHERE id = ?1",
        )
        .bind(&req.order_id)
        .bind(paid)
        .bind(change)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;

        // 每一份印出去的時候還不知道總共幾份（「我先出 500」的下一句可能是
        // 「剩下他付」也可能是「再拆兩份」）。收到最後一筆才補齊，讓報表與
        // 日後查帳看到的是完整的 n/N。
        if plan.index > 1 {
            sqlx::query("UPDATE bills SET split_count = ?2 WHERE order_id = ?1")
                .bind(&req.order_id)
                .bind(plan.index)
                .execute(uow.conn())
                .await?;
        }
    } else {
        sqlx::query(
            "UPDATE orders SET rev = rev + 1, paid_total = paid_total + ?2,
                               change_total = change_total + ?3, updated_at = ?4
              WHERE id = ?1",
        )
        .bind(&req.order_id)
        .bind(paid)
        .bind(change)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

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
        if plan.is_last {
            "settled"
        } else {
            "part_settled"
        },
        Some(&head.status),
        if plan.is_last {
            Some("settled")
        } else {
            Some(&head.status)
        },
        ctx,
        &now,
    )
    .await?;

    // 桌位釋放 —— **只有在這是那一桌最後一張未結的單時**。
    //
    // 一桌可以有很多張單（分開結帳、續攤、加點開新單）。看到「結完帳就關檯」
    // 很直覺，但它會把同桌其他還沒結的單留在一個已關的 session 上：那些單
    // 從桌位圖上消失，帳卻還在。桌位圖上看不到的帳等於收不到的錢。
    if let (true, Some(sid)) = (plan.is_last, &head.table_session_id) {
        let still_open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM orders
              WHERE table_session_id = ?1 AND id <> ?2
                AND status NOT IN ('settled', 'voided')",
        )
        .bind(sid)
        .bind(&req.order_id)
        .fetch_one(uow.conn())
        .await?;
        if still_open == 0 {
            sqlx::query(
                "UPDATE table_sessions SET status = 'closed', closed_at = ?2, closed_by = ?3,
                                           updated_at = ?2
                  WHERE id = ?1",
            )
            .bind(sid)
            .bind(now.iso())
            .bind(&ctx.actor.user_id)
            .execute(uow.conn())
            .await?;
        }
    }

    let remaining_after = grand_total - billed_so_far(&mut uow, &req.order_id).await?;
    enqueue_receipt(
        &mut uow,
        ctx,
        &store,
        &req.order_id,
        &bill_id,
        &bill_no,
        change,
        &plan,
        remaining_after,
        &now,
    )
    .await?;

    let order = load_order_view(&mut uow, &req.order_id).await?;
    let result = SettleResult {
        order,
        bill_no,
        change,
        remaining: remaining_after,
        split_index: plan.index,
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
    /// 作廢與稽核要記「動了多少錢」，所以 head 就要帶著它。
    grand_total: i64,
}

async fn load_order_head(uow: &mut SqliteUow, id: &str) -> AppResult<OrderHead> {
    let r = sqlx::query(
        "SELECT rev, status, channel, business_date, table_session_id, grand_total
           FROM orders WHERE id = ?1",
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
        grand_total: r.get("grand_total"),
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
    /// 這一行的來源（一般品項 / 方案本身 / 方案內 0 元品項）。
    origin: crate::services::dining::LineOrigin,
    /// 屬於哪個吃到飽方案。方案結束後仍留著 —— 帳要看得出當時是吃到飽。
    dining_plan_id: Option<String>,
}

async fn resolve_line(
    uow: &mut SqliteUow,
    l: &NewLine,
    plan: Option<&crate::services::dining::DiningPlan>,
) -> AppResult<ResolvedLine> {
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
        return Err(AppError::Validation(format!("「{name}」已售完").into()));
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

    // ── 吃到飽：方案內的品項是 0 元 ──────────────────────────────
    //
    // ★ 它仍然是一行，只是不收錢。廚房要知道要做什麼，所以不能不建立這一行；
    //   Airレジ 也是這樣做的（手持機上印「（放）」與 ¥0）。
    //
    // ★ 規格的加價也一起歸零。「大杯珍奶」在吃到飽方案裡不該因為是大杯就收 10 元 ——
    //   客人付的是吃到飽的錢，不是單杯的錢。
    //
    // ★ 不在方案裡的照常收錢。那就是加價品（和牛 +200、酒水另計），
    //   而它是**預設行為**不是特例 —— 沒有一行程式碼在處理它。
    let category_id: Option<String> = r.get("category_id");
    let mut origin = crate::services::dining::LineOrigin::Item;
    let mut dining_plan_id = None;
    if let Some(p) = plan {
        if p.covers(&l.item_id, category_id.as_deref()) {
            unit_price = 0;
            origin = crate::services::dining::LineOrigin::PlanMember;
            dining_plan_id = Some(p.id.clone());
        } else if l.item_id == p.item_id {
            // 方案本身。人頭費就是這一行 × 人數。
            origin = crate::services::dining::LineOrigin::Plan;
            dining_plan_id = Some(p.id.clone());
        }
    }

    // ★ 選項要真的是**這個品項提供的**。
    //
    //   只檢查「這個選項存在而且沒停用」是不夠的：那樣就可以把「加珍珠」
    //   掛到滷肉飯上。今天唯一的呼叫端是收銀機，所以不構成漏洞；但 v1.3 的
    //   掃碼點餐會把下單這條路開到客人的手機上，而那時再補會是一個
    //   「已經有人這樣點過了」的補。
    //
    //   價格一律由伺服器決定、組合也一律由伺服器驗 —— 這是同一條原則。
    let mut modifiers = Vec::new();
    for mid in &l.modifier_ids {
        let m = sqlx::query(
            "SELECT m.id, m.name, m.price, m.group_id, g.name AS group_name,
                    g.selection_type,
                    EXISTS(SELECT 1 FROM item_modifier_groups img
                            WHERE img.item_id = ?2 AND img.group_id = m.group_id) AS offered
               FROM modifiers m JOIN modifier_groups g ON g.id = m.group_id
              WHERE m.id = ?1 AND m.deleted_at IS NULL AND m.is_active = 1
                AND g.deleted_at IS NULL",
        )
        .bind(mid)
        .bind(&l.item_id)
        .fetch_optional(uow.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("這個選項已經停用了".into()))?;

        if m.get::<i64, _>("offered") == 0 {
            return Err(AppError::Validation(
                format!(
                    "「{}」沒有提供「{}」這個選項。請重新整理菜單再試一次。",
                    r.get::<String, _>("name"),
                    m.get::<String, _>("name")
                )
                .into(),
            ));
        }
        modifiers.push((
            m.get::<String, _>("id"),
            m.get::<String, _>("group_name"),
            m.get::<String, _>("name"),
            m.get::<i64, _>("price"),
            m.get::<String, _>("group_id"),
            m.get::<String, _>("selection_type"),
        ));
    }

    // 必選的群組沒選到時的處理，刻意分成兩種：
    //
    // * 這一組**有預設**（正常糖、正常冰）→ 用店家自己寫下來的那個答案。
    //   伺服器沒有在猜 —— 它用的是店家設定的預設值，跟收銀機畫面預先勾起來的
    //   是同一個。這樣任何程式化的呼叫端（匯入、外送平台、之後的掃碼點餐）
    //   都不必知道每一組的預設是什麼。
    // * 這一組**沒有預設** → 擋下來。店家從來沒說過「標準」是什麼，
    //   那就沒有人猜得出來，而猜錯要重做一杯。
    //
    // 單選群組被塞了兩個一律擋 —— 那不是「忘了選」，是不可能成立的組合。
    let required = sqlx::query(
        "SELECT g.id, g.name, g.selection_type, COALESCE(img.min_select, g.min_select) AS min_select
           FROM item_modifier_groups img
           JOIN modifier_groups g ON g.id = img.group_id AND g.deleted_at IS NULL
          WHERE img.item_id = ?1
          ORDER BY img.sort_order",
    )
    .bind(&l.item_id)
    .fetch_all(uow.conn())
    .await?;
    for g in &required {
        let gid: String = g.get("id");
        let picked = modifiers.iter().filter(|m| m.4 == gid).count() as i64;

        if g.get::<String, _>("selection_type") == "single" && picked > 1 {
            return Err(AppError::Validation(
                format!("「{}」只能選一個。", g.get::<String, _>("name")).into(),
            ));
        }

        let min: i64 = g.get("min_select");
        if picked >= min {
            continue;
        }
        let fallback = sqlx::query(
            "SELECT id, name, price FROM modifiers
              WHERE group_id = ?1 AND is_default = 1 AND is_active = 1 AND deleted_at IS NULL
              ORDER BY sort_order LIMIT 1",
        )
        .bind(&gid)
        .fetch_optional(uow.conn())
        .await?;
        match fallback {
            Some(d) => modifiers.push((
                d.get::<String, _>("id"),
                g.get::<String, _>("name"),
                d.get::<String, _>("name"),
                d.get::<i64, _>("price"),
                gid,
                "single".to_string(),
            )),
            None => {
                return Err(AppError::Validation(
                    format!(
                        "「{}」要選「{}」。",
                        r.get::<String, _>("name"),
                        g.get::<String, _>("name")
                    )
                    .into(),
                ))
            }
        }
    }

    let modifiers: Vec<(String, String, String, i64)> = modifiers
        .into_iter()
        .map(|(id, group, name, price, _, _)| (id, group, name, price))
        .collect();

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
        origin,
        dining_plan_id,
    })
}

/// 回傳新建立的那一行的 id。
///
/// 呼叫端需要它：加點單**只能印這一次新增的行**（見 `enqueue_kitchen_ticket`）。
/// 資料庫存的折扣種類 → 定價引擎的型別。
///
/// 認不得的種類當成「沒有折扣」而不是報錯：一筆壞掉的折扣資料不該讓
/// 整張單算不出金額，而收銀員正站在客人面前。
fn discount_kind(type_snapshot: &str, value: i64) -> pricing::DiscountKind {
    match type_snapshot {
        "percent" => pricing::DiscountKind::Percent(value),
        "amount" => pricing::DiscountKind::Amount(value),
        "comp" => pricing::DiscountKind::Comp,
        _ => pricing::DiscountKind::Amount(0),
    }
}

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
                                  line_origin, dining_plan_id,
                                  created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?11, ?14, ?16,
                 'pending', ?17, ?18, NULL, ?15, ?15)",
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
    .bind(r.origin.as_str())
    .bind(&r.dining_plan_id)
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

        // 這一行自己的折扣。定價引擎會先扣它，再算整單折扣的分攤。
        let line_discounts = sqlx::query(
            "SELECT type_snapshot, value_snapshot FROM order_discounts
              WHERE order_item_id = ?1 ORDER BY created_at, id",
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
            discounts: line_discounts
                .iter()
                .map(|d| pricing::LineDiscount {
                    kind: discount_kind(
                        &d.get::<String, _>("type_snapshot"),
                        d.get("value_snapshot"),
                    ),
                    max_amount: None,
                })
                .collect(),
        });
        ids.push(id);
    }

    // 整單折扣（order_item_id 是 NULL 的那些）。
    let order_discount_rows = sqlx::query(
        "SELECT type_snapshot, value_snapshot FROM order_discounts
          WHERE order_id = ?1 AND order_item_id IS NULL ORDER BY created_at, id",
    )
    .bind(order_id)
    .fetch_all(uow.conn())
    .await?;
    let order_discounts: Vec<pricing::OrderDiscount> = order_discount_rows
        .iter()
        .map(|d| pricing::OrderDiscount {
            kind: discount_kind(
                &d.get::<String, _>("type_snapshot"),
                d.get("value_snapshot"),
            ),
            max_amount: None,
            // 台灣兩種做法都存在（打折後收 10%、或按原價收 10%）。
            // 預設「折扣後才算服務費」—— 對客人比較有利的那一種。
            before_service_charge: true,
        })
        .collect();

    let out = pricing::compute(&PricingInput {
        channel,
        service_charge_rate_bp: service_rate_for(store, channel),
        rounding: store.rounding,
        tax_rate_bp: store.tax_rate_bp,
        lines,
        order_discounts,
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

    // 分項分帳已經結掉的那幾行。畫面上要能把它們畫成「已結」——
    // 不然第二個人會再選一次，然後才被伺服器擋下來。
    let paid_lines: Vec<String> = sqlx::query_scalar(
        "SELECT bl.order_item_id FROM bill_lines bl
           JOIN bills b ON b.id = bl.bill_id
          WHERE b.order_id = ?1 AND b.status = 'settled'",
    )
    .bind(id)
    .fetch_all(uow.conn())
    .await?;

    let mut lines = Vec::with_capacity(items.len());
    for it in &items {
        let line_id: String = it.get("id");
        let line_id_for_paid = line_id.clone();
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
            paid: paid_lines.contains(&line_id_for_paid),
        });
    }

    // 分帳進度。畫面要靠它算出「還差多少」與下一份是第幾份。
    let bills = sqlx::query(
        "SELECT COALESCE(SUM(grand_total), 0) AS billed, COUNT(*) AS n,
                MAX(CASE WHEN split_mode <> 'none' THEN split_mode END) AS mode,
                MAX(CASE WHEN split_count > 0 THEN split_count END) AS parts
           FROM bills WHERE order_id = ?1 AND status = 'settled'",
    )
    .bind(id)
    .fetch_one(uow.conn())
    .await?;

    // 每人低消。單獨查一次而不是塞進上面那條 JOIN —— `stores` 只有一列，
    // 而把設定混進訂單查詢會讓「這個欄位到底屬於誰」變得看不出來。
    let min_per_head: i64 =
        sqlx::query_scalar("SELECT min_charge_per_head FROM stores WHERE deleted_at IS NULL ORDER BY id LIMIT 1")
            .fetch_optional(uow.conn())
            .await?
            .unwrap_or(0);
    let channel: String = r.get("channel");
    let guests: i64 = r.get("guest_count");
    let subtotal: i64 = r.get("subtotal");
    // 低消看的是**點了多少東西**（subtotal），不是最後收多少：
    // 服務費是店家加的、抹零是店家讓的，兩者都不該算進客人的消費額。
    // 而低消是桌位政策，外帶外送沒有這回事。
    let min_charge_shortfall = if channel == channel_str(Channel::DineIn)
        && min_per_head > 0
        && guests > 0
    {
        (min_per_head * guests - subtotal).max(0)
    } else {
        0
    };

    Ok(OrderView {
        id: r.get("id"),
        order_no: r.get("order_no"),
        status: r.get("status"),
        rev: r.get("rev"),
        channel,
        table_id: r.get("table_id"),
        table_label: r.get("table_code"),
        guest_count: guests,
        business_date: r.get("business_date"),
        lines,
        subtotal,
        service_charge: r.get("service_charge"),
        rounding_adjustment: r.get("rounding_adjustment"),
        grand_total: r.get("grand_total"),
        sales_amount: r.get("sales_amount"),
        tax_amount: r.get("tax_amount"),
        paid_total: r.get("paid_total"),
        change_total: r.get("change_total"),
        min_charge_shortfall,
        billed_total: bills.get("billed"),
        bill_count: bills.get("n"),
        split_mode: bills.get("mode"),
        split_count: bills.get("parts"),
    })
}

/// 分帳試算：這一份要收多少。
///
/// # 為什麼要多一支指令，而不是在前端算
///
/// 因為「四個人分 101 元」的答案是 26/25/25/25，而它是**最大餘數法**算出來的。
/// 在 TypeScript 再實作一次同一套進位規則，就是在等兩邊哪天不一樣 ——
/// 而不一樣的那天，螢幕上的金額與資料庫裡的金額會差一元，沒有人找得到原因。
/// 本機 IPC 往返不到 1ms，遠比一個對不起來的收銀系統便宜。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitPreview {
    /// 這一份應收多少。
    pub due: i64,
    pub index: i64,
    pub count: Option<i64>,
    pub order_total: i64,
    /// 已經收掉多少。
    pub billed: i64,
    /// 收完這一份還差多少。
    pub remaining_after: i64,
}

pub async fn preview_split(
    ctx: &Ctx,
    order_id: String,
    split: Option<SplitReq>,
) -> AppResult<SplitPreview> {
    // 唯讀，但 plan_split 吃 UnitOfWork（分帳計畫要看得到同一份快照），
    // 所以開一個交易再丟掉。
    let mut uow = ctx.db.begin_write().await?;
    let grand_total: i64 = sqlx::query_scalar("SELECT grand_total FROM orders WHERE id = ?1")
        .bind(&order_id)
        .fetch_optional(uow.conn())
        .await?
        .ok_or_else(|| AppError::NotFound(format!("找不到訂單 {order_id}")))?;
    let billed = billed_so_far(&mut uow, &order_id).await?;
    let plan = plan_split(&mut uow, &order_id, grand_total, split.as_ref()).await?;
    uow.rollback().await?;

    Ok(SplitPreview {
        due: plan.due,
        index: plan.index,
        count: plan.count,
        order_total: grand_total,
        billed,
        remaining_after: grand_total - billed - plan.due,
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
/// 丟一張單進 outbox。交易內嚴禁外部 I/O，所以列印一律走這裡。
pub(crate) async fn enqueue_print(
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
                    // 這條路徑是印廚房單用的，出單機不在乎誰付了錢。
                    paid: false,
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
        split: None,
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

#[allow(clippy::too_many_arguments)]
async fn enqueue_receipt(
    uow: &mut SqliteUow,
    _ctx: &Ctx,
    store: &StoreConfig,
    order_id: &str,
    bill_id: &str,
    bill_no: &str,
    change: i64,
    plan: &SplitPlan,
    remaining_after: i64,
    now: &Stamp,
) -> AppResult<()> {
    // 分項分帳的收據只印**這個人點的東西**。印整桌的話，付錢的那位會
    // 對著一張跟自己金額對不起來的明細，然後開始問。
    let only: Option<std::collections::HashSet<String>> = if plan.mode == "by_item" {
        Some(plan.line_ids.iter().cloned().collect())
    } else {
        None
    };
    let mut data = build_ticket_data(
        uow,
        store,
        order_id,
        TicketReason::Settle,
        now,
        only.as_ref(),
    )
    .await?;
    data.change = change;
    if plan.mode != "none" {
        let (sales, tax) =
            crate::core::money::split_tax_inclusive(Money(plan.due), store.tax_rate_bp);
        data.split = Some(crate::receipt::templates::SplitLabel {
            index: plan.index,
            count: plan.count,
            part_total: plan.due,
            order_total: data.grand_total,
            remaining: remaining_after,
        });
        // 版型層只認 `grand_total`，所以把「這一份的錢」換進去，整單金額
        // 交給 SplitLabel 帶。小計／折扣／服務費歸零：那些是整張單的事，
        // 印在一份帳單上只會讓兩邊的數字對不起來。
        data.grand_total = plan.due;
        data.sales_amount = sales.0;
        data.tax_amount = tax.0;
        data.subtotal = plan.due;
        data.discount_total = 0;
        data.service_charge = 0;
        data.rounding_adjustment = 0;
    }
    let payments = sqlx::query(
        "SELECT method_name_snapshot, amount, tendered
           FROM payments WHERE bill_id = (SELECT id FROM bills WHERE bill_no = ?2)
             AND order_id = ?1 ORDER BY id",
    )
    .bind(order_id)
    .bind(bill_no)
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
    // ★ 收據原稿存到帳單上。補印重送的就是它 ——
    //   不去翻列印佇列，是因為還沒接印表機時照樣結得了帳，而客人還是要收據。
    sqlx::query("UPDATE bills SET receipt_doc = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(bill_id)
        .bind(
            serde_json::to_string(&doc)
                .map_err(|e| AppError::Internal(format!("收據序列化失敗：{e}")))?,
        )
        .bind(now.iso())
        .execute(uow.conn())
        .await?;

    // billId 是補印用的：分帳之後一張訂單有好幾張帳單，只靠 orderId
    // 找不回客人手上是哪一份。
    let payload = serde_json::json!({
        "orderId": order_id,
        "billId": bill_id,
        "billNo": bill_no,
        "doc": doc,
    });
    enqueue_print(uow, "print.receipt", &business_date, &payload, now).await
}

/// 這一次要收多少、算第幾份。
struct SplitPlan {
    /// 這一份應收多少。
    due: i64,
    index: i64,
    /// 總共幾份。按金額分帳時收到最後一筆才知道，所以是 Option。
    count: Option<i64>,
    /// bills.split_mode
    mode: &'static str,
    /// 只有 by_item 有：這一份含哪幾行。
    line_ids: Vec<String>,
    /// 結完這一份，整張單就付清了。
    is_last: bool,
}

/// 算出這一次結帳要收多少。
///
/// # 這裡唯一不能出錯的事
///
/// **Σ 每一份 == 訂單總額，嚴格相等。** 差一元的話，不是店家吃掉就是客人多付，
/// 而且它會出現在日結的現金差異裡卻找不到原因。所以平分走最大餘數法
/// （四個人分 101 是 26/25/25/25，不是四個 25），按品項走每一行的
/// `taxable_amount`（定價引擎保證它的加總嚴格等於總額）。
async fn plan_split(
    uow: &mut SqliteUow,
    order_id: &str,
    grand_total: i64,
    split: Option<&SplitReq>,
) -> AppResult<SplitPlan> {
    let row = sqlx::query(
        "SELECT COALESCE(SUM(grand_total), 0) AS billed, COUNT(*) AS n
           FROM bills WHERE order_id = ?1 AND status = 'settled'",
    )
    .bind(order_id)
    .fetch_one(uow.conn())
    .await?;
    let billed: i64 = row.get("billed");
    let done: i64 = row.get("n");
    let remaining = grand_total - billed;
    if remaining <= 0 {
        return Err(AppError::Conflict("這張單已經結清了".into()));
    }

    let Some(split) = split else {
        // 沒指定就是「把剩下的全部結掉」。分帳分到一半按一般結帳，
        // 收的就是尾款 —— 這正是店員最後那一下會做的事。
        return Ok(SplitPlan {
            due: remaining,
            index: done + 1,
            count: Some(done + 1),
            mode: if done > 0 { "by_amount" } else { "none" },
            line_ids: vec![],
            is_last: true,
        });
    };

    match split {
        SplitReq::Even { parts } => {
            let parts = *parts;
            if parts < 2 {
                return Err(AppError::Validation("平分至少要兩份".into()));
            }
            if done >= parts {
                return Err(AppError::Conflict(format!(
                    "這張單已經分成 {done} 份結完了"
                )));
            }
            // 換模式要擋下來：混著分會讓「第幾份」對不上金額。
            ensure_same_split_mode(uow, order_id, done, "even", Some(parts)).await?;

            let shares = crate::core::money::allocate(grand_total, &vec![1; parts as usize]);
            let due = shares[done as usize];
            Ok(SplitPlan {
                due,
                index: done + 1,
                count: Some(parts),
                mode: "even",
                line_ids: vec![],
                is_last: done + 1 == parts,
            })
        }
        SplitReq::Amount { amount } => {
            let amount = *amount;
            if amount <= 0 {
                return Err(AppError::Validation("分帳金額要大於 0".into()));
            }
            if amount > remaining {
                return Err(AppError::Validation(
                    format!("這張單只剩 {remaining} 元沒結，收不了 {amount} 元").into(),
                ));
            }
            ensure_same_split_mode(uow, order_id, done, "by_amount", None).await?;
            Ok(SplitPlan {
                due: amount,
                index: done + 1,
                count: if amount == remaining {
                    Some(done + 1)
                } else {
                    None
                },
                mode: "by_amount",
                line_ids: vec![],
                is_last: amount == remaining,
            })
        }
        SplitReq::Items { line_ids } => {
            if line_ids.is_empty() {
                return Err(AppError::Validation("請先選要結哪幾項".into()));
            }
            ensure_same_split_mode(uow, order_id, done, "by_item", None).await?;

            let paid: Vec<String> = sqlx::query_scalar(
                "SELECT bl.order_item_id FROM bill_lines bl
                   JOIN bills b ON b.id = bl.bill_id
                  WHERE b.order_id = ?1 AND b.status = 'settled'",
            )
            .bind(order_id)
            .fetch_all(uow.conn())
            .await?;

            let holes = (0..line_ids.len())
                .map(|i| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT id, name_snapshot, taxable_amount FROM order_items
                  WHERE order_id = ?1 AND voided_at IS NULL AND id IN ({holes})"
            );
            let mut q = sqlx::query(&sql).bind(order_id);
            for id in line_ids {
                q = q.bind(id);
            }
            let rows = q.fetch_all(uow.conn()).await?;
            if rows.len() != line_ids.len() {
                return Err(AppError::Validation(
                    "有品項不在這張單上（或已經退掉了），請重新整理".into(),
                ));
            }

            let mut due = 0i64;
            for r in &rows {
                let id: String = r.get("id");
                if paid.contains(&id) {
                    let name: String = r.get("name_snapshot");
                    return Err(AppError::Conflict(format!("「{name}」已經結過帳了")));
                }
                due += r.get::<i64, _>("taxable_amount");
            }
            if due <= 0 {
                return Err(AppError::Validation(
                    "這幾項的金額是 0，不需要結帳（招待的品項請直接跟著整單結）".into(),
                ));
            }
            Ok(SplitPlan {
                due,
                index: done + 1,
                count: if due == remaining {
                    Some(done + 1)
                } else {
                    None
                },
                mode: "by_item",
                line_ids: line_ids.clone(),
                is_last: due == remaining,
            })
        }
    }
}

/// 同一張訂單不能混著兩種分法。
///
/// 混了之後「第 2 份」到底是平分的四分之一還是某個金額，沒有人說得準 ——
/// 而客人正在等著知道自己要付多少。
async fn ensure_same_split_mode(
    uow: &mut SqliteUow,
    order_id: &str,
    done: i64,
    want: &str,
    want_count: Option<i64>,
) -> AppResult<()> {
    if done == 0 {
        return Ok(());
    }
    let row = sqlx::query(
        "SELECT split_mode, split_count FROM bills
          WHERE order_id = ?1 AND status = 'settled'
          ORDER BY split_index DESC LIMIT 1",
    )
    .bind(order_id)
    .fetch_optional(uow.conn())
    .await?;
    let Some(row) = row else { return Ok(()) };
    let mode: String = row.get("split_mode");
    let count: i64 = row.get("split_count");
    if mode != want {
        // 這兩句話是**組出來的句子**，不是表格裡的一個欄位，所以它們留在後端 ——
        // 但文字本身搬進 i18n 目錄，三種語言擺在一起，少一種編譯就不會過。
        //
        // 這裡就 render 而不是把 `Msg` 帶出去，是因為 `AppError` 目前吃的還是
        // `String`（一百多個建構點，不是這一條任務該一次改完的東西）。
        // 用 `current()` 而不是 `Display`：`Display` 固定是中文，那等於在這裡
        // 就把語言寫死。等 `AppError` 改成帶 `Msg`（見 `i18n.rs` 的模組說明），
        // 這一行只要把 `.render(...)` 拿掉。
        return Err(AppError::Conflict(
            Msg::keyed(split_mode_label(&mode), Vec::new()).render(crate::i18n::current()),
        ));
    }
    if let (Some(want), true) = (want_count, count > 0) {
        if want != count {
            return Err(AppError::Conflict(
                msg!("order.split_count_locked", count = count, want = want)
                    .render(crate::i18n::current()),
            ));
        }
    }
    Ok(())
}

/// 這一種分法對應的訊息鍵。
///
/// 以前這裡回的是中文（「平分」「分項」），再 `format!` 進句子裡。那樣的句子
/// 翻不了 —— 就算外面那一句進了目錄，中文的分法名還是會原封不動留在英文與
/// 日文的句子裡。`Msg` 的參數只放資料（數字、代碼），不放已經寫死語言的字。
///
/// 所以現在**分法的名字跟整句話一起**放在 i18n 目錄裡（`order.split_mode_locked_*`），
/// 這個函式只負責挑出是哪一種分法。四種分法各一條句子，翻譯的人看得到整句。
fn split_mode_label(mode: &str) -> &'static str {
    match mode {
        "even" => "order.split_mode_locked_even",
        "by_item" => "order.split_mode_locked_by_item",
        "by_amount" => "order.split_mode_locked_by_amount",
        _ => "order.split_mode_locked_whole",
    }
}

/// 這張訂單已經收進去多少（所有已結的帳單加總）。
async fn billed_so_far(uow: &mut SqliteUow, order_id: &str) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(SUM(grand_total), 0) FROM bills
          WHERE order_id = ?1 AND status = 'settled'",
    )
    .bind(order_id)
    .fetch_one(uow.conn())
    .await?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;

    /// 分法的訊息鍵一定要真的在目錄裡。
    ///
    /// 打錯一個字不會編譯失敗，也不會 panic —— `render` 查不到就把鍵名原樣
    /// 吐出來，於是收銀員在螢幕上看到的是 `order.split_mode_locked_even`。
    /// 那是一種只有店家會遇到、我們自己永遠不會遇到的壞法。
    #[test]
    fn every_split_mode_has_a_real_sentence_in_all_three_languages() {
        for mode in ["even", "by_item", "by_amount", "none", "沒見過的分法"] {
            let key = split_mode_label(mode);
            let m = Msg::keyed(key, Vec::new());
            for l in Locale::ALL {
                let s = m.render(l);
                assert_ne!(s, key, "{key} 在 {} 查不到，畫面上會出現鍵名", l.as_str());
                assert!(!s.is_empty(), "{key} 在 {} render 出空字串", l.as_str());
            }
        }
    }
}
