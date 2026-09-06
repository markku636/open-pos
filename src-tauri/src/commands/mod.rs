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
