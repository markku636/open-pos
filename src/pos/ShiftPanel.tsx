import { useCallback, useEffect, useState } from 'react'

import AuditPanel from './AuditPanel'
import {
  reportApi,
  shiftApi,
  type AppError,
  type DayReport,
  type DayStatus,
  type DenomCount,
  type ShiftReport,
} from '@/shared/api'
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
  const [view, setView] = useState<'shift' | 'audit'>('shift')

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

  const shift = status?.shift ?? null
  const dayClosed = status?.status === 'closed' || status?.status === 'locked'

  if (view === 'audit') {
    return (
      <div className="flex h-full min-h-0 flex-col">
        <SubTabs view={view} onView={setView} />
        <div className="min-h-0 flex-1">
          <AuditPanel />
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
          <div className="text-xs text-slate-500">營業日</div>
          <div className="font-mono text-lg">{status?.businessDate ?? '—'}</div>
        </div>
        <div>
          <div className="text-xs text-slate-500">狀態</div>
          <div className={dayClosed ? 'text-amber-300' : 'text-emerald-300'}>
            {dayStatusLabel(status?.status)}
          </div>
        </div>
        {shift && (
          <>
            <div>
              <div className="text-xs text-slate-500">目前班別</div>
              <div className="font-mono">{shift.shiftNo}</div>
            </div>
            <div>
              <div className="text-xs text-slate-500">開班時間</div>
              <div className="font-mono text-sm">{hhmm(shift.openedAt)}</div>
            </div>
            <div>
              <div className="text-xs text-slate-500">準備金</div>
              <div className="font-mono">{formatMoney(shift.openingFloat)}</div>
            </div>
          </>
        )}
        <span className="ml-auto text-xs text-slate-600">
          今天已關 {status?.closedShifts ?? 0} 班
        </span>
      </section>

      {/* ─── 開班 ─────────────────────────────────────── */}
      {!shift && !dayClosed && (
        <section className="rounded border border-slate-800 bg-slate-900/40 p-4">
          <h2 className="mb-1 text-sm font-semibold text-slate-300">開班</h2>
          <p className="mb-3 text-xs text-slate-500">
            準備金是抽屜裡先放的零錢。它會被記成一筆現金異動 ——
            不記的話，關班時抽屜裡的錢從哪裡來就查不出來。
          </p>
          <div className="flex flex-wrap items-end gap-3">
            <label className="flex flex-col gap-1 text-xs text-slate-400">
              準備金
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
              開班
            </button>
          </div>
        </section>
      )}

      {/* ─── 關班（盲盤）─────────────────────────────── */}
      {shift && (
        <section className="rounded border border-slate-800 bg-slate-900/40 p-4">
          <h2 className="mb-1 text-sm font-semibold text-slate-300">關班盤點</h2>
          <p className="mb-3 text-xs text-slate-500">
            照面額把抽屜裡的錢數一遍。
            <span className="text-slate-400">
              系統會等你數完才顯示應有金額與差異
            </span>
            —— 先顯示的話，短少的人會直接照抄。
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
              <div className="text-xs text-slate-500">數到的總額</div>
              <div className="font-mono text-2xl">{formatMoney(countedTotal)}</div>
            </div>
            <input
              className="min-w-[12rem] flex-1 rounded bg-slate-800 px-3 py-2 text-sm"
              placeholder="備註（差異原因、交接對象…）"
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
              關班
            </button>
          </div>
        </section>
      )}

      {/* ─── 關班結果 ─────────────────────────────────── */}
      {closed && (
        <section className="rounded border border-slate-700 bg-slate-900 p-4">
          <div className="mb-3 flex items-baseline gap-3">
            <h2 className="text-sm font-semibold text-slate-300">{closed.shiftNo} 關班結果</h2>
            <span className="text-xs text-slate-500">{closed.businessDate}</span>
            <button
              className="ml-auto text-xs text-slate-500 hover:text-slate-300"
              onClick={() => setClosed(null)}
            >
              關閉
            </button>
          </div>
          <ReportBody report={closed} />
        </section>
      )}

      {/* ─── X 報表與日結 ─────────────────────────────── */}
      <section className="flex flex-wrap gap-2">
        <button
          className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-40"
          disabled={busy || !shift}
          title="不關班，中途看目前的數字"
          onClick={() =>
            void run(async () => {
              setClosed(await shiftApi.xReport())
            })
          }
        >
          X 報表（中途查看）
        </button>
        <button
          className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-40"
          disabled={busy || dayClosed}
          title="所有班別都關完之後才能日結"
          onClick={() =>
            void run(async () => {
              setDay(await shiftApi.closeDay())
            })
          }
        >
          日結（Z 報表）
        </button>
      </section>

      {day && (
        <section className="rounded border border-slate-700 bg-slate-900 p-4">
          <div className="mb-3 flex items-baseline gap-3">
            <h2 className="text-sm font-semibold text-slate-300">
              {day.businessDate} 日結
            </h2>
            <span className="font-mono text-xs text-slate-500">{day.zReportNo}</span>
            <button
              className="ml-auto rounded bg-slate-800 px-3 py-1 text-xs hover:bg-slate-700"
              disabled={busy}
              title="給記帳的人。Excel 開起來中文不會亂碼，金額是數值可以直接加總"
              onClick={() =>
                void run(async () => {
                  const dir = prompt('匯出到哪個資料夾？例如 D: 或一個現有的資料夾')?.trim()
                  if (!dir) return
                  const path = await reportApi.exportDayCsv(day.businessDate, dir)
                  setError(`已匯出：${path}`)
                })
              }
            >
              匯出 CSV
            </button>
            <button
              className="text-xs text-slate-500 hover:text-slate-300"
              onClick={() => setDay(null)}
            >
              關閉
            </button>
          </div>

          <Totals sales={day.sales} />

          {day.payments.length > 0 && (
            <div className="mt-3">
              <div className="mb-1 text-xs text-slate-500">收款方式</div>
              {day.payments.map((p) => (
                <Row key={p.code} label={`${p.name}（${p.count} 筆）`} value={p.amount} />
              ))}
            </div>
          )}

          <div className="mt-3">
            <div className="mb-1 text-xs text-slate-500">各班現金差異</div>
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
              <div className="mb-1 text-xs text-slate-500">品項排行</div>
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

          <p className="mt-4 text-xs text-slate-500">
            日結之後這一天就鎖住了：不能再新增或修改這一天的單。
            數字是當下算好的快照，之後永不重算 —— 一份會自己變的報表在稽核上站不住。
          </p>
        </section>
      )}
    </div>
  )
}

