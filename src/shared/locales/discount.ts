import type { Catalog } from '@/shared/i18n'

/**
 * 折扣面板。
 *
 * # 「打幾折」是翻不過去的
 *
 * 中文說「9 折」，英文說「10% off」，日文說「10%引」—— 同一顆按鈕，
 * 三種語言算的是**相反的那一半**：中文寫剩下多少，英文與日文寫折掉多少。
 * 所以這幾則不是同一句話的三種寫法，而是三種算法各寫一次；
 * 鍵名照折掉的百分比取（`off10` = 9 折），改折數時三行要一起看。
 *
 * 直接把「9 折」翻成「90% off」是這裡最容易犯、也最貴的錯：
 * 收銀員按下去之前不會發現，客人結完帳才會。
 *
 * # 招待不是折扣
 *
 * 「招待」在報表上跟折扣是分開的兩筆 —— 老闆看折扣是看行銷成效，
 * 看招待是看有沒有人在送人情。日文用餐飲現場的「サービス」，
 * 不是字面的「招待」（那是邀請客人來，不是這一項免費）。
 *
 * # 金額用佔位符
 *
 * 品名與金額走 `{name}`、`{amount}`，不要把句子拆成兩半再用 JSX 接 ——
 * 中文的全形括號在英文要換成半形加一個空格，拆開之後接不回來。
 */
export const discount = {
  titleLine: { 'zh-TW': '單品折扣', en: 'Line discount', ja: '明細値引き' },
  titleOrder: { 'zh-TW': '整單折扣', en: 'Order discount', ja: '伝票全体の値引き' },
  targetLine: { 'zh-TW': '{name}（{amount}）', en: '{name} ({amount})', ja: '{name}（{amount}）' },
  targetOrder: {
    'zh-TW': '整單 {amount}',
    en: 'Whole order {amount}',
    ja: '伝票全体 {amount}',
  },

  // 四顆折數按鈕。中文寫「剩幾折」，英文與日文寫「折掉幾 %」。
  off10: { 'zh-TW': '9 折', en: '10% off', ja: '10%引' },
  off15: { 'zh-TW': '85 折', en: '15% off', ja: '15%引' },
  off20: { 'zh-TW': '8 折', en: '20% off', ja: '20%引' },
  off25: { 'zh-TW': '75 折', en: '25% off', ja: '25%引' },

  amountPlaceholder: {
    'zh-TW': '折抵多少元',
    en: 'Discount amount',
    ja: '値引き額',
  },
  // 輸入框右邊那顆，寬度只有 px-4：三種語言都只能放兩個字。
  applyAmount: { 'zh-TW': '折抵', en: 'Apply', ja: '値引' },

  comp: { 'zh-TW': '招待（免費）', en: 'Comp (free)', ja: 'サービス（無料）' },
  compTitle: {
    'zh-TW': '整行免費。報表上會進「招待」而不是「折扣」',
    en: 'Makes the whole line free. It is reported as a comp, not a discount.',
    ja: 'この明細を無料にします。レポートでは値引きではなくサービスとして集計されます。',
  },
} satisfies Catalog
