import type { Catalog } from '@/shared/i18n'

/**
 * 金流商名稱、說明，與每一個憑證欄位的標籤 + 「去哪裡拿」。
 *
 * # 為什麼這些從 Rust 搬過來
 *
 * 它們是文案。後端只回鍵值（`gateway.rs` 的 `fields_for` 現在只回
 * `["merchant_id", "hash_key", "hash_iv"]`），標籤與說明在這裡查。
 *
 * # 鍵值必須跟後端一字不差
 *
 * 這裡的 key 就是後端回的字串。對不上的症狀是欄位標籤變成一串
 * `hash_iv` —— 不會壞掉，只是很醜，而且沒有人會回報。
 * `gatewayFields.test.ts` 會盯著這件事。
 */
export const gwProvider = {
  manual: {
    'zh-TW': '不串接（自己抄授權碼）',
    en: 'Not integrated (type in the authorisation code)',
    ja: '連携なし（承認番号を手入力）',
  },
  linepay: { 'zh-TW': 'LINE Pay', en: 'LINE Pay', ja: 'LINE Pay' },
  newebpay: { 'zh-TW': '藍新金流', en: 'NewebPay', ja: '藍新（NewebPay）' },
} satisfies Catalog

export const gwProviderNote = {
  manual: {
    'zh-TW':
      '刷卡機是銀行給的那一台。收銀員刷完把授權碼抄進 POS，這裡只記帳。不需要網路，也不需要任何憑證。',
    en: 'The card terminal is the one your bank gave you. The cashier swipes there and types the authorisation code into the POS, which only records it. No internet and no credentials needed.',
    ja: 'カード端末は銀行から借りているものです。決済後に承認番号をPOSへ入力するだけで、ここでは記録のみ行います。ネットワークも認証情報も不要です。',
  },
  linepay: {
    'zh-TW': '掃客人手機出示的付款碼。需要 LINE Pay 商家帳號。',
    en: "Scan the payment code on the customer's phone. Requires a LINE Pay merchant account.",
    ja: 'お客様のスマホに表示された支払いコードを読み取ります。LINE Pay の加盟店アカウントが必要です。',
  },
  newebpay: {
    'zh-TW': '信用卡。需要跟藍新簽約後拿到的商店代號與兩把金鑰。',
    en: 'Credit cards. Requires the merchant ID and two keys you get after signing with NewebPay.',
    ja: 'クレジットカード。藍新（NewebPay）と契約後に発行される商店番号と2つの鍵が必要です。',
  },
} satisfies Catalog

/** 憑證欄位的標籤。key 對應後端 `fields_for()` 回的字串。 */
export const gwField = {
  channel_id: { 'zh-TW': 'Channel ID', en: 'Channel ID', ja: 'Channel ID' },
  channel_secret: {
    'zh-TW': 'Channel Secret',
    en: 'Channel Secret',
    ja: 'Channel Secret',
  },
  merchant_id: {
    'zh-TW': '商店代號 MerchantID',
    en: 'Merchant ID',
    ja: '商店番号（MerchantID）',
  },
  hash_key: { 'zh-TW': 'HashKey', en: 'HashKey', ja: 'HashKey' },
  hash_iv: { 'zh-TW': 'HashIV', en: 'HashIV', ja: 'HashIV' },
} satisfies Catalog

/**
 * 這個值要去哪裡拿。
 *
 * **每一則都必須自己講得完整。** 一則寫著「跟上面那個同一頁」的說明，
 * 在畫面上欄位順序改過之後就變成錯的，而且沒有人會發現 ——
 * 這是我自己在第一版踩過的，測試現在盯著它。
 */
export const gwFieldHint = {
  channel_id: {
    'zh-TW': 'LINE Pay 商家後台 → 管理付款連結 → 線上技術串接資訊',
    en: 'LINE Pay merchant console -> Payment link management -> Online integration info',
    ja: 'LINE Pay 加盟店管理画面 → 決済リンク管理 → オンライン連携情報',
  },
  channel_secret: {
    'zh-TW':
      'LINE Pay 商家後台，跟 Channel ID 在同一個畫面。這一組等於你的收款權限，不要外流',
    en: 'LINE Pay merchant console, on the same screen as the Channel ID. This pair is your ability to take payments - keep it private',
    ja: 'LINE Pay 加盟店管理画面の Channel ID と同じ画面にあります。この組み合わせは入金権限そのものなので、外部に出さないでください',
  },
  merchant_id: {
    'zh-TW': '藍新後台 → 商店資料設定',
    en: 'NewebPay console -> Store profile settings',
    ja: '藍新（NewebPay）管理画面 → 商店情報設定',
  },
  hash_key: {
    'zh-TW': '藍新後台 → 商店資料設定 → 串接程式設定',
    en: 'NewebPay console -> Store profile settings -> Integration settings',
    ja: '藍新（NewebPay）管理画面 → 商店情報設定 → 連携プログラム設定',
  },
  hash_iv: {
    'zh-TW': '藍新後台 → 商店資料設定 → 串接程式設定，跟 HashKey 一起發給你的',
    en: 'NewebPay console -> Store profile settings -> Integration settings; issued together with the HashKey',
    ja: '藍新（NewebPay）管理画面 → 商店情報設定 → 連携プログラム設定。HashKey と同時に発行されます',
  },
} satisfies Catalog

/**
 * 「還缺 X、Y」中間的分隔符。
 *
 * 中文與日文用頓号，英文用逗號加空白 —— 把 `、` 寫死在程式裡，
 * 英文畫面上會出現一個突兀的全形符號。
 */
export const gwListSep = {
  'zh-TW': '、',
  en: ', ',
  ja: '、',
}
