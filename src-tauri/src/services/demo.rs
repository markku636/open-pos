//! 一鍵示範資料。
//!
//! 三個用途，每一個都真的會用到：
//!
//! 1. **貢獻者第一次 clone 下來**就有東西可以按。空白的 POS 什麼都試不了，
//!    而「先花二十分鐘建一份菜單才能開始開發」會勸退大部分人。
//! 2. **回報 bug 時的共同基準** —— 「用 --demo 的菜單、點兩杯珍奶」是一句
//!    所有人都重現得出來的話。
//! 3. **店家評估時的試玩** —— 老闆想先看看長什麼樣子，不是先建菜單。
//!
//! 刻意做成**冪等**：已經有商品就整個跳過。不然每次重開 daemon 都多一份
//! 珍珠奶茶，而那正是「示範資料汙染了真實資料」的開始。
//!
//! ★ 它不掛在 `lan::router` 上。寫入型的管理指令一律只走 Tauri IPC 或 CLI，
//!   任何連上店內 Wi-Fi 的人都不該能塞一份菜單進來。

use crate::ctx::Ctx;
use crate::error::AppResult;
use crate::services::menu::{self, CategoryInput, ItemInput};

/// 一份台式早午餐店的菜單。
///
/// 價格刻意選成會考驗定價引擎的數字：45 / 55 / 65 這種含稅價在
/// 「先算稅額再倒扣銷售額」的算法下會產生尾差，正好每天都在驗它。
struct DemoCategory {
    name: &'static str,
    color: &'static str,
    /// (品名, 含稅售價)
    items: &'static [(&'static str, i64)],
}

/// 品項數量刻意接近真實規模（一個分類十幾樣）。
///
/// 四個品項的假菜單看起來很乾淨，但它會讓兩件事測不出來：畫面在**塞滿**時
/// 好不好按（那才是尖峰時間的樣子），以及分類切換有沒有意義。
const MENU: &[DemoCategory] = &[
    DemoCategory {
        name: "飲料",
        color: "#0ea5e9",
        items: &[
            ("珍珠奶茶", 60),
            ("紅茶拿鐵", 55),
            ("四季青茶", 35),
            ("冬瓜檸檬", 45),
            ("古早味紅茶", 30),
            ("鮮榨柳橙汁", 70),
            ("豆漿", 25),
            ("米漿", 25),
            ("美式咖啡", 50),
            ("拿鐵咖啡", 65),
            ("卡布奇諾", 65),
            ("熱可可", 55),
        ],
    },
    DemoCategory {
        name: "主餐",
        color: "#f97316",
        items: &[
            ("滷肉飯", 45),
            ("雞腿便當", 110),
            ("排骨便當", 100),
            ("控肉飯", 85),
            ("乾麵", 45),
            ("餛飩湯麵", 70),
            ("牛肉麵", 140),
            ("蛋餅", 40),
            ("蘿蔔糕", 40),
            ("鮪魚三明治", 55),
            ("總匯漢堡", 75),
            ("鐵板麵", 60),
        ],
    },
    DemoCategory {
        name: "小菜",
        color: "#22c55e",
        items: &[
            ("燙青菜", 30),
            ("滷蛋", 15),
            ("豆干海帶", 35),
            ("嘴邊肉", 60),
            ("味噌湯", 25),
            ("貢丸湯", 35),
            ("酸辣湯", 40),
            ("涼拌小黃瓜", 30),
        ],
    },
    DemoCategory {
        name: "加購",
        color: "#a855f7",
        items: &[
            ("加飯", 10),
            ("加麵", 10),
            ("加蛋", 15),
            ("加辣", 0),
            ("加珍珠", 10),
            ("免洗餐具", 5),
        ],
    },
];

/// 示範菜單有多大：（分類數, 品項數）。
///
/// 給啟動訊息用。**不要在別處手寫這兩個數字** —— 訊息與實際內容一旦對不上，
/// 使用者就會開始懷疑其他訊息是不是也在唬爛。
pub fn demo_menu_size() -> (usize, usize) {
    (MENU.len(), MENU.iter().map(|c| c.items.len()).sum())
}

/// 建一份示範菜單。已經有商品就不動任何東西，回傳 false。
pub async fn seed_demo_menu(ctx: &Ctx) -> AppResult<bool> {
    let existing = menu::menu_tree(ctx).await?;
    let has_items = !existing.uncategorized.is_empty()
        || existing.categories.iter().any(|c| !c.items.is_empty());
    if has_items {
        return Ok(false);
    }

    for (order, group) in MENU.iter().enumerate() {
        let cat = menu::upsert_category(
            ctx,
            CategoryInput {
                id: None,
                name: group.name.to_string(),
                color: Some(group.color.to_string()),
                sort_order: Some(order as i64 * 10),
                is_active: Some(true),
            },
        )
        .await?;

        for (i, (name, price)) in group.items.iter().enumerate() {
            menu::upsert_item(
                ctx,
                ItemInput {
                    id: None,
                    category_id: Some(cat.id.clone()),
                    name: (*name).to_string(),
                    short_name: None,
                    base_price: *price,
                    tax_code: None,
                    is_open_price: None,
                    sold_out_until: None,
                    sort_order: Some(i as i64 * 10),
                    is_active: Some(true),
                },
            )
            .await?;
        }
    }
    Ok(true)
}
