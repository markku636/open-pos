//! 分帳的整合測試。
//!
//! # 這裡唯一不能出錯的事
//!
//! **Σ 每一份 == 訂單總額，嚴格相等。** 差一元的話不是店家吃掉就是客人多付，
//! 而且它會出現在日結的現金差異裡卻找不到原因。
//!
//! 第二重要的是**「還沒收完」不能看起來像收完了**：分帳最貴的意外是三個人
//! 各付各的、第三個人走掉而沒有人發現。所以訂單只有在付清時才變成 settled，
//! 桌位也只有在那時才釋放。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use open_pos::core::clock::Stamp;
use open_pos::core::pricing::Channel;
use open_pos::ctx::{AppCtx, Ctx};
use open_pos::infra::db::sqlite::SqliteDb;
use open_pos::paths::DataLayout;
use open_pos::services::{demo, menu, order, table};

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
        "openpos_split_{}_{}_{}",
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
    async fn order_with(&self, table_id: Option<&str>, items: &[&str]) -> order::OrderView {
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
                channel: if table_id.is_some() {
                    Channel::DineIn
                } else {
                    Channel::Takeout
                },
                table_id: table_id.map(str::to_string),
                guest_count: Some(3),
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

    /// 結一份。金額由 plan 決定，這裡一律用剛好的現金付。
    async fn pay(
        &self,
        order_id: &str,
        split: Option<order::SplitReq>,
        amount: i64,
    ) -> open_pos::error::AppResult<order::SettleResult> {
        let o = order::get_order(&self.ctx, order_id).await.unwrap();
        order::settle(
            &self.ctx,
            order::SettleReq {
                order_id: order_id.to_string(),
                expected_rev: o.rev,
                payments: vec![order::PaymentReq {
                    method_code: "cash".into(),
                    amount,
                    tendered: Some(amount),
                    ref_no: None,
                }],
                idem_key: open_pos::core::ids::Id::new().to_string(),
                split,
            },
        )
        .await
    }

    /// 這張單的每一份分別收了多少。
    async fn parts(&self, order_id: &str) -> Vec<i64> {
        sqlx::query_scalar(
            "SELECT grand_total FROM bills
              WHERE order_id = ?1 AND status = 'settled' ORDER BY split_index",
        )
        .bind(order_id)
        .fetch_all(self.ctx.db.reader())
        .await
        .unwrap()
    }
}

/// ★ 平分：每一份的加總必須嚴格等於訂單總額。
///
/// 三個人分 101 是 34/34/33，不是三個 33（少收 2 元）也不是三個 34（多收 1 元）。
#[tokio::test]
async fn even_shares_add_up_to_the_order_exactly() {
    let e = env("even").await;
    let o = e.order_with(None, &["珍珠奶茶", "滷肉飯", "豆漿"]).await;
    let total = o.grand_total;

    for i in 1..=3 {
        let r = e
            .pay(&o.id, Some(order::SplitReq::Even { parts: 3 }), 10_000)
            .await
            .unwrap();
        assert_eq!(r.split_index, i);
        if i < 3 {
            assert!(r.remaining > 0, "第 {i} 份結完之後不該顯示已收完");
            assert_ne!(r.order.status, "settled", "還沒收完就不能算已結帳");
        } else {
            assert_eq!(r.remaining, 0);
            assert_eq!(r.order.status, "settled");
        }
    }

    let parts = e.parts(&o.id).await;
    assert_eq!(parts.len(), 3);
    assert_eq!(parts.iter().sum::<i64>(), total, "三份加起來要等於整單");
    let (max, min) = (*parts.iter().max().unwrap(), *parts.iter().min().unwrap());
    assert!(max - min <= 1, "平分的差距不該超過一元：{parts:?}");

    e.ctx.db.close().await;
}

/// ★ 沒收完的單不能從桌位圖上消失。
#[tokio::test]
async fn a_half_paid_table_is_still_occupied() {
    let e = env("halfpaid").await;
    let a1 = table::list_tables(&e.ctx)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.code == "A1")
        .unwrap();
    let o = e.order_with(Some(&a1.id), &["珍珠奶茶", "滷肉飯"]).await;

    e.pay(&o.id, Some(order::SplitReq::Even { parts: 2 }), 10_000)
        .await
        .unwrap();

    let seated = table::list_tables(&e.ctx)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.code == "A1")
        .unwrap();
    assert!(
        seated.session.is_some(),
        "還有一半沒收，這一桌不能被釋放 —— 不然沒有人會想起要去收"
    );

    e.pay(&o.id, Some(order::SplitReq::Even { parts: 2 }), 10_000)
        .await
        .unwrap();
    let freed = table::list_tables(&e.ctx)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.code == "A1")
        .unwrap();
    assert!(freed.session.is_none(), "收完了桌就該空出來");

    e.ctx.db.close().await;
}

