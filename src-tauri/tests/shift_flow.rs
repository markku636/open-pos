//! 班別交接與日結的整合測試。
//!
//! 重點不在 CRUD，而在幾條**做錯了店家會賠錢或對不了帳**的性質：
//! 盲盤不能提前洩漏應有金額、關班快照永不重算、日結之後那一天要真的鎖住。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::{demo, menu, order, shift};

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
        "openpos_shift_{}_{}_{}",
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
    /// 賣一份東西並收現金。回傳這一單的總額。
    async fn sell(&self, item_name: &str, tendered: i64) -> i64 {
        let tree = menu::menu_tree(&self.ctx).await.unwrap();
        let item = tree
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| i.name == item_name)
            .unwrap();

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
                lines: vec![order::NewLine {
                    item_id: item.id.clone(),
                    variant_id: None,
                    modifier_ids: vec![],
                    qty_milli: None,
                    note: None,
                }],
            },
        )
        .await
        .unwrap();

        let total = o.grand_total;
        order::settle(
            &self.ctx,
            order::SettleReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                payments: vec![order::PaymentReq {
                    method_code: "cash".into(),
                    amount: total,
                    tendered: Some(tendered),
                    ref_no: None,
                }],
                idem_key: format!("t-{}-{}", o.id, o.rev),
            },
        )
        .await
        .unwrap();
        total
    }
}

fn counts(pairs: &[(i64, i64)]) -> Vec<shift::DenomCount> {
    pairs
        .iter()
        .map(|(d, c)| shift::DenomCount {
            denomination: *d,
            count: *c,
        })
        .collect()
}

#[tokio::test]
async fn a_shift_tracks_the_cash_it_took() {
    let e = env("cash").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();

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

    let a = e.sell("珍珠奶茶", 100).await; // 60
    let b = e.sell("滷肉飯", 50).await; // 45
    assert_eq!(a + b, 105);

    // 剛好數對：1000 準備金 + 105 現金銷售。
    let report = shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: counts(&[(1000, 1), (100, 1), (5, 1)]),
            note: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(report.cash.opening_float, 1000);
    assert_eq!(
        report.cash.cash_sales, 105,
        "現金銷售算錯：{:?}",
        report.cash
    );
    assert_eq!(report.cash.expected, 1105);
    assert_eq!(report.cash.counted, Some(1105));
    assert_eq!(report.cash.variance, Some(0));
    assert_eq!(report.sales.bills, 2);
    assert_eq!(report.sales.total, 105);
    // sales + tax == total 是財政部的硬檢核，班別報表上也必須成立。
    assert_eq!(report.sales.sales + report.sales.tax, report.sales.total);

    e.ctx.db.close().await;
}

/// ★ 短少要算得出來，而且要是負數。
#[tokio::test]
async fn a_short_drawer_shows_a_negative_variance() {
    let e = env("short").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
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
    e.sell("珍珠奶茶", 100).await; // 60

    // 應有 1060，只數到 1000 —— 少了 60。
    let report = shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: counts(&[(1000, 1)]),
            note: Some("抽屜少了一張".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(report.cash.expected, 1060);
    assert_eq!(report.cash.variance, Some(-60), "短少要是負數");

    e.ctx.db.close().await;
}

/// ★ 盲盤：關班前拿不到應有現金。
///
/// 這不是防呆是防弊 —— 先看到應有金額的話，短少的人會直接照抄，
/// 而那正是「現金差異永遠是零」的原因。
#[tokio::test]
async fn the_expected_cash_is_not_revealed_before_counting() {
    let e = env("blind").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
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
    e.sell("珍珠奶茶", 100).await;

    let open = shift::current_shift(&e.ctx).await.unwrap().unwrap();
    assert_eq!(open.status, "open");
    assert_eq!(open.expected_cash, None, "★ 還沒盤點就把應有現金送給前端了");
    assert_eq!(open.counted_cash, None);
    assert_eq!(open.cash_variance, None);

    // 沒有盤點就不准關班。
    let err = shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: vec![],
            note: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");
    assert!(err.message().contains("盤點"), "{}", err.message());

    e.ctx.db.close().await;
}

#[tokio::test]
async fn two_shifts_cannot_be_open_at_once() {
    let e = env("two").await;
    let req = || shift::OpenShiftReq {
        opening_float: 500,
        counts: None,
        note: None,
    };
    shift::open_shift(&e.ctx, req()).await.unwrap();
    let err = shift::open_shift(&e.ctx, req()).await.unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    assert!(err.message().contains("關班"), "{}", err.message());

    e.ctx.db.close().await;
}

#[tokio::test]
async fn cash_paid_in_and_out_move_the_expected_amount() {
    let e = env("movements").await;
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

    shift::record_cash_movement(
        &e.ctx,
        shift::CashMovementReq {
            kind: "paid_out".into(),
            amount: 300,
            reason_id: None,
            note: Some("買冰塊".into()),
        },
    )
    .await
    .unwrap();
    shift::record_cash_movement(
        &e.ctx,
        shift::CashMovementReq {
            kind: "paid_in".into(),
            amount: 50,
            reason_id: None,
            note: None,
        },
    )
    .await
    .unwrap();

    let report = shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: counts(&[(500, 1), (100, 2), (50, 1)]),
            note: None,
        },
    )
    .await
    .unwrap();
    // 1000 − 300 + 50 = 750
    assert_eq!(report.cash.expected, 750, "{:?}", report.cash);
    assert_eq!(report.cash.counted, Some(750));
    assert_eq!(report.cash.variance, Some(0));

    e.ctx.db.close().await;
}

