import { useCallback, useEffect, useState } from 'react'

import AuditPanel from './AuditPanel'
import DayReportPanel from './DayReportPanel'
import InsightPanel from './InsightPanel'
import {
  reportApi,
  shiftApi,
  type AppError,
  type DayReport,
  type DayStatus,
  type DenomCount,
  type ShiftReport,
} from '@/shared/api'
import { useT, type Msg } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { shift } from '@/shared/locales/shift'
import { formatMoney } from '@/shared/money'
import { hhmm } from '@/shared/time'

/** 台幣現行流通面額，由大到小。數錢的人是從大鈔開始數的。 */
const DENOMINATIONS = [1000, 500, 200, 100, 50, 10, 5, 1]

/**
 * 班別交接與日結。
 *
 * 這一頁最重要的設計是**盲盤**：關班時畫面上不顯示應有現金，
 * 收銀員照面額數完、按下關班之後才揭曉差異。
 *
 * 這不是防呆，是防弊。先把應有金額顯示出來，短少的人會直接照抄 ——
 * 而那正是「現金差異永遠是零」的原因：不是沒有問題，是看不到問題。
 */
/**
 * 班別日結，以及**稽核紀錄**。
 *
 * 兩者放在同一個分頁下，是因為看的是同一個人（老闆或店長）在問同一類問題：
 * 「今天收了多少、對不對得起來、有沒有人動了不該動的錢」。
 */
