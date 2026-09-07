import type { Catalog } from '@/shared/i18n'

/**
 * 吃到飽 / 無限暢飲方案的文案。
 *
 * # 這裡的文案要教會老闆兩件他猜不到的事
 *
 * 1. **人頭分級要去改商品規格**，不是在這一頁設。因為方案本身就是一個商品，
 *    「大人 599 / 小孩 299」就是那個商品的兩個規格。
 * 2. **時限只會提醒，不會自動加價也不會擋單。** 這是查過的 25 套產品的一致
 *    做法，但老闆很容易以為設了時限就會自動收超時費 —— 那個誤會要等到
 *    客人坐了三小時、帳單上什麼都沒有的時候才會發現。
 *
 * 日文用日本餐飲業的說法：吃到飽是「食べ放題」、無限暢飲是「飲み放題」、
 * 時限是「制限時間」、事前提醒是「事前通知」（スマレジ 用語）。
 */
export const plan = {
  title: { 'zh-TW': '吃到飽方案', en: 'All-you-can-eat plans', ja: '食べ放題プラン' },
  newTitle: { 'zh-TW': '新增方案', en: 'New plan', ja: 'プランを追加' },
  editTitle: { 'zh-TW': '設定方案', en: 'Edit plan', ja: 'プランの設定' },

  intro: {
    'zh-TW': '吃到飽的收費方式是「一個人多少錢」，所以方案本身就是一個商品 —— 人頭費就是那個商品點 N 份。',
    en: 'A buffet is priced per person, so the plan itself is just an item: the per-head charge is that item rung once per guest.',
    ja: '食べ放題は一人あたりの料金なので、プラン自体が一つの商品です。人数分をその商品で登録すれば、それが人数料金になります。',
  },
  introTiers: {
    'zh-TW': '想做「大人 599 / 小孩 299」？去那個商品加規格就好，這一頁不必再設一次。',
    en: 'Want adult 599 / child 299? Add variants to that item; there is nothing to set up again here.',
    ja: '「大人 599 / 子供 299」にしたい場合は、その商品にバリエーションを追加してください。ここで設定し直す必要はありません。',
  },

  empty: { 'zh-TW': '還沒有吃到飽方案。', en: 'No plans yet.', ja: 'プランがまだありません。' },
  emptyHint: {
    'zh-TW': '單點店不需要設定這一頁 —— 沒有方案的桌一切照舊。',
    en: 'A la carte shops can ignore this page: tables without a plan behave exactly as before.',
    ja: '単品のみのお店では設定不要です。プランのないテーブルはこれまでどおり動作します。',
  },

  name: { 'zh-TW': '方案名稱', en: 'Plan name', ja: 'プラン名' },
  namePlaceholder: {
    'zh-TW': '例：晚餐吃到飽',
    en: 'e.g. Dinner buffet',
    ja: '例：ディナー食べ放題',
  },

  chargeItem: { 'zh-TW': '收費的商品', en: 'The item that carries the price', ja: '料金となる商品' },
  chargeItemHint: {
    'zh-TW': '人頭費就是這個商品 × 人數。要分大人小孩，就去這個商品加規格。',
    en: 'The per-head charge is this item multiplied by the guest count. For adult/child tiers, add variants to this item.',
    ja: '人数料金はこの商品 × 人数です。大人／子供で分ける場合は、この商品にバリエーションを追加してください。',
  },

  membersTitle: { 'zh-TW': '方案包含什麼', en: 'What the plan covers', ja: 'プランに含まれるもの' },
  membersHint: {
    'zh-TW': '選整個分類最方便 —— 之後新增飲料不必再回來加。沒選到的東西照原價收（那就是加價品）。',
    en: 'Picking a whole category is easiest: new drinks added later are covered automatically. Anything not covered is charged at its normal price, which is how premium extras work.',
    ja: 'カテゴリー単位で選ぶのが簡単です。後から追加したドリンクも自動的に含まれます。含まれない商品は通常価格で加算され、それが追加料金の商品になります。',
  },
  members: {
    'zh-TW': '包含：{cats}，另加 {n} 個品項',
    en: 'Covers: {cats}, plus {n} item(s)',
    ja: '対象：{cats}、および {n} 品',
  },
  noMembers: {
    'zh-TW': '還沒選要包含什麼 —— 目前吃什麼都要另外收錢',
    en: 'Nothing covered yet, so everything is still charged separately',
    ja: '対象が未設定のため、現在はすべて別途料金になります',
  },
  pickItems: { 'zh-TW': '個別挑品項（已選 {n}）', en: 'Pick individual items ({n})', ja: '個別に商品を選ぶ（{n}）' },

  limit: { 'zh-TW': '用餐時限（分鐘，0 = 不限）', en: 'Time limit (minutes, 0 = none)', ja: '制限時間（分、0 = なし）' },
  notice: { 'zh-TW': '提前幾分鐘提醒', en: 'Warn this many minutes before', ja: '事前通知（分前）' },
  limitWarning: {
    'zh-TW': '時間到只會在桌位圖上變色與提示，不會自動加價，也不會擋住點餐。',
    en: 'At the limit the table just changes colour and warns. It never adds a surcharge and never blocks ordering.',
    ja: '制限時間になってもテーブルの色が変わって通知するだけです。自動で追加課金することも、注文を止めることもありません。',
  },
  minutes: { 'zh-TW': '{n} 分鐘', en: '{n} min', ja: '{n} 分' },

  enabled: { 'zh-TW': '啟用', en: 'Enabled', ja: '有効' },
  inactive: { 'zh-TW': '停用', en: 'Disabled', ja: '無効' },
  confirmDelete: {
    'zh-TW': '刪除「{name}」？已經開過的桌不受影響。',
    en: 'Delete "{name}"? Tables already opened with it are unaffected.',
    ja: '「{name}」を削除しますか？すでに適用済みのテーブルには影響しません。',
  },
} satisfies Catalog
