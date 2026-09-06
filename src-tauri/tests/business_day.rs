//! 一整天的模擬營業。
//!
//! 這是整個系統唯一一條**橫跨所有模組**的測試：開班 → 點餐 → 折扣 → 招待
//! → 加點 → 退點 → 混合支付 → 結帳後作廢 → 關班盤點 → 日結 → 出單。
//!
//! 每一個數字都在下面手算過一次並寫在註解裡。這一份的價值不在於「會不會過」，
//! 而在於**它壞掉的時候會告訴你哪一個環節在說謊** —— 單元測試各自都綠，
//! 但把它們串起來之後金額對不上，才是真正會讓店家對不了帳的那種 bug。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::{demo, menu, order, refund, shift, table};

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
        "openpos_day_{}_{}_{}",
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

struct Till<'a> {
    ctx: &'a Ctx,
    menu: menu::MenuTree,
    seq: std::cell::Cell<u32>,
}

impl<'a> Till<'a> {
    async fn new(ctx: &'a Ctx) -> Till<'a> {
        Till {
            menu: menu::menu_tree(ctx).await.unwrap(),
            ctx,
            seq: std::cell::Cell::new(0),
        }
    }

    fn item(&self, name: &str) -> String {
        self.menu
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| i.name == name)
            .unwrap_or_else(|| panic!("菜單裡沒有 {name}"))
            .id
            .clone()
    }

    async fn open(&self, channel: Channel, items: &[&str]) -> order::OrderView {
        let o = order::open_order(
            self.ctx,
            order::OpenOrderReq {
                channel,
                table_id: None,
                guest_count: if channel == Channel::DineIn {
                    Some(2)
                } else {
                    None
                },
                client_id: None,
            },
        )
        .await
        .unwrap();
        self.add(&o, items).await
    }

    /// 內用開檯。桌位是內用整條動線的掛點，所以 golden day 要走一次真的桌。
    async fn seat(&self, table_id: &str, guests: i64, items: &[&str]) -> order::OrderView {
        let o = order::open_order(
            self.ctx,
            order::OpenOrderReq {
                channel: Channel::DineIn,
                table_id: Some(table_id.to_string()),
                guest_count: Some(guests),
                client_id: None,
            },
        )
        .await
        .unwrap();
        self.add(&o, items).await
    }

