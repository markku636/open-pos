import { describe, expect, it } from 'vitest'

import { LOCALES } from '@/shared/i18n'
import { gwField, gwFieldHint, gwProvider, gwProviderNote } from './gatewayFields'

/**
 * 這幾條測試接手了原本在 Rust 那邊的保證。
 *
 * 憑證欄位的標籤與說明從 `services/gateway/config.rs` 搬到前端字典之後，
 * 那邊的 `every_provider_says_where_to_get_its_credentials` 就守不到文案了。
 * 保證沒有不見，它搬到這裡 —— 因為文案搬到這裡。
 */

/** 後端 `fields_for()` 宣告的欄位鍵。改後端就要改這裡，反之亦然。 */
const BACKEND_FIELD_KEYS = {
  linepay: ['channel_id', 'channel_secret'],
  newebpay: ['merchant_id', 'hash_key', 'hash_iv'],
  manual: [],
} as const

describe('金流憑證的文案', () => {
  it('每一個後端會回的欄位鍵，字典裡都查得到標籤與說明', () => {
    // 對不上的症狀不是壞掉，是欄位標籤變成一串 hash_iv ——
    // 很醜，而且沒有人會回報。
    for (const keys of Object.values(BACKEND_FIELD_KEYS)) {
      for (const key of keys) {
        expect(key in gwField, `缺欄位標籤：${key}`).toBe(true)
        expect(key in gwFieldHint, `缺欄位說明：${key}`).toBe(true)
      }
    }
  })

  it('字典裡沒有後端不會回的多餘欄位', () => {
    const known: Set<string> = new Set(Object.values(BACKEND_FIELD_KEYS).flat())
    for (const key of Object.keys(gwField)) {
      expect(known.has(key), `${key} 後端不會回，是不是拼錯或已經拿掉了？`).toBe(true)
    }
  })

  it('每一則說明都自己講得完整，不依賴旁邊那一欄', () => {
    // 我第一版寫過「跟上面那個同一頁」。欄位順序一改它就是錯的，
    // 而且畫面上看起來完全正常。
    const LEANS_ON_NEIGHBOURS = [
      '同一頁',
      '同上',
      '上面那',
      'same as above',
      'as above',
      '同じページ',
      '上記と同じ',
    ]
    for (const [key, msg] of Object.entries(gwFieldHint)) {
      for (const locale of LOCALES) {
        const text = msg[locale]
        for (const bad of LEANS_ON_NEIGHBOURS) {
          expect(
            text.toLowerCase().includes(bad.toLowerCase()),
            `${key} 的 ${locale} 說明靠旁邊那一欄才讀得懂：${text}`,
          ).toBe(false)
        }
      }
    }
  })

  it('每一則說明都說得出「去哪裡拿」', () => {
    // 一個只寫著「Channel Secret」的欄位，對沒串接過的店家等於沒有寫。
    const POINTS_SOMEWHERE = ['後台', 'console', '管理画面', '設定', 'settings']
    for (const [key, msg] of Object.entries(gwFieldHint)) {
      for (const locale of LOCALES) {
        const text = msg[locale].toLowerCase()
        expect(
          POINTS_SOMEWHERE.some((w) => text.includes(w.toLowerCase())),
          `${key} 的 ${locale} 說明沒指出去哪裡拿：${msg[locale]}`,
        ).toBe(true)
      }
    }
  })

  it('三種語言都齊，而且不是三次中文', () => {
    for (const [name, cat] of Object.entries({
      gwProvider,
      gwProviderNote,
      gwField,
      gwFieldHint,
    })) {
      for (const [key, msg] of Object.entries(cat)) {
        for (const locale of LOCALES) {
          expect(msg[locale]?.trim(), `${name}.${key} 缺 ${locale}`).toBeTruthy()
        }
      }
    }
    // 說明類的三種語言必須真的不一樣（產品名可以一樣，例如 LINE Pay）。
    for (const [key, msg] of Object.entries(gwFieldHint)) {
      expect(msg.en, `${key} 的英文跟中文一樣，應該是漏翻了`).not.toBe(msg['zh-TW'])
      expect(msg.ja, `${key} 的日文跟中文一樣，應該是漏翻了`).not.toBe(msg['zh-TW'])
    }
  })
})
