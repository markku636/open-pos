//! 路由與 RPC 分派。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;

use crate::ctx::Ctx;
use crate::error::{AppError, AppResult};
use crate::services;

/// 這個端點需要什麼身分。
///
/// v1.0 只有 `Public` 與 `Staff` 兩級真的用得到；`Kds` 與 `Customer` 先立好，
/// 因為 v1.1 / v1.3 一定會用到，而**事後才補授權比一開始就有難得多** ——
/// 補的時候要回頭檢視每一個既有端點，很容易漏。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    /// 不需要身分（健康檢查、版本）。
    Public,
    /// 已配對的廚房裝置。
    Kds,
    /// 顧客手機（只能對自己那桌操作）。
    Customer,
}

pub fn build(ctx: Ctx) -> Router {
    build_with_ui(ctx, None)
}

/// 加上靜態頁面服務。
///
/// 平板開 `/kds.html`、顧客手機開 `/order.html` —— 兩個頁面都是從**同一個**
/// server 出去的，所以是同源，CORS 因此可以完全關閉（不設任何
/// Access-Control-Allow-Origin）。那是一層免費的防護，不要為了開發方便而放寬。
///
/// 收銀機那一頁（index.html）由 Tauri 視窗自己載入，不經過這裡。
pub fn build_with_ui(ctx: Ctx, ui_dir: Option<std::path::PathBuf>) -> Router {
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/rpc/{name}", post(rpc))
        .with_state(ctx);

    match ui_dir {
        Some(dir) if dir.is_dir() => {
            tracing::info!(dir = %dir.display(), "提供靜態頁面（僅 KDS 與掃碼點餐）");
            let files = tower_http::services::ServeDir::new(dir);
            api.fallback_service(tower::service_fn(move |req| {
                let files = files.clone();
                async move { serve_public_page(files, req).await }
            }))
        }
        Some(dir) => {
            // 不要安靜地略過 —— 使用者指定了目錄卻沒生效，最後只會看到 404 而不知原因。
            tracing::warn!(dir = %dir.display(), "指定的頁面目錄不存在，只提供 API");
            api
        }
        None => api,
    }
}

/// 這個路徑可以從區網拿到嗎。
///
/// ★ **收銀機那一頁（index.html）刻意不在名單裡。**
///
/// 它是給 Tauri 視窗載入的，走 IPC；從瀏覽器開它，每個寫入動作都會撞上
/// 「這個指令不在區網端點上」而變成一條死路。更重要的是，任何連上店內
/// Wi-Fi 的人都不該能載入收銀介面 —— 那等於把整個後台的畫面與 API 形狀
/// 攤開給客人看。
///
/// 連它的 JS chunk（`assets/index-*.js`）也一起擋掉，否則猜檔名還是拿得到。
fn is_public_path(path: &str) -> bool {
    let p = path.trim_start_matches('/');
    match p {
        "kds.html" | "order.html" => true,
        // 品牌圖示。三個 entry 的 <head> 都指到它們，擋掉只會換來一堆 404，
        // 而它們裡面沒有任何一個位元組是機密。
        "favicon.ico" | "app-icon.png" => true,
        _ if p.starts_with("assets/") => {
            // Vite 的 chunk 檔名是 `<entry>-<hash>.js`，收銀機那一支叫 index-*。
            let file = p.trim_start_matches("assets/");
            !file.starts_with("index-")
        }
        // 根路徑導到掃碼點餐頁：客人掃 QR 掃到的就是它。
        "" => false,
        _ => false,
    }
}

#[cfg(feature = "server")]
async fn serve_public_page(
    files: tower_http::services::ServeDir,
    req: axum::http::Request<axum::body::Body>,
) -> Result<axum::response::Response, std::convert::Infallible> {
    use tower::ServiceExt;

    if !is_public_path(req.uri().path()) {
        return Ok((
            StatusCode::NOT_FOUND,
            "這個頁面只能在收銀機上開啟。
平板請開 /kds.html，顧客手機請掃桌上的 QR。",
        )
            .into_response());
    }
    match files.oneshot(req).await {
        Ok(res) => Ok(res.map(axum::body::Body::new)),
        Err(_) => Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

async fn health(State(ctx): State<Ctx>) -> impl IntoResponse {
    match services::app::health(&ctx).await {
        Ok(h) => {
            // 不健康時回 503：讓監看工具與 kiosk 瀏覽器的重載邏輯看得懂，
            // 而不是只有人類讀得懂的 JSON。
            let code = if h.ok {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            };
            (code, Json(serde_json::to_value(h).unwrap_or(Value::Null))).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// 單一 RPC 端點。
///
/// 用一張明文的分派表而不是巨集，是刻意的：巨集會讓「這個端點需要什麼權限」
/// 這件事多一層間接，而它正是最需要一眼看完的東西。60 行樣板換一眼可讀，划算。
async fn rpc(
    State(ctx): State<Ctx>,
    Path(name): Path<String>,
    body: Option<Json<Value>>,
) -> axum::response::Response {
    let args = body.map(|Json(v)| v).unwrap_or(Value::Null);
    match dispatch(&ctx, &name, args).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => e.into_response(),
    }
}

// `_args`：v1.0 的三個端點都不吃參數。留著簽章是因為下一個端點（KDS 取單）就會用到，
// 到時候不必動所有呼叫端。
async fn dispatch(ctx: &Ctx, name: &str, _args: Value) -> AppResult<Value> {
    let (access, value) = match name {
        "app_info" => (
            Access::Public,
            to_value(services::app::app_info(ctx).await?)?,
        ),
        "health" => (Access::Public, to_value(services::app::health(ctx).await?)?),
        // 唯讀的菜單樹。KDS 與掃碼點餐都要它 —— 客人手機上要看得到品名與價格。
        // 商品的**維護**（新增 / 改價 / 刪除）刻意不在這裡，只走 Tauri IPC。
        "menu_tree" => (
            Access::Public,
            to_value(services::menu::menu_tree(ctx).await?)?,
        ),
        other => {
            return Err(AppError::NotFound(format!(
                "未知的指令「{other}」。管理類指令刻意不在區網端點上提供 —— \
                 改菜單、看報表、設定印表機只能在收銀機上操作。"
            )))
        }
    };

    // v1.0 的端點都是 Public。裝置配對與桌位 token 於 v1.1 / v1.3 接上，
    // 屆時這裡會變成真正的檢查而不是 debug_assert。
    debug_assert_eq!(
        access,
        Access::Public,
        "非 Public 的端點必須先接上身分驗證才能開放"
    );
    Ok(value)
}

fn to_value<T: serde::Serialize>(v: T) -> AppResult<Value> {
    serde_json::to_value(v).map_err(|e| AppError::Internal(format!("序列化失敗：{e}")))
}
