//! 吃到飽方案的端到端測試。
//!
//! 設計依據見 `docs/dining-modes.md`（查了 25 套市售產品）。這裡驗的是那份
//! 設計最核心的三句話：
//!
//! 1. 人頭費就是**方案商品點 N 份**，它是一般的明細行 —— 所以稅、分帳、
//!    報表全部自動適用，定價引擎一行都不用改。
//! 2. 方案內的品項是 **0 元但仍然是一行** —— 廚房要知道要做什麼。
//! 3. 不在方案裡的**照常收錢**（加價品），而那是預設行為不是特例。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::dining::{self, PlanInput};
use open_pos::services::menu::{self, CategoryInput, ItemInput};
use open_pos::services::order::{self, AddLinesReq, NewLine, OpenOrderReq};

static SEQ: AtomicU32 = AtomicU32::new(0);

struct Env {
    root: std::path::PathBuf,
    ctx: Ctx,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn env(tag: &str) -> Env {
    let root = std::env::temp_dir().join(format!(
        "openpos_plan_{}_{}_{}",
        tag,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let layout = DataLayout::new(root.clone());
    layout.ensure().unwrap();
    let db = SqliteDb::open(&layout.db_file(), Some(2)).await.unwrap();
    let now = Stamp::now();
    let mut uow = db.begin_write().await.unwrap();
    open_pos::services::seed::apply(&mut uow, &now)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let actor = open_pos::services::seed::default_actor(&db).await.unwrap();
    Env {
        root,
        ctx: Arc::new(AppCtx {
            db,
            layout,
            started_at: now.at,
            actor,
        }),
    }
}

fn line(item_id: &str, qty: i64) -> NewLine {
    NewLine {
        item_id: item_id.into(),
        variant_id: None,
        qty_milli: Some(qty * 1000),
        modifier_ids: vec![],
        note: None,
    }
}

/// 一間吃到飽店：方案 599、飲料整類無限、和牛要加價。
struct Shop {
    plan_item: String,
    tea: String,
    wagyu: String,
    plan_id: String,
}

async fn seed_buffet(ctx: &Ctx) -> Shop {
    let mk_cat = |name: &str| CategoryInput {
        id: None,
        name: name.into(),
        color: None,
        sort_order: None,
        is_active: None,
    };
    let drinks = menu::upsert_category(ctx, mk_cat("飲料")).await.unwrap();
    let mains = menu::upsert_category(ctx, mk_cat("主食")).await.unwrap();

    let mk = |cat: &str, name: &str, price: i64| ItemInput {
        id: None,
        category_id: Some(cat.to_string()),
        name: name.into(),
        short_name: None,
        base_price: price,
        tax_code: None,
        is_open_price: None,
        sold_out_until: None,
        sort_order: None,
        is_active: None,
    };

    // 方案商品：599 一位。人頭費就是它。
    let plan_item = menu::upsert_item(ctx, mk(&mains.id, "晚餐吃到飽", 599))
        .await
        .unwrap();
    // 飲料整類都在方案裡。
    let tea = menu::upsert_item(ctx, mk(&drinks.id, "紅茶", 45))
        .await
        .unwrap();
    // 和牛不在方案裡 —— 加價品。
    let wagyu = menu::upsert_item(ctx, mk(&mains.id, "和牛", 200))
        .await
        .unwrap();

    let plan = dining::upsert(
        ctx,
        PlanInput {
            id: None,
            item_id: plan_item.id.clone(),
            name: "晚餐吃到飽".into(),
            limit_minutes: Some(120),
            notice_minutes: Some(30),
            print_members_on_bill: Some(false),
            is_active: Some(true),
            member_items: vec![],
            // 整個飲料分類無限暢飲 —— 不必一個一個勾，
            // 新增一款飲料也不必記得回來加。
            member_categories: vec![drinks.id.clone()],
        },
    )
    .await
    .unwrap();

    Shop {
        plan_item: plan_item.id,
        tea: tea.id,
        wagyu: wagyu.id,
        plan_id: plan.id,
    }
}

/// 把一張單的桌位 session 綁上方案（正式流程之後由開桌 UI 做）。
async fn apply_plan(ctx: &Ctx, order_id: &str, plan_id: &str) {
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await.unwrap();
    sqlx::query(
        "UPDATE table_sessions SET dining_plan_id = ?2, plan_started_at = ?3
          WHERE id = (SELECT table_session_id FROM orders WHERE id = ?1)",
    )
    .bind(order_id)
    .bind(plan_id)
    .bind(now.iso())
    .execute(uow.conn())
    .await
    .unwrap();
    uow.commit().await.unwrap();
}

async fn open_at_table(ctx: &Ctx, guests: i64) -> String {
    // 每次開一張新桌 —— 一桌同時只能有一個未關的 session（partial unique index），
    // 共用同一桌會讓第二個測試撞上那個約束。
    let code = format!("T{}", SEQ.fetch_add(1, Ordering::Relaxed));
    let table = open_pos::services::table::upsert_table(
        ctx,
        open_pos::services::table::TableInput {
            id: None,
            code,
            name: None,
            seats: Some(6),
            area_name: None,
            is_active: Some(true),
        },
    )
    .await
    .unwrap()
    .id;
    order::open_order(
        ctx,
        OpenOrderReq {
            channel: Channel::DineIn,
            table_id: Some(table),
            guest_count: Some(guests),
            client_id: None,
        },
    )
    .await
    .unwrap()
    .id
}

// ─────────────────────────────────────────────────────────────────────

/// ★ 主線：四位吃到飽，喝飲料不加錢，和牛要加錢。
#[tokio::test]
async fn a_buffet_table_pays_per_head_drinks_are_free_and_wagyu_is_not() {
    let e = env("main").await;
    let shop = seed_buffet(&e.ctx).await;

    let order_id = open_at_table(&e.ctx, 4).await;
    apply_plan(&e.ctx, &order_id, &shop.plan_id).await;

    let v = order::get_order(&e.ctx, &order_id).await.unwrap();

    // 人頭費 = 方案商品 × 4 位。市售產品全部是這樣做的
    // （Eats365 的 representing item、Airレジ 的方案名商品）。
    let v = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: order_id.clone(),
            expected_rev: v.rev,
            lines: vec![line(&shop.plan_item, 4)],
        },
    )
    .await
    .unwrap();
    assert_eq!(v.grand_total, 599 * 4, "四位 × 599");

    // 飲料在方案裡 → 0 元，但**仍然是一行**（廚房要知道要泡兩杯）。
    let v = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: order_id.clone(),
            expected_rev: v.rev,
            lines: vec![line(&shop.tea, 2)],
        },
    )
    .await
    .unwrap();
    assert_eq!(v.grand_total, 599 * 4, "喝飲料不該加錢");
    assert_eq!(v.lines.len(), 2, "0 元也要留一行給廚房");

    // 和牛不在方案裡 → 照常收錢。這是預設行為，沒有一行程式在處理它。
    let v = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: order_id.clone(),
            expected_rev: v.rev,
            lines: vec![line(&shop.wagyu, 1)],
        },
    )
    .await
    .unwrap();
    assert_eq!(v.grand_total, 599 * 4 + 200, "加價品要收錢");

    // ★ 稅的恆等式在吃到飽下仍然成立 —— 這是整個設計的前提：
    //   人頭費是明細行，所以它自動走完定價引擎。
    assert_eq!(
        v.sales_amount + v.tax_amount,
        v.grand_total,
        "銷售額 + 稅額必須等於總額，否則發票整批退件"
    );
}

