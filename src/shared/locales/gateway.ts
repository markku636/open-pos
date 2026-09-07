import type { Catalog } from '@/shared/i18n'

/**
 * 金流設定頁。
 *
 * # 那段警告的三個鍵是故意拆開的
 *
 * 「備份出去的 .db 檔等於你的收款權限」在畫面上是被 `<span>` 標亮的一句，
 * 前後各接一段普通文字。整段併成一個鍵就沒辦法只亮中間那句 ——
 * 而這一頁唯一真正要人看進去的就是那一句。
 *
 * 收尾那個鍵開頭帶著標點（中日文的「。」、英文的「. 」），
 * 因為標點屬於哪一段、要不要空格，本來就是每個語言各自的事。
 *
 * # 日文用詞
 *
 * 走日本餐飲業的講法，不是字面直譯：結帳是「会計」不是「決済処理」、
 * 日結是「日次締め」、支付方式是「支払方法」。
 * 「金流商」在日本的說法是「決済事業者」（或「決済代行会社」），
 * 直譯成「資金流通業者」日本店員看得懂，但一看就知道是外國軟體。
 *
 * 按鈕標籤要短。這一頁的卡片是兩欄排版，標題右邊只剩兩顆小按鈕的寬度，
 * 標籤一長就換行把卡片撐高，一頁看得到的金流少一筆。
 */
export const gateway = {
  // 刪除確認。第二句比第一句重要：真正要讓人知道的是「舊帳不會壞掉」，
  // 不然店家會不敢按，改用停用的方式留一堆廢設定在列表裡。
  deleteConfirm: {
    'zh-TW': '刪除「{name}」？\n已經收過的款不會受影響，之後不能再用這條線收款。',
    en: 'Delete "{name}"?\nPayments already taken are unaffected. You just cannot take new ones through it.',
    ja: '「{name}」を削除しますか？\nすでに受け付けた決済には影響しません。今後この経路では受け付けられなくなります。',
  },

  warnTitle: {
    'zh-TW': '金流憑證存在這台電腦的資料庫裡，沒有加密。',
    en: 'Payment credentials are stored on this machine, in the database, unencrypted.',
    ja: '決済の認証情報は、このパソコンのデータベースに暗号化せず保存されます。',
  },
  warnLead: { 'zh-TW': '也就是說：', en: 'Which means: ', ja: 'つまり、' },
  warnBackup: {
    'zh-TW': '備份出去的 .db 檔等於你的收款權限',
    en: 'a .db backup is your ability to take money',
    ja: 'バックアップした .db ファイルは決済権限そのものです',
  },
  warnTail: {
    'zh-TW': '。 請把它當成跟保險箱鑰匙一樣的東西保管，不要丟到公開的雲端資料夾。',
    en: '. Keep it like the key to the safe. Do not put it in a shared cloud folder.',
    ja: '。金庫の鍵と同じように保管し、共有のクラウドフォルダーには置かないでください。',
  },

  configured: { 'zh-TW': '已設定的金流', en: 'Configured', ja: '設定済みの決済' },

  emptyTitle: {
    'zh-TW': '還沒有設定任何金流。',
    en: 'No payment gateway configured yet.',
    ja: '決済はまだ設定されていません。',
  },
  emptyHint: {
    'zh-TW': '沒有設定也能營業 —— 現金與「自己抄授權碼的刷卡機」本來就不需要串接。',
    en: 'You can still trade without one. Cash, and a card terminal you copy the approval code off by hand, need no integration at all.',
    ja: '設定しなくても営業できます。現金や、承認番号を手で書き写すタイプのカード端末は連携そのものが不要です。',
  },

  active: { 'zh-TW': '啟用中', en: 'On', ja: '有効' },
  inactive: { 'zh-TW': '停用', en: 'Off', ja: '無効' },
  // 中文的「測試」、日文的「テスト」在這一頁從頭到尾都是同一個字根，英文也要
  // 只留一個 —— 標章寫 Test、說明與表單卻寫 Sandbox，店家會以為那是兩種模式。
  // 以 Sandbox 為準：那是金流業界的講法，而且下面的 sandboxLabel 已經這樣寫了。
  sandboxBadge: { 'zh-TW': '測試', en: 'Sandbox', ja: 'テスト' },
  sandboxTip: {
    'zh-TW': '測試環境不會真的收到錢',
    en: 'Sandbox takes no real money',
    ja: 'テスト環境では実際の入金はありません',
  },
  methodOf: {
    'zh-TW': '收款方式：{name}',
    en: 'Method: {name}',
    ja: '支払方法：{name}',
  },
  fieldUnset: { 'zh-TW': '未設定', en: 'Not set', ja: '未設定' },
  missing: {
    'zh-TW': '還缺 {fields}，填完才能啟用。',
    en: 'Missing {fields}. Fill them in before enabling.',
    ja: '{fields} が未入力です。入力すると有効にできます。',
  },
  // 缺漏欄位串起來時的分隔符號。中日文用頓號，英文用逗號 ——
  // 頓號在英文字型裡會變成一個看不懂的方塊。
  fieldSeparator: { 'zh-TW': '、', en: ', ', ja: '、' },

  editTitle: { 'zh-TW': '設定金流', en: 'Configure gateway', ja: '決済の設定' },
  newTitle: { 'zh-TW': '新增金流', en: 'Add gateway', ja: '決済の追加' },

  provider: { 'zh-TW': '金流商', en: 'Provider', ja: '決済事業者' },
  displayName: { 'zh-TW': '顯示名稱', en: 'Display name', ja: '表示名' },
  displayNameHint: {
    'zh-TW': '收銀員在結帳畫面上看到的就是這個',
    en: 'This is what the cashier sees at checkout',
    ja: '会計画面でレジ担当者に表示される名前です',
  },
  methodField: {
    'zh-TW': '對應的收款方式',
    en: 'Linked payment method',
    ja: '紐づける支払方法',
  },
  methodNone: { 'zh-TW': '（不綁定）', en: '(not linked)', ja: '（紐づけない）' },
  methodHint: {
    'zh-TW': '綁定之後，結帳選這個收款方式就會走這條線。日結報表上也照這個分類。',
    en: 'Once linked, picking this method at checkout goes through this gateway. Day-end reports group it the same way.',
    ja: '紐づけると、会計でこの支払方法を選んだ分がこの経路を通ります。日次締めのレポートも同じ区分で集計されます。',
  },

  credentials: { 'zh-TW': '憑證', en: 'Credentials', ja: '認証情報' },
  // 末四碼夠讓人確認「是不是我貼的那一組」，又不足以拿去用。
  credSet: {
    'zh-TW': '已設定 ••••{tail}（留空不修改）',
    en: 'Set ••••{tail} (leave blank to keep)',
    ja: '設定済み ••••{tail}（空欄なら変更しません）',
  },
  credUnset: { 'zh-TW': '尚未設定', en: 'Not set', ja: '未設定' },

  sandboxLabel: { 'zh-TW': '測試環境', en: 'Sandbox', ja: 'テスト環境' },
  sandboxNote: {
    'zh-TW': '（不會真的收到錢）',
    en: '(no real money)',
    ja: '（実際の入金はありません）',
  },
  enable: { 'zh-TW': '啟用', en: 'Enable', ja: '有効にする' },
} satisfies Catalog
