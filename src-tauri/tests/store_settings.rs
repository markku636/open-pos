//! 店家設定。
//!
//! 這一支存在的理由只有一個：**這一頁改的是稅率**。
//!
//! 畫面上寫著「稅率與服務費只影響之後開的單，已經結帳的單不會變」——
//! 那句話是給老闆看的承諾，而承諾要有測試撐著才算數。
//! `settled_orders_do_not_move_when_the_tax_rate_changes` 就是那句話的證明。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::error::AppError;
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::menu::{self, CategoryInput, ItemInput};
use open_pos::services::order::{self, AddLinesReq, NewLine, OpenOrderReq, PaymentReq, SettleReq};
use open_pos::services::table::{self, TableInput};
use open_pos::services::store::{self, StoreInput};

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
        "openpos_store_{}_{}_{}",
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

/// 把目前的設定原封不動讀出來當成輸入 —— 測試只想改一個欄位，
/// 其餘欄位要維持原樣，否則每個測試都在無意間改掉別的東西。
async fn input_from_current(ctx: &Ctx) -> StoreInput {
    let s = store::get(ctx).await.unwrap();
    StoreInput {
        name: s.name,
        tax_id: s.tax_id,
        address: s.address,
        phone: s.phone,
        business_day_cutoff: s.business_day_cutoff,
        tax_rate_bp: s.tax_rate_bp,
        service_charge_rate_bp: s.service_charge_rate_bp,
        rounding_policy: s.rounding_policy,
        min_charge_per_head: s.min_charge_per_head,
    }
}

async fn audit_count(ctx: &Ctx) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_logs WHERE entity_type = 'store'")
        .fetch_one(ctx.db.reader())
        .await
        .unwrap()
}

#[tokio::test]
async fn the_shop_can_change_its_own_name_and_rates() {
    let e = env("update").await;

    let mut input = input_from_current(&e.ctx).await;
    input.name = "  阿明小吃部  ".into();
    input.tax_id = Some("12345678".into());
    input.phone = Some("02-1234-5678".into());
    input.service_charge_rate_bp = 1000;
    input.rounding_policy = "floor_ten".into();
    input.min_charge_per_head = 150;

    let s = store::update(&e.ctx, input).await.unwrap();

    // 前後空白要吃掉：收據上「  阿明小吃部  」會被誤認成排版錯誤。
    assert_eq!(s.name, "阿明小吃部");
    assert_eq!(s.tax_id.as_deref(), Some("12345678"));
    assert_eq!(s.service_charge_rate_bp, 1000);
    assert_eq!(s.rounding_policy, "floor_ten");
    assert_eq!(s.min_charge_per_head, 150);

    // 重讀一次，確認真的落地了而不是只回傳了輸入值。
    assert_eq!(store::get(&e.ctx).await.unwrap().name, "阿明小吃部");
    assert_eq!(audit_count(&e.ctx).await, 1, "改設定要留下稽核紀錄");
}

/// 空字串的統編要存成 NULL，不是空字串。
///
/// 差別在收據：`Option::is_some()` 對 `Some("")` 是真的，於是收據上會印出
/// 「統一編號：」後面什麼都沒有。那不是資料，那是排版事故。
#[tokio::test]
async fn a_blank_tax_id_becomes_null_not_an_empty_string() {
    let e = env("blank").await;

    let mut input = input_from_current(&e.ctx).await;
    input.tax_id = Some("   ".into());
    input.address = Some("".into());
    let s = store::update(&e.ctx, input).await.unwrap();

    assert_eq!(s.tax_id, None);
    assert_eq!(s.address, None);
}