/// ★ 分項：同一項不能被結兩次。
#[tokio::test]
async fn an_item_cannot_be_paid_for_twice() {
    let e = env("byitem").await;
    let o = e.order_with(None, &["珍珠奶茶", "滷肉飯", "豆漿"]).await;
    let total = o.grand_total;
    let bubble = o.lines[0].id.clone();
    let rest: Vec<String> = o.lines[1..].iter().map(|l| l.id.clone()).collect();

    let r = e
        .pay(
            &o.id,
            Some(order::SplitReq::Items {
                line_ids: vec![bubble.clone()],
            }),
            10_000,
        )
        .await
        .unwrap();
    assert_eq!(r.remaining, total - o.lines[0].amount);

    let err = e
        .pay(
            &o.id,
            Some(order::SplitReq::Items {
                line_ids: vec![bubble],
            }),
            10_000,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    assert!(err.to_string().contains("珍珠奶茶"), "訊息要指出是哪一項");

    let r = e
        .pay(
            &o.id,
            Some(order::SplitReq::Items { line_ids: rest }),
            10_000,
        )
        .await
        .unwrap();
    assert_eq!(r.remaining, 0);
    assert_eq!(r.order.status, "settled");
    assert_eq!(e.parts(&o.id).await.iter().sum::<i64>(), total);

    e.ctx.db.close().await;
}

/// 指定金額：收不了比剩下還多的錢。
#[tokio::test]
async fn you_cannot_collect_more_than_what_is_left() {
    let e = env("overpay").await;
    let o = e.order_with(None, &["珍珠奶茶"]).await;

    let err = e
        .pay(
            &o.id,
            Some(order::SplitReq::Amount {
                amount: o.grand_total + 1,
            }),
            10_000,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    // 剛好收完是可以的，而且那一份就是最後一份。
    let r = e
        .pay(
            &o.id,
            Some(order::SplitReq::Amount {
                amount: o.grand_total,
            }),
            10_000,
        )
        .await
        .unwrap();
    assert_eq!(r.remaining, 0);
    assert_eq!(r.order.status, "settled");

    e.ctx.db.close().await;
}

/// 指定金額分帳可以只收一部分，剩下的用一般結帳收尾。
///
/// 「我先出 500」→ 店員按分帳收 500 →「剩下他付」→ 店員直接按結帳。
#[tokio::test]
async fn a_partial_amount_can_be_finished_by_a_plain_settle() {
    let e = env("tail").await;
    let o = e.order_with(None, &["珍珠奶茶", "滷肉飯"]).await;
    let total = o.grand_total;

    let r = e
        .pay(&o.id, Some(order::SplitReq::Amount { amount: 50 }), 50)
        .await
        .unwrap();
    assert_eq!(r.remaining, total - 50);

    // 沒帶 split 就是「把剩下的全部結掉」。
    let r = e.pay(&o.id, None, 10_000).await.unwrap();
    assert_eq!(r.remaining, 0);
    assert_eq!(r.split_index, 2);

    assert_eq!(e.parts(&o.id).await, vec![50, total - 50]);
    e.ctx.db.close().await;
}

/// 同一張單不能混著兩種分法 —— 混了之後「第 2 份」是多少沒有人說得準。
#[tokio::test]
async fn you_cannot_switch_split_mode_halfway() {
    let e = env("switch").await;
    let o = e.order_with(None, &["珍珠奶茶", "滷肉飯"]).await;

    e.pay(&o.id, Some(order::SplitReq::Even { parts: 3 }), 10_000)
        .await
        .unwrap();

    let err = e
        .pay(&o.id, Some(order::SplitReq::Amount { amount: 10 }), 10)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");

    // 份數也不能中途改。
    let err = e
        .pay(&o.id, Some(order::SplitReq::Even { parts: 4 }), 10_000)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    assert!(err.to_string().contains('3'), "訊息要說出原本是幾份：{err}");

    e.ctx.db.close().await;
}

#[tokio::test]
async fn a_settled_order_refuses_another_part() {
    let e = env("done").await;
    let o = e.order_with(None, &["珍珠奶茶"]).await;
    e.pay(&o.id, None, 10_000).await.unwrap();

    let err = e
        .pay(&o.id, Some(order::SplitReq::Amount { amount: 10 }), 10)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_CONFLICT");
    e.ctx.db.close().await;
}

#[tokio::test]
async fn splitting_into_fewer_than_two_is_refused() {
    let e = env("one").await;
    let o = e.order_with(None, &["珍珠奶茶"]).await;
    let err = e
        .pay(&o.id, Some(order::SplitReq::Even { parts: 1 }), 10_000)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");
    e.ctx.db.close().await;
}

/// 分項分帳要選得到東西，而且要真的是這張單上的東西。
#[tokio::test]
async fn by_item_needs_real_lines_from_this_order() {
    let e = env("bogus").await;
    let o = e.order_with(None, &["珍珠奶茶"]).await;

    let err = e
        .pay(&o.id, Some(order::SplitReq::Items { line_ids: vec![] }), 1)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    let other = e.order_with(None, &["滷肉飯"]).await;
    let err = e
        .pay(
            &o.id,
            Some(order::SplitReq::Items {
                line_ids: vec![other.lines[0].id.clone()],
            }),
            10_000,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "ERR_VALIDATION");

    e.ctx.db.close().await;
}

/// ★ 每一張帳單自己的 `銷售額 + 稅額 == 總額`。
///
/// 這是財政部的硬檢核。分帳時各拆各的，而不是把整單稅額分攤下去 ——
/// 分攤會讓某一份差一元，然後整批發票被退件。
#[tokio::test]
async fn every_part_balances_its_own_tax() {
    let e = env("tax").await;
    let o = e
        .order_with(None, &["珍珠奶茶", "滷肉飯", "豆漿", "美式咖啡"])
        .await;

    for _ in 0..3 {
        e.pay(&o.id, Some(order::SplitReq::Even { parts: 3 }), 10_000)
            .await
            .unwrap();
    }

    let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT sales_amount, tax_amount, grand_total FROM bills WHERE order_id = ?1",
    )
    .bind(&o.id)
    .fetch_all(e.ctx.db.reader())
    .await
    .unwrap();
    assert_eq!(rows.len(), 3);
    for (sales, tax, total) in &rows {
        assert_eq!(sales + tax, *total, "銷售額加稅額要等於總額");
    }
    assert_eq!(
        rows.iter().map(|r| r.2).sum::<i64>(),
        o.grand_total,
        "三份的總額加起來要等於整單"
    );

    e.ctx.db.close().await;
}