    /// 分帳收一份（現金剛好）。
    async fn part(&self, order_id: &str, split: order::SplitReq) -> order::SettleResult {
        self.seq.set(self.seq.get() + 1);
        let o = order::get_order(self.ctx, order_id).await.unwrap();
        order::settle(
            self.ctx,
            order::SettleReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                payments: vec![order::PaymentReq {
                    method_code: "cash".into(),
                    // 分帳的應收由後端算，這裡給一個一定夠的金額，
                    // 多的部分會變成找零 —— 而現金找零不影響應有現金。
                    amount: 10_000,
                    tendered: Some(10_000),
                    ref_no: None,
                }],
                idem_key: format!("day-{}", self.seq.get()),
                split: Some(split),
            },
        )
        .await
        .unwrap()
    }

    async fn add(&self, o: &order::OrderView, items: &[&str]) -> order::OrderView {
        order::add_lines(
            self.ctx,
            order::AddLinesReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                lines: items
                    .iter()
                    .map(|n| order::NewLine {
                        item_id: self.item(n),
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

    async fn cash(&self, o: &order::OrderView, tendered: i64) -> order::SettleResult {
        self.seq.set(self.seq.get() + 1);
        order::settle(
            self.ctx,
            order::SettleReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                payments: vec![order::PaymentReq {
                    method_code: "cash".into(),
                    amount: o.grand_total,
                    tendered: Some(tendered),
                    ref_no: None,
                }],
                idem_key: format!("day-{}", self.seq.get()),
                split: None,
            },
        )
        .await
        .unwrap()
    }

    /// 混合支付：先刷一部分卡，剩下收現金。
    async fn split(&self, o: &order::OrderView, card: i64, tendered: i64) -> order::SettleResult {
        self.seq.set(self.seq.get() + 1);
        order::settle(
            self.ctx,
            order::SettleReq {
                order_id: o.id.clone(),
                expected_rev: o.rev,
                payments: vec![
                    order::PaymentReq {
                        method_code: "credit".into(),
                        amount: card,
                        tendered: None,
                        ref_no: Some("4242".into()),
                    },
                    order::PaymentReq {
                        method_code: "cash".into(),
                        amount: o.grand_total - card,
                        tendered: Some(tendered),
                        ref_no: None,
                    },
                ],
                idem_key: format!("day-{}", self.seq.get()),
                split: None,
            },
        )
        .await
        .unwrap()
    }
}

/// ★ 一整天。每個數字都在註解裡手算過。
#[tokio::test]
async fn a_whole_business_day_adds_up() {
    let e = env("golden").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();

    // 預設不收服務費（多數台灣小店確實不收），這裡開起來 10% ——
    // 「服務費要進稅基」是這條測試要守的其中一個不變量。
    let mut uow = e.ctx.db.begin_write().await.unwrap();
    sqlx::query("UPDATE stores SET service_charge_rate_bp = 1000")
        .execute(uow.conn())
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let till = Till::new(&e.ctx).await;

    shift::open_shift(
        &e.ctx,
        shift::OpenShiftReq {
            opening_float: 2000,
            counts: None,
            note: None,
        },
    )
    .await
    .unwrap();

    // ── ① 一般外帶：珍奶 60 + 滷肉飯 45 = 105，客人給 200。
    let o1 = till.open(Channel::Takeout, &["珍珠奶茶", "滷肉飯"]).await;
    assert_eq!(o1.grand_total, 105);
    let r1 = till.cash(&o1, 200).await;
    assert_eq!(r1.change, 95);

    // ── ② 內用要收 10% 服務費，而服務費**要進稅基**。
    //     雞腿便當 110 → 服務費 11 → 121。
    let o2 = till.open(Channel::DineIn, &["雞腿便當"]).await;
    assert_eq!(o2.service_charge, 11, "內用沒有加服務費");
    assert_eq!(o2.grand_total, 121);
    assert_eq!(
        o2.sales_amount + o2.tax_amount,
        o2.grand_total,
        "服務費沒有進稅基"
    );
    till.cash(&o2, 121).await;

    // ── ③ 加點之後打 9 折：牛肉麵 140 + 燙青菜 30 = 170
    //     → 折扣 17 → 153，混合支付：刷卡 100 + 現金 53（客人給 100）。
    let o3 = till.open(Channel::Takeout, &["牛肉麵"]).await;
    let o3 = till.add(&o3, &["燙青菜"]).await;
    assert_eq!(o3.grand_total, 170);
    let o3 = order::apply_discount(
        &e.ctx,
        order::DiscountReq {
            order_id: o3.id.clone(),
            expected_rev: o3.rev,
            line_id: None,
            kind: "percent".into(),
            value: 9000,
            reason_id: None,
            note: None,
            approver_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(o3.grand_total, 153);
    let r3 = till.split(&o3, 100, 100).await;
    assert_eq!(r3.change, 47, "現金那一段的找零算錯了");

    // ── ④ 招待一杯：拿鐵 65 + 蛋餅 40 = 105，招待拿鐵 → 40，現金付。
    let o4 = till.open(Channel::Takeout, &["拿鐵咖啡", "蛋餅"]).await;
    let latte = o4
        .lines
        .iter()
        .find(|l| l.name == "拿鐵咖啡")
        .unwrap()
        .id
        .clone();
    let o4 = order::apply_discount(
        &e.ctx,
        order::DiscountReq {
            order_id: o4.id.clone(),
            expected_rev: o4.rev,
            line_id: Some(latte),
            kind: "comp".into(),
            value: 0,
            reason_id: None,
            note: Some("熟客".into()),
            approver_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(o4.grand_total, 40);
    till.cash(&o4, 50).await;

    // ── ⑤ 點錯退掉一項：排骨便當 100 + 味噌湯 25，退掉湯 → 100。
    let o5 = till.open(Channel::Takeout, &["排骨便當", "味噌湯"]).await;
    let soup = o5
        .lines
        .iter()
        .find(|l| l.name == "味噌湯")
        .unwrap()
        .id
        .clone();
    let o5 = order::void_line(&e.ctx, o5.id.clone(), o5.rev, soup, None)
        .await
        .unwrap();
    assert_eq!(o5.grand_total, 100);
    till.cash(&o5, 100).await;

    // ── ⑥ 結帳後作廢：控肉飯 85 收了現金，之後整單作廢。
    //     這一筆**不能算進當天的營業額**，但現金要退回去。
    let o6 = till.open(Channel::Takeout, &["控肉飯"]).await;
    assert_eq!(o6.grand_total, 85);
    till.cash(&o6, 85).await;
    let o6 = order::get_order(&e.ctx, &o6.id).await.unwrap();
    let reason: String = sqlx::query_scalar(
        "SELECT id FROM reason_codes WHERE kind = 'void' ORDER BY sort_order LIMIT 1",
    )
    .fetch_one(e.ctx.db.reader())
    .await
    .unwrap();
    order::void_order(
        &e.ctx,
        order::VoidOrderReq {
            order_id: o6.id.clone(),
            expected_rev: o6.rev,
            reason_id: Some(reason),
            note: Some("客人反悔".into()),
            approver_id: None,
        },
    )
    .await
    .unwrap();

    // ── ⑦ 內用開檯 + 平分：A1 三位，滷肉飯 45 + 味噌湯 25 = 70
    //     → 服務費 7 → 77，兩個人平分 = 39 / 38（最大餘數法，加起來剛好 77）。
    let a1 = table::list_tables(&e.ctx)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.code == "A1")
        .expect("示範資料要有 A1");
    let o7 = till.seat(&a1.id, 3, &["滷肉飯", "味噌湯"]).await;
    assert_eq!(o7.grand_total, 77, "內用的服務費算錯了");

    let seated = table::list_tables(&e.ctx)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.code == "A1")
        .unwrap();
    assert_eq!(seated.session.as_ref().map(|s| s.total), Some(77));
    assert_eq!(seated.session.as_ref().map(|s| s.guest_count), Some(3));

    let p1 = till.part(&o7.id, order::SplitReq::Even { parts: 2 }).await;
    assert_eq!(p1.remaining, 38, "第一份收完還差 38");
    assert!(
        table::list_tables(&e.ctx)
            .await
            .unwrap()
            .iter()
            .any(|t| t.code == "A1" && t.session.is_some()),
        "★ 只收了一半，這一桌不能被釋放"
    );

    let p2 = till.part(&o7.id, order::SplitReq::Even { parts: 2 }).await;
    assert_eq!(p2.remaining, 0);
    assert!(
        table::list_tables(&e.ctx)
            .await
            .unwrap()
            .iter()
            .any(|t| t.code == "A1" && t.session.is_none()),
        "收完了桌就該空出來"
    );

    // ── ⑧ 退款：①那張 105 的單退 20 元現金。
    //     營業額**不動**（東西確實賣出去了），但抽屜裡少 20。
    let bill = refund::find_bills(
        &e.ctx,
        refund::FindBillsReq {
            business_date: None,
            bill_no: Some(r1.bill_no.clone()),
        },
    )
    .await
    .unwrap()
    .into_iter()
    .next()
    .unwrap();
    let refund_reason: String = sqlx::query_scalar(
        "SELECT id FROM reason_codes WHERE kind = 'refund' ORDER BY sort_order LIMIT 1",
    )
    .fetch_one(e.ctx.db.reader())
    .await
    .unwrap();
    refund::refund(
        &e.ctx,
        refund::RefundReq {
            bill_id: bill.id,
            payment_id: None,
            amount: 20,
            reason_id: Some(refund_reason),
            note: Some("珍奶做錯了".into()),
            approver_id: None,
            idem_key: "day-refund".into(),
        },
    )
    .await
    .unwrap();

    // ── 手算 ──────────────────────────────────────────────
    // 有效帳單（①②③④⑤ 各一張，⑦ 兩張）：
    //   105 + 121 + 153 + 40 + 100 + 39 + 38 = 596，共 7 張
    // ⑥ 已作廢，不計。
    // ⑧ 是退款，**不從營業額裡扣** —— 東西確實賣出去了。
    //
    // 現金收到的：
    //   ① 105、② 121、③ 53、④ 40、⑤ 100、⑦ 39 + 38  = 496
    //   ⑥ 85 收了又退 → 帳單作廢，那 85 元也退還給客人，所以不算。
    // 刷卡：③ 100
    // 現金退款：⑧ 20（要從抽屜扣）
    //
    // 抽屜裡應該有：2000（準備金）+ 496 − 20 = 2476
    let x = shift::x_report(&e.ctx).await.unwrap();
    assert_eq!(x.sales.bills, 7, "有效帳單數不對（作廢的不算、分帳算兩張）");
    assert_eq!(x.sales.total, 596, "當天營業額不對");
    assert_eq!(
        x.sales.sales + x.sales.tax,
        x.sales.total,
        "sales + tax != total —— 這是財政部的硬檢核"
    );
    assert_eq!(x.refunds.count, 1);
    assert_eq!(x.refunds.amount, 20);
    assert_eq!(x.refunds.cash_amount, 20);
    assert_eq!(x.cash.cash_refunds, 20);
    assert_eq!(x.cash.expected, 2476, "應有現金不對：{:?}", x.cash);

    let card = x.payments.iter().find(|p| p.code == "credit").unwrap();
    assert_eq!(card.amount, 100, "刷卡金額不對");

    // ── 關班：故意少數 100 元，差異要是 −100 ──────────────
    let report = shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: vec![
                shift::DenomCount {
                    denomination: 1000,
                    count: 2,
                },
                shift::DenomCount {
                    denomination: 100,
                    count: 3,
                },
                shift::DenomCount {
                    denomination: 50,
                    count: 1,
                },
                shift::DenomCount {
                    denomination: 10,
                    count: 2,
                },
                shift::DenomCount {
                    denomination: 5,
                    count: 1,
                },
                shift::DenomCount {
                    denomination: 1,
                    count: 1,
                },
            ],
            note: Some("少了一張百元".into()),
        },
    )
    .await
    .unwrap();
    // 2000 + 300 + 50 + 20 + 5 + 1 = 2376，應有 2476 → 差 −100
    assert_eq!(report.cash.counted, Some(2376));
    assert_eq!(report.cash.variance, Some(-100), "短少算錯了");

    // ── 日結 ─────────────────────────────────────────────
    let day = shift::close_business_day(&e.ctx).await.unwrap();
    assert_eq!(day.sales.bills, 7);
    assert_eq!(day.sales.total, 596);
    assert_eq!(day.refunds.amount, 20, "退款要出現在 Z 報表上");
    assert_eq!(day.shifts.len(), 1);
    assert_eq!(day.shifts[0].cash_variance, Some(-100));
    // 招待掉的拿鐵不該出現在品項排行的金額裡（它是 0 元）。
    let latte_row = day.top_items.iter().find(|i| i.name == "拿鐵咖啡");
    assert_eq!(
        latte_row.map(|i| i.amount),
        Some(0),
        "招待的品項金額應該是 0"
    );

    // 日結之後那一天就鎖住了。
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

    // ── 出單：這一天總共該印出多少張 ─────────────────────
    // 廚房單：① 新單、② 新單、③ 新單 + 加點、④ 新單、⑤ 新單 + 退點、
    //         ⑥ 新單 + 作廢整單、⑦ 新單 = 10 張
    // 收據：①②③④⑤⑥ 各一 + ⑦ 分帳兩張 = 8 張
    // 退款單：⑧ 一張
    // 報表：交接單 + Z 報表 = 2 張
    let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox WHERE kind LIKE 'print.%'")
        .fetch_one(e.ctx.db.reader())
        .await
        .unwrap();
    assert_eq!(queued, 21, "該印的張數不對 —— 漏印比重複印嚴重得多");

    e.ctx.db.close().await;
}
