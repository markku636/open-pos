import { useCallback, useEffect, useState } from 'react'

import DiscountDialog from './DiscountDialog'
import ItemDialog, { defaultChoice, needsDialog, type Chosen } from './ItemDialog'
import PaymentPanel from './PaymentPanel'
import {
  demoApi,
  discountApi,
  menuApi,
  orderApi,
  type AppError,
  type Channel,
  type DiningTable,
  type Item,
  type MenuTree,
  type Order,
} from '@/shared/api'
import { useT } from '@/shared/i18n'
// 字典取名 `msg`：這個檔案裡的 `order` 已經是「那張單」，
// 兩個東西不能共用一個名字。
import { order as msg } from '@/shared/locales/order'
import { formatMoney } from '@/shared/money'

/** 從桌位圖帶過來的一桌。`guestCount` 只在這一桌還沒開檯時用得到。 */
export interface Seat {
  table: DiningTable
  guestCount: number
}

/**
 * 點餐畫面。
 *
 * 版面是「左菜單、右購物車」—— 收銀員的動作是「按品項 → 看右邊 → 收錢」，
 * 這條動線一路往右，不需要回頭找。
 *
 * 觸控考量：品項按鈕做得大（最小 88px 高），因為收銀員是站著用食指戳，
 * 而且尖峰時間手上還拿著東西。
 *
 * # 桌位
 *
 * 從桌位圖點進來時會帶著一張桌子（`seat`）。內用的加點動線是
 * 「桌位圖 → 點這一桌 → 直接接著點」，所以進來要**自動接上那一桌
 * 已經開著的單**，而不是開一張新的 —— 一桌兩張單在結帳時才會被發現，
 * 那時客人已經站在櫃檯前了。
 */
