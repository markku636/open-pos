import { describe, expect, it } from 'vitest'

import { segments } from './Emphasis'

/** 把切好的段落壓成一句好讀的字串：`平常[粗體]平常`。 */
const shape = (s: string) =>
  segments(s)
    .map((x) => (x.bold ? `[${x.text}]` : x.text))
    .join('')

describe('segments', () => {
  it('把成對的 ** 標成粗體', () => {
    expect(shape('**已經結帳的單不會變** —— 存的是當時的金額')).toBe(
      '[已經結帳的單不會變] —— 存的是當時的金額',
    )
    expect(shape('但**不會自動補一行差額** —— 市售產品都這樣')).toBe(
      '但[不會自動補一行差額] —— 市售產品都這樣',
    )
    expect(shape('**A** 與 **B**')).toBe('[A] 與 [B]')
  })

  it('沒有標記就原樣輸出', () => {
    expect(shape('台灣是 5%，內含在售價裡。')).toBe('台灣是 5%，內含在售價裡。')
    expect(shape('')).toBe('')
  })

  /**
   * ★ 真正要守的一條：**譯者少打一個星號，文字不能消失。**
   *
   * 這是三語系字典最現實的失敗模式 —— 沒有人會逐字檢查日文譯文裡的
   * markdown 標記，而「整句話從畫面上不見」在一個講稅率的頁面上是災難。
   */
  it('標記沒閉合時寧可不加粗，也不吃掉文字', () => {
    expect(shape('稅率是 **5%')).toBe('稅率是 5%')
    expect(shape('**')).toBe('')
    expect(shape('a**b**c**d')).toBe('a[b]cd')
  })

  it('切出來的文字接回去要跟原文一樣（除了標記本身）', () => {
    for (const s of ['**a**b', 'a **b** c', '沒有標記', '**', '***x***']) {
      expect(segments(s).map((x) => x.text).join('')).toBe(s.split('**').join(''))
    }
  })
})
