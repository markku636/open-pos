//! 出單的端到端測試。
//!
//! 用檔案 driver 當「印表機」：從點餐、結帳、排版、編碼一路到送出去，
//! 只有最後一吋不是真的熱感紙。**這一份測試不需要任何硬體** ——
//! 一個要有特定機器才驗得了的開源專案，外部貢獻者是進不來的。
//!
//! 送出去的位元組會用同一份解碼器解回文字再斷言，所以測到的不只是
//! 「有沒有寫出檔案」，而是「印出來的內容對不對」。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::infra::printer::escpos::{decode, CjkEncoding};
use open_pos::infra::printer::routing::BindingMode;
use open_pos::infra::printer::Transport;
use open_pos::paths::DataLayout;
use open_pos::receipt::PaperWidth;
use open_pos::services::{demo, menu, order, print_worker, printer};

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
        "openpos_print_{}_{}_{}",
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
    fn spool(&self, name: &str) -> std::path::PathBuf {
        self.root.join("spool").join(name).join("job.bin")
    }

    /// 建一台把位元組寫到檔案的「印表機」。
    async fn file_printer(&self, name: &str) -> String {
        printer::upsert_printer(
            &self.ctx,
            printer::PrinterInput {
                id: None,
                name: name.into(),
                transport: Transport::File {
                    path: self.spool(name).to_string_lossy().into_owned(),
                    append: false,
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
        .unwrap()
        .id
    }

    /// 這台「印表機」收到的所有內容，解碼成人看的文字。
    fn printed(&self, name: &str) -> Vec<String> {
        let dir = self.spool(name);
        let dir = dir.parent().unwrap();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut paths: Vec<_> = entries.map(|e| e.unwrap().path()).collect();
        paths.sort();
        paths
            .iter()
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bin"))
            .map(|p| {
                let bytes = std::fs::read(p).unwrap();
                let ops = decode::decode(&bytes, CjkEncoding::Big5);
                assert!(
                    !ops.iter().any(|o| matches!(o, decode::Op::Unknown(_))),
                    "送出去的位元組有解不出來的段落 —— 模擬器與真機會看到不同的東西"
                );
                decode::render_human(&ops)
            })
            .collect()
    }

    async fn add_one(&self, item_name: &str) -> String {
        let tree = menu::menu_tree(&self.ctx).await.unwrap();
        let item = tree
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| i.name == item_name)
            .unwrap_or_else(|| panic!("示範菜單裡沒有「{item_name}」"));

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
        o.id
    }
}

/// ★ 從點餐到位元組落地的完整一條路。
#[tokio::test]
async fn an_order_reaches_the_printer_and_says_the_right_thing() {
    let e = env("endtoend").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e.file_printer("櫃檯").await;

    e.add_one("珍珠奶茶").await;

    let report = print_worker::tick(&e.ctx).await.unwrap();
    assert_eq!(report.fanned_out, 1, "應該展開成一張單");
    assert_eq!(report.printed, 1, "{report:?}");

    let out = e.printed("櫃檯");
    assert_eq!(out.len(), 1);
    assert!(
        out[0].contains("珍珠奶茶"),
        "印出來的內容不對：\n{}",
        out[0]
    );
    // 廚房單**不印金額** —— 廚師不需要知道，印了只會讓單變長。
    // 只看品項那一行：單頭有訂單號與日期，裡面本來就會有數字。
    let item_line = out[0]
        .lines()
        .find(|l| l.contains("珍珠奶茶"))
        .expect("找不到品項行");
    assert!(!item_line.contains("60"), "廚房單不該有金額：{item_line}");
    // 印在紙上的時間必須是店家的當地時間，不是 UTC ——
    // 店員在晚上九點半拿到一張寫著下午一點的單，只會以為系統壞了。
    let now_local = chrono::Utc::now().with_timezone(&chrono_tz::Asia::Taipei);
    assert!(
        out[0].contains(&now_local.format("%Y-%m-%d %H:").to_string()),
        "單上的時間不是台北時間：{}",
        out[0]
    );
    assert!(out[0].contains("切紙"), "少了切紙指令：\n{}", out[0]);

    e.ctx.db.close().await;
}

/// 跑第二次不會再印一次。worker 可能因為當機而重跑。
#[tokio::test]
async fn running_the_worker_twice_does_not_print_twice() {
    let e = env("idempotent").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e.file_printer("櫃檯").await;
    e.add_one("滷肉飯").await;

    assert_eq!(print_worker::tick(&e.ctx).await.unwrap().printed, 1);
    assert_eq!(print_worker::tick(&e.ctx).await.unwrap().printed, 0);
    assert_eq!(e.printed("櫃檯").len(), 1);

    e.ctx.db.close().await;
}

/// ★ 兩個分區各印各的：飲料吧不該看到熱炒的品項。
#[tokio::test]
async fn each_station_only_sees_its_own_items() {
    let e = env("stations").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    let drink_printer = e.file_printer("飲料吧").await;
    let kitchen_printer = e.file_printer("廚房").await;

    let drink = printer::upsert_station(
        &e.ctx,
        printer::StationInput {
            id: None,
            name: "飲料吧".into(),
            template: Some("drink".into()),
            split_per_item: None,
            sort_order: Some(0),
            is_active: None,
            printers: Some(vec![printer::StationPrinter {
                printer_id: drink_printer,
                priority: 0,
                mode: BindingMode::Failover,
            }]),
        },
    )
    .await
    .unwrap();
    let kitchen = printer::upsert_station(
        &e.ctx,
        printer::StationInput {
            id: None,
            name: "熱炒區".into(),
            template: Some("kitchen".into()),
            split_per_item: None,
            sort_order: Some(10),
            is_active: None,
            printers: Some(vec![printer::StationPrinter {
                printer_id: kitchen_printer,
                priority: 0,
                mode: BindingMode::Failover,
            }]),
        },
    )
    .await
    .unwrap();

    // 分類綁分區：飲料 → 飲料吧、主餐 → 熱炒區。
    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    for (cat_name, station_id) in [("飲料", &drink.id), ("主餐", &kitchen.id)] {
        let cat = tree
            .categories
            .iter()
            .find(|c| c.category.name == cat_name)
            .unwrap();
        let mut uow = e.ctx.db.begin_write().await.unwrap();
        sqlx::query("UPDATE categories SET default_station_id = ?2 WHERE id = ?1")
            .bind(&cat.category.id)
            .bind(station_id)
            .execute(uow.conn())
            .await
            .unwrap();
        uow.commit().await.unwrap();
    }

    // 一張單同時有飲料與主餐。
    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    let find = |name: &str| {
        tree.categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| i.name == name)
            .unwrap()
            .id
            .clone()
    };
    let o = order::open_order(
        &e.ctx,
        order::OpenOrderReq {
            channel: Channel::DineIn,
            table_id: None,
            guest_count: Some(2),
            client_id: None,
        },
    )
    .await
    .unwrap();
    order::add_lines(
        &e.ctx,
        order::AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![
                order::NewLine {
                    item_id: find("珍珠奶茶"),
                    variant_id: None,
                    modifier_ids: vec![],
                    qty_milli: None,
                    note: None,
                },
                order::NewLine {
                    item_id: find("滷肉飯"),
                    variant_id: None,
                    modifier_ids: vec![],
                    qty_milli: None,
                    note: None,
                },
            ],
        },
    )
    .await
    .unwrap();

    let report = print_worker::tick(&e.ctx).await.unwrap();
    assert_eq!(report.printed, 2, "兩個分區各一張：{report:?}");

    let drink_out = e.printed("飲料吧").join("\n");
    let kitchen_out = e.printed("廚房").join("\n");
    assert!(drink_out.contains("珍珠奶茶"), "\n{drink_out}");
    assert!(
        !drink_out.contains("滷肉飯"),
        "飲料吧不該看到主餐：\n{drink_out}"
    );
    assert!(kitchen_out.contains("滷肉飯"), "\n{kitchen_out}");
    assert!(
        !kitchen_out.contains("珍珠奶茶"),
        "熱炒區不該看到飲料：\n{kitchen_out}"
    );

    e.ctx.db.close().await;
}

