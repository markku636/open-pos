import type { Catalog } from '@/shared/i18n'

/**
 * 班別交接、日結，以及過去的日結報表（ShiftPanel / DayReportPanel）。
 *
 * # 日文用詞
 *
 * 收銀相關的詞用日本餐飲業的慣用語，不是字面直譯：
 * 關班是「シフト締め」、日結是「日次締め」、應有現金是「理論在高」、
 * 實際盤點是「実在高」、差異是「過不足」。
 * 「差異」直譯成「差異」日本店員看得懂，但那不是他們每天在講的詞 ——
 * 而這一頁的讀者正是每天要對這個數字負責的人。
 *
 * # 佔位符
 *
 * 帶數字的句子整句一則，用 `{n}` 之類的佔位符，三種語言都要出現同一組。
 * 拆成「前綴 + 數字 + 後綴」三段在畫面上拼起來的話一定會歪：
 * 中文「帳單 3 張」的量詞在最後，英文 `3 bills` 根本沒有量詞。
 *
 * 品名與付款方式名稱來自資料庫，不翻譯 —— 所以它們是佔位符 `{name}`，
 * 只有外面的括號與量詞跟著語言走。
 */
export const shift = {
  // ─── 子分頁 ───────────────────────────────────────
  // 標籤要短：這一排在報表上方，換行會把報表往下推。
  tabShift: { 'zh-TW': '班別與日結', en: 'Shifts', ja: 'シフト' },
  tabDay: { 'zh-TW': '日報表', en: 'Daily', ja: '日報' },
  tabInsight: { 'zh-TW': '營運分析', en: 'Insights', ja: '分析' },
  tabAudit: { 'zh-TW': '稽核紀錄', en: 'Audit', ja: '監査ログ' },

  // ─── 目前狀態列 ───────────────────────────────────
  businessDate: { 'zh-TW': '營業日', en: 'Business day', ja: '営業日' },
  status: { 'zh-TW': '狀態', en: 'Status', ja: '状態' },
  currentShift: { 'zh-TW': '目前班別', en: 'Shift', ja: '現在のシフト' },
  openedAt: { 'zh-TW': '開班時間', en: 'Opened', ja: '開始時刻' },
  openingFloat: { 'zh-TW': '準備金', en: 'Float', ja: '釣銭準備金' },
  closedShiftsToday: {
    'zh-TW': '今天已關 {n} 班',
    en: '{n} shifts closed today',
    ja: '本日 {n} シフト締め済み',
  },

  statusNotStarted: { 'zh-TW': '尚未開始', en: 'Not started', ja: '未開始' },
  statusOpen: { 'zh-TW': '營業中', en: 'Open', ja: '営業中' },
  statusClosing: { 'zh-TW': '結算中', en: 'Closing', ja: '締め処理中' },
  statusClosed: { 'zh-TW': '已日結', en: 'Day closed', ja: '日次締め済み' },
  statusLocked: { 'zh-TW': '已鎖定', en: 'Locked', ja: 'ロック済み' },

  // ─── 開班 ─────────────────────────────────────────
  openShift: { 'zh-TW': '開班', en: 'Open shift', ja: 'シフト開始' },
  openHint: {
    'zh-TW':
      '準備金是抽屜裡先放的零錢。它會被記成一筆現金異動 —— 不記的話，關班時抽屜裡的錢從哪裡來就查不出來。',
    en: 'The float is the change you put in the drawer to start. It is recorded as a cash movement — without it, there is no way to tell where the cash in the drawer came from at close.',
    ja: '釣銭準備金は開始時にドロアへ入れる釣銭です。現金異動として記録されます。記録しないと、シフト締めのときドロアの現金がどこから来たのか追えません。',
  },

  // ─── 關班盤點（盲盤）─────────────────────────────
  countTitle: { 'zh-TW': '關班盤點', en: 'Cash count', ja: '現金実査' },
  countHintLead: {
    'zh-TW': '照面額把抽屜裡的錢數一遍。',
    en: 'Count the drawer by denomination.',
    ja: '金種ごとにドロアの現金を数えてください。',
  },
  countHintStrong: {
    'zh-TW': '系統會等你數完才顯示應有金額與差異',
    en: ' The expected amount and the variance appear only after you finish',
    ja: '理論在高と過不足は数え終わってから表示されます',
  },
  countHintTail: {
    'zh-TW': '—— 先顯示的話，短少的人會直接照抄。',
    en: ' — show it first and anyone short just copies it.',
    ja: '。先に見せると、不足があっても数字を写すだけになります。',
  },
  countedTotal: { 'zh-TW': '數到的總額', en: 'Counted', ja: '実在高' },
  notePlaceholder: {
    'zh-TW': '備註（差異原因、交接對象…）',
    en: 'Note (reason for variance, handover to…)',
    ja: 'メモ（過不足の理由、引き継ぎ相手…）',
  },
  closeShift: { 'zh-TW': '關班', en: 'Close shift', ja: 'シフト締め' },
  closeResult: { 'zh-TW': '{no} 關班結果', en: '{no} close-out', ja: '{no} シフト締め結果' },

  // ─── X 報表與日結 ─────────────────────────────────
  xReport: { 'zh-TW': 'X 報表（中途查看）', en: 'X report (mid-shift)', ja: 'X レポート（点検）' },
  xReportHint: {
    'zh-TW': '不關班，中途看目前的數字',
    en: 'See the current numbers without closing the shift',
    ja: 'シフトを締めずに現在の数字を確認します',
  },
  closeDay: { 'zh-TW': '日結（Z 報表）', en: 'Day close (Z)', ja: '日次締め（Z）' },
  closeDayHint: {
    'zh-TW': '所有班別都關完之後才能日結',
    en: 'Available once every shift is closed',
    ja: 'すべてのシフトを締めてから実行できます',
  },
  dayTitle: { 'zh-TW': '{date} 日結', en: '{date} day close', ja: '{date} 日次締め' },
  exportCsv: { 'zh-TW': '匯出 CSV', en: 'Export CSV', ja: 'CSV 出力' },
  exportCsvHint: {
    'zh-TW': '給記帳的人。Excel 開起來中文不會亂碼，金額是數值可以直接加總',
    en: 'For the bookkeeper. Opens in Excel without garbled text, and the amounts are numbers you can sum',
    ja: '経理担当向け。Excel で文字化けせず、金額は数値なので合計できます',
  },
  exportDirPrompt: {
    'zh-TW': '匯出到哪個資料夾？例如 D: 或一個現有的資料夾',
    en: 'Export to which folder? For example D: or any existing folder',
    ja: 'どのフォルダーに書き出しますか？例：D: または既存のフォルダー',
  },
  exported: { 'zh-TW': '已匯出：{path}', en: 'Exported: {path}', ja: '書き出しました：{path}' },

  // ─── 報表本體 ─────────────────────────────────────
  sales: { 'zh-TW': '銷售', en: 'Sales', ja: '売上' },
  billsCount: { 'zh-TW': '帳單 {n} 張', en: '{n} bills', ja: '伝票 {n} 件' },
  discount: { 'zh-TW': '折扣', en: 'Discount', ja: '値引' },
  serviceCharge: { 'zh-TW': '服務費', en: 'Service charge', ja: 'サービス料' },
  rounding: { 'zh-TW': '進位調整', en: 'Rounding', ja: '端数調整' },
  netAndTax: { 'zh-TW': '未稅 {net}　稅 {tax}', en: 'Net {net} · Tax {tax}', ja: '税抜 {net}　税 {tax}' },

  cash: { 'zh-TW': '現金', en: 'Cash', ja: '現金' },
  cashSales: { 'zh-TW': '現金銷售', en: 'Cash sales', ja: '現金売上' },
  paidIn: { 'zh-TW': '現金收入', en: 'Paid in', ja: '入金' },
  paidOut: { 'zh-TW': '現金支出', en: 'Paid out', ja: '出金' },
  expectedCash: { 'zh-TW': '應有現金', en: 'Expected', ja: '理論在高' },
  countedCash: { 'zh-TW': '實際盤點', en: 'Counted', ja: '実在高' },
  variance: { 'zh-TW': '差異', en: 'Variance', ja: '過不足' },
  // 差異剛好是零。寫「0」也對，但數完錢的人要的是一句「對上了」。
  exact: { 'zh-TW': '剛好', en: 'Exact', ja: 'ぴったり' },

  paymentMethods: { 'zh-TW': '收款方式', en: 'Payment methods', ja: '支払方法' },
  paymentLine: { 'zh-TW': '{name}（{n} 筆）', en: '{name} ({n})', ja: '{name}（{n} 件）' },
  paymentCount: { 'zh-TW': '{name}（{n}）', en: '{name} ({n})', ja: '{name}（{n}）' },

  voids: { 'zh-TW': '作廢', en: 'Voids', ja: '取消' },
  voidedLines: { 'zh-TW': '退掉 {n} 項', en: '{n} items voided', ja: '{n} 品を取消' },

  shiftVariances: { 'zh-TW': '各班現金差異', en: 'Cash variance by shift', ja: 'シフト別過不足' },
  topItems: { 'zh-TW': '品項排行', en: 'Top items', ja: '商品ランキング' },

  lockNote: {
    'zh-TW':
      '日結之後這一天就鎖住了：不能再新增或修改這一天的單。數字是當下算好的快照，之後永不重算 —— 一份會自己變的報表在稽核上站不住。',
    en: 'Once the day is closed it is locked: no order for that day can be added or changed. The numbers are a snapshot taken at close and are never recalculated — a report that changes on its own is worthless in an audit.',
    ja: '日次締めを行うとその日はロックされ、伝票の追加も修正もできません。数字は締めた時点のスナップショットで、再計算はされません。あとから変わるレポートは監査に耐えられないからです。',
  },

  // ─── 過去的日結報表（DayReportPanel）─────────────
  noDaysTitle: { 'zh-TW': '還沒有日結過。', en: 'No day has been closed yet.', ja: 'まだ日次締めがありません。' },
  noDaysHow: {
    'zh-TW': '每天打烊時到「班別與日結」按日結，那一天的報表就會存下來 ——',
    en: 'Close the day from Shifts at closing time and that day’s report is saved —',
    ja: '閉店時に「日次締め」から締めると、その日のレポートが保存されます。',
  },
  noDaysWhy: {
    'zh-TW': '而存下來的數字之後永遠不會再變。',
    en: 'and the numbers saved there never change again.',
    ja: '保存された数字は、あとから変わることはありません。',
  },
  zReportNo: { 'zh-TW': 'Z 報表編號', en: 'Z report no.', ja: 'Z レポート番号' },
  billCount: { 'zh-TW': '帳單數', en: 'Bills', ja: '伝票数' },
  revenue: { 'zh-TW': '營業額', en: 'Revenue', ja: '売上高' },
  exportXlsx: { 'zh-TW': '匯出 Excel', en: 'Export Excel', ja: 'Excel 出力' },
  exportXlsxHint: {
    'zh-TW': '日報表 / 付款方式 / 品項排行 / 各班現金 四張表',
    en: 'Four sheets: daily report / payments / top items / cash by shift',
    ja: '日報／支払方法／商品ランキング／シフト別現金 の 4 シート',
  },
  totalSales: { 'zh-TW': '銷售總額', en: 'Total', ja: '売上合計' },
  netSales: { 'zh-TW': '未稅', en: 'Net', ja: '税抜' },
  taxAmount: { 'zh-TW': '稅額', en: 'Tax', ja: '税額' },
  payments: { 'zh-TW': '付款方式', en: 'Payments', ja: '支払方法' },
  refundsAndVoids: { 'zh-TW': '退款與作廢', en: 'Refunds & voids', ja: '返金・取消' },
  refundCount: { 'zh-TW': '退款筆數', en: 'Refunds', ja: '返金件数' },
  refundAmount: { 'zh-TW': '退款金額', en: 'Refund amount', ja: '返金額' },
  refundCash: { 'zh-TW': '其中現金', en: 'Of which cash', ja: 'うち現金' },
  voidedItems: { 'zh-TW': '退掉的品項', en: 'Voided items', ja: '取消品数' },
  voidAmount: { 'zh-TW': '作廢金額', en: 'Void amount', ja: '取消金額' },
} satisfies Catalog
