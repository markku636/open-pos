import type { Catalog } from '@/shared/i18n'

/**
 * 商品維護（分類、品項、規格、選項群組）。
 *
 * # 品名不在這裡
 *
 * 這一頁大部分的字其實是**資料**：分類名、品名、規格名、選項群組名都來自
 * 店家自己建的菜單。字典只收「畫面本來就長在那裡」的字 ——
 * 換語言換的是介面，不是菜單。
 *
 * # 日文用詞
 *
 * 規格（大／中／小）在日本餐飲店叫「サイズ」，不是字面直譯的「規格」；
 * 選項群組（甜度、加購）叫「オプション」。左欄那顆按鈕跟分類並排，
 * 所以標籤取短的「オプション」而不是完整的「オプショングループ」——
 * 那一欄只有 16rem 寬，一長就被截掉。
 *
 * # 確認視窗的字要說清楚後果
 *
 * 刪分類、刪商品的 `confirm()` 都明講「東西不會跟著不見」。
 * 店家最怕的是刪一個分類把整批商品連同歷史訂單一起弄丟 ——
 * 講清楚才敢按下去，而三種語言都要一樣講清楚。
 */
export const menu = {
  loading: { 'zh-TW': '載入中…', en: 'Loading…', ja: '読み込み中…' },

  // -------------------------------------------------------------- 分類
  categories: { 'zh-TW': '分類', en: 'Categories', ja: 'カテゴリ' },
  uncategorized: { 'zh-TW': '未分類', en: 'Uncategorized', ja: '未分類' },
  modifierGroups: { 'zh-TW': '選項群組', en: 'Option groups', ja: 'オプション' },
  // 行內改名的那顆確認鍵，中文刻意只有一個字。英文用 OK 而不是 Save，
  // 因為它旁邊就是輸入框，按鈕一寬名字就沒地方打。
  saveShort: { 'zh-TW': '存', en: 'OK', ja: '保存' },
  renameHint: {
    'zh-TW': '雙擊可改名',
    en: 'Double-click to rename',
    ja: 'ダブルクリックで名前を変更',
  },
  deleteCategoryHint: {
    'zh-TW': '刪除分類（裡面的品項會移到「未分類」，不會一起刪掉）',
    en: 'Delete category (its items move to Uncategorized, they are not deleted)',
    ja: 'カテゴリを削除（中の商品は「未分類」へ移動し、削除されません）',
  },
  deleteCategoryConfirm: {
    'zh-TW': '刪除分類「{name}」？\n\n裡面的 {n} 個品項會移到「未分類」，不會被刪掉。',
    en: 'Delete category "{name}"?\n\nIts {n} items move to Uncategorized. They are not deleted.',
    ja: 'カテゴリ「{name}」を削除しますか？\n\n中の {n} 件の商品は「未分類」へ移動し、削除されません。',
  },
  addCategory: { 'zh-TW': '新增分類…', en: 'New category…', ja: 'カテゴリを追加…' },

  // -------------------------------------------------------------- 品項
  pickCategory: {
    'zh-TW': '請先建立一個分類',
    en: 'Create a category first',
    ja: 'まずカテゴリを作成してください',
  },
  noItems: {
    'zh-TW': '這一類還沒有商品',
    en: 'No items in this category',
    ja: 'このカテゴリに商品はありません',
  },
  itemName: { 'zh-TW': '品名', en: 'Item name', ja: '商品名' },
  itemPrice: { 'zh-TW': '價格', en: 'Price', ja: '価格' },
  deleteItemHint: {
    'zh-TW': '刪除商品（歷史訂單不受影響）',
    en: 'Delete item (past orders are unaffected)',
    ja: '商品を削除（過去の伝票には影響しません）',
  },
  deleteItemConfirm: {
    'zh-TW': '刪除「{name}」？\n\n歷史訂單裡的紀錄不會受影響。',
    en: 'Delete "{name}"?\n\nPast orders are unaffected.',
    ja: '「{name}」を削除しますか？\n\n過去の伝票の記録には影響しません。',
  },

  // -------------------------------------------------------------- 規格
  variantsHint: { 'zh-TW': '規格（大 / 中 / 小）', en: 'Sizes (L / M / S)', ja: 'サイズ（L / M / S）' },
  // 只有 52px 寬的右側欄位，三種語言都得塞得下。
  variantCount: { 'zh-TW': '{n} 規格', en: '{n} sizes', ja: '{n} 種' },
  noVariants: {
    'zh-TW': '還沒有規格。加了之後點餐時會先問客人要哪一種（例如大杯 +10）。',
    en: 'No sizes yet. Add one and the order screen asks which size first (e.g. Large +10).',
    ja: 'サイズはまだありません。追加すると、注文時にどれかを先に確認します（例：Lサイズ +10）。',
  },
  variantName: { 'zh-TW': '規格名稱（大杯）', en: 'Size name (Large)', ja: 'サイズ名（Lサイズ）' },
  addVariant: { 'zh-TW': '加規格', en: 'Add size', ja: 'サイズ追加' },

  // -------------------------------------------------------------- 選項群組
  noGroups: {
    'zh-TW': '還沒有選項群組。到左邊的「選項群組」建一組「甜度」或「加購」，點餐時就問得出來。',
    en: 'No option groups yet. Add one under Option groups on the left, such as Sweetness or Add-ons, and the order screen will ask.',
    ja: 'オプショングループがまだありません。左の「オプション」で「甘さ」や「トッピング」を作ると、注文時に確認できます。',
  },
  askOnOrder: {
    'zh-TW': '點這一項的時候要問：',
    en: 'Ask when this item is ordered:',
    ja: 'この商品の注文時に確認する：',
  },
  // 群組名旁邊的單字標記：單選 / 複選。
  selectSingle: { 'zh-TW': '單', en: 'one', ja: '単' },
  selectMulti: { 'zh-TW': '複', en: 'many', ja: '複数可' },
} satisfies Catalog
