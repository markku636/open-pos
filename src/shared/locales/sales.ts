import type { Catalog } from '@/shared/i18n'

/**
 * 銷售記錄與帳單退款兩頁。
 *
 * # 為什麼兩頁共用一份字典
 *
 * 「分帳 1/2」「已退 200」這幾個短語兩頁都在用，而且講的是同一件事。
 * 拆成兩份的下場是同一個詞在兩頁翻得不一樣 —— 老闆在銷售記錄看到「返金」、
 * 在退款頁看到「払い戻し」，會以為那是兩種東西。
 *
 * # 日文用詞
 *
 * 用日本餐飲業的慣用語，不是字面直譯：
 * 帳單是「伝票」不是「請求書」（後者是月結的請款單，不是收銀機吐的那張）、
 * 退款是「返金」、補印是「再印刷」、結帳是「会計」。
 * 補印跟出單機那一頁（printer.ts）一定要用同一個詞 —— 同一件事在兩頁
 * 講成「再印刷」跟「再発行」，店家會以為那是兩種不一樣的補單。
 *
 * 按鈕標籤要**短**。「補印收據」的日文寫全是「レシート再印刷」，
 * 但那顆按鈕在帳單標題列的最右邊，字一長整列就換行 —— 所以只留「再印刷」，
 * 前後文已經說明了是哪一張單。
 *
 * # 金額與單號不進字典
 *
 * `formatMoney()` 的輸出、單號、品名都是資料不是文案，只當參數帶進來。
 * 三種語言的佔位符必須一模一樣，少一個就是那個語言的畫面上少一個數字。
 */
