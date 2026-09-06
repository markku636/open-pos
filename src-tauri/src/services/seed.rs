//! 種子資料。
//!
//! # 為什麼不放在 migration 裡
//!
//! 種子資料放 migration 有一個致命問題：使用者刪掉之後永遠回不來 ——
//! 而 migration 的 checksum 又逼你不能修改已套用的檔案。
//! 所以改成應用層在每次啟動時同步。
//!
//! 兩種資料的處理方式刻意不同：
//!
//! * **參照資料**（權限碼、系統角色、稅別）每次啟動都 upsert。
//!   新版本加了一個權限碼，升級後就會自動出現，不需要 migration。
//! * **店家資料**（店、終端、付款方式、原因碼）只在**完全空的資料庫**上建立一次。
//!   老闆把「悠遊卡」停用掉之後，不該在下次開機時又冒出來。

use crate::core::clock::Stamp;
use crate::core::ids::Id;
use crate::error::AppResult;
use crate::infra::db::sqlite::SqliteUow;
use crate::services::rbac::Actor;

/// 權限碼。`(code, group_code, group_name, name)`。
///
/// 分組是給設定畫面用的；真正重要的是**每一個會動到錢或動到紀錄的操作都有
/// 自己的權限碼**。特別是 `order.void.after_settle` —— 結完帳再作廢是餐飲業
/// 最大的防弊點（等於把現金放進口袋），它必須能被單獨收回。
const PERMISSIONS: &[(&str, &str, &str, &str)] = &[
    ("order.create", "order", "訂單", "建立訂單"),
    ("order.item.void", "order", "訂單", "刪除單一品項"),
    ("order.void", "order", "訂單", "作廢訂單"),
    (
        "order.void.after_fire",
        "order",
        "訂單",
        "已送廚房後仍可作廢",
    ),
    (
        "order.void.after_settle",
        "order",
        "訂單",
        "結帳後作廢（高風險）",
    ),
    ("order.table.move", "order", "訂單", "轉桌"),
    ("order.merge", "order", "訂單", "併桌"),
    ("order.split", "order", "訂單", "分帳"),
    ("item.price.override", "price", "價格", "改價"),
    ("discount.line", "price", "價格", "單品折扣"),
    ("discount.order", "price", "價格", "整單折扣"),
    ("discount.comp", "price", "價格", "招待"),
    ("payment.refund", "payment", "收款", "退款"),
    ("payment.drawer.open", "payment", "收款", "無交易開錢箱"),
    ("print.receipt.reprint", "print", "列印", "重印收據"),
    ("shift.open", "shift", "班別", "開班"),
    ("shift.close", "shift", "班別", "關班"),
    ("shift.review", "shift", "班別", "覆核班別"),
    ("report.daily", "report", "報表", "查看日報"),
    ("report.z", "report", "報表", "產生 Z 報表"),
    ("report.export", "report", "報表", "匯出報表"),
    ("report.audit", "report", "報表", "查看稽核紀錄"),
    ("settings.store", "settings", "設定", "店家設定"),
    ("settings.item", "settings", "設定", "菜單設定"),
    ("settings.printer", "settings", "設定", "印表機設定"),
    ("settings.user", "settings", "設定", "人員設定"),
];

/// 系統角色。`(name, display_name, sort_order)`。
const ROLES: &[(&str, &str, i64)] = &[
    ("owner", "老闆", 0),
    ("manager", "店長", 1),
    ("shift_lead", "領班", 2),
    ("cashier", "收銀員", 3),
    ("kitchen", "廚房", 4),
];

/// 收銀員能做的事。刻意保守：**預設值就是我們的安全立場**，
/// 90% 的店家不會去調整它。要放寬是店長的決定，不是我們的預設。
const CASHIER_PERMS: &[&str] = &[
    "order.create",
    "order.item.void",
    "order.table.move",
    "order.split",
    "shift.open",
    "shift.close",
    "print.receipt.reprint",
];

/// 領班 = 收銀員 + 這些。仍然拿不到「結帳後作廢」與「退款」。
const SHIFT_LEAD_EXTRA: &[&str] = &[
    "order.void",
    "order.void.after_fire",
    "order.merge",
    "discount.line",
    "report.daily",
];

const KITCHEN_PERMS: &[&str] = &["order.create"];

/// 稅別。`(code, name, rate_bp, mig_tax_type, is_default)`。
const TAX_RATES: &[(&str, &str, i64, i64, i64)] = &[
    ("TAXABLE", "應稅", 500, 1, 1),
    ("ZERO", "零稅率", 0, 2, 0),
    ("FREE", "免稅", 0, 3, 0),
];

