/**
 * 時間顯示。
 *
 * 後端一律用 UTC 的 ISO-8601 字串（固定寬度、字典序即時序、換 PG 免轉換），
 * 所以**任何要給人看的時間都必須經過這裡**。
 *
 * 直接 `iso.slice(11, 16)` 會顯示 UTC：一位台灣店員在晚上十點看到
 * 「開班時間 14:15」，不會想到那是時差，只會以為系統壞了。
 * 這個 bug 每出現一次就要重新解釋一次，所以值得一個模組。
 *
 * 用瀏覽器的時區而不是店家設定的時區：收銀機就放在店裡，兩者一定一致；
 * 而在別台機器上遠端查看時，顯示查看者的當地時間反而才是對的。
 */

/** 壞掉的時間字串顯示成這個，而不是 "Invalid Date"。 */
const UNKNOWN = '—'

function parse(iso: string | null | undefined): Date | null {
  if (!iso) return null
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? null : d
}

/** 只要時分：`14:15`。畫面上最常用的形式。 */
export function hhmm(iso: string | null | undefined): string {
  const d = parse(iso)
  if (!d) return UNKNOWN
  return d.toLocaleTimeString('zh-TW', {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })
}

/** 日期加時分：`09/06 14:15`。跨日的清單（列印紀錄）用它。 */
export function dateTime(iso: string | null | undefined): string {
  const d = parse(iso)
  if (!d) return UNKNOWN
  return `${d.toLocaleDateString('zh-TW', {
    month: '2-digit',
    day: '2-digit',
  })} ${hhmm(iso)}`
}