/// 打錯的設定要**整筆退回**，不能改一半。
///
/// 一個把店名改對、稅率打錯的請求如果只退稅率那一半，老闆會看到店名變了
/// 就以為存成功了 —— 而稅率其實還是舊的。
#[tokio::test]
async fn a_bad_value_rejects_the_whole_request() {
    let e = env("reject").await;
    let before = store::get(&e.ctx).await.unwrap();

    /// 「把某一個欄位改壞」的一種方法。取個名字是為了讓 clippy 別再說
    /// 這個型別太複雜 —— 它確實複雜，但拆開來會讓表格更難讀。
    type Break = Box<dyn Fn(&mut StoreInput)>;

    let cases: Vec<(&str, Break)> = vec![
        ("店名空白", Box::new(|i: &mut StoreInput| i.name = "   ".into())),
        // 多打一個零：500 → 5000。症狀是每張單多收 45%，而收銀員不會知道為什麼。
        (
            "稅率超過 100%",
            Box::new(|i: &mut StoreInput| i.tax_rate_bp = 10_001),
        ),
        (
            "稅率負數",
            Box::new(|i: &mut StoreInput| i.tax_rate_bp = -1),
        ),
        (
            "服務費超過 100%",
            Box::new(|i: &mut StoreInput| i.service_charge_rate_bp = 10_001),
        ),
        (
            "低消負數",
            Box::new(|i: &mut StoreInput| i.min_charge_per_head = -1),
        ),
        (
            "不認得的抹零方式",
            Box::new(|i: &mut StoreInput| i.rounding_policy = "round_up".into()),
        ),
        (
            "切點不是時間",
            Box::new(|i: &mut StoreInput| i.business_day_cutoff = "早上五點".into()),
        ),
    ];

    for (why, mutate) in cases {
        let mut input = input_from_current(&e.ctx).await;
        // 同時改一個合法欄位 —— 要證明的是它**也**沒被寫進去。
        input.name = "被改掉的店名".into();
        mutate(&mut input);

        let err = store::update(&e.ctx, input).await.unwrap_err();
        assert!(
            matches!(err, AppError::Validation(_)),
            "{why} 應該是驗證錯誤，實際是 {err:?}"
        );

        let after = store::get(&e.ctx).await.unwrap();
        assert_eq!(after.name, before.name, "{why}：整筆都不該寫進去");
        assert_eq!(after.tax_rate_bp, before.tax_rate_bp, "{why}");
    }

    assert_eq!(audit_count(&e.ctx).await, 0, "失敗的設定不該留下稽核紀錄");
}

/// ★ 畫面上那句承諾的證明：**改稅率不會回頭動已經結掉的單。**
///
/// 這不是效能考量也不是實作偷懶，是刻意的：三個月前的帳不該因為今天改了設定
/// 而變動 —— 一份會自己變的報表在稅務查核上完全站不住。
///
/// 反過來說，**還沒結帳的單會用新費率重算**，因為它還沒有「當時算好的金額」
/// 這回事。這一點畫面上也要講清楚，不然老闆會以為改設定完全不影響今天。
#[tokio::test]
async fn settled_orders_do_not_move_when_the_tax_rate_changes() {
    let e = env("retro").await;

    let c = menu::upsert_category(
        &e.ctx,
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
    let rice = menu::upsert_item(
        &e.ctx,
        ItemInput {
            id: None,
            category_id: Some(c.id),
            name: "滷肉飯".into(),
            short_name: None,
            base_price: 105,
            tax_code: None,
            is_open_price: None,
            sold_out_until: None,
            sort_order: None,
            is_active: None,
        },
    )
    .await
    .unwrap();

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
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![NewLine {
                item_id: rice.id.clone(),
                variant_id: None,
                qty_milli: Some(1000),
                modifier_ids: vec![],
                note: None,
            }],
        },
    )
    .await
    .unwrap();

    // 105 元內含 5%：稅額 105×500/10500 = 5，銷售額 100。
    assert_eq!(o.grand_total, 105);
    assert_eq!(o.tax_amount, 5);
    assert_eq!(o.sales_amount, 100);

    let settled = order::settle(
        &e.ctx,
        SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![PaymentReq {
                method_code: "cash".into(),
                amount: 105,
                tendered: Some(105),
                ref_no: None,
            }],
            idem_key: "retro-1".into(),
            split: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(settled.order.status, "settled");

    // 把稅率改成 10%。
    let mut input = input_from_current(&e.ctx).await;
    input.tax_rate_bp = 1000;
    store::update(&e.ctx, input).await.unwrap();

    let after = order::get_order(&e.ctx, &o.id).await.unwrap();
    assert_eq!(after.grand_total, 105, "已結帳的總額不能變");
    assert_eq!(after.tax_amount, 5, "已結帳的稅額不能變");
    assert_eq!(after.sales_amount, 100, "已結帳的銷售額不能變");
    assert_eq!(
        after.sales_amount + after.tax_amount,
        after.grand_total,
        "銷售額 + 稅額 == 總額 是財政部的硬檢核，任何時候都要成立"
    );

    // 而**之後**開的單用新稅率：210 元內含 10% → 稅 19、銷售額 191。
    let n = order::open_order(
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
    let n = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: n.id.clone(),
            expected_rev: n.rev,
            lines: vec![NewLine {
                item_id: rice.id,
                variant_id: None,
                qty_milli: Some(2000),
                modifier_ids: vec![],
                note: None,
            }],
        },
    )
    .await
    .unwrap();
    assert_eq!(n.grand_total, 210);
    assert_eq!(n.tax_amount, 19, "210×1000/11000 = 19.09 → 四捨五入 19");
    assert_eq!(n.sales_amount, 191);
}

