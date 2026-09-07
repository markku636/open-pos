import { useState } from 'react'

import { discountApi, type AppError, type DiscountInput, type Order } from '@/shared/api'
import { useT } from '@/shared/i18n'
import { discount } from '@/shared/locales/discount'
import { ui } from '@/shared/locales/nav'
import { formatMoney } from '@/shared/money'

/** 台灣講折扣是「打幾折」，所以按鈕直接寫折數。9000 bp = 9 折。 */
// 但英文與日文寫的是折掉多少（10% off / 10%引），所以字典鍵照折掉的
// 百分比取名 —— 對照 bp 看是「剩下多少」，兩邊差一個減法，別直譯。
const PRESETS = [
  { msg: discount.off10, bp: 9000 },
  { msg: discount.off15, bp: 8500 },
  { msg: discount.off20, bp: 8000 },
  { msg: discount.off25, bp: 7500 },
]

/**
 * 折扣面板。
 *
 * 按鈕寫的是「9 折」而不是「10% off」—— 台灣的說法是打幾折，
 * 而收銀員在客人面前不該需要換算。
 *
 * **招待與折扣是分開的兩顆按鈕**，因為它們在權限上與報表上都是兩件事：
 * 老闆看折扣是看行銷成效，看招待是看有沒有人在送人情。
 */
export default function DiscountDialog({
  order,
  lineId,
  onClose,
  onDone,
}: {
  order: Order
  /** 給了就是單品折扣，沒給就是整單。 */
  lineId?: string
  onClose: () => void
  onDone: (order: Order) => void
}) {
  const t = useT()
  const [amount, setAmount] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const line = lineId ? order.lines.find((l) => l.id === lineId) : undefined
  const target = line
    ? t(discount.targetLine, { name: line.name, amount: formatMoney(line.amount) })
    : t(discount.targetOrder, { amount: formatMoney(order.grandTotal) })

  const apply = async (patch: Pick<DiscountInput, 'kind' | 'value'>) => {
    setBusy(true)
    setError(null)
    try {
      onDone(
        await discountApi.apply({
          orderId: order.id,
          expectedRev: order.rev,
          lineId: lineId ?? null,
          ...patch,
        }),
      )
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60 p-6" onClick={onClose}>
      <div
        className="w-full max-w-sm rounded-lg bg-slate-900 p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-baseline justify-between">
          <h2 className="text-lg font-semibold">
            {t(lineId ? discount.titleLine : discount.titleOrder)}
          </h2>
          <span className="text-sm text-slate-400">{target}</span>
        </div>

        <div className="mt-4 grid grid-cols-2 gap-2">
          {PRESETS.map((p) => (
            <button
              key={p.bp}
              className="rounded bg-slate-800 py-4 text-lg hover:bg-slate-700 disabled:opacity-40"
              disabled={busy}
              onClick={() => void apply({ kind: 'percent', value: p.bp })}
            >
              {t(p.msg)}
            </button>
          ))}
        </div>

        <div className="mt-3 flex gap-2">
          <input
            className="min-w-0 flex-1 rounded bg-slate-800 px-3 py-3 text-right text-lg"
            placeholder={t(discount.amountPlaceholder)}
            inputMode="numeric"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
          />
          <button
            className="rounded bg-slate-800 px-4 hover:bg-slate-700 disabled:opacity-40"
            disabled={busy || !(Number(amount) > 0)}
            onClick={() => void apply({ kind: 'amount', value: Number(amount) })}
          >
            {t(discount.applyAmount)}
          </button>
        </div>

        {/* 招待與折扣分開：報表上它們是兩件事。 */}
        <button
          className="mt-3 w-full rounded bg-amber-900/60 py-3 text-amber-200 hover:bg-amber-800/60 disabled:opacity-40"
          disabled={busy}
          title={t(discount.compTitle)}
          onClick={() => void apply({ kind: 'comp', value: 0 })}
        >
          {t(discount.comp)}
        </button>

        {error && (
          <p className="mt-3 whitespace-pre-line rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
            {error}
          </p>
        )}

        <button
          className="mt-4 w-full rounded bg-slate-800 py-2.5 hover:bg-slate-700"
          onClick={onClose}
        >
          {t(ui.cancel)}
        </button>
      </div>
    </div>
  )
}
