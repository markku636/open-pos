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
  // 動作名稱（{label}）是後端給的，不在這裡翻。
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
} satisfies Catalog