/// 每人低消：**提醒，但不擋**。
///
/// 查過的市售產品沒有一套會因為低消沒到就不讓你結帳，也沒有一套會自動補一行
/// 差額。那個決定是店長的：常客差二十塊、帶小孩的桌、雨天沒人的下午，
/// 三種情況三個答案。軟體能做也該做的，只有「讓收銀員在按下結帳之前看見」。
#[tokio::test]
async fn a_minimum_spend_warns_but_never_blocks_the_sale() {
    let e = env("mincharge").await;

    let mut input = input_from_current(&e.ctx).await;
    input.min_charge_per_head = 150;
    store::update(&e.ctx, input).await.unwrap();

    let c = menu::upsert_category(
        &e.ctx,
        CategoryInput {
            id: None,
            name: "飲料".into(),
            color: None,
            sort_order: None,
            is_active: None,
        },
    )
    .await
    .unwrap();
    let tea = menu::upsert_item(
        &e.ctx,
        ItemInput {
            id: None,
            category_id: Some(c.id),
            name: "紅茶".into(),
            short_name: None,
            base_price: 100,
            tax_code: None,
            is_open_price: None,
            sold_out_until: None,
            sort_order: None,
            is_active: None,
        },
    )
    .await
    .unwrap();

    // 兩個人開一桌 —— 低消 150 × 2 = 300。開檯這件事由 open_order 帶著
    // table_id + guest_count 一起做，沒有獨立的「開桌」指令。
    // 種子資料只建空的店與權限，桌位屬於示範資料，所以這裡自己建一張。
    let table_id = table::upsert_table(
        &e.ctx,
        TableInput {
            id: None,
            code: "A1".into(),
            name: None,
            seats: Some(4),
            area_name: None,
            is_active: None,
        },
    )
    .await
    .unwrap()
    .id;

    let o = order::open_order(
        &e.ctx,
        OpenOrderReq {
            channel: Channel::DineIn,
            table_id: Some(table_id),
            guest_count: Some(2),
            client_id: None,
        },
    )
    .await
    .unwrap();

    // 只點一杯 100 —— 差 200。
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![NewLine {
                item_id: tea.id.clone(),
                variant_id: None,
                qty_milli: Some(1000),
                modifier_ids: vec![],
                note: None,
            }],
        },
    )
    .await
    .unwrap();
    assert_eq!(o.subtotal, 100);
    assert_eq!(o.min_charge_shortfall, 200, "150 × 2 − 100");

    // ★ 但結帳照樣過。低消不是一道鎖。
    let paid = order::settle(
        &e.ctx,
        SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![PaymentReq {
                method_code: "cash".into(),
                amount: o.grand_total,
                tendered: Some(o.grand_total),
                ref_no: None,
            }],
            idem_key: "min-1".into(),
            split: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(paid.order.status, "settled");
    // **而且沒有多出任何一行。** 客人不會在收據上看到他沒點過的東西。
    assert_eq!(paid.order.lines.len(), 1);
    assert_eq!(paid.order.grand_total, o.grand_total);
}

/// 低消是**桌位**政策，外帶沒有這回事。
///
/// 這條分得不清楚的話，外帶買一杯五十塊的紅茶會跳出「未達低消」——
/// 而收銀員完全不知道那是什麼意思。
#[tokio::test]
async fn a_minimum_spend_never_applies_to_takeout() {
    let e = env("mincharge_takeout").await;

    let mut input = input_from_current(&e.ctx).await;
    input.min_charge_per_head = 150;
    store::update(&e.ctx, input).await.unwrap();

    let c = menu::upsert_category(
        &e.ctx,
        CategoryInput {
            id: None,
            name: "飲料".into(),
            color: None,
            sort_order: None,
            is_active: None,
        },
    )
    .await
    .unwrap();
    let tea = menu::upsert_item(
        &e.ctx,
        ItemInput {
            id: None,
            category_id: Some(c.id),
            name: "紅茶".into(),
            short_name: None,
            base_price: 50,
            tax_code: None,
            is_open_price: None,
            sold_out_until: None,
            sort_order: None,
            is_active: None,
        },
    )
    .await
    .unwrap();

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
    let o = order::add_lines(
        &e.ctx,
        AddLinesReq {
            order_id: o.id,
            expected_rev: o.rev,
            lines: vec![NewLine {
                item_id: tea.id,
                variant_id: None,
                qty_milli: Some(1000),
                modifier_ids: vec![],
                note: None,
            }],
        },
    )
    .await
    .unwrap();

    assert_eq!(o.min_charge_shortfall, 0, "外帶沒有低消");
}
