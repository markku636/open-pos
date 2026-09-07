import { useCallback, useEffect, useState } from 'react'

import {
  reasonApi,
  refundApi,
  reprintApi,
  type AppError,
  type Bill,
  type BillPayment,
  type ReasonCode,
} from '@/shared/api'
import { useT } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { sales } from '@/shared/locales/sales'
import { formatMoney, parseMoney } from '@/shared/money'
import { hhmm } from '@/shared/time'

/**
 * 帳單與退款。
 *
 * # 退款不是作廢
 *
 * 作廢的語意是「這筆交易沒有發生過」，退款是「發生過，然後退回來」。
 * 少了退款，店家只能用「結帳後作廢」處理「三杯飲料有一杯做錯了」——
 * 那會把整筆銷售抹掉，而且那是餐飲業最大的防弊點，不該天天被用到。
 *
 * # 這一頁的第一件事是「找得到那張單」
 *
 * 客人手上只有那張收據，而收銀員不會想把 `B-20260906-0042` 一字不差打完。
 * 所以搜尋吃的是**末幾碼**，而且有搜尋字串時會跨日找 ——
 * 拿著三天前的收據回來是常態。
 */
export default function BillsPanel() {
  const t = useT()
  const [bills, setBills] = useState<Bill[] | null>(null)
  const [query, setQuery] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [picked, setPicked] = useState<Bill | null>(null)
  const [done, setDone] = useState<string | null>(null)

  const load = useCallback(async (q: string) => {
    try {
      const r = await refundApi.findBills({ billNo: q.trim() || null })
      setBills(r)
      setError(null)
      return r
    } catch (e) {
      setError((e as AppError).message ?? String(e))
      return null
    }
  }, [])

  useEffect(() => {
    void load('')
  }, [load])

  // 打字的時候等一下再查：收銀員邊看收據邊按，每一鍵都打一次 DB 沒有意義。
  useEffect(() => {
    const id = setTimeout(() => void load(query), 250)
    return () => clearTimeout(id)
  }, [query, load])

  return (
    <div className="flex h-full min-h-0 gap-4">
      <div className="flex min-h-0 w-96 shrink-0 flex-col">
        <input
          className="mb-3 w-full rounded bg-slate-900 px-3 py-2.5"
          placeholder={t(sales.billSearchPlaceholder)}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />

        {error && (
          <div className="mb-3 rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
            {error}
          </div>
        )}

        <div className="min-h-0 flex-1 space-y-1 overflow-y-auto">
          {bills === null ? (
            <p className="py-8 text-center text-sm text-slate-600">{t(ui.loading)}</p>
          ) : bills.length === 0 ? (
            <p className="py-8 text-center text-sm text-slate-600">
              {query ? t(sales.billNotFound) : t(sales.noBillsToday)}
            </p>
          ) : (
            bills.map((b) => (
              <button
                key={b.id}
                className={`flex w-full items-baseline gap-2 rounded px-3 py-2 text-left text-sm ${
                  picked?.id === b.id ? 'bg-sky-900/60' : 'bg-slate-900 hover:bg-slate-800'
                }`}
                onClick={() => {
                  setPicked(b)
                  setDone(null)
                }}
              >
                <span className="font-mono text-xs text-slate-400">
                  {b.billNo.split('-').pop()}
                </span>
                <span className="text-xs text-slate-600">{hhmm(b.settledAt)}</span>
                {b.splitLabel && (
                  <span className="rounded bg-slate-800 px-1 text-xs text-slate-400">
                    {t(sales.splitLabel, { label: b.splitLabel })}
                  </span>
                )}
                <span className="ml-auto font-mono">{formatMoney(b.grandTotal)}</span>
                {b.refundedTotal > 0 && (
                  <span className="text-xs text-amber-400">
                    {t(sales.refundedAmount, { amount: formatMoney(b.refundedTotal) })}
                  </span>
                )}
              </button>
            ))
          )}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {picked ? (
          <RefundForm
            bill={picked}
            done={done}
            onDone={async (msg) => {
              setDone(msg)
              const fresh = await load(query)
              setPicked(fresh?.find((b) => b.id === picked.id) ?? null)
            }}
          />
        ) : (
          <p className="py-24 text-center text-sm text-slate-600">{t(sales.pickBill)}</p>
        )}
      </div>
    </div>
  )
}