export default function OrderScreen({
  seat,
  onLeaveSeat,
}: {
  seat?: Seat | null
  onLeaveSeat?: () => void
} = {}) {
  const t = useT()
  const [tree, setTree] = useState<MenuTree | null>(null)
  const [order, setOrder] = useState<Order | null>(null)
  const [channel, setChannel] = useState<Channel>('takeout')
  const [category, setCategory] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [paying, setPaying] = useState(false)
  /** null = 沒開；'' = 整單折扣；其他 = 那一行的折扣。 */
  const [discounting, setDiscounting] = useState<string | null>(null)
  /** 正在選規格／選項的那一個品項。 */
  const [choosing, setChoosing] = useState<Item | null>(null)

  // 從桌位圖帶進來的那一桌。接上它已經開著的單，沒有的話留白等第一個品項
  // 才真的開單 —— 「按了桌子就產生一張空單」會在桌位圖上留下一堆金額 0
  // 的幽靈桌，而那些桌看起來跟真的有客人一模一樣。
  useEffect(() => {
    if (!seat) {
      // 離開桌位時把那一桌的單也從畫面上收走。留著它的話，下一筆外帶的
      // 第一個品項會被加到剛剛那一桌上 —— 而畫面上完全看不出來。
      setOrder((o) => (o?.tableId ? null : o))
      // 通路也退回外帶。留在「內用」的話，下一張單會是一張**不掛在任何
      // 桌上的內用單** —— 它不會出現在桌位圖上，於是沒有人會想起要去收。
      // （沒有桌位的店不受影響：他們從頭到尾不會經過這裡。）
      setChannel('takeout')
      return
    }
    setChannel('dine_in')
    setError(null)
    orderApi
      .listOpen()
      .then((open) => {
        const existing = open.find((o) => o.tableId === seat.table.id)
        setOrder(existing ?? null)
      })
      .catch((e: AppError) => setError(e.message))
  }, [seat])

  /**
   * 還沒結完的單。
   *
   * ★ 沒有這一條，一張分帳收到一半的外帶單就**消失了** —— 它不掛在任何桌上，
   *   購物車是畫面狀態，關掉程式或按一下取消它就不在任何地方了，而錢還沒收完。
   *   內用的單至少還在桌位圖上看得到，外帶的沒有。
   */
  const [openOrders, setOpenOrders] = useState<Order[]>([])
  const refreshOpen = useCallback(() => {
    orderApi
      .listOpen()
      .then(setOpenOrders)
      .catch(() => {
        /* 這一條是輔助資訊，抓不到就不顯示，不要蓋掉正在做的事 */
      })
  }, [])

  useEffect(() => {
    refreshOpen()
    const id = setInterval(refreshOpen, 10_000)
    return () => clearInterval(id)
  }, [refreshOpen, order])

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

  /** 有規格或選項就先問，沒有的就直接進購物車。 */
  const pickItem = (item: Item) => {
    if (tree && needsDialog(item, tree)) {
      setChoosing(item)
      return
    }
    void addItem(item, tree ? defaultChoice(item, tree) : { variantId: null, modifierIds: [] })
  }

  const addItem = async (item: Item, chosen: Chosen) => {
    let target = order
    if (!target || target.status === 'settled') {
      const tableId = channel === 'dine_in' ? (seat?.table.id ?? null) : null
      target = await run(() => orderApi.open(channel, tableId, seat?.guestCount ?? 1))
      if (!target) return
      setOrder(target)
    }
    const updated = await run(() =>
      orderApi.addLines(target!.id, target!.rev, [
        {
          itemId: item.id,
          variantId: chosen.variantId ?? undefined,
          modifierIds: chosen.modifierIds.length > 0 ? chosen.modifierIds : undefined,
        },
      ]),
    )
    if (updated) setOrder(updated)
  }

  const voidLine = async (lineId: string) => {
    if (!order) return
    const updated = await run(() => orderApi.voidLine(order.id, order.rev, lineId))
    if (updated) setOrder(updated)
  }

  /** 一鍵示範資料。空菜單是新使用者看到的第一個畫面，這顆按鈕就放在那裡。 */
  const seedDemo = async () => {
    const r = await run(() => demoApi.seed())
    if (!r) return
    const tree = await menuApi.tree().catch(() => null)
    if (tree) {
      setTree(tree)
      setCategory(tree.categories[0]?.id ?? null)
    }
    if (!r.created) setError(t(msg.demoAlreadySeeded))
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
              {t(msg.uncategorized)}
            </button>
          )}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto">
          {items.length === 0 ? (
            <div className="py-12 text-center text-sm text-slate-600">
              {!tree ? (
                t(msg.loading)
              ) : isEmpty(tree) ? (
                // 全新安裝看到的第一個畫面。空白畫面加一句「請先建立菜單」
                // 會讓多數人在這裡放棄 —— 先讓他們看到這套東西能動。
                <>
                  <p className="mb-1 text-base text-slate-400">{t(msg.emptyMenuTitle)}</p>
                  <p className="mb-5">{t(msg.emptyMenuHint)}</p>
                  <button
                    className="rounded bg-sky-800 px-5 py-3 text-base text-sky-50 hover:bg-sky-700 disabled:opacity-40"
                    disabled={busy}
                    onClick={() => void seedDemo()}
                  >
                    {t(msg.loadDemo)}
                  </button>
                  <p className="mt-3 text-xs text-slate-700">{t(msg.demoNote)}</p>
                </>
              ) : (
                t(msg.emptyCategory)
              )}
            </div>
          ) : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-2">
              {items.map((it) => (
                <button
                  key={it.id}
                  // 最小 104px：收銀員是站著用食指戳，手上可能還拿著東西。
                  // 尺寸往大的方向錯比往小的方向錯好 —— 按錯品項的代價是
                  // 一份做錯的餐，而畫面上多留一點白沒有任何代價。
                  className="flex min-h-[104px] flex-col justify-between rounded bg-slate-900 p-4 text-left transition hover:bg-slate-800 disabled:opacity-40"
                  disabled={busy || !!it.soldOutUntil}
                  onClick={() => pickItem(it)}
                >
                  <span className="text-base leading-snug">{it.name}</span>
                  <span className="mt-2 text-xl font-semibold text-sky-300">
                    {formatMoney(it.basePrice)}
                  </span>
                  {it.soldOutUntil && (
                    <span className="text-xs text-amber-400">{t(msg.soldOut)}</span>
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
          {seat ? (
            // 桌號要一直在畫面上。「點到別桌去」是內用最貴的錯誤 ——
            // 它會讓兩桌的帳同時錯掉，而且通常在結帳時才被發現。
            <button
              className="flex items-baseline gap-2 rounded bg-emerald-900/50 px-3 py-1.5 text-sm hover:bg-emerald-900/80"
              title={t(msg.leaveTable)}
              onClick={() => onLeaveSeat?.()}
            >
              <span className="text-base font-semibold">{seat.table.code}</span>
              <span className="text-xs text-emerald-300/70">
                {t(msg.guests, { n: order?.guestCount ?? seat.guestCount })}
              </span>
              <span className="text-xs text-slate-500">✕</span>
            </button>
          ) : (
            (['dine_in', 'takeout'] as Channel[]).map((c) => (
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
                {c === 'dine_in' ? t(msg.dineIn) : t(msg.takeout)}
              </button>
            ))
          )}
          <span className="ml-auto font-mono text-xs text-slate-500">
            {order?.orderNo ?? t(msg.noOrderYet)}
          </span>
        </div>

        <OpenOrders
          orders={openOrders.filter((o) => o.id !== order?.id)}
          onPick={(o) => {
            setOrder(o)
            setError(null)
          }}
        />

        {error && (
          <div className="m-3 rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
            {error}
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          {!order || order.lines.length === 0 ? (
            <p className="py-8 text-center text-sm text-slate-600">
              {seat ? t(msg.cartEmptySeat, { code: seat.table.code }) : t(msg.cartEmpty)}
            </p>
          ) : (
            order.lines.map((l) => (
              <div key={l.id} className="group mb-1 flex items-start gap-2 text-sm">
                <span className="w-8 shrink-0 text-slate-500">
                  {l.qtyMilli % 1000 === 0 ? l.qtyMilli / 1000 : (l.qtyMilli / 1000).toFixed(1)}
                </span>
                <span className="min-w-0 flex-1">
                  {l.name}
                  {l.variantName && (
                    <span className="text-slate-400">
                      {t(msg.variantParen, { name: l.variantName })}
                    </span>
                  )}
                  {l.options.length > 0 && (
                    <span className="block text-xs text-slate-500">
                      {l.options.join(t(msg.listSeparator))}
                    </span>
                  )}
                </span>
                <span className="w-16 shrink-0 text-right">{formatMoney(l.amount)}</span>
                <button
                  className="shrink-0 px-1 text-xs text-slate-600 opacity-0 transition group-hover:opacity-100 hover:text-amber-300"
                  disabled={busy}
                  title={t(msg.lineDiscount)}
                  onClick={() => setDiscounting(l.id)}
                >
                  %
                </button>
                <button
                  className="shrink-0 px-1 text-xs text-slate-600 opacity-0 transition group-hover:opacity-100 hover:text-red-400"
                  disabled={busy}
                  title={t(msg.voidLine)}
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
            <Row label={t(msg.subtotal)} value={order.subtotal} />
            {order.serviceCharge > 0 && (
              <Row label={t(msg.serviceCharge)} value={order.serviceCharge} />
            )}
            {order.roundingAdjustment !== 0 && (
              <Row label={t(msg.rounding)} value={order.roundingAdjustment} />
            )}
            <div className="mt-2 flex items-baseline justify-between border-t border-slate-800 pt-2">
              <span className="text-slate-400">{t(msg.total)}</span>
              <span className="text-2xl font-semibold">
                {formatMoney(order.grandTotal)}
              </span>
            </div>
            <p className="mt-1 text-right text-xs text-slate-600">
              {t(msg.taxLine, {
                net: formatMoney(order.salesAmount),
                tax: formatMoney(order.taxAmount),
              })}
            </p>

            {/* 低消沒到要在**按下結帳之前**看見 —— 結完帳才發現就只剩兩條路：
                跟客人重開一次，或是店家自己吞。所以它放在結帳鈕正上方。
                而它不擋結帳：要不要通融是店長的判斷，不是軟體的。 */}
            {order.minChargeShortfall > 0 && (
              <p className="mt-2 rounded bg-amber-950/40 px-2 py-1.5 text-center text-sm text-amber-300">
                {t(msg.minChargeShort, {
                  amount: formatMoney(order.minChargeShortfall),
                  n: order.guestCount,
                  need: formatMoney(order.subtotal + order.minChargeShortfall),
                })}
              </p>
            )}

            {/* 分帳收到一半的單長得跟一般的單一模一樣 —— 除非把它寫出來。 */}
            {order.billedTotal > 0 && order.billedTotal < order.grandTotal && (
              <p className="mt-2 rounded bg-amber-950/40 px-2 py-1.5 text-center text-sm text-amber-300">
                {t(msg.billedParts, {
                  n: order.billCount,
                  amount: formatMoney(order.billedTotal),
                })}
                <span className="ml-2 font-semibold">
                  {t(msg.short, { amount: formatMoney(order.grandTotal - order.billedTotal) })}
                </span>
              </p>
            )}

            <div className="mt-3 flex gap-2">
              <button
                className="flex-1 rounded bg-slate-800 py-2.5 text-sm hover:bg-slate-700 disabled:opacity-40"
                disabled={busy || order.status === 'settled'}
                onClick={() => setDiscounting('')}
              >
                {t(msg.discountOrder)}
              </button>
              <button
                className="flex-1 rounded bg-slate-800 py-2.5 text-sm text-slate-400 hover:bg-red-900/60 hover:text-red-200 disabled:opacity-40"
                disabled={busy}
                title={t(msg.voidOrderTitle)}
                onClick={() => {
                  if (!confirm(t(msg.voidOrderConfirm))) return
                  void run(() => discountApi.voidOrder(order.id, order.rev)).then((r) => {
                    if (r) {
                      setOrder(null)
                      onLeaveSeat?.()
                    }
                  })
                }}
              >
                {t(msg.voidOrder)}
              </button>
            </div>

            <button
              className="mt-2 w-full rounded bg-emerald-700 py-3 text-lg font-semibold hover:bg-emerald-600 disabled:opacity-40"
              disabled={busy || order.status === 'settled'}
              onClick={() => setPaying(true)}
            >
              {order.billedTotal > 0
                ? t(msg.payRemaining, {
                    amount: formatMoney(order.grandTotal - order.billedTotal),
                  })
                : t(msg.checkout)}
            </button>
          </div>
        )}
      </aside>

      {choosing && tree && (
        <ItemDialog
          item={choosing}
          tree={tree}
          onCancel={() => setChoosing(null)}
          onAdd={(chosen) => {
            const it = choosing
            setChoosing(null)
            void addItem(it, chosen)
          }}
        />
      )}

      {discounting !== null && order && (
        <DiscountDialog
          order={order}
          lineId={discounting || undefined}
          onClose={() => setDiscounting(null)}
          onDone={(updated) => {
            setOrder(updated)
            setDiscounting(null)
            setError(null)
          }}
        />
      )}

      {paying && order && (
        <PaymentPanel
          order={order}
          onCancel={() => setPaying(false)}
          onSettled={(result) => {
            setPaying(false)
            setError(null)

            // ★ 分帳還沒收完就不能收單。
            //
            //   把單清掉、把桌放掉是「結完帳」的動作，而這裡只是收了其中一份。
            //   收完第一份就讓畫面回到空白，是這個功能最容易犯也最貴的錯：
            //   剩下的錢會在沒有人記得的情況下走出店門。
            if (result.remaining > 0) {
              setOrder(result.order)
              setError(
                t(msg.splitPaid, { n: result.splitIndex, no: result.billNo }) +
                  (result.change > 0
                    ? t(msg.changeSuffix, { amount: formatMoney(result.change) })
                    : '') +
                  t(msg.stillShortSuffix, { amount: formatMoney(result.remaining) }),
              )
              return
            }

            setOrder(null)
            // 結完帳就離開這一桌，否則下一位客人的第一個品項會被加到
            // 剛剛那一桌上。
            onLeaveSeat?.()
            // 找零要停留在畫面上讓收銀員數錢，不要一閃而過。
            if (result.change > 0) {
              setError(
                t(msg.settled, {
                  no: result.billNo,
                  amount: formatMoney(result.change),
                }),
              )
            }
          }}
        />
      )}
    </div>
  )
}

/**
 * 還沒結完的單。
 *
 * 分帳收到一半的單排在最前面而且是琥珀色：**它是唯一一種「錢已經收了一部分、
 * 剩下的可能被忘掉」的狀態**，而忘掉的代價是客人走出店門。
 */
function OpenOrders({ orders, onPick }: { orders: Order[]; onPick: (o: Order) => void }) {
  const t = useT()
  if (orders.length === 0) return null
  const sorted = [...orders].sort((a, b) => (b.billedTotal > 0 ? 1 : 0) - (a.billedTotal > 0 ? 1 : 0))
  return (
    <div className="flex flex-wrap gap-1 border-b border-slate-800 px-3 py-2">
      <span className="mr-1 self-center text-xs text-slate-600">{t(msg.openOrders)}</span>
      {sorted.map((o) => {
        const partial = o.billedTotal > 0
        return (
          <button
            key={o.id}
            className={`rounded px-2 py-1 text-xs ${
              partial
                ? 'bg-amber-900/60 text-amber-100 hover:bg-amber-800/60'
                : 'bg-slate-800 text-slate-300 hover:bg-slate-700'
            }`}
            title={partial ? t(msg.openPartialTitle) : t(msg.openOrderTitle)}
            onClick={() => onPick(o)}
          >
            {o.tableLabel ?? o.orderNo.split('-').pop()}
            <span className="ml-1.5 font-mono">
              {formatMoney(partial ? o.grandTotal - o.billedTotal : o.grandTotal)}
            </span>
            {partial && <span className="ml-1">{t(msg.openPartialTag)}</span>}
          </button>
        )
      })}
    </div>
  )
}

/** 整份菜單是不是空的（不只是這一類空）。 */
function isEmpty(tree: MenuTree): boolean {
  return tree.uncategorized.length === 0 && tree.categories.every((c) => c.items.length === 0)
}

function Row({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex justify-between text-sm text-slate-400">
      <span>{label}</span>
      <span>{formatMoney(value)}</span>
    </div>
  )
}