/// 付款方式。`(code, name, kind, opens_drawer, allows_change, counts_as_cash, needs_ref)`。
const PAYMENT_METHODS: &[(&str, &str, &str, i64, i64, i64, i64)] = &[
    // 只有現金能找零、只有現金計入關班的應有現金。
    ("cash", "現金", "cash", 1, 1, 1, 0),
    ("credit", "信用卡", "card", 0, 0, 0, 1),
    ("linepay", "LINE Pay", "mobile", 0, 0, 0, 1),
    ("jkopay", "街口支付", "mobile", 0, 0, 0, 1),
    ("easycard", "悠遊卡", "stored_value", 0, 0, 0, 1),
];

/// 原因碼。`(kind, code, name, requires_note)`。
const REASON_CODES: &[(&str, &str, &str, i64)] = &[
    ("void", "customer_changed", "客人改變主意", 0),
    ("void", "wrong_entry", "點錯", 0),
    ("void", "out_of_stock", "食材售完", 0),
    ("void", "quality", "品質問題", 1),
    ("void", "other", "其他", 1),
    ("discount", "staff", "員工價", 0),
    ("discount", "promo", "活動優惠", 0),
    ("discount", "other", "其他", 1),
    ("comp", "service_recovery", "服務補償", 1),
    ("comp", "vip", "招待貴賓", 1),
    ("refund", "quality", "品質問題", 1),
    ("refund", "wrong_charge", "收錯金額", 1),
    ("refund", "other", "其他", 1),
    ("cash_in", "float_top_up", "補零錢", 0),
    ("cash_out", "purchase", "採買", 1),
    ("cash_out", "utility", "水電瓦斯", 1),
    ("cash_out", "drop", "投保險箱", 0),
];

/// 每次啟動都跑。回傳是否建立了新店家（給首次啟動精靈用）。
pub async fn apply(uow: &mut SqliteUow, now: &Stamp) -> AppResult<bool> {
    sync_reference_data(uow, now).await?;
    let created = ensure_store(uow, now).await?;
    Ok(created)
}

