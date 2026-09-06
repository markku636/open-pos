//! 點餐到結帳的端到端測試。
//!
//! 這一支對應 README 上「一台電腦一台印表機就能開店」的核心承諾：
//! 開桌 → 點餐 → 加點 → 退點 → 收現金找零，而且每一步的金額都要對得起來。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::menu::{self, CategoryInput, ItemInput};
use open_pos::services::order::{self, AddLinesReq, NewLine, OpenOrderReq, PaymentReq, SettleReq};

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
        "openpos_order_{}_{}_{}",
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

/// 建一份最小菜單，回傳 (珍奶 60, 滷肉飯 55)。
async fn seed_menu(ctx: &Ctx) -> (String, String) {
    let c = menu::upsert_category(
        ctx,
        CategoryInput {
            id: None,
            name: "主食".into(),
            color: None,
            sort_order: None,
            is_active: None,
        },
    )
    .await
    .unwrap();

    let mk = |name: &str, price: i64| ItemInput {
        id: None,
        category_id: Some(c.id.clone()),
        name: name.into(),
        short_name: None,
        base_price: price,
        tax_code: None,
        is_open_price: None,
        sold_out_until: None,
        sort_order: None,
        is_active: None,
    };
    let tea = menu::upsert_item(ctx, mk("珍珠奶茶", 60)).await.unwrap();
    let rice = menu::upsert_item(ctx, mk("滷肉飯", 55)).await.unwrap();
    (tea.id, rice.id)
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

async fn outbox_kinds(ctx: &Ctx) -> Vec<String> {
    sqlx::query_scalar::<_, String>("SELECT kind FROM outbox ORDER BY created_at, id")
        .fetch_all(ctx.db.reader())
        .await
        .unwrap()
}

/// ★ 一整條主線：開單 → 點兩樣 → 收現金 → 找零。
#[tokio::test]
async fn takeout_order_from_open_to_cash_payment() {
    let e = env("main").await;
    let (tea, rice) = seed_menu(&e.ctx).await;

    let o = order::open_order(
        &e.ctx,
        OpenOrderReq {
            channel: Channel::Takeout,
            table_id: None,
            guest_count: None,
            client_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(o.status, "draft");
    assert_eq!(o.rev, 0);
    assert!(o.order_no.starts_with('A'), "單號要好唸：{}", o.order_no);

    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 2), line(&rice, 1)],
        },
    )
    .await
    .unwrap();

    assert_eq!(o.status, "placed");
    assert_eq!(o.rev, 1, "每次寫入都要 bump rev");
    assert_eq!(o.lines.len(), 2);
    assert_eq!(o.subtotal, 175); // 60×2 + 55
                                 // 外帶不收服務費 —— 台灣慣例。
    assert_eq!(o.service_charge, 0);
    assert_eq!(o.grand_total, 175);
    assert_eq!(o.sales_amount + o.tax_amount, o.grand_total);
    // 各行加總必須等於總額，否則發票的品項對不上總計。
    assert_eq!(o.lines.iter().map(|l| l.amount).sum::<i64>(), 175);

    let r = order::settle(
        &e.ctx,
        SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![PaymentReq {
                method_code: "cash".into(),
                amount: 175,
                tendered: Some(500),
                ref_no: None,
            }],
            idem_key: "idem-main-1".into(),
        },
    )
    .await
    .unwrap();

    assert_eq!(r.change, 325, "500 − 175 應找 325");
    assert!(r.bill_no.starts_with('B'));
    assert_eq!(r.order.status, "settled");

    // 廚房單在下單時進 outbox、收據在結帳時進。兩者都不在交易裡碰印表機。
    assert_eq!(
        outbox_kinds(&e.ctx).await,
        vec!["print.kitchen", "print.receipt"]
    );
}

#[tokio::test]
async fn dine_in_adds_service_charge_and_it_sits_inside_the_tax_base() {
    let e = env("service").await;
    let (tea, _) = seed_menu(&e.ctx).await;

    // 開一成服務費。
    let now = Stamp::now();
    let mut uow = e.ctx.db.begin_write().await.unwrap();
    sqlx::query("UPDATE stores SET service_charge_rate_bp = 1000, updated_at = ?1")
        .bind(now.iso())
        .execute(uow.conn())
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let o = order::open_order(
        &e.ctx,
        OpenOrderReq {
            channel: Channel::DineIn,
            table_id: None,
            guest_count: Some(2),
            client_id: None,
        },
    )
    .await
    .unwrap();

    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 10)], // 600
        },
    )
    .await
    .unwrap();

    assert_eq!(o.subtotal, 600);
    assert_eq!(o.service_charge, 60);
    assert_eq!(o.grand_total, 660);
    // 服務費在稅基內：稅是從 660 拆出來的，不是從 600。
    assert_eq!(o.sales_amount + o.tax_amount, 660);
    assert_eq!(o.tax_amount, 31); // round(660 × 500 / 10500)
}

