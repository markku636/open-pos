import type { Catalog } from '@/shared/i18n'

/**
 * 店家設定的文案。
 *
 * # 這一頁的文案要做兩件事
 *
 * 1. **把 basis point 翻譯成人話。** 老闆想的是「收 10% 服務費」，
 *    不是「1000 個 basis point」。所以輸入框收的是百分比，
 *    換算藏在程式裡。
 * 2. **講清楚改了會影響誰。** 已經結帳的單不會變、還開著的單會重算 ——
 *    這兩件事要分開講，否則老闆會以為改了之後今天的報表會跟著變，
 *    然後在月底發現數字對不上時以為是系統壞了。
 */
export const store = {
  title: { 'zh-TW': '店家設定', en: 'Store settings', ja: '店舗設定' },
  saved: { 'zh-TW': '已儲存', en: 'Saved', ja: '保存しました' },

  /**
   * ★ 最重要的一句：改設定不會回頭重算。
   *
   * 措辭刻意寫成「已結帳／未結帳」而不是「之後開的單」——
   * 後者不準確：現在開著還沒結的單，下次動它時會用新費率重算，
   * 因為它還沒有「當時算好的金額」這回事。
   * 這一頁講的是稅，講得含糊等於沒講。
   * （`src-tauri/tests/store_settings.rs` 有這句話的證明測試。）
   */
  notRetroactive: {
    'zh-TW': '**已經結帳的單不會變** —— 存的是當時算好的金額，一份會自己變的報表在稅務查核上站不住。還開著沒結的單則會用新費率重算，所以尖峰時段中途改費率要小心。',
    en: '**Settled orders never change.** They keep the amounts computed at the time, because a report that rewrites itself has no standing in an audit. Orders still open will be recomputed at the new rate, so think twice before changing rates mid-service.',
    ja: '**会計済みの伝票は変わりません。** 当時計算された金額のまま保存されます。後から数字が変わる帳票は税務調査で通用しないためです。まだ開いている伝票は新しい料率で再計算されるため、営業中の変更にはご注意ください。',
  },

  basics: { 'zh-TW': '基本資料', en: 'Basics', ja: '基本情報' },
  name: { 'zh-TW': '店名', en: 'Store name', ja: '店舗名' },
  taxId: { 'zh-TW': '統一編號', en: 'Tax ID', ja: '事業者番号' },
  taxIdHint: {
    'zh-TW': '收據上會印，開發票也要用。',
    en: 'Printed on receipts and required for invoicing.',
    ja: 'レシートに印字され、インボイスにも使用します。',
  },
  phone: { 'zh-TW': '電話', en: 'Phone', ja: '電話番号' },
  address: { 'zh-TW': '地址', en: 'Address', ja: '住所' },

  money: { 'zh-TW': '金額規則', en: 'Money rules', ja: '金額のルール' },
  taxRate: { 'zh-TW': '營業稅率（%）', en: 'Tax rate (%)', ja: '消費税率（%）' },
  taxRateHint: {
    'zh-TW': '台灣是 5%，內含在售價裡。',
    en: 'Taiwan is 5%, included in the listed price.',
    ja: '台湾は 5%、表示価格に内税で含まれます。',
  },
  serviceCharge: { 'zh-TW': '內用服務費（%）', en: 'Dine-in service charge (%)', ja: 'イートインのサービス料（%）' },
  serviceChargeHint: {
    'zh-TW': '只對內用收，外帶與外送不收。它要計入稅基 —— 服務費在稅法上就是銷售額的一部分。',
    en: 'Charged on dine-in only, never on takeout or delivery. It is part of the taxable base, because a service charge is legally part of the sale.',
    ja: 'イートインのみに適用し、テイクアウトやデリバリーには課しません。サービス料は法律上、売上の一部として課税対象になります。',
  },
  rounding: { 'zh-TW': '抹零', en: 'Rounding', ja: '端数処理' },
  roundNone: { 'zh-TW': '不抹零', en: 'None', ja: 'なし' },
  roundToFive: { 'zh-TW': '四捨五入到 5 元', en: 'Round to nearest 5', ja: '5 円単位に四捨五入' },
  roundFloorFive: { 'zh-TW': '捨去到 5 元（對客人有利）', en: 'Round down to 5 (in the guest’s favour)', ja: '5 円単位に切り捨て（客に有利）' },
  roundFloorTen: { 'zh-TW': '捨去到 10 元（夜市 / 小吃常見）', en: 'Round down to 10 (common at night markets)', ja: '10 円単位に切り捨て（屋台などで一般的）' },
  roundingHint: {
    'zh-TW': '抹零後的金額才是發票上的總計。抹零前開發票、抹零後收錢等於短開。',
    en: 'The rounded total is what goes on the invoice. Invoicing before rounding but collecting after under-declares the sale.',
    ja: '端数処理後の金額がインボイスの合計になります。処理前に発行して処理後に受け取ると、売上の過少計上になります。',
  },

  ops: { 'zh-TW': '營運', en: 'Operations', ja: '運用' },
  cutoff: { 'zh-TW': '營業日切點', en: 'Business day cutoff', ja: '営業日の切り替え時刻' },
  cutoffHint: {
    'zh-TW': '凌晨兩點的單要算前一天。設 05:00 的話，05:00 之前的單都算前一天的業績。',
    en: 'A 2am order belongs to the previous day. With 05:00, anything before 05:00 counts towards the previous day.',
    ja: '深夜 2 時の伝票は前日の売上として扱います。05:00 に設定すると、05:00 より前の伝票はすべて前日分になります。',
  },
  minCharge: { 'zh-TW': '每人低消（元）', en: 'Minimum spend per head', ja: '一人あたりの最低消費（円）' },
  minChargeHint: {
    'zh-TW': '結帳時如果沒達到會提醒，但**不會自動補一行差額** —— 查過的市售產品幾乎都是這樣做的。0 = 沒有低消。',
    en: 'Warns at checkout when the bill falls short, but never adds a top-up line: that is how nearly every product surveyed does it. 0 means no minimum.',
    ja: '会計時に金額が不足していれば警告しますが、差額の行を自動で追加することはありません。調査したほぼすべての製品がこの方式です。0 は最低消費なしです。',
  },

  /**
   * 開桌費 / お通し / テーブルチャージ。
   *
   * 文案要回答老闆的兩個問題：**收多少**（那是商品的售價，不在這裡設）
   * 與**什麼時候收**（開檯一次，加點不再收）。第二個問題不寫出來，
   * 第一通客服電話就會是「為什麼同一桌收了兩次開桌費」——
   * 而答案其實是「沒有」，只是他沒辦法從畫面上確認。
   */
  coverCharge: { 'zh-TW': '開桌費', en: 'Cover charge', ja: 'お通し / 席料' },
  coverChargeNone: { 'zh-TW': '不收開桌費', en: 'None', ja: '徴収しない' },
  coverChargeHint: {
    'zh-TW': '選一個商品，開檯時自動點上「人數」份。**同一桌加點不會再收一次。** 金額是那個商品的售價 —— 在商品維護改，這裡不用動。',
    en: 'Pick a menu item; it is added once per guest when the table is seated. **Adding more items to the same table never charges it again.** The amount is that item’s price, so change it in Menu, not here.',
    ja: '商品を一つ選ぶと、着席時に「人数」分だけ自動で登録されます。**同じ卓に追加注文しても、二重には徴収されません。** 金額はその商品の価格です（変更は商品管理から）。',
  },
  coverChargeMissing: {
    'zh-TW': '目前設定的開桌費商品已經下架了，開檯時不會收 —— 請重新選一個，或改成「不收開桌費」。',
    en: 'The item set as the cover charge is no longer on sale, so nothing is charged at seating. Pick another, or set it to None.',
    ja: '設定中のお通し商品は現在販売停止のため、着席時に徴収されません。別の商品を選ぶか、「徴収しない」にしてください。',
  },
} satisfies Catalog
