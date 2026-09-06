/**
 * 前端唯一的金額邏輯：顯示格式化。
 *
 * ★ **定價計算不在這裡，也不該在這裡。**
 * 所有金額計算只在 Rust 的 core/pricing 實作一次，前端每次改購物車就
 * invoke('pricing_preview')。本機 Tauri IPC 往返不到 1ms，遠低於人眼可感；
 * 換來的是「螢幕上的金額與資料庫裡的金額由同一份程式碼產生」。
 *
 * 前後端各算一次，是 POS 最常見的客訴來源。
 */

/** 後端傳來的金額一律是**整數元**（見 src-tauri/src/core/money.rs 的 ADR）。 */
export type Money = number

/** 顯示用格式化：1234 -> "$1,234"。 */
export function formatMoney(amount: Money, opts?: { sign?: boolean }): string {
  const sign = opts?.sign && amount > 0 ? '+' : ''
  const abs = Math.abs(amount).toLocaleString('zh-TW')
  return amount < 0 ? `-$${abs}` : `${sign}$${abs}`
}

/** 收銀輸入解析：容忍全形數字、逗號、空白與貨幣符號。 */
export function parseMoney(input: string): Money | null {
  const normalized = input
    .replace(/[０-９]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0xfee0))
    .replace(/[,\s$＄]/g, '')
  if (normalized === '' || !/^-?\d+$/.test(normalized)) return null
  return Number.parseInt(normalized, 10)
}
