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
/// 一個示範選項：`(名稱, 加價, 是否預設)`。
type DemoOption = (&'static str, i64, bool);

/// 一組示範選項：`(名稱, 是否複選, 最少選幾個, 選項)`。
type DemoGroup = (&'static str, bool, i64, &'static [DemoOption]);

/// 示範的選項群組。
///
/// 沒有這幾組，「珍奶半糖少冰」在系統裡就記不下來 —— 而那是台灣飲料店
/// 每一杯都會發生的事。
const MODIFIER_GROUPS: &[DemoGroup] = &[
    (
        "甜度",
        false,
        1,
        &[
            ("正常糖", 0, true),
            ("少糖", 0, false),
            ("半糖", 0, false),
            ("微糖", 0, false),
            ("無糖", 0, false),
        ],
    ),
    (
        "冰塊",
        false,
        1,
        &[
            ("正常冰", 0, true),
            ("少冰", 0, false),
            ("去冰", 0, false),
            ("熱飲", 0, false),
        ],
    ),
    (
        "加料",
        true,
        0,
        &[
            ("加珍珠", 10, false),
            ("加椰果", 10, false),
            ("加布丁", 15, false),
        ],
    ),
    (
        "飯麵加點",
        true,
        0,
        &[("加蛋", 10, false), ("加飯", 10, false), ("加辣", 0, false)],
    ),
];

/// 哪一個分類要問哪幾組。
const CATEGORY_GROUPS: &[(&str, &[&str])] =
    &[("飲料", &["甜度", "冰塊", "加料"]), ("主餐", &["飯麵加點"])];

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

/// 示範用的桌位。兩個區域是刻意的 —— 一個區域看不出分區長什麼樣子。
const DEMO_TABLES: [&str; 6] = ["A1", "A2", "A3", "A4", "B1", "B2"];

/// 示範菜單有多大：（分類數, 品項數）。
///
/// 給啟動訊息用。**不要在別處手寫這兩個數字** —— 訊息與實際內容一旦對不上，
/// 使用者就會開始懷疑其他訊息是不是也在唬爛。
pub fn demo_menu_size() -> (usize, usize) {
    // +1 是吃到飽方案的代表商品（seed_demo_plan 另外建的，不在 MENU 表裡）。
    // 這個數字會顯示在畫面上，說 38 卻建了 39 個會讓人以為有東西沒進去。
    (
        MENU.len(),
        MENU.iter().map(|c| c.items.len()).sum::<usize>() + 1,
    )
}

/// 種示範資料的結果。
///
/// 帶著數量回去是為了讓畫面能說「建了 6 個分類、31 個品項」而不是
/// 一句「完成」—— 使用者要能一眼確認**東西真的進去了**。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoResult {
    /// false = 已經有商品，什麼都沒動。
    pub created: bool,
    pub categories: usize,
    pub items: usize,
    pub tables: usize,
}

/// 建一份示範菜單（給 UI 用的包裝）。
pub async fn seed_demo(ctx: &Ctx) -> AppResult<DemoResult> {
    let created = seed_demo_menu(ctx).await?;
    let (categories, items) = demo_menu_size();
    Ok(DemoResult {
        created,
        categories,
        items,
        tables: DEMO_TABLES.len(),
    })
}