/// ★ 還沒設定任何印表機時，單**不能消失**。
#[tokio::test]
async fn with_no_printer_configured_the_intent_is_kept_and_said_out_loud() {
    let e = env("noprinter").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e.add_one("珍珠奶茶").await;

    let report = print_worker::tick(&e.ctx).await.unwrap();
    assert_eq!(report.fanned_out, 0);
    assert_eq!(report.printed, 0);

    let status = printer::queue_status(&e.ctx).await.unwrap();
    assert_eq!(status.unrouted, 1, "意圖被丟掉了");
    assert!(status.needs_attention, "沒有設定印表機卻沒有提醒任何人");
    // 斷言的是**狀態**而不是文案 —— 句子現在在前端的字典裡（三種語言各一份），
    // 後端只負責分類。這樣改文案不會弄紅測試，而分類錯了一定會。
    assert_eq!(
        status.state,
        open_pos::services::printer::QueueState::Unrouted,
        "有單等著印又沒有印表機，應該是 unrouted"
    );

    // 設定完成之後，同一張單要自己流出去。
    e.file_printer("櫃檯").await;
    let report = print_worker::tick(&e.ctx).await.unwrap();
    assert_eq!(report.printed, 1, "補設定之後那張單沒有印出來：{report:?}");
    assert!(e.printed("櫃檯")[0].contains("珍珠奶茶"));

    e.ctx.db.close().await;
}