/// 樂觀鎖：兩個人同時改同一張單時，後到的那個要被擋下來而不是默默覆蓋。
#[tokio::test]
async fn stale_revision_is_rejected_instead_of_silently_overwriting() {
    let e = env("rev").await;
    let (tea, _) = seed_menu(&e.ctx).await;
    let o = order::open_order(&e.ctx, takeout()).await.unwrap();

    // 收銀員 A 加了一杯。
    let after_a = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 1)],
        },
    )
    .await
    .unwrap();

    // 服務生 B 手上還是舊版本。
    let err = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev, // 過期
            lines: vec![line(&tea, 5)],
        },
    )
    .await
    .unwrap_err();

    assert_eq!(err.code(), "ERR_CONFLICT");
    assert!(err.message().contains("重新整理"), "{}", err.message());

    // A 的那一杯必須還在 —— 這正是不默默覆蓋的意義。
    let now = order::get_order(&e.ctx, &o.id).await.unwrap();
    assert_eq!(now.rev, after_a.rev);
    assert_eq!(now.lines.len(), 1);
}

fn takeout() -> OpenOrderReq {
    OpenOrderReq {
        channel: Channel::Takeout,
        table_id: None,
        guest_count: None,
        client_id: None,
    }
}

/// ★ 重送不能重複收款。這不是為了「主機掛掉」，是為了每天都在發生的 Wi-Fi 抖動。
#[tokio::test]
async fn resending_a_settlement_does_not_charge_twice() {
    let e = env("idem").await;
    let (tea, _) = seed_menu(&e.ctx).await;
    let o = order::open_order(&e.ctx, takeout()).await.unwrap();
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 1)],
        },
    )
    .await
    .unwrap();

    let req = || SettleReq {
        order_id: o.id.clone(),
        expected_rev: o.rev,
        payments: vec![PaymentReq {
            method_code: "cash".into(),
            amount: 60,
            tendered: Some(100),
            ref_no: None,
        }],
        idem_key: "same-key".into(),
    };

    let first = order::settle(&e.ctx, req()).await.unwrap();
    let second = order::settle(&e.ctx, req()).await.unwrap();

    assert_eq!(first.bill_no, second.bill_no, "重送要回同一張帳單");
    assert_eq!(first.change, second.change);

    let bills: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bills")
        .fetch_one(e.ctx.db.reader())
        .await
        .unwrap();
    let payments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payments")
        .fetch_one(e.ctx.db.reader())
        .await
        .unwrap();
    assert_eq!(bills, 1, "不該產生第二張帳單");
    assert_eq!(payments, 1, "不該收兩次錢");
}

#[tokio::test]
async fn underpayment_is_rejected_with_the_shortfall_spelled_out() {
    let e = env("short").await;
    let (tea, _) = seed_menu(&e.ctx).await;
    let o = order::open_order(&e.ctx, takeout()).await.unwrap();
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 2)], // 120
        },
    )
    .await
    .unwrap();

    let err = order::settle(
        &e.ctx,
        SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![PaymentReq {
                method_code: "cash".into(),
                amount: 100,
                tendered: Some(100),
                ref_no: None,
            }],
            idem_key: "short-1".into(),
        },
    )
    .await
    .unwrap_err();

    // 訊息要直接說還差多少，收銀員才不用自己心算。
    assert!(err.message().contains("還差 20 元"), "{}", err.message());
}

#[tokio::test]
async fn a_settled_order_cannot_be_modified() {
    let e = env("locked").await;
    let (tea, _) = seed_menu(&e.ctx).await;
    let o = order::open_order(&e.ctx, takeout()).await.unwrap();
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 1)],
        },
    )
    .await
    .unwrap();
    order::settle(
        &e.ctx,
        SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![PaymentReq {
                method_code: "cash".into(),
                amount: 60,
                tendered: None,
                ref_no: None,
            }],
            idem_key: "lock-1".into(),
        },
    )
    .await
    .unwrap();

    let after = order::get_order(&e.ctx, &o.id).await.unwrap();
    let err = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: after.rev,
            lines: vec![line(&tea, 1)],
        },
    )
    .await
    .unwrap_err();
    assert!(err.message().contains("已經結帳"), "{}", err.message());
}

/// 退點也要出單 —— 廚房已經在做了，不通知的話那份餐會照樣做出來。
#[tokio::test]
async fn voiding_a_line_recomputes_totals_and_tells_the_kitchen() {
    let e = env("void").await;
    let (tea, rice) = seed_menu(&e.ctx).await;
    let o = order::open_order(&e.ctx, takeout()).await.unwrap();
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 1), line(&rice, 1)],
        },
    )
    .await
    .unwrap();
    assert_eq!(o.grand_total, 115);

    let target = o.lines[0].id.clone();
    let o = order::void_line(&e.ctx, o.id.clone(), o.rev, target, None)
        .await
        .unwrap();

    assert_eq!(o.lines.len(), 1);
    assert_eq!(o.grand_total, 55);
    assert_eq!(o.lines.iter().map(|l| l.amount).sum::<i64>(), 55);

    let kinds = outbox_kinds(&e.ctx).await;
    assert_eq!(kinds, vec!["print.kitchen", "print.kitchen"]);

    // 退點必須留下可查的稽核，而且金額差要記下來。
    let rows: Vec<(String, Option<i64>)> =
        sqlx::query_as("SELECT entity_type, amount_delta FROM audit_logs WHERE action = 'void'")
            .fetch_all(e.ctx.db.reader())
            .await
            .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, Some(-60));
}

