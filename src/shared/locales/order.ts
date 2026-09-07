import type { Catalog } from '@/shared/i18n'

/**
 * 點餐、結帳、選項維護三個畫面的字。
 *
 * # 為什麼放在同一份字典
 *
 * 這三個畫面是同一條動線：按品項 → 選規格 → 進購物車 → 收錢。
 * 同一個詞（「必選」「售完」「找零」）會在動線上出現好幾次，
 * 拆成三份字典的下場是同一個概念三種講法。
 *
 * # 標籤要短
 *
 * 分帳那排頁籤（整單／平分／指定金額／分項）擠在結帳面板裡，
 * 而日文與英文都比中文長。所以英文用 `Even` 而不是 `Split evenly`，
 * 日文用「割り勘」而不是「均等に分割」—— 一換行，應收金額就被擠出畫面。
 *
 * # 日文用詞
 *
 * 用日本餐飲業櫃檯真的會講的話，不是字面直譯：
 * 結帳是「会計」、找零是「お釣り」、外帶是「テイクアウト」、
 * 作廢是「取消」、加點是「追加注文」。直譯的日文讀得懂，
 * 但一看就知道是外國軟體翻過來的。
 */
export const order = {
  // ── 菜單區 ──
  loading: { 'zh-TW': '載入中…', en: 'Loading…', ja: '読み込み中…' },
  uncategorized: { 'zh-TW': '未分類', en: 'Uncategorized', ja: '未分類' },
  soldOut: { 'zh-TW': '已售完', en: 'Sold out', ja: '売切' },

  // 全新安裝看到的第一個畫面。這幾句要留住第一次打開它的人。
  emptyMenuTitle: { 'zh-TW': '還沒有商品', en: 'No items yet', ja: '商品がありません' },
  emptyMenuHint: {
    'zh-TW': '建立自己的菜單，或先載一份示範資料看看這套東西怎麼運作。',
    en: 'Build your own menu, or load sample data to see how this works.',
    ja: 'メニューを作るか、サンプルデータを読み込んで動きを確認できます。',
  },
  loadDemo: {
    'zh-TW': '載入示範菜單與桌位',
    en: 'Load sample menu & tables',
    ja: 'サンプルのメニューとテーブルを読み込む',
  },
  demoNote: {
    'zh-TW': '示範資料就是一般商品，之後可以直接改或刪掉。',
    en: 'Sample data is ordinary items. Edit or delete them anytime.',
    ja: 'サンプルは通常の商品です。あとから編集・削除できます。',
  },
  demoAlreadySeeded: {
    'zh-TW': '已經有商品了，示範菜單沒有動任何東西。',
    en: 'Items already exist. The sample menu changed nothing.',
    ja: '既に商品があります。サンプルメニューは何も変更していません。',
  },
  emptyCategory: {
    'zh-TW': '這一類還沒有商品 —— 請先到「商品維護」建立菜單',
    en: 'Nothing in this category. Add items under Menu first',
    ja: 'このカテゴリーに商品がありません。「商品管理」で追加してください',
  },

  // ── 購物車抬頭 ──
  leaveTable: { 'zh-TW': '離開這一桌', en: 'Leave this table', ja: 'テーブルを離れる' },
  guests: { 'zh-TW': '{n} 位', en: '{n} guests', ja: '{n} 名' },
  dineIn: { 'zh-TW': '內用', en: 'Dine in', ja: 'イートイン' },
  takeout: { 'zh-TW': '外帶', en: 'Takeout', ja: 'テイクアウト' },
  noOrderYet: { 'zh-TW': '尚未開單', en: 'No order', ja: '伝票なし' },
  cartEmpty: {
    'zh-TW': '點左邊的商品開始',
    en: 'Tap an item on the left to start',
    ja: '左の商品をタップして開始',
  },
  cartEmptySeat: {
    'zh-TW': '{code} —— 點左邊的商品開始',
    en: '{code} · Tap an item on the left to start',
    ja: '{code} · 左の商品をタップして開始',
  },

  // 全形括號與頓號是中文標點：英文介面裡夾一個「（Large）」看起來就是沒翻完。
  variantParen: { 'zh-TW': '（{name}）', en: ' ({name})', ja: '（{name}）' },
  listSeparator: { 'zh-TW': '、', en: ', ', ja: '、' },

  lineDiscount: {
    'zh-TW': '這一項打折或招待',
    en: 'Discount or comp this line',
    ja: 'この明細を値引き・サービス',
  },
  voidLine: {
    'zh-TW': '退掉這一項（會通知廚房）',
    en: 'Void this line (kitchen is notified)',
    ja: 'この明細を取消（厨房へ通知）',
  },

  // ── 金額 ──
  subtotal: { 'zh-TW': '小計', en: 'Subtotal', ja: '小計' },
  serviceCharge: { 'zh-TW': '服務費', en: 'Service', ja: 'サービス料' },
  rounding: { 'zh-TW': '進位調整', en: 'Rounding', ja: '端数調整' },
  total: { 'zh-TW': '合計', en: 'Total', ja: '合計' },
  taxLine: {
    'zh-TW': '未稅 {net}　稅 {tax}',
    en: 'Net {net} · Tax {tax}',
    ja: '税抜 {net} · 税 {tax}',
  },
  billedParts: {
    'zh-TW': '已收 {n} 份 {amount}',
    en: '{n} paid · {amount}',
    ja: '{n} 件 会計済み {amount}',
  },
  short: { 'zh-TW': '還差 {amount}', en: '{amount} left', ja: '残り {amount}' },

  // ── 購物車按鈕 ──
  discountOrder: { 'zh-TW': '整單折扣', en: 'Discount', ja: '全体値引き' },
  voidOrderTitle: {
    'zh-TW': '整張單不做了。已經送到廚房的品項會印一張取消單',
    en: 'Cancel the whole order. Items already sent to the kitchen print a void slip',
    ja: '伝票全体を取消します。厨房へ送信済みの商品は取消票を印刷します',
  },
  voidOrderConfirm: {
    'zh-TW': '要作廢整張單嗎？\n\n已經送到廚房的品項會印一張取消單。',
    en: 'Void the whole order?\n\nItems already sent to the kitchen will print a void slip.',
    ja: '伝票全体を取消しますか？\n\n厨房へ送信済みの商品は取消票を印刷します。',
  },
  voidOrder: { 'zh-TW': '作廢整單', en: 'Void order', ja: '伝票取消' },
  checkout: { 'zh-TW': '結帳', en: 'Pay', ja: '会計' },
  payRemaining: {
    'zh-TW': '收剩下的 {amount}',
    en: 'Collect rest {amount}',
    ja: '残り {amount} を会計',
  },

  // 結完一份之後留在畫面上的那句話。找零要停著讓收銀員照著數錢。
  splitPaid: {
    'zh-TW': '已收第 {n} 份 {no}',
    en: 'Split {n} paid · {no}',
    ja: '{n} 件目 会計済み {no}',
  },
  changeSuffix: {
    'zh-TW': '　找零 {amount}',
    en: ' · Change {amount}',
    ja: ' · お釣り {amount}',
  },
  stillShortSuffix: {
    'zh-TW': '　⚠ 這張單還差 {amount}',
    en: ' · ⚠ {amount} left on this order',
    ja: ' · ⚠ この伝票は残り {amount}',
  },
  settled: {
    'zh-TW': '已結帳 {no}　找零 {amount}',
    en: 'Paid {no} · Change {amount}',
    ja: '会計済み {no} · お釣り {amount}',
  },

  // ── 未結的單 ──
  openOrders: { 'zh-TW': '未結', en: 'Open', ja: '未会計' },
  openPartialTitle: {
    'zh-TW': '這張單分帳還沒收完',
    en: 'Split bill not fully paid',
    ja: '分割会計が完了していません',
  },
  openOrderTitle: { 'zh-TW': '回到這張單', en: 'Back to this order', ja: 'この伝票に戻る' },
  openPartialTag: { 'zh-TW': '未收', en: 'due', ja: '未収' },

  // ── 結帳面板 ──
  payTitle: { 'zh-TW': '結帳', en: 'Payment', ja: 'お会計' },
  pickMethod: { 'zh-TW': '請選付款方式', en: 'Pick a payment method', ja: '支払方法を選択' },
  collectVia: {
    'zh-TW': '以「{method}」收取 {amount}',
    en: 'Collect {amount} by {method}',
    ja: '「{method}」で {amount} を会計',
  },
  noChangeMethod: {
    'zh-TW': '這種付款方式不能找零',
    en: 'This method gives no change',
    ja: 'この支払方法はお釣りが出せません',
  },
  confirmPayment: { 'zh-TW': '確認收款', en: 'Confirm', ja: '会計確定' },

  // 分帳頁籤。四個字以內，換行就把應收金額擠出畫面。
  splitFull: { 'zh-TW': '整單', en: 'Full', ja: '一括' },
  splitBalance: { 'zh-TW': '收尾款', en: 'Balance', ja: '残額' },
  splitEven: { 'zh-TW': '平分', en: 'Even', ja: '割り勘' },
  splitAmount: { 'zh-TW': '指定金額', en: 'Amount', ja: '金額指定' },
  splitItems: { 'zh-TW': '分項', en: 'Items', ja: '品目別' },

  // 份數已經鎖住時的說明。份數本身另外放大顯示，所以這一句接在那個數字後面。
  splitLockedRest: {
    'zh-TW': '等分，已經收了 {n} 份',
    en: 'ways · {n} paid',
    ja: '等分 · {n} 件 会計済み',
  },
  amountPlaceholder: {
    'zh-TW': '這一份收多少',
    en: 'Amount for this split',
    ja: 'この分の金額',
  },
  allPaid: { 'zh-TW': '都結完了', en: 'All paid', ja: 'すべて会計済み' },
  due: { 'zh-TW': '應收', en: 'Due', ja: 'ご請求' },
  previewTotal: { 'zh-TW': '全單 {amount}', en: 'Order {amount}', ja: '伝票計 {amount}' },
  previewBilled: { 'zh-TW': ' · 已收 {amount}', en: ' · Paid {amount}', ja: ' · 会計済み {amount}' },
  previewIndexOf: { 'zh-TW': ' · 第 {i}／{n} 份', en: ' · Split {i}/{n}', ja: ' · {i}／{n} 件目' },
  previewIndex: { 'zh-TW': ' · 第 {i} 筆', en: ' · Payment {i}', ja: ' · {i} 件目' },
  remainingAfter: {
    'zh-TW': '收完還差 {amount}',
    en: '{amount} left after',
    ja: '会計後 残り {amount}',
  },

  // ── 現金鈔票鍵 ──
  exactTitle: {
    'zh-TW': '客人剛好給整數',
    en: 'Customer paid the exact amount',
    ja: 'ちょうど受け取る',
  },
  exact: { 'zh-TW': '剛好', en: 'Exact', ja: 'ちょうど' },
  tenderedPlaceholder: { 'zh-TW': '客人給多少', en: 'Amount received', ja: 'お預かり' },
  change: { 'zh-TW': '找零', en: 'Change', ja: 'お釣り' },

  // ── 選項群組維護 ──
  groupsTitle: { 'zh-TW': '選項群組', en: 'Option groups', ja: 'オプショングループ' },
  groupsHint: {
    'zh-TW': '建好之後到品項那邊勾選要問哪幾組',
    en: 'Then attach groups to items in the item editor',
    ja: '作成後、商品側で使うグループを選びます',
  },
  groupNamePlaceholder: {
    'zh-TW': '群組名稱（甜度 / 加購）',
    en: 'Group name (Sweetness / Add-ons)',
    ja: 'グループ名（甘さ / トッピング）',
  },
  singleToggle: { 'zh-TW': '單選（必選一個）', en: 'Single (pick one)', ja: '単一選択（必須）' },
  multipleToggle: { 'zh-TW': '複選（可不選）', en: 'Multiple (optional)', ja: '複数選択（任意）' },
  addGroup: { 'zh-TW': '新增群組', en: 'Add group', ja: 'グループ追加' },
  noGroups: {
    'zh-TW': '還沒有選項群組。',
    en: 'No option groups yet.',
    ja: 'オプショングループがありません。',
  },
  noGroupsExample: {
    'zh-TW': '建一組「甜度」（單選）跟一組「加購」（複選），',
    en: 'Add "Sweetness" (single) and "Add-ons" (multiple),',
    ja: '「甘さ」（単一選択）と「トッピング」（複数選択）を作れば、',
  },
  noGroupsResult: {
    'zh-TW': '點餐時就問得出「珍奶半糖少冰加珍珠」。',
    en: 'and you can ring up "milk tea, half sugar, less ice, extra pearls".',
    ja: '「タピオカミルクティー 甘さ半分 氷少なめ タピオカ追加」が受けられます。',
  },
  single: { 'zh-TW': '單選', en: 'Single', ja: '単一選択' },
  multiple: { 'zh-TW': '複選', en: 'Multiple', ja: '複数選択' },
  required: { 'zh-TW': '必選', en: 'Required', ja: '必須' },
  optionCount: { 'zh-TW': '{n} 個選項', en: '{n} options', ja: '{n} 件' },
  deleteGroupTitle: {
    'zh-TW': '刪掉整組。已經賣出去的訂單不受影響（存的是當時的名稱與價格）',
    en: 'Delete the whole group. Past orders keep the name and price recorded at the time',
    ja: 'グループごと削除します。売上済みの伝票は当時の名称と価格を保持します',
  },
  deleteGroupConfirm: {
    'zh-TW': '要刪掉「{name}」整組嗎？\n\n掛著它的品項會一起解除。',
    en: 'Delete the group "{name}"?\n\nItems using it will be unlinked.',
    ja: '「{name}」をグループごと削除しますか？\n\n使用中の商品からも外れます。',
  },
  noOptions: {
    'zh-TW': '還沒有選項。加價填 0 就是免費選項（半糖、去冰）。',
    en: 'No options yet. Set the surcharge to 0 for free options (half sugar, no ice).',
    ja: 'オプションがありません。追加料金 0 で無料オプション（甘さ半分・氷なし）になります。',
  },
  optionNamePlaceholder: {
    'zh-TW': '選項名稱（半糖 / 加珍珠）',
    en: 'Option name (Half sugar / Extra pearls)',
    ja: 'オプション名（甘さ半分 / タピオカ追加）',
  },
  surcharge: { 'zh-TW': '加價', en: 'Extra', ja: '追加料金' },
  addOption: { 'zh-TW': '加選項', en: 'Add option', ja: 'オプション追加' },
  free: { 'zh-TW': '免費', en: 'Free', ja: '無料' },
  defaultTitle: {
    'zh-TW': '點餐時先勾起來',
    en: 'Pre-selected when ordering',
    ja: '注文時に既定で選択',
  },
  defaultTag: { 'zh-TW': '預設', en: 'Default', ja: '既定' },

  // ── 選規格與選項的對話框 ──
  variants: { 'zh-TW': '規格', en: 'Variant', ja: '種類' },
  multipleAllowed: { 'zh-TW': '可複選', en: 'multiple', ja: '複数可' },
  stillNeed: { 'zh-TW': '還要選：{list}', en: 'Still need: {list}', ja: '未選択：{list}' },
  addToCart: { 'zh-TW': '加入 {amount}', en: 'Add {amount}', ja: '追加 {amount}' },
  soldOutShort: { 'zh-TW': '售完', en: 'Out', ja: '売切' },
} satisfies Catalog