/// 連不上的機器要重試、要留下分類，而不是安靜消失。
#[tokio::test]
async fn an_unreachable_printer_retries_and_records_why() {
    let e = env("unreachable").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    printer::upsert_printer(
        &e.ctx,
        printer::PrinterInput {
            id: None,
            name: "壞掉的機器".into(),
            // 埠 1 一定沒有人在聽，connect 會立刻 refused。
            transport: Transport::Network {
                host: "127.0.0.1".into(),
                port: 1,
            },
            paper: PaperWidth::Mm80,
            encoding: Some(CjkEncoding::Big5),
            cutter: None,
            drawer: None,
            status_query: None,
            render_mode: None,
            is_active: Some(true),
        },
    )
    .await
    .unwrap();
    e.add_one("珍珠奶茶").await;

    let report = print_worker::tick(&e.ctx).await.unwrap();
    assert_eq!(report.printed, 0);
    assert_eq!(report.failed, 1, "{report:?}");

    let jobs = printer::list_print_jobs(&e.ctx, None).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, "pending", "應該還會再試");
    assert_eq!(jobs[0].attempts, 1);
    // 三類不是兩類：店員看到「連線失敗」會去看網路線。
    assert_eq!(jobs[0].last_error_class.as_deref(), Some("transient"));
    assert!(
        jobs[0]
            .last_error
            .as_deref()
            .unwrap()
            .contains("127.0.0.1:1"),
        "錯誤訊息要說出是哪一台：{:?}",
        jobs[0].last_error
    );

    e.ctx.db.close().await;
}

/// 收據要印金額，而且未稅與稅額都要在上面。
#[tokio::test]
async fn the_customer_receipt_carries_the_numbers_that_must_match_the_invoice() {
    let e = env("receipt").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e.file_printer("櫃檯").await;

    let order_id = e.add_one("雞腿便當").await;
    let o = order::get_order(&e.ctx, &order_id).await.unwrap();
    order::settle(
        &e.ctx,
        order::SettleReq {
            order_id,
            expected_rev: o.rev,
            payments: vec![order::PaymentReq {
                method_code: "cash".into(),
                amount: o.grand_total,
                tendered: Some(200),
                ref_no: None,
            }],
            idem_key: format!("test-settle-{}", o.rev),
            split: None,
        },
    )
    .await
    .unwrap();

    print_worker::tick(&e.ctx).await.unwrap();
    let out = e.printed("櫃檯").join("\n=====\n");
    assert!(out.contains("雞腿便當"), "\n{out}");
    assert!(out.contains("110"), "收據要印金額：\n{out}");
    // 未稅與稅額是發票上要對的兩個數字，收據上必須看得到。
    assert!(out.contains("稅"), "\n{out}");

    // ★ 收據上這三個數字必須自己對得起來：合計 110、現金 200、找零 90。
    //   印成「現金 110 / 找零 90」的話，客人會當場問，而店員解釋不出來。
    let cash_line = out
        .lines()
        .find(|l| l.contains("現金"))
        .expect("收據上沒有付款方式");
    assert!(
        cash_line.contains("200"),
        "現金那一行要印客人給了多少，不是沖銷了多少：{cash_line}"
    );
    let change_line = out
        .lines()
        .find(|l| l.contains("找零"))
        .expect("收據上沒有找零");
    assert!(change_line.contains("90"), "{change_line}");

    e.ctx.db.close().await;
}