#[tokio::test]
async fn a_sold_out_item_cannot_be_ordered() {
    // 售完檢查在服務層而不是 UI：掃碼點餐的客人手機上是舊資料，
    // 而客人不會知道「剛剛賣完了」。
    let e = env("soldout").await;
    let (tea, _) = seed_menu(&e.ctx).await;

    let now = Stamp::now();
    let mut uow = e.ctx.db.begin_write().await.unwrap();
    sqlx::query("UPDATE items SET sold_out_until = ?2 WHERE id = ?1")
        .bind(&tea)
        .bind(now.iso())
        .execute(uow.conn())
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let o = order::open_order(&e.ctx, takeout()).await.unwrap();
    let err = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 1)],
        },
    )
    .await
    .unwrap_err();
    assert!(err.message().contains("已售完"), "{}", err.message());
}

#[tokio::test]
async fn reopening_with_the_same_client_id_returns_the_same_order() {
    // 手機在 Wi-Fi 邊緣重送開單請求，不該開出兩張單。
    let e = env("clientid").await;
    let req = || OpenOrderReq {
        channel: Channel::Takeout,
        table_id: None,
        guest_count: None,
        client_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
    };
    let a = order::open_order(&e.ctx, req()).await.unwrap();
    let b = order::open_order(&e.ctx, req()).await.unwrap();
    assert_eq!(a.id, b.id);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders")
        .fetch_one(e.ctx.db.reader())
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn order_numbers_restart_each_business_day_and_are_sequential() {
    let e = env("seq").await;
    let mut nos = Vec::new();
    for _ in 0..3 {
        nos.push(order::open_order(&e.ctx, takeout()).await.unwrap().order_no);
    }
    assert!(nos[0].ends_with("0001"), "{}", nos[0]);
    assert!(nos[1].ends_with("0002"));
    assert!(nos[2].ends_with("0003"));
}

/// 混合支付：現金 + 刷卡。找零只會出現在現金那一筆上。
#[tokio::test]
async fn mixed_payment_puts_the_change_on_the_cash_leg_only() {
    let e = env("mixed").await;
    let (tea, rice) = seed_menu(&e.ctx).await;
    let o = order::open_order(&e.ctx, takeout()).await.unwrap();
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 2), line(&rice, 1)], // 175
        },
    )
    .await
    .unwrap();

    let r = order::settle(
        &e.ctx,
        SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![
                PaymentReq {
                    method_code: "credit".into(),
                    amount: 100,
                    tendered: None,
                    ref_no: Some("1234".into()),
                },
                PaymentReq {
                    method_code: "cash".into(),
                    amount: 75,
                    tendered: Some(200),
                    ref_no: None,
                },
            ],
            idem_key: "mixed-1".into(),
        },
    )
    .await
    .unwrap();

    assert_eq!(r.change, 125, "200 − 75 應找 125");

    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT method_code_snapshot, amount, change_amount FROM payments ORDER BY method_code_snapshot",
    )
    .fetch_all(e.ctx.db.reader())
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    // 刷卡那一筆不該有找零。
    assert_eq!(rows[0], ("cash".into(), 75, 125));
    assert_eq!(rows[1], ("credit".into(), 100, 0));
}

/// 刷超過不能當作找零 —— 默默吞掉會讓當天的現金短少。
#[tokio::test]
async fn a_card_cannot_give_change() {
    let e = env("card").await;
    let (tea, _) = seed_menu(&e.ctx).await;
    let o = order::open_order(&e.ctx, takeout()).await.unwrap();
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 1)], // 60
        },
    )
    .await
    .unwrap();

    let err = order::settle(
        &e.ctx,
        SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![PaymentReq {
                method_code: "credit".into(),
                amount: 60,
                tendered: Some(100),
                ref_no: None,
            }],
            idem_key: "card-1".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(err.message().contains("不能找零"), "{}", err.message());
}

/// 只輸入「客人給了 500」也要能結 —— 這是另一種常見的收銀操作習慣。
#[tokio::test]
async fn entering_only_the_tendered_amount_also_works() {
    let e = env("tender").await;
    let (tea, _) = seed_menu(&e.ctx).await;
    let o = order::open_order(&e.ctx, takeout()).await.unwrap();
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(&tea, 1)], // 60
        },
    )
    .await
    .unwrap();

    let r = order::settle(
        &e.ctx,
        SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![PaymentReq {
                method_code: "cash".into(),
                // 收銀員直接打「500」，沒有分開填 amount 與 tendered。
                amount: 500,
                tendered: None,
                ref_no: None,
            }],
            idem_key: "tender-1".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(r.change, 440);

    let paid: i64 = sqlx::query_scalar("SELECT paid_total FROM bills")
        .fetch_one(e.ctx.db.reader())
        .await
        .unwrap();
    assert_eq!(
        paid, 60,
        "實收要記沖銷掉帳單的金額，不是客人遞出來的鈔票面額"
    );
}
