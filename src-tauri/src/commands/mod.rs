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
///
/// 沒有 `server` feature 就沒有區網可言，所以回 None 而不是編譯失敗 ——
/// feature 表上寫著 gui 與 server 各自獨立，那就要真的獨立。
#[tauri::command]
pub fn lan_info(port: u16) -> Option<String> {
    #[cfg(feature = "server")]
    {
        crate::lan::lan_base_url(port)
    }
    #[cfg(not(feature = "server"))]
    {
        let _ = port;
        None
    }
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

// ---------------------------------------------------------------- 班別與日結
//
// 同樣不掛在區網上：關班、日結、X 報表都是會看到營業額的操作。

#[tauri::command]
pub async fn day_status(ctx: State<'_, Ctx>) -> AppResult<services::shift::DayStatus> {
    services::shift::day_status(&ctx).await
}

#[tauri::command]
pub async fn open_shift(
    ctx: State<'_, Ctx>,
    req: services::shift::OpenShiftReq,
) -> AppResult<services::shift::ShiftView> {
    services::shift::open_shift(&ctx, req).await
}

#[tauri::command]
pub async fn close_shift(
    ctx: State<'_, Ctx>,
    req: services::shift::CloseShiftReq,
) -> AppResult<services::shift::ShiftReport> {
    services::shift::close_shift(&ctx, req).await
}

#[tauri::command]
pub async fn record_cash_movement(
    ctx: State<'_, Ctx>,
    req: services::shift::CashMovementReq,
) -> AppResult<()> {
    services::shift::record_cash_movement(&ctx, req).await
}

/// X 報表：不關班，中途看。需要 report.daily —— 收銀員預設拿不到，
/// 那正是盲盤的前提。
#[tauri::command]
pub async fn x_report(ctx: State<'_, Ctx>) -> AppResult<services::shift::ShiftReport> {
    services::shift::x_report(&ctx).await
}

#[tauri::command]
pub async fn close_business_day(ctx: State<'_, Ctx>) -> AppResult<services::shift::DayReport> {
    services::shift::close_business_day(&ctx).await
}

// ---------------------------------------------------------------- 備份與還原

#[tauri::command]
pub async fn get_settings(ctx: State<'_, Ctx>) -> AppResult<crate::infra::settings::AppSettings> {
    services::backup::get_settings(&ctx).await
}

#[tauri::command]
pub async fn save_settings(
    ctx: State<'_, Ctx>,
    settings: crate::infra::settings::AppSettings,
) -> AppResult<crate::infra::settings::AppSettings> {
    services::backup::save_settings(&ctx, settings).await
}

#[tauri::command]
pub async fn run_backup(ctx: State<'_, Ctx>) -> AppResult<services::backup::BackupRunResult> {
    // 手動備份歸在「每小時」那一組：它跟自動備份是同一種東西，
    // 分開放只會讓輪替規則變成兩套。
    services::backup::run_backup(&ctx, crate::infra::backup::BackupBucket::Hourly).await
}

#[tauri::command]
pub async fn list_backups(ctx: State<'_, Ctx>) -> AppResult<Vec<services::backup::BackupFile>> {
    services::backup::list_backups(&ctx).await
}

/// 準備還原。真正的替換發生在下一次啟動 —— 資料庫在程式跑的時候是開著的。
#[tauri::command]
pub async fn stage_restore(ctx: State<'_, Ctx>, path: String) -> AppResult<String> {
    services::backup::stage_restore(&ctx, path).await
}

#[tauri::command]
pub async fn cancel_restore(ctx: State<'_, Ctx>) -> AppResult<()> {
    services::backup::cancel_restore(&ctx).await
}

#[tauri::command]
pub async fn pending_restore(
    ctx: State<'_, Ctx>,
) -> AppResult<Option<services::backup::PendingRestore>> {
    services::backup::pending_restore(&ctx).await
}

// ---------------------------------------------------------------- 診斷

/// 產生診斷資訊（純文字）。
///
/// 「今天中午印不出來，現在又好了」這種回報，維護者沒有任何辦法重現 ——
/// 一人維護的專案通常不是死在寫程式，是死在無法診斷的回報上。
#[tauri::command]
pub async fn diagnostics_report(ctx: State<'_, Ctx>) -> AppResult<String> {
    services::diagnostics::report(&ctx).await
}

#[tauri::command]
pub async fn export_diagnostics(ctx: State<'_, Ctx>, dir: String) -> AppResult<String> {
    services::diagnostics::export(&ctx, dir).await
}

/// 把某一天的日結匯出成 CSV。讀的是日結當下的快照，不是重算 ——
/// 三個月後叫出來的數字必須跟當時印的那張紙一模一樣。
#[tauri::command]
pub async fn export_day_csv(
    ctx: State<'_, Ctx>,
    business_date: String,
    dir: String,
) -> AppResult<String> {
    let report = services::shift::stored_day_report(&ctx, &business_date).await?;
    services::report_export::export_day_csv(&ctx, &report, dir).await
}

/// 打折。招待與折扣是兩個權限 —— 老闆看折扣是看行銷成效，
/// 看招待是看有沒有人在送人情。
#[tauri::command]
pub async fn apply_discount(
    ctx: State<'_, Ctx>,
    req: services::order::DiscountReq,
) -> AppResult<services::order::OrderView> {
    services::order::apply_discount(&ctx, req).await
}

/// 作廢整張單。結帳後作廢走獨立權限並留簽核紀錄。
#[tauri::command]
pub async fn void_order(
    ctx: State<'_, Ctx>,
    req: services::order::VoidOrderReq,
) -> AppResult<services::order::OrderView> {
    services::order::void_order(&ctx, req).await
}

// ---------------------------------------------------------------- 桌位

#[tauri::command]
pub async fn list_tables(ctx: State<'_, Ctx>) -> AppResult<Vec<services::table::TableView>> {
    services::table::list_tables(&ctx).await
}

#[tauri::command]
pub async fn upsert_table(
    ctx: State<'_, Ctx>,
    input: services::table::TableInput,
) -> AppResult<services::table::TableView> {
    services::table::upsert_table(&ctx, input).await
}

#[tauri::command]
pub async fn delete_table(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::table::delete_table(&ctx, id).await
}

/// 清桌。只有在沒有未結帳的單時才允許 —— 否則它會變成一個把帳丟掉的按鈕。
#[tauri::command]
pub async fn close_table(ctx: State<'_, Ctx>, table_id: String) -> AppResult<()> {
    services::table::close_table(&ctx, table_id).await
}

// ---------------------------------------------------------------- 示範資料

/// 一鍵示範資料：一份菜單加幾張桌子。
///
/// 這顆按鈕的存在理由是**第一印象**：裝起來看到一片空白的人，多半不會有耐心
/// 先去建十個品項才知道這套東西長什麼樣。已經有商品時它什麼都不做。
#[tauri::command]
pub async fn seed_demo(ctx: State<'_, Ctx>) -> AppResult<services::demo::DemoResult> {
    services::demo::seed_demo(&ctx).await
}

/// 分帳試算。金額由 Rust 算一次，前端只負責顯示。
#[tauri::command]
pub async fn preview_split(
    ctx: State<'_, Ctx>,
    order_id: String,
    split: Option<services::order::SplitReq>,
) -> AppResult<services::order::SplitPreview> {
    services::order::preview_split(&ctx, order_id, split).await
}

// ---------------------------------------------------------------- 退款

/// 找帳單（退款前要先找到原單）。
#[tauri::command]
pub async fn find_bills(
    ctx: State<'_, Ctx>,
    req: services::refund::FindBillsReq,
) -> AppResult<Vec<services::refund::BillView>> {
    services::refund::find_bills(&ctx, req).await
}

/// 退款。需要 `payment.refund` 權限，一定要選原因並留下簽核紀錄。
#[tauri::command]
pub async fn refund(
    ctx: State<'_, Ctx>,
    req: services::refund::RefundReq,
) -> AppResult<services::refund::RefundResult> {
    services::refund::refund(&ctx, req).await
}

/// 原因代碼。作廢／折扣／退款的下拉選單靠它。
#[tauri::command]
pub async fn list_reasons(
    ctx: State<'_, Ctx>,
    kind: String,
) -> AppResult<Vec<services::reason::ReasonCode>> {
    services::reason::list_reasons(&ctx, kind).await
}

/// 補印收據。重送的是當初那一張的快照，並印上「※ 補印 第 N 次 ※」。
#[tauri::command]
pub async fn reprint_receipt(ctx: State<'_, Ctx>, bill_id: String) -> AppResult<()> {
    services::printer::reprint_receipt(&ctx, bill_id).await
}

/// 稽核查詢。需要 `report.audit`。
#[tauri::command]
pub async fn audit_query(
    ctx: State<'_, Ctx>,
    query: services::audit::AuditQuery,
) -> AppResult<services::audit::AuditReport> {
    services::audit::query(&ctx, query).await
}

/// 一段期間的營運分析（時段分布、折扣統計、品項排行）。需要 `report.daily`。
#[tauri::command]
pub async fn insight(
    ctx: State<'_, Ctx>,
    query: services::analytics::InsightQuery,
) -> AppResult<services::analytics::Insight> {
    services::analytics::insight(&ctx, query).await
}

/// 區網連線狀態：平板要開的網址、候選網卡、IP 有沒有換過。
///
/// 裝機第一天的頭號故障就是「平板連不上收銀機」，而三個成因（防火牆按了取消、
/// 路由器重開換了 IP、多網卡挑錯）都不會出現在任何錯誤訊息裡。
#[cfg(feature = "server")]
#[tauri::command]
pub async fn lan_status(ctx: State<'_, Ctx>) -> AppResult<services::network::LanStatus> {
    services::network::lan_status(&ctx, crate::lan::DEFAULT_PORT).await
}

// ---------------------------------------------------------------- 選項群組

/// 建立或更新一組選項（甜度 / 冰塊 / 加購）。
#[tauri::command]
pub async fn upsert_modifier_group(
    ctx: State<'_, Ctx>,
    input: services::menu::ModifierGroupInput,
) -> AppResult<services::menu::ModifierGroup> {
    services::menu::upsert_modifier_group(&ctx, input).await
}

#[tauri::command]
pub async fn delete_modifier_group(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::menu::delete_modifier_group(&ctx, id).await
}

#[tauri::command]
pub async fn upsert_modifier(
    ctx: State<'_, Ctx>,
    input: services::menu::ModifierInput,
) -> AppResult<services::menu::Modifier> {
    services::menu::upsert_modifier(&ctx, input).await
}

#[tauri::command]
pub async fn delete_modifier(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::menu::delete_modifier(&ctx, id).await
}

/// 這個品項要問哪幾組選項。
#[tauri::command]
pub async fn set_item_modifier_groups(
    ctx: State<'_, Ctx>,
    item_id: String,
    group_ids: Vec<String>,
) -> AppResult<()> {
    services::menu::set_item_modifier_groups(&ctx, item_id, group_ids).await
}

// ---------------------------------------------------------------- 銷售記錄與報表

/// 銷售記錄。日期區間 + 通路 + 單號片段，每一筆帶明細與付款。
#[tauri::command]
pub async fn sales_history(
    ctx: State<'_, Ctx>,
    query: services::sales::SalesQuery,
) -> AppResult<services::sales::SalesReport> {
    services::sales::history(&ctx, query).await
}

/// 過去某一天的 Z 報表（日結當下算好的那一份快照）。
#[tauri::command]
pub async fn day_report(
    ctx: State<'_, Ctx>,
    business_date: String,
) -> AppResult<services::shift::DayReport> {
    services::sales::day_report(&ctx, business_date).await
}

/// 有日結報表的營業日清單（新到舊）。
#[tauri::command]
pub async fn closed_days(ctx: State<'_, Ctx>) -> AppResult<Vec<String>> {
    services::sales::closed_days(&ctx).await
}

/// 把某一天的日報表匯出成 Excel。回傳寫出去的檔案路徑。
#[tauri::command]
pub async fn export_day_xlsx(
    ctx: State<'_, Ctx>,
    business_date: String,
    dir: String,
) -> AppResult<String> {
    let report = services::sales::day_report(&ctx, business_date).await?;
    let dir = services::sales::ensure_dir(&dir)?;
    services::xlsx::write_day_report(&report, &dir)
}

/// 把一段期間的銷售記錄匯出成 Excel（帳單 / 品項明細 / 付款方式三張表）。
#[tauri::command]
pub async fn export_sales_xlsx(
    ctx: State<'_, Ctx>,
    query: services::sales::SalesQuery,
    dir: String,
) -> AppResult<String> {
    let report = services::sales::history(&ctx, query).await?;
    let dir = services::sales::ensure_dir(&dir)?;
    services::xlsx::write_sales(&report, &dir)
}

/// 選一個資料夾（匯出用）。取消時回 None。
///
/// **只存在於 Tauri 這一側。** 讓區網上的裝置能叫主機跳出檔案對話框，
/// 等於把「在收銀機上開任意路徑」送給店裡的每一個人。
///
/// 用原生對話框而不是叫使用者打字：最常見的錯路徑是「已經拔掉的隨身碟」
/// 與「打錯一個字的桌面路徑」，而那兩個都是選單能直接消滅的問題。
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    // blocking_pick_folder 不能在主執行緒上呼叫，會鎖死事件迴圈。
    let picked =
        tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
            .await
            .map_err(|e| crate::error::AppError::Internal(format!("開不了資料夾選單：{e}")))?;
    Ok(picked.map(|p| p.to_string()))
}

// ---------------------------------------------------------------- 金流設定

/// 金流設定清單。**憑證只回「有沒有設定」與末四碼，不回明文。**
#[tauri::command]
pub async fn list_gateways(
    ctx: State<'_, Ctx>,
) -> AppResult<Vec<services::gateway::config::GatewayView>> {
    services::gateway::config::list(&ctx).await
}

#[tauri::command]
pub async fn upsert_gateway(
    ctx: State<'_, Ctx>,
    input: services::gateway::config::GatewayInput,
) -> AppResult<services::gateway::config::GatewayView> {
    services::gateway::config::upsert(&ctx, input).await
}

#[tauri::command]
pub async fn delete_gateway(ctx: State<'_, Ctx>, id: String) -> AppResult<()> {
    services::gateway::config::delete(&ctx, id).await
}

/// 有哪些金流商可以選、各自要哪些憑證欄位。
///
/// 不需要權限 —— 它回的是一張寫死在程式裡的表，沒有任何一家店的資料。
#[tauri::command]
pub fn gateway_providers() -> Vec<services::gateway::config::ProviderDef> {
    services::gateway::config::providers()
}