function RefundForm({
  bill,
  done,
  onDone,
}: {
  bill: Bill
  done: string | null
  onDone: (msg: string) => void | Promise<void>
}) {
  const t = useT()
  const [reasons, setReasons] = useState<ReasonCode[]>([])
  const [reasonId, setReasonId] = useState('')
  const [note, setNote] = useState('')
  const [paymentId, setPaymentId] = useState<string | null>(null)
  const [amount, setAmount] = useState('')
  const [busy, setBusy] = useState(false)
  const [reprinting, setReprinting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const reprint = async () => {
    setReprinting(true)
    try {
      await reprintApi.receipt(bill.id)
      setError(null)
      await onDone(t(sales.reprintQueued))
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setReprinting(false)
    }
  }

  useEffect(() => {
    reasonApi
      .list('refund')
      .then(setReasons)
      .catch(() => setReasons([]))
  }, [])

  // 換一張帳單就重來。留著上一張的金額是最容易造成誤退的一件事。
  useEffect(() => {
    setPaymentId(bill.payments.length === 1 ? bill.payments[0].id : null)
    setAmount('')
    setError(null)
  }, [bill.id, bill.payments])

  const payment = bill.payments.find((p) => p.id === paymentId) ?? null
  const max = payment?.refundable ?? 0
  const value = parseMoney(amount)
  const reason = reasons.find((r) => r.id === reasonId)
  const ok =
    !busy &&
    !!payment &&
    !!reasonId &&
    value !== null &&
    value > 0 &&
    value <= max &&
    (!reason?.requiresNote || note.trim().length > 0)

  const submit = async () => {
    if (!payment || value === null) return
    setBusy(true)
    try {
      const r = await refundApi.refund({
        billId: bill.id,
        paymentId: payment.id,
        amount: value,
        reasonId,
        note: note.trim() || null,
      })
      setAmount('')
      setNote('')
      setError(null)
      await onDone(
        t(sales.refundDone, {
          amount: formatMoney(r.amount),
          method: r.methodName,
          total: formatMoney(r.refundedTotal),
        }),
      )
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="max-w-lg">
      <div className="flex items-baseline gap-3">
        <h2 className="font-mono text-lg">{bill.billNo}</h2>
        <span className="text-sm text-slate-500">{bill.orderNo}</span>
        <span className="ml-auto text-2xl font-semibold">{formatMoney(bill.grandTotal)}</span>
      </div>
      <div className="mt-1 flex items-center gap-3">
        {/* 這幾段之間的間距用 gap-x 排，不要在字串前面塞全形空白 ——
            全形空白在英文畫面上是一個突兀的寬洞，而間距本來就是版面的事。 */}
        <p className="flex flex-wrap items-baseline gap-x-4 text-xs text-slate-600">
          <span>
            {bill.businessDate} {hhmm(bill.settledAt)}
          </span>
          {bill.splitLabel && <span>{t(sales.splitLabel, { label: bill.splitLabel })}</span>}
          {bill.refundedTotal > 0 && (
            <span>{t(sales.refundedAmount, { amount: formatMoney(bill.refundedTotal) })}</span>
          )}
        </p>
        {/* 補印重送的是當初那一張的快照，而且單上會寫第幾次 ——
            兩張一樣的收據可以拿去做假帳。 */}
        <button
          className="ml-auto rounded bg-slate-800 px-3 py-1 text-xs text-slate-300 hover:bg-slate-700 disabled:opacity-40"
          disabled={reprinting}
          title={t(sales.reprintHint)}
          onClick={() => void reprint()}
        >
          {t(sales.reprintReceipt)}
        </button>
      </div>

      {done && (
        <p className="mt-3 rounded border border-emerald-800 bg-emerald-950/50 px-3 py-2 text-sm text-emerald-200">
          {done}
        </p>
      )}

      {bill.refundable === 0 ? (
        <p className="mt-6 rounded bg-slate-900 px-3 py-6 text-center text-sm text-slate-500">
          {t(sales.fullyRefunded)}
        </p>
      ) : (
        <>
          {/* ★ 原路退回：刷卡收的錢用現金退，帳面兩邊都平但抽屜裡少了錢。
              所以這裡只能選「退哪一筆收款」，不能選「用什麼方式退」。 */}
          <h3 className="mt-6 mb-2 text-sm text-slate-400">{t(sales.whichPayment)}</h3>
          <div className="space-y-1">
            {bill.payments.map((p) => (
              <PaymentRow
                key={p.id}
                payment={p}
                active={paymentId === p.id}
                onPick={() => {
                  setPaymentId(p.id)
                  setAmount('')
                }}
              />
            ))}
          </div>

          <div className="mt-4 flex gap-2">
            <input
              className="flex-1 rounded bg-slate-800 px-3 py-2.5 text-right text-xl"
              placeholder={t(sales.refundAmountPlaceholder)}
              inputMode="numeric"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
            <button
              className="shrink-0 rounded bg-slate-800 px-4 text-sm hover:bg-slate-700 disabled:opacity-40"
              disabled={!payment || max <= 0}
              title={t(sales.refundAllHint)}
              onClick={() => setAmount(String(max))}
            >
              {t(sales.refundAll, { amount: formatMoney(max) })}
            </button>
          </div>

          <h3 className="mt-4 mb-2 text-sm text-slate-400">{t(sales.reasonRequired)}</h3>
          <div className="flex flex-wrap gap-1">
            {reasons.map((r) => (
              <button
                key={r.id}
                className={`rounded px-3 py-2 text-sm ${
                  reasonId === r.id
                    ? 'bg-sky-800 text-sky-50'
                    : 'bg-slate-800 text-slate-300 hover:bg-slate-700'
                }`}
                onClick={() => setReasonId(r.id)}
              >
                {r.name}
              </button>
            ))}
          </div>

          <input
            className="mt-2 w-full rounded bg-slate-800 px-3 py-2 text-sm"
            placeholder={t(reason?.requiresNote ? sales.noteRequired : sales.noteOptional)}
            value={note}
            onChange={(e) => setNote(e.target.value)}
          />

          {error && (
            <p className="mt-3 rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
              {error}
            </p>
          )}

          <button
            className="mt-4 w-full rounded bg-amber-700 py-3 text-lg font-semibold hover:bg-amber-600 disabled:opacity-40"
            disabled={!ok}
            onClick={() => void submit()}
          >
            {t(sales.refund)}
          </button>
          <p className="mt-2 text-center text-xs text-slate-600">{t(sales.refundFootnote)}</p>
        </>
      )}
    </div>
  )
}

function PaymentRow({
  payment,
  active,
  onPick,
}: {
  payment: BillPayment
  active: boolean
  onPick: () => void
}) {
  const t = useT()
  const spent = payment.refundable === 0
  return (
    <button
      className={`flex w-full items-baseline gap-3 rounded px-3 py-2 text-sm disabled:opacity-40 ${
        active ? 'bg-sky-900/60' : 'bg-slate-900 hover:bg-slate-800'
      }`}
      disabled={spent}
      onClick={onPick}
    >
      <span>{payment.methodName}</span>
      <span className="font-mono text-slate-400">{formatMoney(payment.amount)}</span>
      <span className="ml-auto text-xs">
        {spent ? (
          <span className="text-slate-600">{t(sales.paymentFullyRefunded)}</span>
        ) : (
          <span className="text-slate-400">
            {t(sales.paymentRefundable, { amount: formatMoney(payment.refundable) })}
          </span>
        )}
      </span>
    </button>
  )
}