function SubTabs({
  view,
  onView,
}: {
  view: 'shift' | 'audit'
  onView: (v: 'shift' | 'audit') => void
}) {
  return (
    <div className="mb-3 flex gap-1">
      {(
        [
          ['shift', '班別與日結'],
          ['audit', '稽核紀錄'],
        ] as const
      ).map(([k, label]) => (
        <button
          key={k}
          className={`rounded px-3 py-1.5 text-sm ${
            view === k ? 'bg-slate-800 text-slate-100' : 'text-slate-400 hover:text-slate-200'
          }`}
          onClick={() => onView(k)}
        >
          {label}
        </button>
      ))}
    </div>
  )
}

function ReportBody({ report }: { report: ShiftReport }) {
  return (
    <>
      <Totals sales={report.sales} />

      <div className="mt-3">
        <div className="mb-1 text-xs text-slate-500">現金</div>
        <Row label="準備金" value={report.cash.openingFloat} />
        <Row label="現金銷售" value={report.cash.cashSales} />
        {report.cash.paidIn > 0 && <Row label="現金收入" value={report.cash.paidIn} />}
        {report.cash.paidOut > 0 && <Row label="現金支出" value={-report.cash.paidOut} />}
        <Row label="應有現金" value={report.cash.expected} strong />
        {report.cash.counted != null && <Row label="實際盤點" value={report.cash.counted} strong />}
        {report.cash.variance != null && (
          <div className="mt-1 flex items-baseline justify-between border-t border-slate-800 pt-1">
            <span className="text-slate-400">差異</span>
            <span className={`text-xl font-semibold ${varianceColor(report.cash.variance)}`}>
              {report.cash.variance === 0 ? '剛好' : formatMoney(report.cash.variance)}
            </span>
          </div>
        )}
      </div>

      {report.payments.length > 0 && (
        <div className="mt-3">
          <div className="mb-1 text-xs text-slate-500">收款方式</div>
          {report.payments.map((p) => (
            <Row key={p.code} label={`${p.name}（${p.count} 筆）`} value={p.amount} />
          ))}
        </div>
      )}

      {report.voids.voidedLines > 0 && (
        <div className="mt-3">
          <div className="mb-1 text-xs text-slate-500">作廢</div>
          <Row
            label={`退掉 ${report.voids.voidedLines} 項`}
            value={-report.voids.voidedAmount}
          />
        </div>
      )}
    </>
  )
}

function Totals({ sales }: { sales: ShiftReport['sales'] }) {
  return (
    <div>
      <div className="mb-1 text-xs text-slate-500">銷售</div>
      <Row label={`帳單 ${sales.bills} 張`} value={sales.total} strong />
      {sales.discount !== 0 && <Row label="折扣" value={-sales.discount} />}
      {sales.serviceCharge !== 0 && <Row label="服務費" value={sales.serviceCharge} />}
      {sales.rounding !== 0 && <Row label="進位調整" value={sales.rounding} />}
      <div className="mt-1 text-right text-xs text-slate-600">
        未稅 {formatMoney(sales.sales)}　稅 {formatMoney(sales.tax)}
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

function dayStatusLabel(s: string | undefined): string {
  return (
    ({
      not_started: '尚未開始',
      open: '營業中',
      closing: '結算中',
      closed: '已日結',
      locked: '已鎖定',
    } as Record<string, string>)[s ?? ''] ?? (s ?? '—')
  )
}
