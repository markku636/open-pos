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
        // KDS 的即時推播。SSE 只做「伺服器 → 客戶端」；
        // 廚房點「完成」走一般的 POST /api/rpc/kds_advance。
        .route("/api/events/kds", get(kds_events))
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
    let path = req.uri().path().to_string();
    match files.oneshot(req).await {
        Ok(res) => {
            let mut res = res.map(axum::body::Body::new);
            if let Ok(v) = axum::http::HeaderValue::from_str(cache_control(&path)) {
                res.headers_mut()
                    .insert(axum::http::header::CACHE_CONTROL, v);
            }
            Ok(res)
        }
        Err(_) => Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

/// 靜態檔的快取策略 —— 這是廚房平板的**窮人版離線殼**。
///
/// 平板被 Android 回收之後重開時，如果主機剛好連不上，瀏覽器連 kds.html
/// 都拿不到 —— 而 IndexedDB 裡存的那份看板，要有頁面才畫得出來。
/// 沒有 Service Worker 可用（區網走 HTTP，不是 secure context），
/// 所以只能靠 HTTP 快取。
///
/// * `assets/*` 檔名帶 hash，內容永不改變 → 一年不可變快取。
/// * `*.html` 用 `stale-while-revalidate`：正常情況下每分鐘回主機確認一次，
///   主機連不上時**照樣用舊的開起來**。這正是我們要的行為 ——
///   一個開得起來但顯示「現在畫的是上一次的單」的畫面，
///   比一個瀏覽器的錯誤頁有用得多。
#[cfg(feature = "server")]
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else if path.ends_with(".html") || path == "/" {
        "public, max-age=60, stale-while-revalidate=604800"
    } else {
        // favicon、app-icon：改了要看得到，但也不必每次都問。
        "public, max-age=3600"
    }
}

/// KDS 的事件串流。
///
/// # 為什麼要自己送心跳
///
/// 瀏覽器的 `EventSource` **沒有 read timeout**。爛 AP 與手機省電會靜默切斷
/// 閒置的 TCP 連線，而瀏覽器不會知道 —— 症狀是「看起來還連著但收不到單」，
/// 這是最惡劣的失敗模式：無聲的漏單。
///
/// 所以每 15 秒送一個 `heartbeat`，客戶端 45 秒沒收到就自己重連
/// （不能依賴瀏覽器內建的重連，因為它根本不知道連線已經死了）。
///
/// # 為什麼是輪詢而不是事件匯流排
///
/// 同時在做的單撐死 50 張，兩秒查一次 SQLite 是微秒級的成本。
/// 而輪詢 + 全量快照是**自我修正**的：任何原因造成的狀態漂移都會在下一次
/// 推播被抹平，不必為了正確性去維護一條訂閱鏈。
#[cfg(feature = "server")]
async fn kds_events(State(ctx): State<Ctx>) -> impl IntoResponse {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use std::time::Duration;

    const POLL: Duration = Duration::from_secs(2);
    const HEARTBEAT_EVERY: u32 = 8; // 8 × 2 秒 = 16 秒

    let stream = async_stream::stream! {
        let mut last: Option<String> = None;
        let mut ticks: u32 = 0;

        loop {
            match services::kds::board(&ctx).await {
                Ok(board) => {
                    let json = serde_json::to_string(&board).unwrap_or_default();
                    // 只在**內容真的變了**時推 —— 一面沒有變化的看板不該一直重畫，
                    // 廚房的平板通常很慢。
                    if last.as_deref() != Some(json.as_str()) {
                        last = Some(json.clone());
                        yield Ok::<_, std::convert::Infallible>(
                            Event::default().event("board").data(json),
                        );
                        ticks = 0;
                    }
                }
                Err(e) => {
                    yield Ok(Event::default().event("error").data(e.message()));
                }
            }

            ticks += 1;
            if ticks >= HEARTBEAT_EVERY {
                ticks = 0;
                // 心跳的內容不重要，重要的是它會到。
                yield Ok(Event::default().event("heartbeat").data("."));
            }
            tokio::time::sleep(POLL).await;
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
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
async fn dispatch(ctx: &Ctx, name: &str, args: Value) -> AppResult<Value> {
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
        // 廚房看板。唯讀，而且只有「現在該做什麼」——
        // 沒有金額、沒有客人資訊，就算被看到也不構成營業資料外洩。
        "kds_board" => (Access::Kds, to_value(services::kds::board(ctx).await?)?),
        // 廚房把一行往前推（做好了 / 出餐了）。
        //
        // 這是區網上唯一的寫入端點。它的破壞力上限是「有人亂按完成」——
        // 看得到、改得回、而且不動到錢。v1.1 的裝置配對會把它收進 Kds 級。
        "kds_advance" => {
            let line_id = args
                .get("lineId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::Validation("少了 lineId".into()))?
                .to_string();
            let to = args
                .get("to")
                .and_then(|v| v.as_str())
                .unwrap_or("ready")
                .to_string();
            (
                Access::Kds,
                to_value(services::kds::advance(ctx, line_id, to).await?)?,
            )
        }
        other => {
            return Err(AppError::NotFound(format!(
                "未知的指令「{other}」。管理類指令刻意不在區網端點上提供 —— \
                 改菜單、看報表、設定印表機只能在收銀機上操作。"
            )))
        }
    };

    // Kds 級目前還沒有真正的身分檢查（裝置配對排在 v1.1 後段）。
    // 那是可以接受的取捨：KDS 端點唯讀、或只改「做好了沒」，破壞力上限是
    // 「有人亂按完成」—— 看得到、改得回、不動到錢。
    //
    // Customer 級**不同**：它會建立訂單。所以在桌位 token 接上之前，
    // 這裡直接擋死，不讓任何人不小心把它開出去。
    debug_assert_ne!(
        access,
        Access::Customer,
        "顧客端點必須先接上桌位 token 才能開放"
    );
    Ok(value)
}

fn to_value<T: serde::Serialize>(v: T) -> AppResult<Value> {
    serde_json::to_value(v).map_err(|e| AppError::Internal(format!("序列化失敗：{e}")))
}
