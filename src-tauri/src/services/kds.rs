//! 廚房顯示（KDS）。
//!
//! # 這一頁的第一原則：紙本才是第一真相
//!
//! **紙本廚房出單機是第一真相來源，KDS 是輔助顯示。**
//! 純 KDS、沒有出單機的店家，主機故障時沒有安全的降級路徑 —— 這一句誠實的話
//! 比任何程式碼都值錢，所以它也寫在 README 裡。
//!
//! # 為什麼推播是「整份快照」而不是增量
//!
//! 廚房同時在做的單撐死 50 張，整份狀態很小。而快照是**自我修正的**：
//! 任何原因造成的狀態漂移（bug、時鐘、競態、斷線期間的變化）都會在下一次
//! 推播被抹平。增量重播則會讓錯誤永久累積 —— 而在廚房，累積的錯誤是漏掉的餐。
//!
//! # 為什麼「完成」走一般的 HTTP POST
//!
//! 而不是雙向的 WebSocket：樂觀更新與錯誤處理都是熟悉的請求／回應模型，
//! 不必管兩套連線狀態。KDS 需要的雙向只有這一個動作，為它扛一整套 WS
//! 的狀態機並不划算。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KdsLine {
    pub id: String,
    pub name: String,
    pub variant_name: Option<String>,
    pub options: Vec<String>,
    pub note: Option<String>,
    pub qty_milli: i64,
    /// pending / fired / cooking / ready
    pub status: String,
    pub station_id: Option<String>,
    pub station_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KdsTicket {
    pub order_id: String,
    pub order_no: String,
    /// 通路代碼：dine_in / takeout / delivery。
    ///
    /// 這裡刻意給代碼而不是「內用」那三個字 —— 廚房平板可能是中文、英文或
    /// 日文的，而後端不知道現在是誰在看。代碼是資料，顯示文字是畫面的事，
    /// 字典本來就在前端，翻兩份只會有一份先過時。
    pub channel: String,
    pub table_label: Option<String>,
    /// 這張單開了多久（秒）。廚房看的是「等最久的那一張」而不是時間點。
    pub waiting_seconds: i64,
    pub placed_at: String,
    pub lines: Vec<KdsLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KdsBoard {
    pub generated_at: String,
    pub tickets: Vec<KdsTicket>,
}

/// 目前廚房該做的東西。
///
/// 已經上菜（served）或取消的行不會出現。整份很小 —— 同時在做的單撐死 50 張。
pub async fn board(ctx: &Ctx) -> AppResult<KdsBoard> {
    let now = Stamp::now();
    let rows = sqlx::query(
        "SELECT oi.id, oi.order_id, oi.line_no, oi.name_snapshot, oi.variant_name_snapshot,
                oi.note, oi.qty_milli, oi.kitchen_status, oi.station_id,
                ps.name AS station_name,
                o.order_no, o.channel, o.opened_at,
                t.name AS table_name
           FROM order_items oi
           JOIN orders o ON o.id = oi.order_id
           LEFT JOIN print_stations ps ON ps.id = oi.station_id AND ps.deleted_at IS NULL
           LEFT JOIN dining_tables t ON t.id = o.table_id
          WHERE oi.voided_at IS NULL
            AND oi.kitchen_status NOT IN ('served', 'cancelled')
            AND o.status NOT IN ('voided', 'draft')
          ORDER BY o.opened_at, o.order_no, oi.line_no",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let mut tickets: Vec<KdsTicket> = Vec::new();
    for r in &rows {
        let order_id: String = r.get("order_id");
        let line_id: String = r.get("id");

        let options: Vec<String> = sqlx::query_scalar(
            "SELECT name_snapshot FROM order_item_modifiers WHERE order_item_id = ?1 ORDER BY id",
        )
        .bind(&line_id)
        .fetch_all(ctx.db.reader())
        .await?;

        let line = KdsLine {
            id: line_id,
            name: r.get("name_snapshot"),
            variant_name: r.get("variant_name_snapshot"),
            options,
            note: r.get("note"),
            qty_milli: r.get("qty_milli"),
            status: r.get("kitchen_status"),
            station_id: r.get("station_id"),
            station_name: r.get("station_name"),
        };

        match tickets.iter_mut().find(|t| t.order_id == order_id) {
            Some(t) => t.lines.push(line),
            None => {
                let opened_at: String = r.get("opened_at");
                tickets.push(KdsTicket {
                    order_id,
                    order_no: r.get("order_no"),
                    channel: r.get("channel"),
                    table_label: r.get("table_name"),
                    waiting_seconds: waited(&opened_at, &now),
                    placed_at: opened_at,
                    lines: vec![line],
                });
            }
        }
    }

    Ok(KdsBoard {
        generated_at: now.iso().to_string(),
        tickets,
    })
}

fn waited(opened_at: &str, now: &Stamp) -> i64 {
    chrono::DateTime::parse_from_rfc3339(opened_at)
        .map(|t| {
            (now.at - t.with_timezone(&chrono::Utc))
                .num_seconds()
                .max(0)
        })
        .unwrap_or(0)
}

/// 廚房把一行往前推：pending → cooking → ready → served。
///
/// **只能往前推，不能往回。** 廚師誤觸的成本是「那一項看起來做好了」，
/// 而讓它可以往回，成本是「有人可以把已經上桌的東西改回未完成」——
/// 後者會讓這面板不再可信。真的需要退回時走收銀機。
pub async fn advance(ctx: &Ctx, line_id: String, to: String) -> AppResult<KdsBoard> {
    const ORDER: [&str; 5] = ["pending", "fired", "cooking", "ready", "served"];
    let target = ORDER
        .iter()
        .position(|s| *s == to)
        .ok_or_else(|| AppError::Validation(format!("不認得的狀態：{to}").into()))?;

    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;

    let current: Option<String> =
        sqlx::query_scalar("SELECT kitchen_status FROM order_items WHERE id = ?1")
            .bind(&line_id)
            .fetch_optional(uow.conn())
            .await?;
    let current = current.ok_or_else(|| AppError::NotFound("找不到這一個品項".into()))?;
    let from = ORDER.iter().position(|s| *s == current).unwrap_or(0);

    if target <= from {
        // 不是錯誤：兩台平板同時點同一張單是正常的，重複的那一次靜靜忽略就好。
        // 對廚師來說「按了沒反應」比「跳出一個錯誤」好得多。
        uow.rollback().await?;
        return board(ctx).await;
    }

    // 時間戳只在第一次進入該狀態時寫入 —— 出餐時間是拿來算「等多久」的，
    // 被後來的操作覆寫就失去意義了。
    let column = match to.as_str() {
        "cooking" => "fired_at",
        "ready" => "ready_at",
        "served" => "served_at",
        _ => "fired_at",
    };
    let sql = format!(
        "UPDATE order_items SET kitchen_status = ?2, {column} = COALESCE({column}, ?3),
                                updated_at = ?3
          WHERE id = ?1"
    );
    sqlx::query(&sql)
        .bind(&line_id)
        .bind(&to)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    uow.commit().await?;

    board(ctx).await
}
