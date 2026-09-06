//! Tauri command 薄殼。
//!
//! 每一個都只有一行：轉呼 `services::*`。
//! 業務邏輯一律不寫在這裡 —— 因為區網那邊的 `lan::router` 呼叫的是同一組
//! service 函式，任何寫進 command 裡的東西，KDS 與掃碼點餐就看不到。

use tauri::State;

use crate::ctx::Ctx;
use crate::error::AppResult;
use crate::services;

#[tauri::command]
pub async fn app_info(ctx: State<'_, Ctx>) -> AppResult<services::app::AppInfo> {
    services::app::app_info(&ctx).await
}

#[tauri::command]
pub async fn health(ctx: State<'_, Ctx>) -> AppResult<services::app::Health> {
    services::app::health(&ctx).await
}

/// 區網連線資訊。收銀機要把它顯示出來（並印進桌卡 QR），
/// 店員才知道要在平板上輸入什麼。
#[tauri::command]
pub fn lan_info(port: u16) -> Option<String> {
    crate::lan::lan_base_url(port)
}

/// 用系統預設瀏覽器開啟外部連結。
///
/// **刻意只存在於 Tauri 這一側，不掛進 `lan::router`。**
/// 區網那邊的呼叫者是顧客的手機與廚房平板；讓他們能叫主機開瀏覽器，
/// 等於把「打開任意網址」這個能力送給店裡的每一個人。
///
/// 只放行 http / https：`file://` 會變成任意檔案讀取，
/// 而 Windows 上某些 scheme 可以直接帶起執行檔。
#[tauri::command]
pub async fn open_external(url: String) -> AppResult<()> {
    let u = url.trim();
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return Err(crate::error::AppError::Validation(
            "只能開啟 http / https 連結".into(),
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：不然每點一次連結就閃一個黑色主控台視窗。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", u]);
        c.creation_flags(CREATE_NO_WINDOW);
        let _ = c.spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(u).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(u).spawn();
    }
    Ok(())
}

// ---------------------------------------------------------------- 商品維護

#[tauri::command]
pub async fn menu_tree(ctx: State<'_, Ctx>) -> AppResult<services::menu::MenuTree> {
    services::menu::menu_tree(&ctx).await
}

#[tauri::command]
pub async fn upsert_category(
    ctx: State<'_, Ctx>,
    input: services::menu::CategoryInput,
) -> AppResult<services::menu::Category> {
    services::menu::upsert_category(&ctx, input).await
}

#[tauri::command]
pub async fn delete_category(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::menu::delete_category(&ctx, id).await
}

#[tauri::command]
pub async fn upsert_item(
    ctx: State<'_, Ctx>,
    input: services::menu::ItemInput,
) -> AppResult<services::menu::Item> {
    services::menu::upsert_item(&ctx, input).await
}

#[tauri::command]
pub async fn delete_item(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::menu::delete_item(&ctx, id).await
}

#[tauri::command]
pub async fn upsert_variant(
    ctx: State<'_, Ctx>,
    input: services::menu::VariantInput,
) -> AppResult<services::menu::Variant> {
    services::menu::upsert_variant(&ctx, input).await
}

#[tauri::command]
pub async fn delete_variant(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::menu::delete_variant(&ctx, id).await
}

// ---------------------------------------------------------------- 點餐與結帳

#[tauri::command]
pub async fn open_order(
    ctx: State<'_, Ctx>,
    req: services::order::OpenOrderReq,
) -> AppResult<services::order::OrderView> {
    services::order::open_order(&ctx, req).await
}

#[tauri::command]
pub async fn add_lines(
    ctx: State<'_, Ctx>,
    req: services::order::AddLinesReq,
) -> AppResult<services::order::OrderView> {
    services::order::add_lines(&ctx, req).await
}

#[tauri::command]
pub async fn void_line(
    ctx: State<'_, Ctx>,
    order_id: String,
    expected_rev: i64,
    line_id: String,
    reason_id: Option<String>,
) -> AppResult<services::order::OrderView> {
    services::order::void_line(&ctx, order_id, expected_rev, line_id, reason_id).await
}

#[tauri::command]
pub async fn settle(
    ctx: State<'_, Ctx>,
    req: services::order::SettleReq,
) -> AppResult<services::order::SettleResult> {
    services::order::settle(&ctx, req).await
}

#[tauri::command]
pub async fn get_order(ctx: State<'_, Ctx>, id: String) -> AppResult<services::order::OrderView> {
    services::order::get_order(&ctx, &id).await
}

#[tauri::command]
pub async fn list_open_orders(ctx: State<'_, Ctx>) -> AppResult<Vec<services::order::OrderView>> {
    services::order::list_open_orders(&ctx).await
}

/// 付款方式清單。結帳畫面要用它排按鈕，而不是把方式寫死在前端 ——
/// 店家停用「悠遊卡」之後，按鈕就該跟著消失。
#[tauri::command]
pub async fn payment_methods(
    ctx: State<'_, Ctx>,
) -> AppResult<Vec<services::app::PaymentMethodView>> {
    services::app::payment_methods(&ctx).await
}

// ---------------------------------------------------------------- 出單機
//
// ★ 這一組**刻意不掛在 `lan::router` 上**。
//   區網那邊的呼叫者是顧客手機與廚房平板；就算區網服務有漏洞，
//   攻擊面也只到「亂送單」，到不了「改設定」或「看失敗的單」。

#[tauri::command]
pub async fn list_printers(ctx: State<'_, Ctx>) -> AppResult<Vec<services::printer::PrinterView>> {
    services::printer::list_printers(&ctx).await
}

#[tauri::command]
pub async fn upsert_printer(
    ctx: State<'_, Ctx>,
    input: services::printer::PrinterInput,
) -> AppResult<services::printer::PrinterView> {
    services::printer::upsert_printer(&ctx, input).await
}

#[tauri::command]
pub async fn delete_printer(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::printer::delete_printer(&ctx, id).await
}

#[tauri::command]
pub async fn probe_printer(
    ctx: State<'_, Ctx>,
    id: String,
) -> AppResult<services::printer::ProbeResult> {
    services::printer::probe_printer(&ctx, id).await
}

#[tauri::command]
pub async fn test_print(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::printer::test_print(&ctx, id).await
}

#[tauri::command]
pub async fn list_stations(ctx: State<'_, Ctx>) -> AppResult<Vec<services::printer::StationView>> {
    services::printer::list_stations(&ctx).await
}

#[tauri::command]
pub async fn upsert_station(
    ctx: State<'_, Ctx>,
    input: services::printer::StationInput,
) -> AppResult<services::printer::StationView> {
    services::printer::upsert_station(&ctx, input).await
}

#[tauri::command]
pub async fn delete_station(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::printer::delete_station(&ctx, id).await
}

#[tauri::command]
pub async fn print_queue_status(
    ctx: State<'_, Ctx>,
) -> AppResult<services::printer::PrintQueueStatus> {
    services::printer::queue_status(&ctx).await
}

#[tauri::command]
pub async fn list_print_jobs(
    ctx: State<'_, Ctx>,
    limit: Option<i64>,
) -> AppResult<Vec<services::printer::PrintJobView>> {
    services::printer::list_print_jobs(&ctx, limit).await
}

#[tauri::command]
pub async fn retry_print_job(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::printer::retry_print_job(&ctx, id).await
}

#[tauri::command]
pub async fn cancel_print_job(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::printer::cancel_print_job(&ctx, id).await
}