/// 0 元的行不會把稅算壞。
#[tokio::test]
async fn zero_priced_plan_lines_do_not_break_the_tax_split() {
    let e = env("tax").await;
    let shop = seed_buffet(&e.ctx).await;
    let order_id = open_at_table(&e.ctx, 2).await;
    apply_plan(&e.ctx, &order_id, &shop.plan_id).await;

    let v = order::get_order(&e.ctx, &order_id).await.unwrap();
    let v = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: order_id.clone(),
            expected_rev: v.rev,
            lines: vec![line(&shop.plan_item, 2), line(&shop.tea, 5)],
        },
    )
    .await
    .unwrap();

    assert_eq!(v.grand_total, 599 * 2);
    assert_eq!(v.sales_amount + v.tax_amount, v.grand_total);

    // 每一行的可稅金額加起來要等於總額（發票的品項加總對得上總計）。
    let sum: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(taxable_amount), 0) FROM order_items
          WHERE order_id = ?1 AND voided_at IS NULL",
    )
    .bind(&order_id)
    .fetch_one(e.ctx.db.reader())
    .await
    .unwrap();
    assert_eq!(sum, v.grand_total, "Σ 每行可稅金額必須等於總額");
}

/// 沒有套方案的桌完全照舊 —— 這條在守「不影響單點店」。
#[tokio::test]
async fn a_table_without_a_plan_is_completely_unaffected() {
    let e = env("plain").await;
    let shop = seed_buffet(&e.ctx).await;

    let order_id = open_at_table(&e.ctx, 2).await;
    // 刻意不套方案。

    let v = order::get_order(&e.ctx, &order_id).await.unwrap();
    let v = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id,
            expected_rev: v.rev,
            lines: vec![line(&shop.tea, 1)],
        },
    )
    .await
    .unwrap();

    // 紅茶在「方案的成員清單」裡，但這桌沒有套方案 → 原價 45。
    assert_eq!(v.grand_total, 45, "沒套方案就是原價，方案不該外溢");
}
