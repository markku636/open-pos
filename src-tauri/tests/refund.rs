//! 退款的整合測試。
//!
//! 兩條主線：
//!
//! 1. **防弊** —— 收銀員不能自己退款、退款一定要有原因、要留簽核紀錄
//! 2. **對帳** —— 現金退款要從「應有現金」扣掉。少了這一條，每退一次款
//!    關班就短少一次，而數錢的人找不出原因；這種經驗發生兩三次之後，
//!    店員就會開始不信任盤點結果，而盤點是整套防弊的地基。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::rbac::Actor;
use open_pos::services::{demo, menu, order, refund, shift};

static SEQ: AtomicU32 = AtomicU32::new(0);

struct Env {
    root: std::path::PathBuf,
    ctx: Ctx,
    /// 老闆（有 payment.refund）。
    owner: Actor,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn env(tag: &str) -> Env {
    let root = std::env::temp_dir().join(format!(
        "openpos_refund_{}_{}_{}",
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
    let owner = open_pos::services::seed::default_actor(&db).await.unwrap();

    let e = Env {
        root,
        ctx: Arc::new(AppCtx {
            db,
            layout,
            started_at: now.at,
            actor: owner.clone(),
        }),
        owner,
    };
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e
}

impl Env {
    /// 換一個身分操作（同一個資料庫）。
    fn as_actor(&self, actor: Actor) -> Ctx {
        Arc::new(AppCtx {
            db: self.ctx.db.clone(),
            layout: self.ctx.layout.clone(),
            started_at: self.ctx.started_at,
            actor,
        })
    }

    /// 建一個收銀員。種子資料只有老闆，而防弊測試要的正是「別人做不到」。
    async fn cashier(&self) -> Actor {
        let id = open_pos::core::ids::Id::new().to_string();
        let now = Stamp::now();
        let mut uow = self.ctx.db.begin_write().await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, store_id, code, name, is_active, created_at, updated_at)
             SELECT ?1, s.id, ?2, '小美', 1, ?3, ?3 FROM stores s ORDER BY s.id LIMIT 1",
        )
        .bind(&id)
        .bind(format!("c{}", &id[..6]))
        .bind(now.iso())
        .execute(uow.conn())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id, created_at)
             SELECT ?1, r.id, ?2 FROM roles r WHERE r.name = 'cashier'",
        )
        .bind(&id)
        .bind(now.iso())
        .execute(uow.conn())
        .await
        .unwrap();
        uow.commit().await.unwrap();
        Actor {
            user_id: id,
            code: "c".into(),
            name: "小美".into(),
        }
    }

    async fn settled_bill(&self, items: &[&str], method: &str) -> refund::BillView {
        let tree = menu::menu_tree(&self.ctx).await.unwrap();
        let find = |name: &str| {
            tree.categories
                .iter()
                .flat_map(|c| c.items.iter())
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("菜單上沒有「{name}」"))
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
        let o = order::add_lines(
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
        .unwrap();
        let r = order::settle(
            &self.ctx,
            order::SettleReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                payments: vec![order::PaymentReq {
                    method_code: method.into(),
                    amount: o.grand_total,
                    tendered: Some(o.grand_total),
                    ref_no: None,
                }],
                idem_key: open_pos::core::ids::Id::new().to_string(),
                split: None,
            },
        )
        .await
        .unwrap();
        self.bill(&r.bill_no).await
    }

    async fn bill(&self, bill_no: &str) -> refund::BillView {
        refund::find_bills(
            &self.ctx,
            refund::FindBillsReq {
                business_date: None,
                bill_no: Some(bill_no.to_string()),
            },
        )
        .await
        .unwrap()
        .into_iter()
        .find(|b| b.bill_no == bill_no)
        .unwrap_or_else(|| panic!("找不到帳單 {bill_no}"))
    }

    async fn reason(&self, code: &str) -> String {
        sqlx::query_scalar("SELECT id FROM reason_codes WHERE kind = 'refund' AND code = ?1")
            .bind(code)
            .fetch_one(self.ctx.db.reader())
            .await
            .unwrap()
    }
}

