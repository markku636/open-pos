import { useCallback, useEffect, useState } from 'react'

import {
  insightApi,
  type AppError,
  type HourBucket,
  type Insight,
  type NamedTotal,
} from '@/shared/api'
import { useT } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { reports } from '@/shared/locales/reports'
import { formatMoney } from '@/shared/money'

/**
 * 營運分析。
 *
 * # 跟 Z 報表的分工
 *
 * Z 報表是當天算好、永不重算的快照 —— 那是稅務與交接的問題。
 * 這一頁是**現算的**，回答的是老闆的問題：哪個時段最忙、這個月折掉多少、
 * 什麼賣得最好。現算才能任意選區間，而且改變主意也不會有人受傷。
 *
 * # 為什麼快捷鍵是「今天／最近 7 天／這個月」
 *
 * 因為那是實際會被問的三個問題。要人自己挑兩個日期才看得到東西的報表，
 * 多數人只會打開一次。
 */
export default function InsightPanel() {
  const t = useT()
  const [data, setData] = useState<Insight | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [range, setRange] = useState<{ from: string; to: string } | null>(null)

  const load = useCallback(async (r: { from: string; to: string } | null) => {
    try {
      setData(await insightApi.query({ from: r?.from ?? null, to: r?.to ?? null }))
      setError(null)
    } catch (e) {
      setData(null)
      setError((e as AppError).message ?? String(e))
    }
  }, [])

  useEffect(() => {
    void load(range)
  }, [load, range])

  return (
    <div className="h-full space-y-5 overflow-y-auto pr-2">
      <section className="flex flex-wrap items-end gap-3 rounded border border-slate-800 bg-slate-900/40 px-4 py-3">
        <div className="flex gap-1">
          <Preset label={t(reports.today)} onClick={() => setRange(null)} active={range === null} />
          <Preset
            label={t(reports.last7Days)}
            active={!!range && daysBetween(range) === 6}
            onClick={() => setRange(lastDays(7))}
          />
          <Preset
            label={t(reports.last30Days)}
            active={!!range && daysBetween(range) === 29}
            onClick={() => setRange(lastDays(30))}
          />
        </div>
        <label>
          <span className="mb-1 block text-xs text-slate-500">{t(reports.from)}</span>
          <input
            type="date"
            className="rounded bg-slate-800 px-2 py-1.5 text-sm"
            value={range?.from ?? data?.from ?? ''}
            onChange={(e) =>
              setRange((r) => ({ from: e.target.value, to: r?.to ?? e.target.value }))
            }
          />
        </label>
        <label>
          <span className="mb-1 block text-xs text-slate-500">{t(reports.to)}</span>
          <input
            type="date"
            className="rounded bg-slate-800 px-2 py-1.5 text-sm"
            value={range?.to ?? data?.to ?? ''}
            onChange={(e) => setRange((r) => ({ from: r?.from ?? e.target.value, to: e.target.value }))}
          />
        </label>

        {data && (
          <div className="ml-auto flex gap-6 text-right">
            <Stat label={t(reports.revenue)} value={formatMoney(data.total)} big />
            <Stat label={t(reports.billCount)} value={String(data.bills)} />
            <Stat label={t(reports.averageBill)} value={formatMoney(data.averageBill)} />
          </div>
        )}
      </section>

      {error && (
        <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}

      {data && data.bills === 0 && (
        <p className="rounded bg-slate-900/40 px-3 py-10 text-center text-sm text-slate-600">
          {t(reports.noSales)}
        </p>
      )}

      {data && data.bills > 0 && (
        <>
          <Hours hours={data.hours} />
          <div className="grid gap-4 md:grid-cols-2">
            <Bars title={t(reports.channels)} rows={data.channels} unit={t(reports.unitBills)} />
            <Bars
              title={t(reports.discounts)}
              rows={data.discounts}
              unit={t(reports.unitTimes)}
              tone="amber"
            />
            <Bars
              title={t(reports.voids)}
              rows={data.voids}
              unit={t(reports.unitLines)}
              tone="amber"
            />
            <Bars
              title={t(reports.topItems)}
              rows={data.items}
              unit={t(reports.unitServings)}
              limit={12}
            />
          </div>
        </>
      )}
    </div>
  )
}

/**
 * 時段分布。
 *
 * 這張圖是整頁最有用的一個 —— 它直接回答「幾點要多排一個人」。
 * 沒有生意的小時不畫，因為一天二十四格裡有十幾格是零，畫出來只會讓
 * 真正有意義的那幾格變窄。
 */
function Hours({ hours }: { hours: HourBucket[] }) {
  const t = useT()
  const peak = Math.max(1, ...hours.map((h) => h.total))
  const busiest = hours.reduce<HourBucket | null>(
    (m, h) => (!m || h.total > m.total ? h : m),
    null,
  )
  return (
    <section className="rounded border border-slate-800 bg-slate-900/40 p-4">
      <h3 className="mb-3 text-sm text-slate-400">
        {t(reports.byHour)}
        {busiest && (
          <span className="ml-2 text-slate-500">
            {t(reports.busiest, {
              from: busiest.hour,
              to: busiest.hour + 1,
              money: formatMoney(busiest.total),
            })}
          </span>
        )}
      </h3>
      {/* 每一欄要 h-full，柱子的百分比高度才有東西可以量 ——
          放在自動高度的容器裡，height: 100% 會算成 0，而圖表看起來就是空的。 */}
      {/* 柱子有寬度上限：只有兩三個時段有生意時（剛開店、或看單一天），
          不設限的話兩根柱子會撐滿整個面板，看起來不像一張圖。 */}
      <div className="flex h-40 gap-1">
        {hours.map((h) => (
          <div
            key={h.hour}
            className="flex h-full max-w-16 flex-1 flex-col justify-end gap-1"
          >
            <span className="text-center font-mono text-[10px] text-slate-500">{h.bills}</span>
            <div
              className={`w-full rounded-t ${h === busiest ? 'bg-sky-500' : 'bg-sky-800'}`}
              style={{ height: `${Math.max(3, (h.total / peak) * 85)}%` }}
              title={t(reports.hourTip, {
                hour: h.hour,
                money: formatMoney(h.total),
                n: h.bills,
              })}
            />
            <span className="text-center font-mono text-[10px] text-slate-500">{h.hour}</span>
          </div>
        ))}
      </div>
    </section>
  )
}

function Bars({
  title,
  rows,
  unit,
  tone = 'slate',
  limit,
}: {
  title: string
  rows: NamedTotal[]
  unit: string
  tone?: 'slate' | 'amber'
  limit?: number
}) {
  const t = useT()
  const shown = limit ? rows.slice(0, limit) : rows
  const peak = Math.max(1, ...shown.map((r) => Math.abs(r.amount)))
  return (
    <section className="rounded border border-slate-800 bg-slate-900/40 p-3">
      <h3 className="mb-2 text-sm text-slate-400">
        {title}
        {limit && rows.length > limit && (
          <span className="ml-2 text-xs text-slate-600">{t(reports.topN, { n: limit })}</span>
        )}
      </h3>
      {shown.length === 0 ? (
        <p className="py-4 text-center text-xs text-slate-600">{t(ui.noData)}</p>
      ) : (
        <ul className="space-y-1">
          {shown.map((r) => (
            <li key={r.label} className="flex items-baseline gap-2 text-sm">
              <span className="w-32 shrink-0 truncate" title={r.label}>
                {r.label}
              </span>
              <span className="w-12 shrink-0 text-right text-xs text-slate-500">
                {r.count} {unit}
              </span>
              <span className="h-2 flex-1 overflow-hidden rounded bg-slate-800">
                <span
                  className={`block h-full ${tone === 'amber' ? 'bg-amber-600' : 'bg-sky-700'}`}
                  style={{ width: `${(Math.abs(r.amount) / peak) * 100}%` }}
                />
              </span>
              <span className="w-20 shrink-0 text-right font-mono text-xs text-slate-400">
                {formatMoney(r.amount)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function Stat({ label, value, big }: { label: string; value: string; big?: boolean }) {
  return (
    <div>
      <div className="text-xs text-slate-500">{label}</div>
      <div className={`font-mono ${big ? 'text-2xl text-emerald-300' : 'text-lg'}`}>{value}</div>
    </div>
  )
}

function Preset({
  label,
  active,
  onClick,
}: {
  label: string
  active: boolean
  onClick: () => void
}) {
  return (
    <button
      className={`rounded px-3 py-1.5 text-sm ${
        active ? 'bg-sky-800 text-sky-50' : 'bg-slate-800 text-slate-300 hover:bg-slate-700'
      }`}
      onClick={onClick}
    >
      {label}
    </button>
  )
}

/**
 * 最近 n 天（含今天）。
 *
 * 用瀏覽器的本地日期，而不是 UTC —— 台灣時間凌晨兩點按下「最近 7 天」，
 * UTC 還是前一天，於是會少算一天而沒有人看得出來。
 */
function lastDays(n: number): { from: string; to: string } {
  const end = new Date()
  const start = new Date()
  start.setDate(start.getDate() - (n - 1))
  return { from: ymd(start), to: ymd(end) }
}

function ymd(d: Date): string {
  const p = (x: number) => String(x).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

function daysBetween(r: { from: string; to: string }): number {
  return Math.round((Date.parse(r.to) - Date.parse(r.from)) / 86_400_000)
}
