import { useEffect, useMemo, useState } from 'react'

import {
  orderApi,
  type AppError,
  type Order,
  type PaymentInput,
  type PaymentMethod,
  type SettleResult,
  type SplitPreview,
  type SplitReq,
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
 *
 * # 分帳
 *
 * 四個模式對應櫃檯真的會聽到的四句話：整單付、我們平分、我先出 500、
 * 我的只有那碗麵。**應收金額一律由 Rust 算**（`previewSplit`），前端只負責顯示 ——
 * 「四個人分 101 元」的答案是 26/25/25/25，在這裡再實作一次同一套進位規則，
 * 就是在等兩邊哪天不一樣。
 */
type Mode = 'full' | 'even' | 'amount' | 'items'

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

  const [mode, setMode] = useState<Mode>('full')
  const [parts, setParts] = useState(2)
  const [partAmount, setPartAmount] = useState('')
  const [picked, setPicked] = useState<string[]>([])
  const [preview, setPreview] = useState<SplitPreview | null>(null)

  useEffect(() => {
    orderApi
      .paymentMethods()
      .then((m) => {
        setMethods(m)
        if (m.length > 0 && !m.some((x) => x.code === 'cash')) setSelected(m[0].code)
      })
      .catch((e: AppError) => setError(e.message))
  }, [])

  // 分帳到一半又開起來的話，直接回到原本那個模式與份數。
  //
  // 混著分、或中途改份數都會被伺服器擋下來，而讓收銀員先按了才知道不行是
  // 最差的做法 —— 系統明明知道當初說好幾份，卻要人自己記。
  useEffect(() => {
    if (order.splitMode === 'even') setMode('even')
    else if (order.splitMode === 'by_item') setMode('items')
    else if (order.splitMode === 'by_amount') setMode('amount')
    if (order.splitCount && order.splitCount > 1) setParts(order.splitCount)
  }, [order.splitMode, order.splitCount])

  const split = useSplitReq(mode, parts, partAmount, picked)

  // 應收金額每次都問後端。這一趟是本機 IPC，遠比一個對不起來的收銀系統便宜。
  useEffect(() => {
    let live = true
    orderApi
      .previewSplit(order.id, split)
      .then((p) => {
        if (!live) return
        setPreview(p)
        setError(null)
      })
      .catch((e: AppError) => {
        if (!live) return
        setPreview(null)
        setError(e.message)
      })
    return () => {
      live = false
    }
  }, [order.id, split])

  const due = preview?.due ?? 0
  const method = methods.find((m) => m.code === selected)
  const given = parseMoney(tendered)
  const change = useMemo(() => {
    if (!method?.allowsChange || given === null) return 0
    return Math.max(0, given - due)
  }, [method, given, due])

  const canSettle =
    !busy &&
    !!method &&
    !!preview &&
    due > 0 &&
    (method.allowsChange ? given !== null && given >= due : given === null || given === due)

  const settle = async () => {
    if (!method || !preview) return
    setBusy(true)
    try {
      const payment: PaymentInput = method.allowsChange
        ? // 現金：把客人遞出來的金額原樣送出去，找零由後端算 ——
          // 前端算一次、後端再算一次是收銀系統對帳不上的經典原因。
          { methodCode: method.code, amount: given ?? due, tendered: given }
        : { methodCode: method.code, amount: due }
      const r = await orderApi.settle(order.id, order.rev, [payment], split ?? undefined)
      onSettled(r)
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60 p-6">
      <div className="max-h-full w-full max-w-md overflow-y-auto rounded-lg bg-slate-900 p-5 shadow-xl">
        <div className="flex items-baseline justify-between">
          <h2 className="text-lg font-semibold">結帳</h2>
          <span className="font-mono text-xs text-slate-500">{order.orderNo}</span>
        </div>

        <SplitPicker
          order={order}
          mode={mode}
          onMode={setMode}
          parts={parts}
          onParts={setParts}
          amount={partAmount}
          onAmount={setPartAmount}
          picked={picked}
          onPicked={setPicked}
          preview={preview}
        />

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
              }}
            >
              {m.name}
            </button>
          ))}
        </div>

        {method?.allowsChange ? (
          <TenderPad
            due={due}
            tendered={tendered}
            onTendered={setTendered}
            change={change}
            onEnter={() => canSettle && void settle()}
          />
        ) : (
          <p className="rounded bg-slate-800/60 px-3 py-4 text-center text-sm text-slate-400">
            {method ? `以「${method.name}」收取 ${formatMoney(due)}` : '請選付款方式'}
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

/**
 * 目前這一份要送給後端的分帳規格。
 *
 * 回傳 null 代表「整單／收尾款」—— 後端看到 null 就是把還沒結的全部結掉。
 * 用 useMemo 是必要的：它是試算那支 effect 的依賴，每次 render 造一個新物件
 * 會讓試算無限重打。
 */
function useSplitReq(mode: Mode, parts: number, amount: string, picked: string[]): SplitReq | null {
  const pickedKey = picked.join(',')
  return useMemo(() => {
    if (mode === 'even') return { mode: 'even', parts }
    if (mode === 'amount') {
      const n = parseMoney(amount)
      return n !== null && n > 0 ? { mode: 'amount', amount: n } : null
    }
    if (mode === 'items') {
      const ids = pickedKey ? pickedKey.split(',') : []
      return ids.length > 0 ? { mode: 'items', lineIds: ids } : null
    }
    return null
  }, [mode, parts, amount, pickedKey])
}

/**
 * 分帳的模式選擇與應收試算。
 *
 * 這一段自己一個元件，是因為它有自己的一整套狀態語意（怎麼分、分幾份、
 * 選了哪幾行），而結帳面板其餘部分只在乎最後那一個「應收多少」。
 */
function SplitPicker({
  order,
  mode,
  onMode,
  parts,
  onParts,
  amount,
  onAmount,
  picked,
  onPicked,
  preview,
}: {
  order: Order
  mode: Mode
  onMode: (m: Mode) => void
  parts: number
  onParts: (n: number) => void
  amount: string
  onAmount: (v: string) => void
  picked: string[]
  onPicked: (f: (p: string[]) => string[]) => void
  preview: SplitPreview | null
}) {
  const due = preview?.due ?? 0
  // 已經分過的單不能改分法，所以那些頁籤也不該還能按。
  const started = order.billCount > 0 && !!order.splitMode
  const locked = order.splitMode === 'even' && order.billCount > 0
  const tab = (m: Mode) => !started || m === 'full' || m === modeOf(order.splitMode)
  return (
    <>
      <div className="mt-3 flex flex-wrap gap-1">
        <ModeTab active={mode === 'full'} onClick={() => onMode('full')}>
          {order.billCount > 0 ? '收尾款' : '整單'}
        </ModeTab>
        <ModeTab active={mode === 'even'} hidden={!tab('even')} onClick={() => onMode('even')}>
          平分
        </ModeTab>
        <ModeTab active={mode === 'amount'} hidden={!tab('amount')} onClick={() => onMode('amount')}>
          指定金額
        </ModeTab>
        <ModeTab active={mode === 'items'} hidden={!tab('items')} onClick={() => onMode('items')}>
          分項
        </ModeTab>
      </div>

      {mode === 'even' &&
        // 已經開始分了就把份數鎖住 —— 它不是還能選的東西，是已經發生的事。
        (locked ? (
          <p className="mt-2 rounded bg-slate-800/60 px-3 py-2.5 text-center text-sm text-slate-400">
            這張單分 <span className="text-lg font-semibold text-slate-100">{parts}</span> 份，
            已經收了 {order.billCount} 份
          </p>
        ) : (
          <div className="mt-2 flex gap-1">
            {[2, 3, 4, 5, 6, 8].map((n) => (
              <button
                key={n}
                className={`flex-1 rounded py-2.5 text-base ${
                  parts === n ? 'bg-sky-800 text-sky-50' : 'bg-slate-800 hover:bg-slate-700'
                }`}
                onClick={() => onParts(n)}
              >
                {n}
              </button>
            ))}
          </div>
        ))}

      {mode === 'amount' && (
        <input
          className="mt-2 w-full rounded bg-slate-800 px-3 py-2.5 text-right text-xl"
          placeholder="這一份收多少"
          inputMode="numeric"
          value={amount}
          onChange={(e) => onAmount(e.target.value)}
        />
      )}

      {mode === 'items' && (
        <div className="mt-2 max-h-44 overflow-y-auto rounded bg-slate-950/40 p-1">
          {order.lines.map((l) => (
            <label
              key={l.id}
              className={`flex items-center gap-2 rounded px-2 py-1.5 text-sm ${
                l.paid ? 'text-slate-600 line-through' : 'hover:bg-slate-800'
              }`}
            >
              <input
                type="checkbox"
                disabled={l.paid}
                checked={picked.includes(l.id)}
                onChange={(e) =>
                  onPicked((p) => (e.target.checked ? [...p, l.id] : p.filter((x) => x !== l.id)))
                }
              />
              <span className="flex-1">
                {l.name}
                {l.variantName && <span className="text-slate-500">（{l.variantName}）</span>}
              </span>
              <span className="font-mono">{formatMoney(l.amount)}</span>
            </label>
          ))}
          {order.lines.every((l) => l.paid) && (
            <p className="px-2 py-3 text-center text-xs text-slate-600">都結完了</p>
          )}
        </div>
      )}

      <div className="my-4 border-y border-slate-800 py-3">
        <div className="flex items-baseline justify-between">
          <span className="text-slate-400">應收</span>
          <span className="text-3xl font-semibold text-emerald-300">{formatMoney(due)}</span>
        </div>
        {/* ★ 分帳最貴的意外是「以為收完了」：三個人各付各的，第三個人走掉
            而沒有人發現。所以只要還沒收完，「還差多少」就要一直在畫面上。 */}
        {preview && (preview.billed > 0 || preview.remainingAfter > 0) && (
          <div className="mt-1.5 flex justify-between gap-2 text-xs">
            <span className="text-slate-500">
              全單 {formatMoney(preview.orderTotal)}
              {preview.billed > 0 && ` · 已收 ${formatMoney(preview.billed)}`}
              {preview.count !== null
                ? ` · 第 ${preview.index}／${preview.count} 份`
                : ` · 第 ${preview.index} 筆`}
            </span>
            {preview.remainingAfter > 0 && (
              <span className="shrink-0 font-semibold text-amber-300">
                收完還差 {formatMoney(preview.remainingAfter)}
              </span>
            )}
          </div>
        )}
      </div>
    </>
  )
}

/** 現金的鈔票鍵與找零。收銀員要照著找零那個數字數錢給客人。 */
function TenderPad({
  due,
  tendered,
  onTendered,
  change,
  onEnter,
}: {
  due: number
  tendered: string
  onTendered: (v: string) => void
  change: number
  onEnter: () => void
}) {
  return (
    <>
      <div className="mb-2 flex gap-1">
        {[100, 500, 1000].map((n) => (
          <button
            key={n}
            className="flex-1 rounded bg-slate-800 py-3 text-sm hover:bg-slate-700"
            onClick={() => onTendered(String(n))}
          >
            {n}
          </button>
        ))}
        <button
          className="flex-1 rounded bg-slate-800 py-3 text-sm hover:bg-slate-700 disabled:opacity-40"
          title="客人剛好給整數"
          disabled={due <= 0}
          onClick={() => onTendered(String(due))}
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
        onChange={(e) => onTendered(e.target.value)}
        onKeyDown={(e) => e.key === 'Enter' && onEnter()}
      />
      <div className="mt-3 flex items-baseline justify-between">
        <span className="text-slate-400">找零</span>
        <span
          className={`text-3xl font-semibold ${change > 0 ? 'text-amber-300' : 'text-slate-600'}`}
        >
          {formatMoney(change)}
        </span>
      </div>
    </>
  )
}

/** 這張單已經用的分法對應哪一個頁籤。 */
function modeOf(splitMode: string | null): Mode {
  if (splitMode === 'even') return 'even'
  if (splitMode === 'by_item') return 'items'
  if (splitMode === 'by_amount') return 'amount'
  return 'full'
}

function ModeTab({
  active,
  hidden,
  onClick,
  children,
}: {
  active: boolean
  hidden?: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  if (hidden) return null
  return (
    <button
      className={`flex-1 rounded px-2 py-1.5 text-sm ${
        active ? 'bg-slate-700 text-slate-100' : 'text-slate-400 hover:text-slate-200'
      }`}
      onClick={onClick}
    >
      {children}
    </button>
  )
}