export const sales = {
  // ── 銷售記錄：查詢條件 ──────────────────────────────────────
  today: { 'zh-TW': '今天', en: 'Today', ja: '本日' },
  last7Days: { 'zh-TW': '最近 7 天', en: 'Last 7 days', ja: '過去7日' },
  last30Days: { 'zh-TW': '最近 30 天', en: 'Last 30 days', ja: '過去30日' },
  dateFrom: { 'zh-TW': '從', en: 'From', ja: '開始日' },
  dateTo: { 'zh-TW': '到', en: 'To', ja: '終了日' },
  // 這個輸入框只有 w-40，英日文一長就被截掉，所以刻意寫得比中文更省字。
  billNoPlaceholder: {
    'zh-TW': '單號末幾碼',
    en: 'Last digits',
    ja: '伝票番号の下桁',
  },
  channelAll: { 'zh-TW': '全部', en: 'All', ja: 'すべて' },
  // ── 通路代碼 → 標籤 ────────────────────────────────────────
  //
  // 後端只回代碼（`dine_in`），中文標籤是在這裡對出來的。以前那個標籤
  // 是後端算好一起吐上來的，於是英文與日文的店員在一切正常的路徑上
  // 看到「內用」—— 而後端從頭到尾就不知道現在是誰在看這台機器。
  //
  // 上面的篩選按鈕與下面列表那一欄**共用這同一組字**。分兩組寫的下場是：
  // 按下去的是「テイクアウト」，列表那欄卻寫「持ち帰り」，看起來像兩種東西。
  // 所以 `SaleRow` 的通路欄寧可放寬，也不另外編一套只有那一欄看得懂的縮寫。
  channelDineIn: { 'zh-TW': '內用', en: 'Dine-in', ja: 'イートイン' },
  channelTakeout: { 'zh-TW': '外帶', en: 'Takeout', ja: 'テイクアウト' },
  channelDelivery: { 'zh-TW': '外送', en: 'Delivery', ja: 'デリバリー' },
  refundedOnly: { 'zh-TW': '只看退過款的', en: 'Refunded only', ja: '返金済みのみ' },

  // ── 銷售記錄：合計與匯出 ────────────────────────────────────
  statCount: { 'zh-TW': '筆數', en: 'Count', ja: '件数' },
  statRevenue: { 'zh-TW': '營業額', en: 'Revenue', ja: '売上' },
  statRefunded: { 'zh-TW': '已退', en: 'Refunded', ja: '返金' },
  exportHint: {
    'zh-TW': '帳單 / 品項明細 / 付款方式三張表，金額是可以直接加總的數字',
    en: 'Three sheets: bills, line items, payment methods. Amounts are numbers you can sum.',
    ja: '伝票・明細・支払方法の3シート。金額はそのまま集計できる数値です。',
  },
  exportExcel: { 'zh-TW': '匯出 Excel', en: 'Export Excel', ja: 'Excel出力' },
  exported: { 'zh-TW': '已匯出：{path}', en: 'Exported: {path}', ja: '出力しました：{path}' },

  // ── 銷售記錄：列表 ──────────────────────────────────────────
  noSalesInRange: {
    'zh-TW': '這段期間沒有結過帳。',
    en: 'No sales in this period.',
    ja: 'この期間の会計はありません。',
  },
  truncated: {
    'zh-TW': '只顯示最近 500 筆（上面的合計與匯出的 Excel 是完整的）',
    en: 'Showing the latest 500 only (the totals above and the Excel export are complete)',
    ja: '直近500件のみ表示（上の合計とExcel出力は全件です）',
  },
  // 品名之間的分隔符號。中日文用頓號，英文用逗號 —— 頓號在英文字型裡
  // 會變成一個看不懂的方塊。
  listSeparator: { 'zh-TW': '、', en: ', ', ja: '、' },
  // 規格名接在品名後面。英文用半形括號並補一個前導空格，
  // 全形括號在英文句子裡會撐出一個突兀的空洞。
  variantSuffix: { 'zh-TW': '（{name}）', en: ' ({name})', ja: '（{name}）' },
  subtotal: { 'zh-TW': '小計 {amount}', en: 'Subtotal {amount}', ja: '小計 {amount}' },
  discount: { 'zh-TW': '折扣 −{amount}', en: 'Discount −{amount}', ja: '割引 −{amount}' },
  serviceCharge: {
    'zh-TW': '服務費 {amount}',
    en: 'Service {amount}',
    ja: 'サービス料 {amount}',
  },
  taxLine: {
    'zh-TW': '未稅 {net}　稅 {tax}',
    en: 'Net {net} · Tax {tax}',
    ja: '税抜 {net}　税 {tax}',
  },
  settledBy: { 'zh-TW': '結帳：{name}', en: 'Settled by {name}', ja: '会計担当：{name}' },

  // ── 兩頁共用 ────────────────────────────────────────────────
  splitLabel: { 'zh-TW': '分帳 {label}', en: 'Split {label}', ja: '分割 {label}' },
  refundedAmount: { 'zh-TW': '已退 {amount}', en: 'Refunded {amount}', ja: '返金 {amount}' },
  refundShort: { 'zh-TW': '退 {amount}', en: 'refunded {amount}', ja: '返金 {amount}' },

  // ── 帳單退款：找單 ──────────────────────────────────────────
  billSearchPlaceholder: {
    'zh-TW': '單號末幾碼（空白＝今天全部）',
    en: 'Last digits of bill no. (blank = all of today)',
    ja: '伝票番号の下桁（空欄＝本日すべて）',
  },
  // 「載入中」不放這裡：nav.ts 的 ui.loading 就是同一句話。各頁各留一份的下場是
  // 同一個等待狀態在銷售記錄寫「查詢中」、在帳單頁寫「載入中」。
  billNotFound: {
    'zh-TW': '找不到這個單號',
    en: 'No bill with that number',
    ja: 'その伝票番号は見つかりません',
  },
  noBillsToday: {
    'zh-TW': '今天還沒有結過帳',
    en: 'No sales yet today',
    ja: '本日の会計はまだありません',
  },
  pickBill: {
    'zh-TW': '左邊選一張帳單。退款需要主管權限，而且一定要選原因。',
    en: 'Pick a bill on the left. Refunds need a manager and a reason.',
    ja: '左から伝票を選んでください。返金には管理者権限と理由が必要です。',
  },

  // ── 帳單退款：補印 ──────────────────────────────────────────
  reprintReceipt: { 'zh-TW': '補印收據', en: 'Reprint', ja: '再印刷' },
  reprintHint: {
    'zh-TW': '再印一張給客人。單上會註明是第幾次補印',
    en: 'Print another copy for the customer. It is marked as a reprint',
    ja: 'お客様用にもう一枚印刷します。再印刷の回数がレシートに印字されます',
  },
  reprintQueued: {
    'zh-TW': '補印的收據已經送進出單佇列。',
    en: 'The reprint has been sent to the print queue.',
    ja: 'レシートの再印刷を印刷キューに送りました。',
  },

  // ── 帳單退款：退款表單 ──────────────────────────────────────
  fullyRefunded: {
    'zh-TW': '這張帳單已經全部退完了。',
    en: 'This bill has been fully refunded.',
    ja: 'この伝票は全額返金済みです。',
  },
  whichPayment: {
    'zh-TW': '退回哪一筆收款',
    en: 'Which payment to refund',
    ja: 'どの支払いに返金するか',
  },
  refundAmountPlaceholder: { 'zh-TW': '退多少', en: 'Amount', ja: '返金額' },
  refundAllHint: { 'zh-TW': '整筆退回', en: 'Refund the whole payment', ja: '全額を返金' },
  refundAll: { 'zh-TW': '全退 {amount}', en: 'All {amount}', ja: '全額 {amount}' },
  reasonRequired: { 'zh-TW': '原因（必填）', en: 'Reason (required)', ja: '理由（必須）' },
  noteRequired: {
    'zh-TW': '說明（這個原因必填）',
    en: 'Note (required for this reason)',
    ja: '備考（この理由では必須）',
  },
  noteOptional: { 'zh-TW': '說明（選填）', en: 'Note (optional)', ja: '備考（任意）' },
  refund: { 'zh-TW': '退款', en: 'Refund', ja: '返金' },
  refundFootnote: {
    'zh-TW': '會印一張退款單給客人簽名，並留下簽核紀錄。',
    en: 'A refund slip prints for the customer to sign, and the approval is logged.',
    ja: 'お客様署名用の返金伝票を印刷し、承認記録を残します。',
  },
  refundDone: {
    'zh-TW': '已退 {amount}（{method}）　這張單累計退了 {total}',
    en: 'Refunded {amount} ({method}) · {total} refunded on this bill in total',
    ja: '{amount} を返金しました（{method}）　この伝票の返金累計は {total}',
  },
  paymentFullyRefunded: { 'zh-TW': '已退完', en: 'Fully refunded', ja: '返金済み' },
  paymentRefundable: {
    'zh-TW': '可退 {amount}',
    en: 'Refundable {amount}',
    ja: '返金可能 {amount}',
  },
} satisfies Catalog
