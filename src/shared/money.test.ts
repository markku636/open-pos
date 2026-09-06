import { describe, expect, it } from 'vitest'

import { formatMoney, parseMoney } from './money'

describe('formatMoney', () => {
  it('加上千分位與貨幣符號', () => {
    expect(formatMoney(0)).toBe('$0')
    expect(formatMoney(65)).toBe('$65')
    expect(formatMoney(1234)).toBe('$1,234')
    expect(formatMoney(1234567)).toBe('$1,234,567')
  })

  it('負數（退款、找零）顯示在符號前面', () => {
    expect(formatMoney(-50)).toBe('-$50')
  })

  it('可選的正號用於顯示調整項', () => {
    expect(formatMoney(18, { sign: true })).toBe('+$18')
    expect(formatMoney(0, { sign: true })).toBe('$0')
  })
})

describe('parseMoney', () => {
  it('接受一般輸入', () => {
    expect(parseMoney('120')).toBe(120)
    expect(parseMoney('1,234')).toBe(1234)
    expect(parseMoney('$500')).toBe(500)
    expect(parseMoney(' 80 ')).toBe(80)
  })

  it('接受全形數字 —— 中文輸入法下很容易打出來', () => {
    expect(parseMoney('１２０')).toBe(120)
    expect(parseMoney('＄５０')).toBe(50)
  })

  it('拒絕小數 —— 金額是整數元，收銀員不該打得出角分', () => {
    expect(parseMoney('12.5')).toBeNull()
  })

  it('拒絕垃圾輸入', () => {
    expect(parseMoney('')).toBeNull()
    expect(parseMoney('abc')).toBeNull()
  })
})