/// 建一份示範菜單。已經有商品就不動任何東西，回傳 false。
pub async fn seed_demo_menu(ctx: &Ctx) -> AppResult<bool> {
    let existing = menu::menu_tree(ctx).await?;
    let has_items = !existing.uncategorized.is_empty()
        || existing.categories.iter().any(|c| !c.items.is_empty());
    if has_items {
        return Ok(false);
    }

    // 幾張桌子。沒有桌位的話「內用」在畫面上就只是一個沒有用的按鈕。
    for code in DEMO_TABLES {
        crate::services::table::upsert_table(
            ctx,
            crate::services::table::TableInput {
                id: None,
                code: code.to_string(),
                name: None,
                seats: Some(if code.starts_with('B') { 6 } else { 4 }),
                area_name: Some(
                    if code.starts_with('B') {
                        "包廂"
                    } else {
                        "大廳"
                    }
                    .to_string(),
                ),
                is_active: Some(true),
            },
        )
        .await?;
    }

    // 選項群組。先建好，等一下品項建完再掛上去。
    let mut group_ids: std::collections::HashMap<&str, String> = Default::default();
    for (i, (name, multiple, min, options)) in MODIFIER_GROUPS.iter().enumerate() {
        let g = menu::upsert_modifier_group(
            ctx,
            menu::ModifierGroupInput {
                id: None,
                name: (*name).to_string(),
                selection_type: Some(if *multiple { "multiple" } else { "single" }.to_string()),
                min_select: Some(*min),
                // 複選不設上限：加珍珠又加椰果是客人的自由。
                max_select: Some(if *multiple { 0 } else { 1 }),
                sort_order: Some(i as i64 * 10),
            },
        )
        .await?;
        for (j, (opt, price, default)) in options.iter().enumerate() {
            menu::upsert_modifier(
                ctx,
                menu::ModifierInput {
                    id: None,
                    group_id: g.id.clone(),
                    name: (*opt).to_string(),
                    price: Some(*price),
                    is_default: Some(*default),
                    sold_out_until: None,
                    sort_order: Some(j as i64 * 10),
                    is_active: Some(true),
                },
            )
            .await?;
        }
        group_ids.insert(name, g.id);
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

        let wanted: Vec<String> = CATEGORY_GROUPS
            .iter()
            .find(|(c, _)| *c == group.name)
            .map(|(_, gs)| {
                gs.iter()
                    .filter_map(|g| group_ids.get(g).cloned())
                    .collect()
            })
            .unwrap_or_default();

        for (i, (name, price)) in group.items.iter().enumerate() {
            let item = menu::upsert_item(
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
            if !wanted.is_empty() {
                menu::set_item_modifier_groups(ctx, item.id, wanted.clone()).await?;
            }
        }
    }

    seed_demo_plan(ctx).await?;
    Ok(true)
}

/// 一個現成的吃到飽方案。
///
/// # 為什麼示範資料要包含它
///
/// 吃到飽是這套 POS 最不直覺的一個設計 ——「方案本身就是一個商品，人頭費就是
/// 那個商品點 N 份」這句話要講才懂。而**一個按得動的例子勝過一段說明**：
/// 打開「吃到飽」那一頁就看得到它長什麼樣、成員怎麼選、時限寫在哪裡。
///
/// 建不起來不算錯（例如店家已經自己建過同名商品），示範資料的失敗
/// 不應該讓「載入示範菜單」整個失敗。
async fn seed_demo_plan(ctx: &Ctx) -> AppResult<()> {
    let tree = menu::menu_tree(ctx).await?;
    let Some(drinks) = tree.categories.iter().find(|c| c.category.name == "飲料") else {
        return Ok(());
    };
    let Some(mains) = tree.categories.iter().find(|c| c.category.name == "主餐") else {
        return Ok(());
    };

    // 方案的代表商品。599 一位 —— 這一個商品就是人頭費。
    let plan_item = menu::upsert_item(
        ctx,
        ItemInput {
            id: None,
            category_id: Some(mains.category.id.clone()),
            name: "晚餐吃到飽".to_string(),
            short_name: None,
            base_price: 599,
            tax_code: None,
            is_open_price: None,
            sold_out_until: None,
            // 排最前面 —— 吃到飽的店，店員第一個要點的就是它。
            sort_order: Some(-10),
            is_active: Some(true),
        },
    )
    .await?;

    crate::services::dining::upsert(
        ctx,
        crate::services::dining::PlanInput {
            id: None,
            item_id: plan_item.id,
            name: "晚餐吃到飽".to_string(),
            // 兩小時，剩三十分鐘先提醒 —— スマレジ 的『事前通知』就是這個用法。
            limit_minutes: Some(120),
            notice_minutes: Some(30),
            print_members_on_bill: Some(false),
            is_active: Some(true),
            member_items: vec![],
            // 整個飲料分類無限暢飲。用分類而不是逐項勾，
            // 因為之後新增一款飲料不該還要記得回來加。
            member_categories: vec![drinks.category.id.clone()],
        },
    )
    .await?;

    Ok(())
}