/// 參照資料：每次啟動 upsert，讓升級後新增的權限碼自動出現。
async fn sync_reference_data(uow: &mut SqliteUow, now: &Stamp) -> AppResult<()> {
    for (code, group_code, group_name, name) in PERMISSIONS {
        sqlx::query(
            "INSERT INTO permissions (id, code, name, group_code, group_name, sort_order, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)
             ON CONFLICT(code) DO UPDATE SET
               name = excluded.name, group_code = excluded.group_code, group_name = excluded.group_name",
        )
        .bind(Id::new().as_str())
        .bind(code)
        .bind(name)
        .bind(group_code)
        .bind(group_name)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    for (name, display, sort) in ROLES {
        sqlx::query(
            "INSERT INTO roles (id, name, display_name, is_system, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?5)
             ON CONFLICT(name) DO UPDATE SET display_name = excluded.display_name",
        )
        .bind(Id::new().as_str())
        .bind(name)
        .bind(display)
        .bind(sort)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    for (code, name, rate_bp, mig, is_default) in TAX_RATES {
        sqlx::query(
            "INSERT INTO tax_rates (code, name, rate_bp, mig_tax_type, is_default, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)
             ON CONFLICT(code) DO UPDATE SET
               name = excluded.name, rate_bp = excluded.rate_bp, mig_tax_type = excluded.mig_tax_type",
        )
        .bind(code)
        .bind(name)
        .bind(rate_bp)
        .bind(mig)
        .bind(is_default)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    grant(uow, "owner", None, now).await?;
    grant(uow, "manager", None, now).await?;
    grant(uow, "cashier", Some(CASHIER_PERMS), now).await?;
    grant(uow, "kitchen", Some(KITCHEN_PERMS), now).await?;

    let mut lead: Vec<&str> = CASHIER_PERMS.to_vec();
    lead.extend_from_slice(SHIFT_LEAD_EXTRA);
    grant(uow, "shift_lead", Some(&lead), now).await?;
    Ok(())
}

/// 授權。`codes = None` 表示全部權限。
///
/// 刻意只做「加上」不做「移除」：店長若手動收回某個角色的某項權限，
/// 升級不該把它加回去。要重設有另外的「還原預設」動作。
async fn grant(
    uow: &mut SqliteUow,
    role: &str,
    codes: Option<&[&str]>,
    now: &Stamp,
) -> AppResult<()> {
    let sql = match codes {
        Some(_) => {
            "INSERT INTO role_permissions (role_id, permission_id, created_at)
             SELECT r.id, p.id, ?2 FROM roles r, permissions p
              WHERE r.name = ?1 AND p.code = ?3
             ON CONFLICT(role_id, permission_id) DO NOTHING"
        }
        None => {
            "INSERT INTO role_permissions (role_id, permission_id, created_at)
             SELECT r.id, p.id, ?2 FROM roles r, permissions p
              WHERE r.name = ?1
             ON CONFLICT(role_id, permission_id) DO NOTHING"
        }
    };

    match codes {
        None => {
            sqlx::query(sql)
                .bind(role)
                .bind(now.iso())
                .execute(uow.conn())
                .await?;
        }
        Some(list) => {
            for c in list {
                sqlx::query(sql)
                    .bind(role)
                    .bind(now.iso())
                    .bind(c)
                    .execute(uow.conn())
                    .await?;
            }
        }
    }
    Ok(())
}

/// 店家資料：只在完全空的資料庫上建立一次。
async fn ensure_store(uow: &mut SqliteUow, now: &Stamp) -> AppResult<bool> {
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stores")
        .fetch_one(uow.conn())
        .await?;
    if existing > 0 {
        return Ok(false);
    }

    let store_id = Id::new();
    sqlx::query(
        "INSERT INTO stores (id, code, name, tz, business_day_cutoff, currency,
                             tax_rate_bp, service_charge_rate_bp, rounding_policy, created_at, updated_at)
         VALUES (?1, 'main', '我的店', 'Asia/Taipei', '05:00', 'TWD', 500, 0, 'none', ?2, ?2)",
    )
    .bind(store_id.as_str())
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    sqlx::query(
        "INSERT INTO terminals (id, store_id, code, name, kind, is_active, created_at, updated_at)
         VALUES (?1, ?2, 'pos-1', '主收銀機', 'pos', 1, ?3, ?3)",
    )
    .bind(Id::new().as_str())
    .bind(store_id.as_str())
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    for (i, (code, name, kind, drawer, change, cash, needs_ref)) in
        PAYMENT_METHODS.iter().enumerate()
    {
        sqlx::query(
            "INSERT INTO payment_methods
               (id, store_id, code, name, kind, opens_drawer, allows_change, allows_tip,
                needs_ref, counts_as_cash, sort_order, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, 1, ?11, ?11)",
        )
        .bind(Id::new().as_str())
        .bind(store_id.as_str())
        .bind(code)
        .bind(name)
        .bind(kind)
        .bind(drawer)
        .bind(change)
        .bind(needs_ref)
        .bind(cash)
        .bind(i as i64)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    for (i, (kind, code, name, requires_note)) in REASON_CODES.iter().enumerate() {
        sqlx::query(
            "INSERT INTO reason_codes
               (id, store_id, kind, code, name, requires_note, is_active, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8)",
        )
        .bind(Id::new().as_str())
        .bind(store_id.as_str())
        .bind(kind)
        .bind(code)
        .bind(name)
        .bind(requires_note)
        .bind(i as i64)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    for dow in 0..7i64 {
        sqlx::query(
            "INSERT INTO business_hours (id, store_id, day_of_week, start_time, end_time, is_closed, created_at, updated_at)
             VALUES (?1, ?2, ?3, '11:00', '21:00', 0, ?4, ?4)",
        )
        .bind(Id::new().as_str())
        .bind(store_id.as_str())
        .bind(dow)
        .bind(now.iso())
        .execute(uow.conn())
        .await?;
    }

    // 預設店長帳號。
    //
    // ⚠️ v1.0 還沒有登入畫面（排在 M4），所以桌面版目前以這個帳號執行所有操作。
    //    稽核紀錄會如實記在它頭上 —— 這比「actor 是 NULL」誠實得多，
    //    等登入接上之後，既有的稽核資料仍然解釋得通。
    //    pin_hash 留空表示「尚未設定密碼」，登入功能上線時會強制要求設定。
    let user_id = Id::new();
    sqlx::query(
        "INSERT INTO users (id, store_id, code, name, is_active, created_at, updated_at)
         VALUES (?1, ?2, 'admin', '店長', 1, ?3, ?3)",
    )
    .bind(user_id.as_str())
    .bind(store_id.as_str())
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id, created_at)
         SELECT ?1, r.id, ?2 FROM roles r WHERE r.name = 'owner'",
    )
    .bind(user_id.as_str())
    .bind(now.iso())
    .execute(uow.conn())
    .await?;

    tracing::info!(store_id = %store_id, "已建立預設店家資料");
    Ok(true)
}

/// 目前的操作者。
///
/// 登入畫面上線前的過渡做法：取第一個具有 owner 角色的在職使用者。
/// 這是**刻意的暫時方案**，不是設計 —— 它讓稽核從第一天就有真實的 actor 可記，
/// 而不是等登入做完才開始有資料（那些空白的日子事後補不回來）。
pub async fn default_actor(db: &crate::infra::db::sqlite::SqliteDb) -> AppResult<Actor> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT u.id, u.code, u.name
           FROM users u
           JOIN user_roles ur ON ur.user_id = u.id
           JOIN roles r ON r.id = ur.role_id
          WHERE r.name = 'owner' AND u.is_active = 1 AND u.deleted_at IS NULL
          ORDER BY u.id
          LIMIT 1",
    )
    .fetch_optional(db.reader())
    .await?;

    row.map(|(user_id, code, name)| Actor {
        user_id,
        code,
        name,
    })
    .ok_or_else(|| {
        crate::error::AppError::Internal("找不到預設的店長帳號 —— 種子資料可能沒跑完".into())
    })
}
