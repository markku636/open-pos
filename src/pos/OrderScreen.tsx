import { useCallback, useEffect, useState } from 'react'

import PaymentPanel from './PaymentPanel'
import {
  menuApi,
  orderApi,
  type AppError,
  type Channel,
  type Item,
  type MenuTree,
  type Order,
} from '@/shared/api'
import { formatMoney } from '@/shared/money'

/**
 * 點餐畫面。
 *
 * 版面是「左菜單、右購物車」—— 收銀員的動作是「按品項 → 看右邊 → 收錢」，
 * 這條動線一路往右，不需要回頭找。
 *
 * 觸控考量：品項按鈕做得大（最小 88px 高），因為收銀員是站著用食指戳，
 * 而且尖峰時間手上還拿著東西。
 */
export default function OrderScreen() {
  const [tree, setTree] = useState<MenuTree | null>(null)
  const [order, setOrder] = useState<Order | null>(null)
  const [channel, setChannel] = useState<Channel>('takeout')
  const [category, setCategory] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [paying, setPaying] = useState(false)

  useEffect(() => {
    menuApi
      .tree()
      .then((t) => {
        setTree(t)
        setCategory((c) => c ?? t.categories[0]?.id ?? null)
      })
      .catch((e: AppError) => setError(e.message))
  }, [])

  const run = useCallback(async <T,>(fn: () => Promise<T>): Promise<T | null> => {
    setBusy(true)
    try {
      const r = await fn()
      setError(null)
      return r
    } catch (e) {
      const err = e as AppError
      setError(err.message ?? String(e))
      // 樂觀鎖衝突：別人剛改過這張單，重讀一次讓畫面回到真實狀態。
      // 這是 409 唯一正確的處理方式 —— 重試前必須先看到別人做了什麼。
      if (err.code === 'ERR_CONFLICT' && order) {
        const fresh = await orderApi.get(order.id).catch(() => null)
        if (fresh) setOrder(fresh)
      }
      return null
    } finally {
      setBusy(false)
    }
  }, [order])

  const addItem = async (item: Item) => {
    let target = order
    if (!target || target.status === 'settled') {
      target = await run(() => orderApi.open(channel))
      if (!target) return
      setOrder(target)
    }
    const updated = await run(() =>
      orderApi.addLines(target!.id, target!.rev, [{ itemId: item.id }]),
    )
    if (updated) setOrder(updated)
  }

  const voidLine = async (lineId: string) => {
    if (!order) return
    const updated = await run(() => orderApi.voidLine(order.id, order.rev, lineId))
    if (updated) setOrder(updated)
  }

  const items: Item[] =
    category === 'uncategorized'
      ? (tree?.uncategorized ?? [])
      : (tree?.categories.find((c) => c.id === category)?.items ?? [])

  return (
    <div className="flex h-full min-h-0 gap-4">
      {/* 左：菜單 */}
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="mb-3 flex flex-wrap gap-1">
          {tree?.categories.map((c) => (
            <button
              key={c.id}
              className={`rounded px-3 py-2 text-sm ${
                category === c.id
                  ? 'bg-sky-800 text-sky-50'
                  : 'bg-slate-900 text-slate-300 hover:bg-slate-800'
              }`}
              onClick={() => setCategory(c.id)}
            >
              {c.name}
            </button>
          ))}
          {tree && tree.uncategorized.length > 0 && (
            <button
              className={`rounded px-3 py-2 text-sm ${
                category === 'uncategorized'
                  ? 'bg-amber-800 text-amber-50'
                  : 'bg-slate-900 text-amber-300/80 hover:bg-slate-800'
              }`}
              onClick={() => setCategory('uncategorized')}
            >
              未分類
            </button>
          )}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto">
          {items.length === 0 ? (
            <p className="py-12 text-center text-sm text-slate-600">
              {tree ? '這一類還沒有商品 —— 請先到「商品維護」建立菜單' : '載入中…'}
            </p>
          ) : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
              {items.map((it) => (
                <button
                  key={it.id}
                  // 最小 88px：收銀員是站著用食指戳，手上可能還拿著東西。
                  className="flex min-h-[88px] flex-col justify-between rounded bg-slate-900 p-3 text-left transition hover:bg-slate-800 disabled:opacity-40"
                  disabled={busy || !!it.soldOutUntil}
                  onClick={() => void addItem(it)}
                >
                  <span className="text-sm leading-snug">{it.name}</span>
                  <span className="mt-2 text-lg font-semibold text-sky-300">
                    {formatMoney(it.basePrice)}
                  </span>
                  {it.soldOutUntil && (
                    <span className="text-xs text-amber-400">已售完</span>
                  )}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* 右：購物車 */}
      <aside className="flex w-96 shrink-0 flex-col rounded bg-slate-900/50">
        <div className="flex items-center gap-1 border-b border-slate-800 p-3">
          {(['dine_in', 'takeout'] as Channel[]).map((c) => (
            <button
              key={c}
              className={`rounded px-3 py-1.5 text-sm ${
                channel === c ? 'bg-slate-700' : 'text-slate-400 hover:text-slate-200'
              }`}
              // 已經開單之後不讓改通路：服務費與價格都跟著它，
              // 中途改會讓已送廚房的品項金額跳動。
              disabled={!!order && order.lines.length > 0}
              onClick={() => setChannel(c)}
            >
              {c === 'dine_in' ? '內用' : '外帶'}
            </button>
          ))}
          <span className="ml-auto font-mono text-xs text-slate-500">
            {order?.orderNo ?? '尚未開單'}
          </span>
        </div>

        {error && (
          <div className="m-3 rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
            {error}
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          {!order || order.lines.length === 0 ? (
            <p className="py-8 text-center text-sm text-slate-600">點左邊的商品開始</p>
          ) : (
            order.lines.map((l) => (
              <div key={l.id} className="group mb-1 flex items-start gap-2 text-sm">
                <span className="w-8 shrink-0 text-slate-500">
                  {l.qtyMilli % 1000 === 0 ? l.qtyMilli / 1000 : (l.qtyMilli / 1000).toFixed(1)}
                </span>
                <span className="min-w-0 flex-1">
                  {l.name}
                  {l.variantName && (
                    <span className="text-slate-400">（{l.variantName}）</span>
                  )}
                  {l.options.length > 0 && (
                    <span className="block text-xs text-slate-500">
                      {l.options.join('、')}
                    </span>
                  )}
                </span>
                <span className="w-16 shrink-0 text-right">{formatMoney(l.amount)}</span>
                <button
                  className="shrink-0 px-1 text-xs text-slate-600 opacity-0 transition group-hover:opacity-100 hover:text-red-400"
                  disabled={busy}
                  title="退掉這一項（會通知廚房）"
                  onClick={() => void voidLine(l.id)}
                >
                  ✕
                </button>
              </div>
            ))
          )}
        </div>

        {order && order.lines.length > 0 && (
          <div className="border-t border-slate-800 p-3">
            <Row label="小計" value={order.subtotal} />
            {order.serviceCharge > 0 && (
              <Row label="服務費" value={order.serviceCharge} />
            )}
            {order.roundingAdjustment !== 0 && (
              <Row label="進位調整" value={order.roundingAdjustment} />
            )}
            <div className="mt-2 flex items-baseline justify-between border-t border-slate-800 pt-2">
              <span className="text-slate-400">合計</span>
              <span className="text-2xl font-semibold">
                {formatMoney(order.grandTotal)}
              </span>
            </div>
            <p className="mt-1 text-right text-xs text-slate-600">
              未稅 {formatMoney(order.salesAmount)}　稅 {formatMoney(order.taxAmount)}
            </p>

            <button
              className="mt-3 w-full rounded bg-emerald-700 py-3 text-lg font-semibold hover:bg-emerald-600 disabled:opacity-40"
              disabled={busy || order.status === 'settled'}
              onClick={() => setPaying(true)}
            >
              結帳
            </button>
          </div>
        )}
      </aside>

      {paying && order && (
        <PaymentPanel
          order={order}
          onCancel={() => setPaying(false)}
          onSettled={(result) => {
            setPaying(false)
            setOrder(null)
            setError(null)
            // 找零要停留在畫面上讓收銀員數錢，不要一閃而過。
            if (result.change > 0) {
              setError(`已結帳 ${result.billNo}　找零 ${formatMoney(result.change)}`)
            }
          }}
        />
      )}
    </div>
  )
}

function Row({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex justify-between text-sm text-slate-400">
      <span>{label}</span>
      <span>{formatMoney(value)}</span>
    </div>
  )
}
