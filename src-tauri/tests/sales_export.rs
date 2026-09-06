//! 銷售記錄與 Excel 匯出。
//!
//! Excel 這一段最容易做出「打得開但沒有用」的檔案：金額變成文字、
//! 明細把帳單那一列重複 N 次讓加總多算好幾倍。所以測試不只驗「有產出檔案」，
//! 還要把 xlsx 拆開來看裡面到底寫了什麼。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::{demo, menu, order, sales, shift, xlsx};

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
        "openpos_sales_{}_{}_{}",
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
    let e = Env {
        root,
        ctx: Arc::new(AppCtx {
            db,
            layout,
            started_at: now.at,
            actor,
        }),
    };
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e
}

impl Env {
    async fn sell(&self, items: &[&str], method: &str) -> order::SettleResult {
        let tree = menu::menu_tree(&self.ctx).await.unwrap();
        let find = |n: &str| {
            tree.categories
                .iter()
                .flat_map(|c| c.items.iter())
                .find(|i| i.name == n)
                .unwrap()
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
        order::settle(
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
        .unwrap()
    }
}

/// ★ 合計是獨立查出來的，不是拿回傳那幾筆加的。
#[tokio::test]
async fn the_totals_line_covers_everything_not_just_the_listed_rows() {
    let e = env("totals").await;
    let a = e.sell(&["珍珠奶茶"], "cash").await;
    let b = e.sell(&["滷肉飯"], "credit").await;

    let r = sales::history(&e.ctx, sales::SalesQuery::default())
        .await
        .unwrap();
    assert_eq!(r.count, 2);
    assert_eq!(r.total, a.order.grand_total + b.order.grand_total);
    assert_eq!(
        r.sales_amount + r.tax_amount,
        r.total,
        "未稅加稅額要等於總額 —— 這是財政部的硬檢核"
    );
    assert!(!r.truncated);

    let names: Vec<&str> = r.by_method.iter().map(|m| m.method.as_str()).collect();
    assert!(names.contains(&"現金"), "{names:?}");
    assert!(names.contains(&"信用卡"), "{names:?}");

    // 每一筆都要帶得出明細與付款 —— 翻帳的人要看得到當時賣了什麼。
    let one = r.sales.iter().find(|s| s.bill_no == a.bill_no).unwrap();
    assert_eq!(one.lines.len(), 1);
    assert_eq!(one.lines[0].name, "珍珠奶茶");
    assert_eq!(one.payments.len(), 1);
    assert!(one.settled_by.is_some(), "翻帳要看得出是誰結的");

    e.ctx.db.close().await;
}

#[tokio::test]
async fn filters_narrow_the_totals_too() {
    let e = env("filter").await;
    let cash = e.sell(&["珍珠奶茶"], "cash").await;
    e.sell(&["滷肉飯"], "credit").await;

    // 單號片段：收銀員手上只有客人那張收據的末幾碼。
    let tail = &cash.bill_no[cash.bill_no.len() - 4..];
    let r = sales::history(
        &e.ctx,
        sales::SalesQuery {
            keyword: Some(tail.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(r.count, 1, "篩選之後合計也要跟著縮，不能還是全部");
    assert_eq!(r.total, cash.order.grand_total);

    let r = sales::history(
        &e.ctx,
        sales::SalesQuery {
            channel: Some("dine_in".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(r.count, 0, "今天沒有內用");
    assert_eq!(r.total, 0);

    e.ctx.db.close().await;
}

/// ★ Excel 裡的金額要是**數字**，不是文字。
///
/// Excel 開 CSV 時 `1,200` 會被當字串，選起來左下角的總和是 0 ——
/// 那份表看起來對，但一加總就露餡。所以這裡把 xlsx 拆開來確認
/// 金額格寫的是數值。
#[tokio::test]
async fn money_cells_are_numbers_so_excel_can_sum_them() {
    let e = env("xlsx").await;
    e.sell(&["珍珠奶茶", "滷肉飯"], "cash").await;

    let report = sales::history(&e.ctx, sales::SalesQuery::default())
        .await
        .unwrap();
    let dir = e.ctx.layout.root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let path = xlsx::write_sales(&report, &dir).unwrap();
    assert!(std::path::Path::new(&path).exists(), "{path}");

    let sheet = read_entry(&path, "xl/worksheets/sheet1.xml");
    let total = report.total.to_string();
    assert!(
        sheet.contains(&format!("<v>{total}</v>")),
        "總計沒有寫成數字（Excel 會加總不出來）"
    );

    let wb = read_entry(&path, "xl/workbook.xml");
    for name in ["帳單", "品項明細", "付款方式"] {
        assert!(wb.contains(name), "少了「{name}」這張表");
    }

    e.ctx.db.close().await;
}

/// 日報表匯出：四張表，而且是**日結當下那一份快照**。
#[tokio::test]
async fn the_day_report_exports_every_section() {
    let e = env("day").await;
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
    e.sell(&["珍珠奶茶"], "cash").await;
    shift::close_shift(
        &e.ctx,
        shift::CloseShiftReq {
            counts: vec![shift::DenomCount {
                denomination: 1000,
                count: 1,
            }],
            note: None,
        },
    )
    .await
    .unwrap();
    let day = shift::close_business_day(&e.ctx).await.unwrap();

    // ★ 日結之後那一天的報表還要讀得回來 —— 在這之前沒有任何指令做得到，
    //   於是「快照、永不重算」存下來的東西沒有任何地方看得見。
    let again = sales::day_report(&e.ctx, day.business_date.clone())
        .await
        .unwrap();
    assert_eq!(again.z_report_no, day.z_report_no);
    assert_eq!(again.sales.total, day.sales.total);

    let days = sales::closed_days(&e.ctx).await.unwrap();
    assert_eq!(days, vec![day.business_date.clone()]);

    let dir = e.ctx.layout.root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let path = xlsx::write_day_report(&again, &dir).unwrap();
    let wb = read_entry(&path, "xl/workbook.xml");
    for name in ["日報表", "付款方式", "品項排行", "各班現金"] {
        assert!(wb.contains(name), "少了「{name}」");
    }

    e.ctx.db.close().await;
}

#[tokio::test]
async fn a_missing_folder_says_what_to_do() {
    // 最常見的錯路徑是「已經拔掉的隨身碟」，而原生錯誤訊息不會告訴他要做什麼。
    let err = sales::ensure_dir("Z:/nope/nope").unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");
    assert!(err.message().contains("隨身碟"), "{}", err.message());
}

/// xlsx 就是一個 zip。拆開來看裡面那一份 XML。
fn read_entry(path: &str, entry: &str) -> String {
    use std::io::Read;
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut f = zip.by_name(entry).unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    s
}
