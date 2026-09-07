import type { Catalog } from '@/shared/i18n'

/**
 * 營運分析與稽核紀錄。
 *
 * # 為什麼兩張報表共用一份字典
 *
 * 它們的骨架是同一個：一段日期區間、幾張統計圖、一張明細表。
 * 日期欄位的字要一模一樣 —— 分成兩份字典的下場是「從／到」在一頁翻成
 * `From`、在另一頁翻成 `Start`，看起來像兩套不同的軟體。
 *
 * # 單位字要分開列
 *
 * 中文的量詞（張／次／項／份）在英文裡不是同一個字，日文也不是：
 * 帳單是「件」、次數是「回」、品項是「点」、賣出的份數是「食」。
 * 全部共用一個 `unit` 會讓「12 times」出現在品項排行上。
 *
 * # 日文用詞
 *
 * 用日本餐飲業的慣用語，不是字面直譯：平均客單是「客単価」不是「平均伝票」、
 * 作廢是「取消」、退點是「返品」、內用外帶是「イートイン／テイクアウト」。
 * 表頭要短 —— 明細表有七欄，一個欄位換行整張表就跟著變高。
 *
 * # 代碼字典（`auditActions`、`insightCodes`）
 *
 * 後端回的是代碼（`refund`、`dine_in`），不是「退款」「內用」。
 * 一個封閉的列舉要顯示成哪一國的字，是這一端的事 —— 後端不知道現在站在
 * 收銀機前面的人讀哪一種語言，而它也不該知道。
 *
 * 所以那兩份字典的**鍵就是後端的代碼**，一個字都不能差：查表用的是
 * `auditActions[row.action]`，沒有中間的轉換表可以出錯。
 */
export const reports = {
  // 兩張報表共用的日期區間欄位。
  from: { 'zh-TW': '從', en: 'From', ja: 'から' },
  to: { 'zh-TW': '到', en: 'To', ja: 'まで' },

  // ── 營運分析 ──────────────────────────────────────────────
  today: { 'zh-TW': '今天', en: 'Today', ja: '本日' },
  last7Days: { 'zh-TW': '最近 7 天', en: 'Last 7 days', ja: '直近 7 日' },
  last30Days: { 'zh-TW': '最近 30 天', en: 'Last 30 days', ja: '直近 30 日' },
  revenue: { 'zh-TW': '營業額', en: 'Sales', ja: '売上' },
  billCount: { 'zh-TW': '帳單數', en: 'Bills', ja: '伝票数' },
  averageBill: { 'zh-TW': '平均客單', en: 'Avg. bill', ja: '客単価' },
  noSales: {
    'zh-TW': '這段期間沒有結過帳。',
    en: 'No sales in this period.',
    ja: 'この期間の会計はありません。',
  },
  byHour: { 'zh-TW': '時段分布', en: 'By hour', ja: '時間帯別' },
  busiest: {
    'zh-TW': '最忙的是 {from}:00–{to}:00（{money}）',
    en: 'Busiest {from}:00–{to}:00 ({money})',
    ja: 'ピークは {from}:00–{to}:00（{money}）',
  },
  // 柱子的 tooltip。中日文用全形空白分欄，英文用間隔點 —— 全形空白在
  // 西文字型裡寬得像跑掉的排版。
  hourTip: {
    'zh-TW': '{hour}:00　{money}　{n} 張',
    en: '{hour}:00 · {money} · {n} bills',
    ja: '{hour}:00　{money}　{n} 件',
  },
  channels: { 'zh-TW': '內用 / 外帶', en: 'Dine-in / Takeout', ja: 'イートイン / テイクアウト' },
  discounts: { 'zh-TW': '折扣與招待', en: 'Discounts & comps', ja: '値引き・サービス' },
  voids: { 'zh-TW': '退點與作廢', en: 'Returns & voids', ja: '返品・取消' },
  topItems: { 'zh-TW': '品項排行', en: 'Top items', ja: '商品ランキング' },
  topN: { 'zh-TW': '（前 {n} 名）', en: '(top {n})', ja: '（上位 {n} 件）' },
  unitBills: { 'zh-TW': '張', en: 'bills', ja: '件' },
  unitTimes: { 'zh-TW': '次', en: 'x', ja: '回' },
  unitLines: { 'zh-TW': '項', en: 'lines', ja: '点' },
  unitServings: { 'zh-TW': '份', en: 'sold', ja: '食' },

  // ── 稽核紀錄 ──────────────────────────────────────────────
  toSameDay: {
    'zh-TW': '到（空白＝同一天）',
    en: 'To (blank = same day)',
    ja: 'まで（空白＝当日）',
  },
  moneyOnly: { 'zh-TW': '只看動到錢的', en: 'Money only', ja: '金額の動きのみ' },
  // {label} 是查 `auditActions` 查出來的字，不是後端給的中文。
  filterOnly: { 'zh-TW': '只看「{label}」✕', en: 'Only: {label} ✕', ja: '「{label}」のみ ✕' },
  moneyMoved: { 'zh-TW': '這段期間動到的錢', en: 'Net change', ja: '期間内の増減額' },
  byAction: { 'zh-TW': '依動作', en: 'By action', ja: '操作別' },
  byActor: { 'zh-TW': '依操作者', en: 'By staff', ja: '担当者別' },
  details: { 'zh-TW': '明細', en: 'Details', ja: '明細' },
  truncated: {
    'zh-TW': '只顯示最近 500 筆（上面的統計是完整的）',
    en: 'Showing latest 500 (totals above are complete)',
    ja: '最新 500 件のみ表示（上の集計は全件）',
  },
  noRecords: {
    'zh-TW': '這段期間沒有紀錄。',
    en: 'No records in this period.',
    ja: 'この期間の記録はありません。',
  },
  colTime: { 'zh-TW': '時間', en: 'Time', ja: '時刻' },
  colAction: { 'zh-TW': '動作', en: 'Action', ja: '操作' },
  colTarget: { 'zh-TW': '對象', en: 'Target', ja: '対象' },
  colActor: { 'zh-TW': '操作者', en: 'Staff', ja: '担当者' },
  colReason: { 'zh-TW': '原因', en: 'Reason', ja: '理由' },
  colApproval: { 'zh-TW': '簽核', en: 'Approver', ja: '承認者' },
  colAmount: { 'zh-TW': '金額', en: 'Amount', ja: '金額' },

  // 折扣／作廢那兩張圖的列名是「前綴＋名字」的組合：前綴是列舉（要翻），
  // 名字是店家自己打的字（不翻）。整句連同標點都放在字典裡 ——
  // 中日文用全形冒號，英文用半形加一個空格，那不是同一個符號。
  noReason: { 'zh-TW': '未填原因', en: 'No reason given', ja: '理由未記入' },
} satisfies Catalog

