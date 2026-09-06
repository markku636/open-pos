//! 折扣、招待與作廢的整合測試。
//!
//! 這一份守的是**防弊**而不是功能：
//!
//! * 折扣有沒有真的算進金額（沒有的話，打了折卻收原價）
//! * 招待與折扣是不是兩個權限（混在一起，「送人情」就藏進「行銷成效」裡）
//! * 結帳後作廢有沒有留下簽核與稽核（結完帳再作廢＝私吞現金，
//!   這是餐飲業最大的防弊點）

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::{demo, menu, order};
use sqlx::Row;

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
        "openpos_disc_{}_{}_{}",
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

impl Env {
    async fn order_with(&self, items: &[&str]) -> order::OrderView {
        let tree = menu::menu_tree(&self.ctx).await.unwrap();
        let find = |name: &str| {
            tree.categories
                .iter()
                .flat_map(|c| c.items.iter())
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("菜單裡沒有 {name}"))
                .id
                .clone()
        };
        let o = order::open_order(
            &self.ctx,
            order::OpenOrderReq {
                channel: Channel::Takeout,
                table_id: None,
                guest_count: None,
                client_id: None,
            },
        )
        .await
        .unwrap();
        order::add_lines(
            &self.ctx,
            order::AddLinesReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                lines: items
                    .iter()
                    .map(|n| order::NewLine {
                        item_id: find(n),
                        variant_id: None,
                        modifier_ids: vec![],
                        qty_milli: None,
                        note: None,
                    })
                    .collect(),
            },
        )
        .await
        .unwrap()
    }
}

fn discount(order_id: &str, rev: i64, kind: &str, value: i64) -> order::DiscountReq {
    order::DiscountReq {
        order_id: order_id.into(),
        expected_rev: rev,
        line_id: None,
        kind: kind.into(),
        value,
        reason_id: None,
        note: None,
        approver_id: None,
    }
}

/// ★ 整單折扣要真的算進金額。
#[tokio::test]
async fn an_order_discount_changes_what_the_customer_pays() {
    let e = env("orderdisc").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();

    let o = e.order_with(&["珍珠奶茶", "滷肉飯"]).await; // 60 + 45 = 105
    assert_eq!(o.grand_total, 105);

    // 打 9 折。
    let o = order::apply_discount(&e.ctx, discount(&o.id, o.rev, "percent", 9000))
        .await
        .unwrap();
    // 105 打 9 折：折扣額 10.5 元，四捨五入成 11 → 收 94。
    // 折扣額本身四捨五入（而不是先算應收再四捨五入）是刻意的：
    // 尾數落在客人這一邊，而不是店家這一邊。
    assert_eq!(o.grand_total, 94, "9 折之後應該是 94");
    // sales + tax == total 是財政部的硬檢核，折扣之後也必須成立。
    assert_eq!(o.sales_amount + o.tax_amount, o.grand_total);

    e.ctx.db.close().await;
}

#[tokio::test]
async fn a_fixed_amount_discount_is_subtracted() {
    let e = env("amount").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["雞腿便當"]).await; // 110

    let o = order::apply_discount(&e.ctx, discount(&o.id, o.rev, "amount", 20))
        .await
        .unwrap();
    assert_eq!(o.grand_total, 90);
    assert_eq!(o.sales_amount + o.tax_amount, o.grand_total);

    e.ctx.db.close().await;
}

/// 單品折扣只動那一行。
#[tokio::test]
async fn a_line_discount_only_touches_that_line() {
    let e = env("linedisc").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶", "滷肉飯"]).await; // 60 + 45

    let target = o
        .lines
        .iter()
        .find(|l| l.name == "珍珠奶茶")
        .unwrap()
        .id
        .clone();
    let mut req = discount(&o.id, o.rev, "percent", 8000); // 8 折
    req.line_id = Some(target.clone());
    let o = order::apply_discount(&e.ctx, req).await.unwrap();

    // 60 × 0.8 = 48，加上 45 = 93
    assert_eq!(o.grand_total, 93, "{:?}", o.lines);
    let bubble = o.lines.iter().find(|l| l.name == "珍珠奶茶").unwrap();
    let rice = o.lines.iter().find(|l| l.name == "滷肉飯").unwrap();
    assert_eq!(bubble.amount, 48);
    assert_eq!(rice.amount, 45, "另一行不該被動到");

    e.ctx.db.close().await;
}

/// ★ 招待是把一整行變成 0。
#[tokio::test]
async fn a_comp_zeroes_the_line() {
    let e = env("comp").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶", "滷肉飯"]).await;

    let target = o
        .lines
        .iter()
        .find(|l| l.name == "珍珠奶茶")
        .unwrap()
        .id
        .clone();
    let mut req = discount(&o.id, o.rev, "comp", 0);
    req.line_id = Some(target);
    let o = order::apply_discount(&e.ctx, req).await.unwrap();

    assert_eq!(o.grand_total, 45, "招待那一行應該變成 0");
    assert_eq!(
        o.lines
            .iter()
            .find(|l| l.name == "珍珠奶茶")
            .unwrap()
            .amount,
        0
    );

    e.ctx.db.close().await;
}

