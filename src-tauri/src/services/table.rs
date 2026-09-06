//! 桌位。
//!
//! # 為什麼「桌」是一級概念而不是一個備註欄
//!
//! 內用的整條動線都掛在桌上：加點要知道加到哪一桌、送餐要知道端去哪裡、
//! 結帳要把那一桌的東西一起算。把桌號寫在備註欄裡的話，這三件事都得靠人腦。
//!
//! # 一桌同時只有一個未關的 session
//!
//! 這是**硬性保證**（DDL 上的 partial unique index），不是應用層的檢查：
//! 兩個收銀員同時點同一桌是會發生的，而應用層的「先查再寫」擋不住它。

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::services::rbac;

const PERM_SETTINGS: &str = "settings.store";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableView {
    pub id: String,
    pub code: String,
    pub name: Option<String>,
    pub area_name: Option<String>,
    pub seats: i64,
    pub is_active: bool,
    /// 正在使用中的話，這一桌現在的樣子。
    pub session: Option<TableSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableSession {
    pub id: String,
    pub guest_count: i64,
    pub opened_at: String,
    /// 這一桌目前累積多少錢。店員最常被問的問題。
    pub total: i64,
    pub order_count: i64,
    /// 這一桌開了多久（秒）。翻桌率看的是它。
    pub seated_seconds: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInput {
    pub id: Option<String>,
    /// 桌號。店員報號用的，所以要短。
    pub code: String,
    pub name: Option<String>,
    pub seats: Option<i64>,
    pub area_name: Option<String>,
    pub is_active: Option<bool>,
}

/// 桌位總覽。**這是內用店家最常看的一頁**，所以帶上每一桌的金額與時間。
pub async fn list_tables(ctx: &Ctx) -> AppResult<Vec<TableView>> {
    let now = Stamp::now();
    let rows = sqlx::query(
        "SELECT t.id, t.code, t.name, t.seats, t.is_active, a.name AS area_name,
                s.id AS session_id, s.guest_count, s.opened_at
           FROM dining_tables t
           LEFT JOIN areas a ON a.id = t.area_id AND a.deleted_at IS NULL
           LEFT JOIN table_sessions s
                  ON s.table_id = t.id AND s.status <> 'closed'
          WHERE t.deleted_at IS NULL
          ORDER BY a.sort_order, t.code",
    )
    .fetch_all(ctx.db.reader())
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let session_id: Option<String> = r.get("session_id");
        let session = match session_id {
            None => None,
            Some(sid) => {
                let sums = sqlx::query(
                    "SELECT COUNT(*) AS n, COALESCE(SUM(grand_total), 0) AS total
                       FROM orders
                      WHERE table_session_id = ?1 AND status NOT IN ('voided')",
                )
                .bind(&sid)
                .fetch_one(ctx.db.reader())
                .await?;
                let opened_at: String = r.get("opened_at");
                Some(TableSession {
                    seated_seconds: chrono::DateTime::parse_from_rfc3339(&opened_at)
                        .map(|t| {
                            (now.at - t.with_timezone(&chrono::Utc))
                                .num_seconds()
                                .max(0)
                        })
                        .unwrap_or(0),
                    id: sid,
                    guest_count: r.get("guest_count"),
                    opened_at,
                    total: sums.get("total"),
                    order_count: sums.get("n"),
                })
            }
        };
        out.push(TableView {
            id: r.get("id"),
            code: r.get("code"),
            name: r.get("name"),
            area_name: r.get("area_name"),
            seats: r.get("seats"),
            is_active: r.get::<i64, _>("is_active") == 1,
            session,
        });
    }
    Ok(out)
}

pub async fn upsert_table(ctx: &Ctx, input: TableInput) -> AppResult<TableView> {
    rbac::require(&ctx.db, &ctx.actor, PERM_SETTINGS).await?;
    let code = input.code.trim();
    if code.is_empty() {
        return Err(AppError::Validation("桌號不能空白".into()));
    }

    let now = Stamp::now();
    let id = input.id.clone().unwrap_or_else(|| Id::new().to_string());

    let mut uow = ctx.db.begin_write().await?;

    // 區域用名字對應，沒有就建一個。店家不會想先建「區域」再建桌 ——
    // 大部分店只有一個區域，那一層對他們是純粹的負擔。
    let area_id: Option<String> = match input.area_name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => {
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT id FROM areas WHERE name = ?1 AND deleted_at IS NULL LIMIT 1",
            )
            .bind(name)
            .fetch_optional(uow.conn())
            .await?;
            match existing {
                Some(id) => Some(id),
                None => {
                    let aid = Id::new().to_string();
                    sqlx::query(
                        "INSERT INTO areas (id, store_id, name, sort_order, created_at, updated_at)
                         SELECT ?1, s.id, ?2, 0, ?3, ?3 FROM stores s ORDER BY s.id LIMIT 1",
                    )
                    .bind(&aid)
                    .bind(name)
                    .bind(now.iso())
                    .execute(uow.conn())
                    .await?;
                    Some(aid)
                }
            }
        }
        _ => None,
    };

    if input.id.is_none() {
        sqlx::query(
            "INSERT INTO dining_tables (id, store_id, area_id, code, name, seats, is_active,
                                        created_at, updated_at)
             SELECT ?1, s.id, ?2, ?3, ?4, ?5, ?6, ?7, ?7 FROM stores s ORDER BY s.id LIMIT 1",
        )
        .bind(&id)
        .bind(&area_id)
        .bind(code)
        .bind(&input.name)
        .bind(input.seats.unwrap_or(4))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    } else {
        let n = sqlx::query(
            "UPDATE dining_tables SET area_id = ?2, code = ?3, name = ?4, seats = ?5,
                                      is_active = ?6, updated_at = ?7
              WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .bind(&area_id)
        .bind(code)
        .bind(&input.name)
        .bind(input.seats.unwrap_or(4))
        .bind(i64::from(input.is_active.unwrap_or(true)))
        .bind(now.iso())
        .execute(uow.conn())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(AppError::NotFound(format!("找不到桌位 {id}")));
        }
    }
    uow.commit().await?;

    list_tables(ctx)
        .await?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::Internal("桌位存好了卻讀不回來".into()))
}

