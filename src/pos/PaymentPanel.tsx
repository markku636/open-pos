import { useEffect, useMemo, useState } from 'react'

import {
  orderApi,
  type AppError,
  type Order,
  type PaymentInput,
  type PaymentMethod,
  type SettleResult,
} from '@/shared/api'
import { formatMoney, parseMoney } from '@/shared/money'

/**
 * 結帳面板。
 *
 * 現金是最常見也最容易按錯的一種，所以給它最大的操作面積：
 * 快捷鈔票鍵（100 / 500 / 1000）加上直接輸入。找零算出來之後停在畫面上，
 * 收銀員要照著數錢給客人。
 *
 * 付款方式從後端拿，不寫死在前端 —— 店家停用「悠遊卡」之後按鈕就該消失，
 * 而不是按下去才發現不能用。
 */
export default function PaymentPanel({
  order,
  onCancel,
  onSettled,
}: {
  order: Order
  onCancel: () => void
  onSettled: (result: SettleResult) => void
}) {
  const [methods, setMethods] = useState<PaymentMethod[]>([])
  const [selected, setSelected] = useState<string>('cash')
  const [tendered, setTendered] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    orderApi
      .paymentMethods()
      .then((m) => {
        setMethods(m)
        if (m.length > 0 && !m.some((x) => x.code === 'cash')) setSelected(m[0].code)
      })
      .catch((e: AppError) => setError(e.message))
  }, [])

  const method = methods.find((m) => m.code === selected)
  const given = parseMoney(tendered)
  const change = useMemo(() => {
    if (!method?.allowsChange || given === null) return 0
    return Math.max(0, given - order.grandTotal)
  }, [method, given, order.grandTotal])

  const canSettle =
    !busy &&
    !!method &&
    (method.allowsChange
      ? given !== null && given >= order.grandTotal
      : given === null || given === order.grandTotal)

  const settle = async () => {
    if (!method) return
    setBusy(true)
    try {
      const payment: PaymentInput = method.allowsChange
        ? // 現金：把客人遞出來的金額原樣送出去，找零由後端算 ——
          // 前端算一次、後端再算一次是收銀系統對帳不上的經典原因。
          { methodCode: method.code, amount: given ?? order.grandTotal, tendered: given }
        : { methodCode: method.code, amount: order.grandTotal }
      const r = await orderApi.settle(order.id, order.rev, [payment])
      onSettled(r)
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60 p-6">
      <div className="w-full max-w-md rounded-lg bg-slate-900 p-5 shadow-xl">
        <div className="flex items-baseline justify-between">
          <h2 className="text-lg font-semibold">結帳</h2>
          <span className="font-mono text-xs text-slate-500">{order.orderNo}</span>
        </div>

        <div className="my-4 flex items-baseline justify-between border-y border-slate-800 py-3">
          <span className="text-slate-400">應收</span>
          <span className="text-3xl font-semibold text-emerald-300">
            {formatMoney(order.grandTotal)}
          </span>
        </div>

        <div className="mb-3 flex flex-wrap gap-1">
          {methods.map((m) => (
            <button
              key={m.code}
              className={`rounded px-3 py-2 text-sm ${
                selected === m.code
                  ? 'bg-sky-800 text-sky-50'
                  : 'bg-slate-800 text-slate-300 hover:bg-slate-700'
              }`}
              onClick={() => {
                setSelected(m.code)
                setTendered('')
                setError(null)
              }}
            >
              {m.name}
            </button>
          ))}
        </div>

        {method?.allowsChange ? (
          <>
            <div className="mb-2 flex gap-1">
              {[100, 500, 1000].map((n) => (
                <button
                  key={n}
                  className="flex-1 rounded bg-slate-800 py-3 text-sm hover:bg-slate-700"
                  onClick={() => setTendered(String(n))}
                >
                  {n}
                </button>
              ))}
              <button
                className="flex-1 rounded bg-slate-800 py-3 text-sm hover:bg-slate-700"
                title="客人剛好給整數"
                onClick={() => setTendered(String(order.grandTotal))}
              >
                剛好
              </button>
            </div>
            <input
              className="w-full rounded bg-slate-800 px-3 py-3 text-right text-2xl"
              placeholder="客人給多少"
              inputMode="numeric"
              autoFocus
              value={tendered}
              onChange={(e) => setTendered(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && canSettle && void settle()}
            />
            <div className="mt-3 flex items-baseline justify-between">
              <span className="text-slate-400">找零</span>
              <span
                className={`text-3xl font-semibold ${
                  change > 0 ? 'text-amber-300' : 'text-slate-600'
                }`}
              >
                {formatMoney(change)}
              </span>
            </div>
          </>
        ) : (
          <p className="rounded bg-slate-800/60 px-3 py-4 text-center text-sm text-slate-400">
            {method ? `以「${method.name}」收取 ${formatMoney(order.grandTotal)}` : '請選付款方式'}
            <span className="mt-1 block text-xs text-slate-600">這種付款方式不能找零</span>
          </p>
        )}

        {error && (
          <p className="mt-3 rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
            {error}
          </p>
        )}

        <div className="mt-5 flex gap-2">
          <button
            className="flex-1 rounded bg-slate-800 py-3 hover:bg-slate-700"
            disabled={busy}
            onClick={onCancel}
          >
            取消
          </button>
          <button
            className="flex-[2] rounded bg-emerald-700 py-3 text-lg font-semibold hover:bg-emerald-600 disabled:opacity-40"
            disabled={!canSettle}
            onClick={() => void settle()}
          >
            確認收款
          </button>
        </div>
      </div>
    </div>
  )
}
