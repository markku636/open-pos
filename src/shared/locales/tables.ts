import type { Catalog } from '@/shared/i18n'

/**
 * 桌位圖。
 *
 * # 日文用的是餐飲現場的講法，不是字面直譯
 *
 * 桌號是「卓番」、開檯是「開卓」、清桌是「片付け」、結帳是「会計」。
 * 直譯出來的「テーブル番号」「使用開始」日本店員讀得懂，
 * 但一看就知道是外國軟體 —— 而收銀機是他們整天在按的東西。
 *
 * # 卡片上的字必須短
 *
 * 一張桌位卡只有 160px 寬，上面要塞桌號、金額、坐了幾分鐘、幾位客人。
 * 日文與英文都比中文長，「加點 / 結帳」一換行，整排卡片就會忽高忽低。
 * 所以按鈕上是「追加 / 会計」而不是更完整的「追加注文 / お会計」。
 *
 * # 空桌用「空席」而不是「空卓」
 *
 * 兩個都通，但店員嘴上講的是「空席」。挑常用的那個。
 */
export const tables = {
  heading: { 'zh-TW': '桌位', en: 'Tables', ja: 'テーブル' },
  inUse: {
    'zh-TW': '{n} / {total} 桌使用中',
    en: '{n} / {total} in use',
    ja: '{n} / {total} 卓 使用中',
  },
  editMode: { 'zh-TW': '編輯桌位', en: 'Edit tables', ja: 'テーブル編集' },
  // 錯誤訊息旁邊那顆 ✕ 的 title。它是「我看到了，收起來」而不是「關閉視窗」。
  dismiss: { 'zh-TW': '知道了', en: 'Dismiss', ja: '閉じる' },
  loading: { 'zh-TW': '載入中…', en: 'Loading…', ja: '読み込み中…' },
  // 空狀態拆成兩句，中間有一個 <br /> —— 併成一句的話窄螢幕會斷在奇怪的位置。
  empty1: {
    'zh-TW': '還沒有桌位。按「編輯桌位 → 新增」建立第一桌，',
    en: 'No tables yet. Use "Edit tables → Add" to create the first one,',
    ja: 'テーブルがまだありません。「テーブル編集 → 追加」で最初の一卓を作成し、',
  },
  empty2: {
    'zh-TW': '或是這家店只做外帶的話，這一頁可以完全不用。',
    en: 'or skip this page entirely if the shop is takeaway only.',
    ja: 'または、テイクアウトのみの店なら、このページは使わなくても構いません。',
  },
  unzoned: { 'zh-TW': '未分區', en: 'Unassigned', ja: 'エリア未設定' },
  confirmClear: {
    'zh-TW': '要清掉 {code} 嗎？\n\n這一桌的帳必須已經結完。',
    en: 'Clear {code}?\n\nThe bill must be paid first.',
    ja: '{code} を片付けますか？\n\n先に会計を済ませてください。',
  },
  confirmDelete: {
    'zh-TW': '要刪掉 {code} 嗎？',
    en: 'Delete {code}?',
    ja: '{code} を削除しますか？',
  },

  // 桌位卡
  seats: { 'zh-TW': '{n} 人桌', en: '{n} seats', ja: '{n} 名席' },
  minutes: { 'zh-TW': '{n} 分', en: '{n} min', ja: '{n} 分' },
  seatedInfo: {
    'zh-TW': '{n} 位 · {time} 入座',
    en: '{n} guests · seated {time}',
    ja: '{n} 名 · {time} 着席',
  },
  orders: { 'zh-TW': '{n} 張單', en: '{n} orders', ja: '伝票 {n} 枚' },
  free: { 'zh-TW': '空桌', en: 'Free', ja: '空席' },
  disabled: { 'zh-TW': '停用中', en: 'Disabled', ja: '無効' },
  addOrPay: { 'zh-TW': '加點 / 結帳', en: 'Add / Pay', ja: '追加 / 会計' },
  clear: { 'zh-TW': '清桌', en: 'Clear', ja: '片付け' },
  clearHint: {
    'zh-TW': '把這一桌清空。必須先結完帳',
    en: 'Clear this table. The bill must be paid first',
    ja: 'テーブルを片付けます。先に会計が必要です',
  },
  edit: { 'zh-TW': '編輯', en: 'Edit', ja: '編集' },

  // 桌位設定
  editTitle: { 'zh-TW': '編輯桌位', en: 'Edit table', ja: 'テーブル編集' },
  newTitle: { 'zh-TW': '新增桌位', en: 'New table', ja: 'テーブル追加' },
  code: { 'zh-TW': '桌號', en: 'Table no.', ja: '卓番' },
  codeHint: {
    'zh-TW': '店員報號用的，越短越好',
    en: 'Staff call it out loud, so keep it short',
    ja: 'スタッフが読み上げるので短く',
  },
  name: { 'zh-TW': '名稱（選填）', en: 'Name (optional)', ja: '名称（任意）' },
  namePlaceholder: { 'zh-TW': '靠窗', en: 'Window', ja: '窓際' },
  seatCount: { 'zh-TW': '座位數', en: 'Seats', ja: '席数' },
  area: { 'zh-TW': '區域（選填）', en: 'Area (optional)', ja: 'エリア（任意）' },
  areaPlaceholder: { 'zh-TW': '大廳', en: 'Main hall', ja: 'ホール' },
  active: {
    'zh-TW': '啟用（停用的桌還會留在圖上，但不能開檯）',
    en: 'Active (disabled tables stay on the map but cannot be opened)',
    ja: '有効（無効なテーブルは表示されますが開卓できません）',
  },

  // 開檯
  openTitle: { 'zh-TW': '{code} 開檯', en: 'Open {code}', ja: '{code} 開卓' },
  guests: { 'zh-TW': '幾位？', en: 'How many guests?', ja: '何名様ですか？' },
  otherCount: { 'zh-TW': '其他人數', en: 'Other count', ja: 'その他の人数' },
  confirm: { 'zh-TW': '確定', en: 'OK', ja: '確定' },

  /**
   * 吃到飽的桌子顯示剩餘時間。
   *
   * 超時是**分開的一句**而不是負數的剩餘時間：「-15 分鐘」要多想一秒，
   * 而店員是在走過去的路上瞄一眼的。
   */
  remaining: { 'zh-TW': '剩 {n} 分', en: '{n} min left', ja: '残り {n} 分' },
  overBy: { 'zh-TW': '超時 {n} 分', en: '{n} min over', ja: '{n} 分超過' },
} satisfies Catalog
