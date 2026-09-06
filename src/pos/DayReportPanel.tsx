import { useCallback, useEffect, useState } from 'react'

import { dialogApi, salesApi, type AppError, type DayReport } from '@/shared/api'
import { formatMoney } from '@/shared/money'

/**
 * 過去的日結報表。
 *
 * # 為什麼這一頁一定要有
 *
 * 關班與日結的整個設計前提是「當下算好、之後永不重算」—— 那份快照存在
 * `business_days.summary_json` 裡。但在這之前**沒有任何指令讀得到它**，
 * 於是日結按下去之後，那一天的報表就再也沒有地方看得見了。
 *
 * 一份存起來卻沒有人看得到的報表，跟沒有存是一樣的。
 *
 * # 為什麼不重算
 *
 * 隔天補一張單、或改了某個品項的價格，重算會讓三個月前的 Z 報表數字跟著變。
 * 而一份會自己變的報表在稽核上完全站不住 —— 所以這裡顯示的一律是快照，
 * 想看「現在的數字」請去營運分析那一頁。
 */
export default function DayReportPanel() {
  const [days, setDays] = useState<string[] | null>(null)
  const [picked, setPicked] = useState<string | null>(null)
  const [report, setReport] = useState<DayReport | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    salesApi
      .closedDays()
      .then((d) => {
        setDays(d)
        setPicked((p) => p ?? d[0] ?? null)
      })
      .catch((e: AppError) => setError(e.message))
  }, [])

  const load = useCallback((date: string) => {
    salesApi
      .dayReport(date)
      .then((r) => {
        setReport(r)
        setError(null)
      })
      .catch((e: AppError) => {
        setReport(null)
        setError(e.message)
      })
  }, [])

  useEffect(() => {
    if (picked) load(picked)
  }, [picked, load])

  const exportXlsx = async () => {
    if (!picked) return
    const dir = await dialogApi.pickFolder().catch(() => null)
    if (!dir) return
    setBusy(true)
    try {
      setNote(`已匯出：${await salesApi.exportDayXlsx(picked, dir)}`)
      setError(null)
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  if (days && days.length === 0) {
    return (
      <p className="rounded bg-slate-900/40 px-3 py-10 text-center text-sm text-slate-600">
        還沒有日結過。
        <br />
        每天打烊時到「班別與日結」按日結，那一天的報表就會存下來 ——
        <br />
        而存下來的數字之後永遠不會再變。
      </p>
    )
  }

  return (
    <div className="h-full space-y-4 overflow-y-auto pr-2">
      <section className="flex flex-wrap items-end gap-3 rounded border border-slate-800 bg-slate-900/40 px-4 py-3">
        <label>
          <span className="mb-1 block text-xs text-slate-500">營業日</span>
          <select
            className="rounded bg-slate-800 px-3 py-2 text-sm"
            value={picked ?? ''}
            onChange={(e) => setPicked(e.target.value)}
          >
            {(days ?? []).map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </label>
        {report && (
          <>
            <div>
              <div className="text-xs text-slate-500">Z 報表編號</div>
              <div className="font-mono text-sm">{report.zReportNo}</div>
            </div>
            <div className="ml-auto flex items-end gap-4">
              <Stat label="帳單數" value={String(report.sales.bills)} />
              <Stat label="營業額" value={formatMoney(report.sales.total)} big />
              <button
                className="rounded bg-emerald-800 px-4 py-2 text-sm hover:bg-emerald-700 disabled:opacity-40"
                disabled={busy}
                title="日報表 / 付款方式 / 品項排行 / 各班現金 四張表"
                onClick={() => void exportXlsx()}
              >
                匯出 Excel
              </button>
            </div>
          </>
        )}
      </section>

      {error && (
        <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}
      {note && (
        <p className="flex items-start gap-3 rounded border border-emerald-800 bg-emerald-950/50 px-3 py-2 text-sm text-emerald-200">
          <span className="flex-1 break-all">{note}</span>
          <button className="shrink-0 text-emerald-400" onClick={() => setNote(null)}>
            ✕
          </button>
        </p>
      )}

      {report && (
        <div className="grid gap-4 md:grid-cols-2">
          <Box title="銷售">
            <Row label="帳單數" value={report.sales.bills} plain />
            <Row label="銷售總額" value={report.sales.total} strong />
            <Row label="未稅" value={report.sales.sales} />
            <Row label="稅額" value={report.sales.tax} />
            <Row label="折扣" value={-report.sales.discount} />
            <Row label="服務費" value={report.sales.serviceCharge} />
          </Box>

          <Box title="付款方式">
            {report.payments.length === 0 ? (
              <Empty />
            ) : (
              report.payments.map((p) => (
                <Row key={p.code} label={`${p.name}（${p.count}）`} value={p.amount} />
              ))
            )}
          </Box>

          <Box title="退款與作廢">
            <Row label="退款筆數" value={report.refunds.count} plain />
            <Row label="退款金額" value={-report.refunds.amount} />
            <Row label="其中現金" value={-report.refunds.cashAmount} />
            <Row label="退掉的品項" value={report.voids.voidedLines} plain />
            <Row label="作廢金額" value={-report.voids.voidedAmount} />
          </Box>

          <Box title="各班現金差異">
            {report.shifts.length === 0 ? (
              <Empty />
            ) : (
              report.shifts.map((s) => (
                <Row
                  key={s.shiftNo}
                  label={s.shiftNo}
                  value={s.cashVariance ?? 0}
                  // 差異是這一整頁最該被看的一欄。
                  warn={(s.cashVariance ?? 0) !== 0}
                />
              ))
            )}
          </Box>

          <Box title="品項排行" wide>
            {report.topItems.length === 0 ? (
              <Empty />
            ) : (
              report.topItems.map((i) => (
                <Row
                  key={i.name}
                  label={`${i.name} ×${i.qtyMilli / 1000}`}
                  value={i.amount}
                />
              ))
            )}
          </Box>
        </div>
      )}
    </div>
  )
}

function Box({
  title,
  wide,
  children,
}: {
  title: string
  wide?: boolean
  children: React.ReactNode
}) {
  return (
    <section
      className={`rounded border border-slate-800 bg-slate-900/40 p-3 ${wide ? 'md:col-span-2' : ''}`}
    >
      <h3 className="mb-2 text-sm text-slate-400">{title}</h3>
      <div className="space-y-0.5">{children}</div>
    </section>
  )
}

function Row({
  label,
  value,
  strong,
  plain,
  warn,
}: {
  label: string
  value: number
  strong?: boolean
  plain?: boolean
  warn?: boolean
}) {
  return (
    <div className="flex justify-between text-sm">
      <span className="text-slate-400">{label}</span>
      <span
        className={`font-mono ${strong ? 'text-lg font-semibold' : ''} ${
          warn && value !== 0 ? 'text-amber-300' : value < 0 ? 'text-amber-300/80' : ''
        }`}
      >
        {plain ? value : formatMoney(value)}
      </span>
    </div>
  )
}

function Empty() {
  return <p className="py-3 text-center text-xs text-slate-600">沒有資料</p>
}

function Stat({ label, value, big }: { label: string; value: string; big?: boolean }) {
  return (
    <div className="text-right">
      <div className="text-xs text-slate-500">{label}</div>
      <div className={`font-mono ${big ? 'text-2xl text-emerald-300' : 'text-lg'}`}>{value}</div>
    </div>
  )
}