/// 網路型（TCP 9100）那條路也要真的走過一次。
///
/// 檔案 driver 驗得了版面與編碼，但驗不到 socket 這一段 —— 而
/// 「連得上卻送不出去」是真機上最常見的一種壞法。這裡起一個假的出單機，
/// 收下位元組再用同一份解碼器解回文字。
#[tokio::test]
async fn bytes_really_travel_over_tcp_to_a_printer() {
    let e = env("tcp").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.unwrap();
        buf
    });

    printer::upsert_printer(
        &e.ctx,
        printer::PrinterInput {
            id: None,
            name: "網路出單機".into(),
            transport: Transport::Network {
                host: "127.0.0.1".into(),
                port,
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

    e.add_one("珍珠奶茶").await;
    let report = print_worker::tick(&e.ctx).await.unwrap();
    assert_eq!(report.printed, 1, "{report:?}");

    let bytes = server.await.unwrap();
    assert!(!bytes.is_empty(), "什麼都沒收到");
    let ops = decode::decode(&bytes, CjkEncoding::Big5);
    assert!(
        !ops.iter().any(|o| matches!(o, decode::Op::Unknown(_))),
        "真的送出去的位元組有解不出來的段落：{ops:?}"
    );
    let text = decode::render_human(&ops);
    assert!(
        text.contains("珍珠奶茶"),
        "
{text}"
    );
    assert!(
        text.contains("切紙"),
        "
{text}"
    );

    e.ctx.db.close().await;
}

/// ★ 加點單只能印**這一次新增的**那幾行。
///
/// 把整張單重印一次，廚師會把已經做好的珍珠奶茶再做一杯 —— 而他不會知道
/// 那是重複的，因為單上看起來就是要做兩杯。這是餐飲 POS 最貴的一種 bug：
/// 錯的是一份餐、一次客訴，而且系統完全不會報錯。
#[tokio::test]
async fn an_add_items_ticket_only_lists_the_new_lines() {
    let e = env("additems").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e.file_printer("櫃檯").await;

    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    let find = |name: &str| {
        tree.categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| i.name == name)
            .unwrap()
            .id
            .clone()
    };
    let line = |item_id: String| order::NewLine {
        item_id,
        variant_id: None,
        modifier_ids: vec![],
        qty_milli: None,
        note: None,
    };

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
            lines: vec![line(find("珍珠奶茶"))],
        },
    )
    .await
    .unwrap();
    // 第二次 = 加點。
    order::add_lines(
        &e.ctx,
        order::AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: vec![line(find("紅茶拿鐵"))],
        },
    )
    .await
    .unwrap();

    print_worker::tick(&e.ctx).await.unwrap();
    let out = e.printed("櫃檯");
    assert_eq!(out.len(), 2, "應該兩張單（新單一張、加點一張）");

    assert!(
        out[0].contains("珍珠奶茶"),
        "新單：
{}",
        out[0]
    );
    assert!(
        !out[0].contains("紅茶拿鐵"),
        "新單不該有還沒點的品項：
{}",
        out[0]
    );

    let add = &out[1];
    assert!(
        add.contains("加點"),
        "加點單要標明原因：
{add}"
    );
    assert!(
        add.contains("紅茶拿鐵"),
        "加點單少了新品項：
{add}"
    );
    assert!(
        !add.contains("珍珠奶茶"),
        "★ 加點單重印了已經做過的品項 —— 廚師會再做一杯：
{add}"
    );

    e.ctx.db.close().await;
}