pub async fn delete_table(ctx: &Ctx, id: String) -> AppResult<()> {
    rbac::require(&ctx.db, &ctx.actor, PERM_SETTINGS).await?;
    let now = Stamp::now();

    let mut uow = ctx.db.begin_write().await?;
    // 有人坐著的桌不能刪：刪掉之後那一桌的單會變成孤兒，而客人還在座位上。
    let seated: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM table_sessions WHERE table_id = ?1 AND status <> 'closed'",
    )
    .bind(&id)
    .fetch_one(uow.conn())
    .await?;
    if seated > 0 {
        return Err(AppError::Conflict(
            "這一桌還有客人在用，請先結帳或清桌。".into(),
        ));
    }

    let n = sqlx::query(
        "UPDATE dining_tables SET deleted_at = ?2, updated_at = ?2
          WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(now.iso())
    .execute(uow.conn())
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("找不到桌位 {id}")));
    }
    uow.commit().await?;
    Ok(())
}

/// 清桌：把這一桌的 session 關掉。
///
/// **只有在沒有未結帳的單時才允許**。否則「清桌」會變成一個把帳丟掉的按鈕，
/// 而那是最容易被拿來吃單的操作。
pub async fn close_table(ctx: &Ctx, table_id: String) -> AppResult<()> {
    let now = Stamp::now();
    let mut uow = ctx.db.begin_write().await?;

    let session_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM table_sessions WHERE table_id = ?1 AND status <> 'closed' LIMIT 1",
    )
    .bind(&table_id)
    .fetch_optional(uow.conn())
    .await?;
    let Some(session_id) = session_id else {
        return Err(AppError::NotFound("這一桌現在沒有客人".into()));
    };

    let unpaid: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM orders
          WHERE table_session_id = ?1 AND status NOT IN ('settled', 'voided')",
    )
    .bind(&session_id)
    .fetch_one(uow.conn())
    .await?;
    if unpaid > 0 {
        return Err(AppError::Conflict(format!(
            "這一桌還有 {unpaid} 張單沒有結帳。請先結帳，或把那些單作廢。"
        )));
    }

    sqlx::query(
        "UPDATE table_sessions SET status = 'closed', closed_at = ?2, closed_by = ?3,
                                   updated_at = ?2
          WHERE id = ?1",
    )
    .bind(&session_id)
    .bind(now.iso())
    .bind(&ctx.actor.user_id)
    .execute(uow.conn())
    .await?;
    uow.commit().await?;
    Ok(())
}