#[tokio::test]
async fn a_nonsense_discount_is_refused() {
    let e = env("bad").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶"]).await;

    for (kind, value) in [
        ("percent", 0),
        ("percent", 20_000),
        ("amount", 0),
        ("amount", -5),
    ] {
        let err = order::apply_discount(&e.ctx, discount(&o.id, o.rev, kind, value))
            .await
            .unwrap_err();
        assert_eq!(err.code(), "ERR_VALIDATION", "{kind} {value} 應該被擋下來");
    }
    let err = order::apply_discount(&e.ctx, discount(&o.id, o.rev, "buy-one-get-one", 1))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    e.ctx.db.close().await;
}

/// 作廢整單：所有品項一起作廢，帳單也跟著作廢。
#[tokio::test]
async fn voiding_an_order_voids_its_lines_and_bill() {
    let e = env("void").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶", "滷肉飯"]).await;

    let o = order::void_order(
        &e.ctx,
        order::VoidOrderReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            reason_id: None,
            note: Some("客人不要了".into()),
            approver_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(o.status, "voided");
    assert!(o.lines.is_empty(), "作廢之後不該還有有效的品項");

    e.ctx.db.close().await;
}

/// ★ 結帳後作廢：餐飲業最大的防弊點。
///
/// 結完帳再作廢等於私吞現金，所以它必須留下**簽核紀錄**與**獨立的稽核動作碼**，
/// 而且沒有原因不准做。
#[tokio::test]
async fn voiding_after_settlement_leaves_a_trail() {
    let e = env("aftersettle").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["雞腿便當"]).await; // 110

    let o = {
        order::settle(
            &e.ctx,
            order::SettleReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                payments: vec![order::PaymentReq {
                    method_code: "cash".into(),
                    amount: o.grand_total,
                    tendered: Some(110),
                    ref_no: None,
                }],
                idem_key: "settle-1".into(),
                split: None,
            },
        )
        .await
        .unwrap();
        order::get_order(&e.ctx, &o.id).await.unwrap()
    };
    assert_eq!(o.status, "settled");

    // 沒有原因不准做 —— 沒有原因的作廢是查不動的。
    let err = order::void_order(
        &e.ctx,
        order::VoidOrderReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            reason_id: None,
            note: None,
            approver_id: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");
    assert!(err.message().contains("原因"), "{}", err.message());

    // 挑一個作廢原因。
    let reason_id: String = sqlx::query_scalar(
        "SELECT id FROM reason_codes WHERE kind = 'void' ORDER BY sort_order LIMIT 1",
    )
    .fetch_one(e.ctx.db.reader())
    .await
    .unwrap();

    let voided = order::void_order(
        &e.ctx,
        order::VoidOrderReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            reason_id: Some(reason_id),
            note: Some("結完帳才發現點錯".into()),
            approver_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(voided.status, "voided");

    // ★ 簽核紀錄。日結時要看得到這一筆。
    let approvals: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM approvals WHERE ref_id = ?1 AND action_code = 'order.void.after_settle'",
    )
    .bind(&o.id)
    .fetch_one(e.ctx.db.reader())
    .await
    .unwrap();
    assert_eq!(approvals, 1, "結帳後作廢沒有留下簽核紀錄");

    // ★ 稽核用的是**獨立的動作碼**。混進一般作廢裡就統計不出來了。
    let audit = sqlx::query(
        "SELECT action, amount_delta FROM audit_logs WHERE entity_id = ?1 AND action = 'void_after_settle'",
    )
    .bind(&o.id)
    .fetch_optional(e.ctx.db.reader())
    .await
    .unwrap()
    .expect("沒有寫進稽核");
    assert_eq!(
        audit.get::<i64, _>("amount_delta"),
        -110,
        "稽核要記得動了多少錢，而且是負的"
    );

    e.ctx.db.close().await;
}

/// 已經作廢的單不能再作廢一次。
#[tokio::test]
async fn a_voided_order_cannot_be_voided_again() {
    let e = env("twice").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let o = e.order_with(&["珍珠奶茶"]).await;

    let o = order::void_order(
        &e.ctx,
        order::VoidOrderReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            reason_id: None,
            note: None,
            approver_id: None,
        },
    )
    .await
    .unwrap();
    let err = order::void_order(
        &e.ctx,
        order::VoidOrderReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            reason_id: None,
            note: None,
            approver_id: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");

    e.ctx.db.close().await;
}