/// 退點單要說出取消的是**哪一項**。
#[tokio::test]
async fn a_void_ticket_names_the_cancelled_item() {
    let e = env("void").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e.file_printer("櫃檯").await;

    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    let find = |name: &str| {
        tree.categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| i.name == name)
            .unwrap()
            .id
            .clone()
    };
    let line = |item_id: String| order::NewLine {
        item_id,
        variant_id: None,
        modifier_ids: vec![],
        qty_milli: None,
        note: None,
    };

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
            lines: vec![line(find("珍珠奶茶")), line(find("滷肉飯"))],
        },
    )
    .await
    .unwrap();

    let target = o
        .lines
        .iter()
        .find(|l| l.name == "珍珠奶茶")
        .unwrap()
        .id
        .clone();
    order::void_line(&e.ctx, o.id.clone(), o.rev, target, None)
        .await
        .unwrap();

    print_worker::tick(&e.ctx).await.unwrap();
    let out = e.printed("櫃檯");
    let void_ticket = out.last().unwrap();
    assert!(
        void_ticket.contains("取消"),
        "
{void_ticket}"
    );
    assert!(
        void_ticket.contains("珍珠奶茶"),
        "
{void_ticket}"
    );
    assert!(
        !void_ticket.contains("滷肉飯"),
        "退點單不該列出沒有被取消的品項 —— 廚師會不知道要取消哪一項：
{void_ticket}"
    );

    e.ctx.db.close().await;
}

/// ★ 連點三下 → 紙上只印一行 ×3。
///
/// 收銀員在尖峰時間是連點的，不是去改數量欄位。沒有這一條，四個人的吃到飽
/// 會在收據上變成四行一模一樣的字，客人數不清楚自己被算了幾份。
/// Clover 的「Group identical items into one line」預設就是開的。
///
/// 合併只發生在**版型層**：`order_items` 那邊還是三行，
/// 因為那是「誰在幾點點了什麼」的真相，也是分項分帳挑得動單一品項的前提。
#[tokio::test]
async fn tapping_the_same_item_three_times_prints_one_line_times_three() {
    let e = env("collapse").await;
    demo::seed_demo_menu(&e.ctx).await.unwrap();
    e.file_printer("櫃檯").await;

    let tree = menu::menu_tree(&e.ctx).await.unwrap();
    let tea = tree
        .categories
        .iter()
        .flat_map(|c| c.items.iter())
        .find(|i| i.name == "古早味紅茶")
        .expect("示範菜單裡應該有古早味紅茶")
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
    // 一次送三行 —— 這正是「連點三下再送出」在 API 上的樣子。
    let o = order::add_lines(
        &e.ctx,
        order::AddLinesReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            lines: (0..3)
                .map(|_| order::NewLine {
                    item_id: tea.id.clone(),
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
    assert_eq!(o.lines.len(), 3, "資料庫那邊仍然是三行，合併只在紙上");

    order::settle(
        &e.ctx,
        order::SettleReq {
            order_id: o.id.clone(),
            expected_rev: o.rev,
            payments: vec![order::PaymentReq {
                method_code: "cash".into(),
                amount: o.grand_total,
                tendered: Some(o.grand_total),
                ref_no: None,
            }],
            idem_key: "collapse-1".into(),
            split: None,
        },
    )
    .await
    .unwrap();

    print_worker::tick(&e.ctx).await.unwrap();
    let out = e.printed("櫃檯");
    // [0] 是廚房單、[1] 是收據。兩張都該只有一行紅茶。
    for paper in &out {
        let n = paper.lines().filter(|l| l.contains("古早味紅茶")).count();
        assert_eq!(n, 1, "同一張單上不該印三行一樣的字：\n{paper}");
    }

    // 靠內容認收據，不靠順序 —— 廚房單不印金額，挑錯了斷言就失去意義。
    let receipt = out
        .iter()
        .find(|p| p.contains("小計"))
        .expect("應該有一張印著小計的收據");
    let item_line = receipt
        .lines()
        .find(|l| l.contains("古早味紅茶"))
        .expect("收據上找不到品項行");
    assert!(item_line.contains('3'), "數量應該合併成 3：{item_line}");
    // ★ 金額也要合併：30 × 3 = 90。各行加總對不上總計的收據，客人會當場抓到。
    assert!(
        item_line.contains("90"),
        "金額應該是合併後的 90：{item_line}"
    );

    e.ctx.db.close().await;
}