/// ★ 收銀員自己退不了款。
#[tokio::test]
async fn a_cashier_needs_a_manager_to_refund() {
    let e = env("perm").await;
    let bill = e.settled_bill(&["珍珠奶茶"], "cash").await;
    let reason = e.reason("quality").await;
    let cashier = e.cashier().await;
    let as_cashier = e.as_actor(cashier);

    let err = refund::refund(
        &as_cashier,
        refund::RefundReq {
            bill_id: bill.id.clone(),
            payment_id: None,
            amount: 10,
            reason_id: Some(reason.clone()),
            note: None,
            approver_id: None,
            idem_key: open_pos::core::ids::Id::new().to_string(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_FORBIDDEN");

    // 主管授權之後就可以，而且簽核紀錄記的是**主管**。
    let r = refund::refund(
        &as_cashier,
        refund::RefundReq {
            bill_id: bill.id.clone(),
            payment_id: None,
            amount: 10,
            reason_id: Some(reason),
            note: None,
            approver_id: Some(e.owner.user_id.clone()),
            idem_key: open_pos::core::ids::Id::new().to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(r.amount, 10);

    let approved_by: String = sqlx::query_scalar(
        "SELECT approved_by FROM approvals WHERE ref_id = ?1 AND action_code = 'payment.refund'",
    )
    .bind(&bill.id)
    .fetch_one(e.ctx.db.reader())
    .await
    .unwrap();
    assert_eq!(approved_by, e.owner.user_id, "簽核要記在主管頭上");

    e.ctx.db.close().await;
}

#[tokio::test]
async fn a_refund_must_say_why() {
    let e = env("reason").await;
    let bill = e.settled_bill(&["珍珠奶茶"], "cash").await;
    let err = refund::refund(
        &e.ctx,
        refund::RefundReq {
            bill_id: bill.id,
            payment_id: None,
            amount: 10,
            reason_id: None,
            note: None,
            approver_id: None,
            idem_key: open_pos::core::ids::Id::new().to_string(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");
    e.ctx.db.close().await;
}

/// 退不了比收到的還多，而且退兩次也不能加起來超過。
#[tokio::test]
async fn you_cannot_refund_more_than_was_paid() {
    let e = env("over").await;
    let bill = e.settled_bill(&["珍珠奶茶"], "cash").await;
    let reason = e.reason("quality").await;
    let total = bill.grand_total;

    let err = e_refund(&e, &bill.id, total + 1, &reason)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    let r = e_refund(&e, &bill.id, total - 10, &reason).await.unwrap();
    assert_eq!(r.bill_status, "partially_refunded");
    assert_eq!(r.refundable, 10);

    let err = e_refund(&e, &bill.id, 11, &reason).await.unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    let r = e_refund(&e, &bill.id, 10, &reason).await.unwrap();
    assert_eq!(r.bill_status, "refunded");
    assert_eq!(r.refundable, 0);
    assert_eq!(r.refunded_total, total);

    e.ctx.db.close().await;
}

/// ★ 現金退款要從「應有現金」扣掉。
///
/// 少了這一條，每退一次款關班就短少一次 —— 而數錢的人找不出原因。
#[tokio::test]
async fn a_cash_refund_lowers_the_expected_cash() {
    let e = env("cash").await;
    shift::open_shift(
        &e.ctx,
        shift::OpenShiftReq {
            opening_float: 1000,
            counts: None,
            note: None,
        },
    )
    .await
    .unwrap();

    let bill = e.settled_bill(&["珍珠奶茶"], "cash").await;
    let before = shift::x_report(&e.ctx).await.unwrap();
    assert_eq!(before.cash.expected, 1000 + bill.grand_total);
    assert_eq!(before.cash.cash_refunds, 0);

    let reason = e.reason("quality").await;
    e_refund(&e, &bill.id, 20, &reason).await.unwrap();

    let after = shift::x_report(&e.ctx).await.unwrap();
    assert_eq!(after.cash.cash_refunds, 20);
    assert_eq!(
        after.cash.expected,
        1000 + bill.grand_total - 20,
        "退出去的現金不扣掉的話，關班會憑空短少 20 元"
    );
    assert_eq!(after.refunds.count, 1);
    assert_eq!(after.refunds.amount, 20);
    // 營業額不動：東西確實賣出去了，退款是另一件事。
    assert_eq!(after.sales.total, before.sales.total);

    e.ctx.db.close().await;
}

/// 刷卡退款不動抽屜 —— 那筆錢從來沒有進過抽屜。
#[tokio::test]
async fn a_card_refund_does_not_touch_the_drawer() {
    let e = env("card").await;
    shift::open_shift(
        &e.ctx,
        shift::OpenShiftReq {
            opening_float: 1000,
            counts: None,
            note: None,
        },
    )
    .await
    .unwrap();

    let bill = e.settled_bill(&["珍珠奶茶"], "credit").await;
    let reason = e.reason("quality").await;
    e_refund(&e, &bill.id, 20, &reason).await.unwrap();

    let r = shift::x_report(&e.ctx).await.unwrap();
    assert_eq!(r.cash.cash_refunds, 0, "刷卡退款不該碰現金");
    assert_eq!(r.cash.expected, 1000);
    assert_eq!(r.refunds.amount, 20, "但它仍然是一筆退款");

    e.ctx.db.close().await;
}

/// ★ 混合支付時不替店家決定退哪一筆。
///
/// 猜錯的話帳面兩邊都平，抽屜裡的錢卻對不上 —— 而那正是「刷卡收現金退」
/// 這種內神通外鬼最方便的掩護。
#[tokio::test]
async fn a_mixed_payment_bill_asks_which_payment_to_refund() {
    let e = env("mixed").await;
    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    let item = tree
        .categories
        .iter()
        .flat_map(|c| c.items.iter())
        .find(|i| i.name == "珍珠奶茶")
        .unwrap()
        .id
        .clone();
    let o = order::open_order(
        &e.ctx,
        order::OpenOrderReq {
            channel: Channel::Takeout,
            table_id: None,
            guest_count: None,
            client_id: None,
        },
    )
    .await
    .unwrap();
    let o = order::add_lines(
        &e.ctx,
        order::AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![order::NewLine {
                item_id: item,
                variant_id: None,
                modifier_ids: vec![],
                qty_milli: None,
                note: None,
            }],
        },
    )
    .await
    .unwrap();
    let r = order::settle(
        &e.ctx,
        order::SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![
                order::PaymentReq {
                    method_code: "credit".into(),
                    amount: 40,
                    tendered: None,
                    ref_no: None,
                },
                order::PaymentReq {
                    method_code: "cash".into(),
                    amount: o.grand_total - 40,
                    tendered: Some(o.grand_total - 40),
                    ref_no: None,
                },
            ],
            idem_key: open_pos::core::ids::Id::new().to_string(),
            split: None,
        },
    )
    .await
    .unwrap();

    let bill = e.bill(&r.bill_no).await;
    assert_eq!(bill.payments.len(), 2);
    let reason = e.reason("quality").await;

    let err = e_refund(&e, &bill.id, 10, &reason).await.unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    // 指定那一筆就可以，而且不能退超過**那一筆**的金額。
    let card = bill
        .payments
        .iter()
        .find(|p| p.method_code == "credit")
        .unwrap();
    let err = refund::refund(
        &e.ctx,
        refund::RefundReq {
            bill_id: bill.id.clone(),
            payment_id: Some(card.id.clone()),
            amount: 41,
            reason_id: Some(reason.clone()),
            note: None,
            approver_id: None,
            idem_key: open_pos::core::ids::Id::new().to_string(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    let done = refund::refund(
        &e.ctx,
        refund::RefundReq {
            bill_id: bill.id.clone(),
            payment_id: Some(card.id.clone()),
            amount: 40,
            reason_id: Some(reason),
            note: None,
            approver_id: None,
            idem_key: open_pos::core::ids::Id::new().to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(done.method_name, "信用卡");

    e.ctx.db.close().await;
}

/// 同一個冪等鍵重送不會退兩次。收銀員連按兩下是每天都在發生的事。
#[tokio::test]
async fn the_same_request_twice_refunds_once() {
    let e = env("idem").await;
    let bill = e.settled_bill(&["珍珠奶茶"], "cash").await;
    let reason = e.reason("quality").await;
    let key = open_pos::core::ids::Id::new().to_string();

    let req = || refund::RefundReq {
        bill_id: bill.id.clone(),
        payment_id: None,
        amount: 10,
        reason_id: Some(reason.clone()),
        note: None,
        approver_id: None,
        idem_key: key.clone(),
    };
    let a = refund::refund(&e.ctx, req()).await.unwrap();
    let b = refund::refund(&e.ctx, req()).await.unwrap();
    assert_eq!(a.refunded_total, b.refunded_total);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refunds WHERE bill_id = ?1")
        .bind(&bill.id)
        .fetch_one(e.ctx.db.reader())
        .await
        .unwrap();
    assert_eq!(n, 1, "同一個請求只該產生一筆退款");

    e.ctx.db.close().await;
}

/// 作廢過的帳單不需要退款 —— 作廢已經把錢還回去了。
#[tokio::test]
async fn a_voided_bill_cannot_be_refunded() {
    let e = env("voided").await;
    let bill = e.settled_bill(&["珍珠奶茶"], "cash").await;
    let order_id: String = sqlx::query_scalar("SELECT order_id FROM bills WHERE id = ?1")
        .bind(&bill.id)
        .fetch_one(e.ctx.db.reader())
        .await
        .unwrap();
    let o = order::get_order(&e.ctx, &order_id).await.unwrap();
    let void_reason: String =
        sqlx::query_scalar("SELECT id FROM reason_codes WHERE kind = 'void' AND code = 'other'")
            .fetch_one(e.ctx.db.reader())
            .await
            .unwrap();
    order::void_order(
        &e.ctx,
        order::VoidOrderReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            reason_id: Some(void_reason),
            note: None,
            approver_id: None,
        },
    )
    .await
    .unwrap();

    let reason = e.reason("quality").await;
    let err = e_refund(&e, &bill.id, 10, &reason).await.unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    e.ctx.db.close().await;
}

/// 找帳單要能用單號末幾碼搜 —— 收銀員手上只有客人那張收據。
#[tokio::test]
async fn bills_can_be_found_by_the_tail_of_the_number() {
    let e = env("find").await;
    let bill = e.settled_bill(&["珍珠奶茶"], "cash").await;
    let tail = &bill.bill_no[bill.bill_no.len() - 4..];

    let found = refund::find_bills(
        &e.ctx,
        refund::FindBillsReq {
            business_date: None,
            bill_no: Some(tail.to_string()),
        },
    )
    .await
    .unwrap();
    assert!(found.iter().any(|b| b.bill_no == bill.bill_no));

    // 今天的單不給搜尋字串也找得到。
    let today = refund::find_bills(
        &e.ctx,
        refund::FindBillsReq {
            business_date: None,
            bill_no: None,
        },
    )
    .await
    .unwrap();
    assert!(today.iter().any(|b| b.bill_no == bill.bill_no));
    assert_eq!(today[0].payments.len(), 1);
    assert_eq!(today[0].payments[0].refundable, today[0].grand_total);

    e.ctx.db.close().await;
}

async fn e_refund(
    e: &Env,
    bill_id: &str,
    amount: i64,
    reason: &str,
) -> open_pos::error::AppResult<refund::RefundResult> {
    refund::refund(
        &e.ctx,
        refund::RefundReq {
            bill_id: bill_id.to_string(),
            payment_id: None,
            amount,
            reason_id: Some(reason.to_string()),
            note: None,
            approver_id: None,
            idem_key: open_pos::core::ids::Id::new().to_string(),
        },
    )
    .await
}