export default function ShiftPanel() {
  const t = useT()
  const [view, setView] = useState<View>('shift')

  const [status, setStatus] = useState<DayStatus | null>(null)
  const [counts, setCounts] = useState<Record<number, string>>({})
  const [openingFloat, setOpeningFloat] = useState('1000')
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [closed, setClosed] = useState<ShiftReport | null>(null)
  const [day, setDay] = useState<DayReport | null>(null)

  const reload = useCallback(async () => {
    try {
      setStatus(await shiftApi.dayStatus())
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true)
    setError(null)
    try {
      await fn()
      await reload()
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  const countList: DenomCount[] = DENOMINATIONS.map((d) => ({
    denomination: d,
    count: Number(counts[d] ?? 0) || 0,
  })).filter((c) => c.count > 0)
  const countedTotal = countList.reduce((s, c) => s + c.denomination * c.count, 0)

  const openShift = status?.shift ?? null
  const dayClosed = status?.status === 'closed' || status?.status === 'locked'

  if (view !== 'shift') {
    return (
      <div className="flex h-full min-h-0 flex-col">
        <SubTabs view={view} onView={setView} />
        <div className="min-h-0 flex-1">
          {view === 'audit' ? (
            <AuditPanel />
          ) : view === 'day' ? (
            <DayReportPanel />
          ) : (
            <InsightPanel />
          )}
        </div>
      </div>
    )
  }

  return (
    <div className="h-full space-y-6 overflow-y-auto pr-2">
      <SubTabs view={view} onView={setView} />

      {error && (
        <p className="whitespace-pre-line rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}

      <section className="flex flex-wrap items-center gap-4 rounded border border-slate-800 bg-slate-900/40 px-4 py-3">
        <div>
          <div className="text-xs text-slate-500">{t(shift.businessDate)}</div>
          <div className="font-mono text-lg">{status?.businessDate ?? '—'}</div>
        </div>
        <div>
          <div className="text-xs text-slate-500">{t(shift.status)}</div>
          <div className={dayClosed ? 'text-amber-300' : 'text-emerald-300'}>
            {dayStatusLabel(t, status?.status)}
          </div>
        </div>
        {openShift && (
          <>
            <div>
              <div className="text-xs text-slate-500">{t(shift.currentShift)}</div>
              <div className="font-mono">{openShift.shiftNo}</div>
            </div>
            <div>
              <div className="text-xs text-slate-500">{t(shift.openedAt)}</div>
              <div className="font-mono text-sm">{hhmm(openShift.openedAt)}</div>
            </div>
            <div>
              <div className="text-xs text-slate-500">{t(shift.openingFloat)}</div>
              <div className="font-mono">{formatMoney(openShift.openingFloat)}</div>
            </div>
          </>
        )}
        <span className="ml-auto text-xs text-slate-600">
          {t(shift.closedShiftsToday, { n: status?.closedShifts ?? 0 })}
        </span>
      </section>

      {/* ─── 開班 ─────────────────────────────────────── */}
      {!openShift && !dayClosed && (
        <section className="rounded border border-slate-800 bg-slate-900/40 p-4">
          <h2 className="mb-1 text-sm font-semibold text-slate-300">{t(shift.openShift)}</h2>
          <p className="mb-3 text-xs text-slate-500">{t(shift.openHint)}</p>
          <div className="flex flex-wrap items-end gap-3">
            <label className="flex flex-col gap-1 text-xs text-slate-400">
              {t(shift.openingFloat)}
              <input
                className="w-32 rounded bg-slate-800 px-3 py-2 text-right font-mono text-lg"
                inputMode="numeric"
                value={openingFloat}
                onChange={(e) => setOpeningFloat(e.target.value)}
              />
            </label>
            <button
              className="rounded bg-emerald-700 px-5 py-2.5 font-medium hover:bg-emerald-600 disabled:opacity-40"
              disabled={busy}
              onClick={() => void run(() => shiftApi.open(Number(openingFloat) || 0))}
            >
              {t(shift.openShift)}
            </button>
          </div>
        </section>
      )}

      {/* ─── 關班（盲盤）─────────────────────────────── */}
      {openShift && (
        <section className="rounded border border-slate-800 bg-slate-900/40 p-4">
          <h2 className="mb-1 text-sm font-semibold text-slate-300">{t(shift.countTitle)}</h2>
          <p className="mb-3 text-xs text-slate-500">
            {t(shift.countHintLead)}
            <span className="text-slate-400">{t(shift.countHintStrong)}</span>
            {t(shift.countHintTail)}
          </p>

          <div className="grid max-w-2xl grid-cols-[repeat(auto-fill,minmax(120px,1fr))] gap-2">
            {DENOMINATIONS.map((d) => (
              <label key={d} className="flex items-center gap-2 rounded bg-slate-800/60 px-3 py-2">
                <span className="w-12 shrink-0 text-right font-mono text-sm text-slate-400">
                  {d}
                </span>
                <span className="text-slate-600">×</span>
                <input
                  className="w-full min-w-0 rounded bg-slate-900 px-2 py-1.5 text-right font-mono"
                  inputMode="numeric"
                  placeholder="0"
                  value={counts[d] ?? ''}
                  onChange={(e) => setCounts({ ...counts, [d]: e.target.value })}
                />
              </label>
            ))}
          </div>

          <div className="mt-3 flex flex-wrap items-end gap-3">
            <div>
              <div className="text-xs text-slate-500">{t(shift.countedTotal)}</div>
              <div className="font-mono text-2xl">{formatMoney(countedTotal)}</div>
            </div>
            <input
              className="min-w-[12rem] flex-1 rounded bg-slate-800 px-3 py-2 text-sm"
              placeholder={t(shift.notePlaceholder)}
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
            <button
              className="rounded bg-amber-700 px-5 py-2.5 font-medium hover:bg-amber-600 disabled:opacity-40"
              disabled={busy || countList.length === 0}
              onClick={() =>
                void run(async () => {
                  const r = await shiftApi.close(countList, note || undefined)
                  setClosed(r)
                  setCounts({})
                  setNote('')
                })
              }
            >
              {t(shift.closeShift)}
            </button>
          </div>
        </section>
      )}

      {/* ─── 關班結果 ─────────────────────────────────── */}
      {closed && (
        <section className="rounded border border-slate-700 bg-slate-900 p-4">
          <div className="mb-3 flex items-baseline gap-3">
            <h2 className="text-sm font-semibold text-slate-300">
              {t(shift.closeResult, { no: closed.shiftNo })}
            </h2>
            <span className="text-xs text-slate-500">{closed.businessDate}</span>
            <button
              className="ml-auto text-xs text-slate-500 hover:text-slate-300"
              onClick={() => setClosed(null)}
            >
              {t(ui.close)}
            </button>
          </div>
          <ReportBody report={closed} />
        </section>
      )}

      {/* ─── X 報表與日結 ─────────────────────────────── */}
      <section className="flex flex-wrap gap-2">
        <button
          className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-40"
          disabled={busy || !openShift}
          title={t(shift.xReportHint)}
          onClick={() =>
            void run(async () => {
              setClosed(await shiftApi.xReport())
            })
          }
        >
          {t(shift.xReport)}
        </button>
        <button
          className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-40"
          disabled={busy || dayClosed}
          title={t(shift.closeDayHint)}
          onClick={() =>
            void run(async () => {
              setDay(await shiftApi.closeDay())
            })
          }
        >
          {t(shift.closeDay)}
        </button>
      </section>

      {day && (
        <section className="rounded border border-slate-700 bg-slate-900 p-4">
          <div className="mb-3 flex items-baseline gap-3">
            <h2 className="text-sm font-semibold text-slate-300">
              {t(shift.dayTitle, { date: day.businessDate })}
            </h2>
            <span className="font-mono text-xs text-slate-500">{day.zReportNo}</span>
            <button
              className="ml-auto rounded bg-slate-800 px-3 py-1 text-xs hover:bg-slate-700"
              disabled={busy}
              title={t(shift.exportCsvHint)}
              onClick={() =>
                void run(async () => {
                  const dir = prompt(t(shift.exportDirPrompt))?.trim()
                  if (!dir) return
                  const path = await reportApi.exportDayCsv(day.businessDate, dir)
                  setError(t(shift.exported, { path }))
                })
              }
            >
              {t(shift.exportCsv)}
            </button>
            <button
              className="text-xs text-slate-500 hover:text-slate-300"
              onClick={() => setDay(null)}
            >
              {t(ui.close)}
            </button>
          </div>

          <Totals sales={day.sales} />

          {day.payments.length > 0 && (
            <div className="mt-3">
              <div className="mb-1 text-xs text-slate-500">{t(shift.paymentMethods)}</div>
              {day.payments.map((p) => (
                <Row
                  key={p.code}
                  label={t(shift.paymentLine, { name: p.name, n: p.count })}
                  value={p.amount}
                />
              ))}
            </div>
          )}

          <div className="mt-3">
            <div className="mb-1 text-xs text-slate-500">{t(shift.shiftVariances)}</div>
            {day.shifts.map((s) => (
              <div key={s.id} className="flex justify-between text-sm">
                <span className="text-slate-400">{s.shiftNo}</span>
                <span className={varianceColor(s.cashVariance)}>
                  {s.cashVariance == null ? '—' : formatMoney(s.cashVariance)}
                </span>
              </div>
            ))}
          </div>

          {day.topItems.length > 0 && (
            <div className="mt-3">
              <div className="mb-1 text-xs text-slate-500">{t(shift.topItems)}</div>
              {day.topItems.slice(0, 10).map((i) => (
                <div key={i.name} className="flex justify-between text-sm">
                  <span className="text-slate-400">
                    {i.name}
                    <span className="ml-2 text-xs text-slate-600">
                      ×{(i.qtyMilli / 1000).toFixed(i.qtyMilli % 1000 === 0 ? 0 : 1)}
                    </span>
                  </span>
                  <span>{formatMoney(i.amount)}</span>
                </div>
              ))}
            </div>
          )}

          <p className="mt-4 text-xs text-slate-500">{t(shift.lockNote)}</p>
        </section>
      )}
    </div>
  )
}

type View = 'shift' | 'day' | 'insight' | 'audit'

function SubTabs({ view, onView }: { view: View; onView: (v: View) => void }) {
  const t = useT()
  return (
    <div className="mb-3 flex gap-1">
      {(
        [
          ['shift', shift.tabShift],
          ['day', shift.tabDay],
          ['insight', shift.tabInsight],
          ['audit', shift.tabAudit],
        ] as const
      ).map(([k, label]) => (
        <button
          key={k}
          className={`rounded px-3 py-1.5 text-sm ${
            view === k ? 'bg-slate-800 text-slate-100' : 'text-slate-400 hover:text-slate-200'
          }`}
          onClick={() => onView(k)}
        >
          {t(label)}
        </button>
      ))}
    </div>
  )
}

function ReportBody({ report }: { report: ShiftReport }) {
  const t = useT()
  return (
    <>
      <Totals sales={report.sales} />

      <div className="mt-3">
        <div className="mb-1 text-xs text-slate-500">{t(shift.cash)}</div>
        <Row label={t(shift.openingFloat)} value={report.cash.openingFloat} />
        <Row label={t(shift.cashSales)} value={report.cash.cashSales} />
        {report.cash.paidIn > 0 && <Row label={t(shift.paidIn)} value={report.cash.paidIn} />}
        {report.cash.paidOut > 0 && <Row label={t(shift.paidOut)} value={-report.cash.paidOut} />}
        <Row label={t(shift.expectedCash)} value={report.cash.expected} strong />
        {report.cash.counted != null && (
          <Row label={t(shift.countedCash)} value={report.cash.counted} strong />
        )}
        {report.cash.variance != null && (
          <div className="mt-1 flex items-baseline justify-between border-t border-slate-800 pt-1">
            <span className="text-slate-400">{t(shift.variance)}</span>
            <span className={`text-xl font-semibold ${varianceColor(report.cash.variance)}`}>
              {report.cash.variance === 0 ? t(shift.exact) : formatMoney(report.cash.variance)}
            </span>
          </div>
        )}
      </div>

      {report.payments.length > 0 && (
        <div className="mt-3">
          <div className="mb-1 text-xs text-slate-500">{t(shift.paymentMethods)}</div>
          {report.payments.map((p) => (
            <Row
              key={p.code}
              label={t(shift.paymentLine, { name: p.name, n: p.count })}
              value={p.amount}
            />
          ))}
        </div>
      )}

      {report.voids.voidedLines > 0 && (
        <div className="mt-3">
          <div className="mb-1 text-xs text-slate-500">{t(shift.voids)}</div>
          <Row
            label={t(shift.voidedLines, { n: report.voids.voidedLines })}
            value={-report.voids.voidedAmount}
          />
        </div>
      )}
    </>
  )
}

function Totals({ sales }: { sales: ShiftReport['sales'] }) {
  const t = useT()
  return (
    <div>
      <div className="mb-1 text-xs text-slate-500">{t(shift.sales)}</div>
      <Row label={t(shift.billsCount, { n: sales.bills })} value={sales.total} strong />
      {sales.discount !== 0 && <Row label={t(shift.discount)} value={-sales.discount} />}
      {sales.serviceCharge !== 0 && (
        <Row label={t(shift.serviceCharge)} value={sales.serviceCharge} />
      )}
      {sales.rounding !== 0 && <Row label={t(shift.rounding)} value={sales.rounding} />}
      <div className="mt-1 text-right text-xs text-slate-600">
        {t(shift.netAndTax, { net: formatMoney(sales.sales), tax: formatMoney(sales.tax) })}
      </div>
    </div>
  )
}

function Row({ label, value, strong }: { label: string; value: number; strong?: boolean }) {
  return (
    <div className="flex justify-between text-sm">
      <span className="text-slate-400">{label}</span>
      <span className={strong ? 'font-semibold text-slate-100' : ''}>{formatMoney(value)}</span>
    </div>
  )
}

function varianceColor(v: number | null | undefined): string {
  if (v == null) return 'text-slate-500'
  if (v === 0) return 'text-emerald-400'
  // 短少用紅色、溢收用琥珀色：兩者都要查，但短少是會賠錢的那一個。
  return v < 0 ? 'text-red-400' : 'text-amber-300'
}

function dayStatusLabel(t: (m: Msg) => string, s: string | undefined): string {
  // 認不得的狀態原樣顯示：後端多了一個狀態時，畫面上看到那個代碼
  // 比看到空白好查 —— 至少知道是新狀態，不是壞掉。
  const msg = (
    {
      not_started: shift.statusNotStarted,
      open: shift.statusOpen,
      closing: shift.statusClosing,
      closed: shift.statusClosed,
      locked: shift.statusLocked,
    } as Record<string, Msg>
  )[s ?? '']
  return msg ? t(msg) : (s ?? '—')
}