/// ★ 日結之後那一天要真的鎖住。
///
/// 少了這道鎖，「日結」只是一個時間戳 —— 事後補進來的單會讓已經印出來的
/// Z 報表對不上，而那是稽核上最難解釋的一種狀況。
#[tokio::test]
async fn a_closed_business_day_refuses_new_orders() {
    let e = env("lock").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    shift::open_shift(
        &e.ctx,
        shift::OpenShiftReq {
            opening_float: 0,
            counts: None,
            note: None,
        },
    )
    .await
    .unwrap();
    e.sell("珍珠奶茶", 60).await;
    shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: counts(&[(50, 1), (10, 1)]),
            note: None,
        },
    )
    .await
    .unwrap();

    let day = shift::close_business_day(&e.ctx).await.unwrap();
    assert_eq!(day.sales.bills, 1);
    assert_eq!(day.sales.total, 60);
    assert!(!day.z_report_no.is_empty());
    assert_eq!(day.shifts.len(), 1);
    assert_eq!(day.shifts[0].cash_variance, Some(0));
    assert!(day.top_items.iter().any(|i| i.name == "珍珠奶茶"));

    let err = order::open_order(
        &e.ctx,
        order::OpenOrderReq {
            channel: Channel::Takeout,
            table_id: None,
            guest_count: None,
            client_id: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    assert!(err.message().contains("日結"), "{}", err.message());

    // 開班也要擋，不然店員會以為只是那一張單有問題。
    let err = shift::open_shift(
        &e.ctx,
        shift::OpenShiftReq {
            opening_float: 0,
            counts: None,
            note: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");

    e.ctx.db.close().await;
}

#[tokio::test]
async fn a_day_cannot_be_closed_while_a_shift_is_open() {
    let e = env("openshift").await;
    shift::open_shift(
        &e.ctx,
        shift::OpenShiftReq {
            opening_float: 0,
            counts: None,
            note: None,
        },
    )
    .await
    .unwrap();
    let err = shift::close_business_day(&e.ctx).await.unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    assert!(err.message().contains("關班"), "{}", err.message());

    e.ctx.db.close().await;
}

/// 關班的數字是快照，之後不重算。
#[tokio::test]
async fn the_shift_snapshot_does_not_move_afterwards() {
    let e = env("snapshot").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    shift::open_shift(
        &e.ctx,
        shift::OpenShiftReq {
            opening_float: 0,
            counts: None,
            note: None,
        },
    )
    .await
    .unwrap();
    e.sell("珍珠奶茶", 60).await;
    let first = shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: counts(&[(50, 1), (10, 1)]),
            note: None,
        },
    )
    .await
    .unwrap();

    // 開第二班再賣一單。第一班的數字不該跟著動。
    shift::open_shift(
        &e.ctx,
        shift::OpenShiftReq {
            opening_float: 0,
            counts: None,
            note: None,
        },
    )
    .await
    .unwrap();
    e.sell("滷肉飯", 45).await;

    let stored: String = sqlx::query_scalar("SELECT summary_json FROM shifts WHERE shift_no = ?1")
        .bind(&first.shift_no)
        .fetch_one(e.ctx.db.reader())
        .await
        .unwrap();
    let snapshot: shift::ShiftReport = serde_json::from_str(&stored).unwrap();
    assert_eq!(snapshot.sales.total, 60, "第一班的快照被後來的單改動了");
    assert_eq!(snapshot.cash.expected, 60);

    e.ctx.db.close().await;
}

/// ★ 交接單與 Z 報表要有紙。
///
/// 交班時兩個人要在同一張紙上對數字、簽名；只存在螢幕上的交接紀錄，
/// 在事後爭議時沒有任何用處。
#[tokio::test]
async fn closing_prints_a_handover_slip_and_a_z_report() {
    use open_pos::infra::printer::escpos::{decode, CjkEncoding};
    use open_pos::infra::printer::Transport;
    use open_pos::receipt::PaperWidth;
    use open_pos::services::{print_worker, printer};

    let e = env("printed").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();

    let spool = e.root.join("spool").join("job.bin");
    printer::upsert_printer(
        &e.ctx,
        printer::PrinterInput {
            id: None,
            name: "櫃檯".into(),
            transport: Transport::File {
                path: spool.to_string_lossy().into_owned(),
                append: true,
            },
            paper: PaperWidth::Mm80,
            encoding: Some(CjkEncoding::Big5),
            cutter: Some(true),
            drawer: None,
            status_query: None,
            render_mode: Some("text".into()),
            is_active: Some(true),
        },
    )
    .await
    .unwrap();

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
    e.sell("珍珠奶茶", 100).await;
    shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: counts(&[(1000, 1), (50, 1), (10, 1)]),
            note: None,
        },
    )
    .await
    .unwrap();
    let day = shift::close_business_day(&e.ctx).await.unwrap();

    print_worker::tick(&e.ctx).await.unwrap();

    let bytes = std::fs::read(&spool).expect("什麼都沒印出來");
    let text = decode::render_human(&decode::decode(&bytes, CjkEncoding::Big5));

    assert!(
        text.contains("交接單"),
        "少了交接單：
{text}"
    );
    assert!(
        text.contains("應有現金"),
        "
{text}"
    );
    assert!(
        text.contains("交班簽名"),
        "交接單要有簽名欄：
{text}"
    );
    assert!(
        text.contains("日結"),
        "少了 Z 報表：
{text}"
    );
    assert!(
        text.contains(&day.z_report_no),
        "Z 報表要印報表號：
{text}"
    );
    // Z 報表號要跟班別號分開：店家對帳時會報這個號碼。
    assert!(day.z_report_no.starts_with("Z-"), "{}", day.z_report_no);

    e.ctx.db.close().await;
}