/**
 * 稽核動作代碼 → 顯示名稱。
 *
 * 鍵是 `audit_logs.action` 存進去的字串，**跟後端的 `AuditAction::as_str()`
 * 一模一樣**。那些字串進了資料庫又被報表 GROUP BY，改動等同於破壞歷史資料，
 * 所以這裡照抄、不另外取名。
 *
 * 認不得的代碼原樣顯示（見 `AuditPanel` 的 `actionLabel`）——
 * 畫面上看到 `foo` 至少查得出來是哪一個動作漏了翻譯，顯示「其他」查不出來。
 *
 * 日文用日本餐飲業的慣用語：作廢是「取消」、退款是「返金」、招待是「サービス」、
 * 補印是「再印刷」、關班是「シフト締め」。
 */
export const auditActions = {
  create: { 'zh-TW': '建立', en: 'Create', ja: '作成' },
  update: { 'zh-TW': '修改', en: 'Update', ja: '変更' },
  delete: { 'zh-TW': '刪除', en: 'Delete', ja: '削除' },
  void: { 'zh-TW': '作廢', en: 'Void', ja: '取消' },
  // 結完帳才作廢的那一種。錢已經收過了，所以它跟一般作廢不是同一件事 ——
  // 英日文也要看得出這個差別，不能都翻成 Void／取消。
  void_after_settle: {
    'zh-TW': '結帳後作廢',
    en: 'Void after settle',
    ja: '会計後取消',
  },
  discount: { 'zh-TW': '折扣', en: 'Discount', ja: '値引き' },
  comp: { 'zh-TW': '招待', en: 'Comp', ja: 'サービス' },
  price_override: { 'zh-TW': '改價', en: 'Price override', ja: '価格変更' },
  refund: { 'zh-TW': '退款', en: 'Refund', ja: '返金' },
  reprint: { 'zh-TW': '補印', en: 'Reprint', ja: '再印刷' },
  drawer_open: { 'zh-TW': '開錢箱', en: 'Drawer open', ja: 'ドロア開放' },
  shift_open: { 'zh-TW': '開班', en: 'Shift open', ja: 'シフト開始' },
  shift_close: { 'zh-TW': '關班', en: 'Shift close', ja: 'シフト締め' },
  settings_change: { 'zh-TW': '改設定', en: 'Settings change', ja: '設定変更' },
  restore: { 'zh-TW': '還原備份', en: 'Restore', ja: 'バックアップ復元' },
} satisfies Catalog

/**
 * 營運分析裡那幾張圖的列名代碼。
 *
 * 鍵是後端 `NamedTotal.code` 回的字串。三種來源混在同一份字典裡是刻意的 ——
 * 它們出現在同一頁的四張圖上，分成三份字典只會讓人找不到要改哪一份。
 *
 * 折扣與作廢那兩組帶 `{name}`：前綴是列舉、名字是店家打的字，
 * 整句連標點都在這裡組完，因為全形冒號跟半形冒號不是同一個排版。
 */
export const insightCodes = {
  // 通路。整列就是一個代碼，沒有名字。
  dine_in: { 'zh-TW': '內用', en: 'Dine-in', ja: 'イートイン' },
  takeout: { 'zh-TW': '外帶', en: 'Takeout', ja: 'テイクアウト' },
  delivery: { 'zh-TW': '外送', en: 'Delivery', ja: 'デリバリー' },
  // 折扣與招待。招待是送出去的東西，折扣是少收的錢 ——
  // 老闆對這兩件事的容忍度不同，所以兩個字要明顯不一樣。
  comp: { 'zh-TW': '招待：{name}', en: 'Comp: {name}', ja: 'サービス：{name}' },
  discount: { 'zh-TW': '折扣：{name}', en: 'Discount: {name}', ja: '値引き：{name}' },
  // 「點錯退掉一項」是日常，「整張單作廢」每一次都值得看一眼。
  void_item: { 'zh-TW': '退點：{name}', en: 'Returned: {name}', ja: '返品：{name}' },
  void_order: {
    'zh-TW': '整單作廢：{name}',
    en: 'Voided bill: {name}',
    ja: '伝票取消：{name}',
  },
} satisfies Catalog
