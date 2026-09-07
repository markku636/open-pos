import { useCallback, useEffect, useState } from 'react'

import {
  dialogApi,
  salesApi,
  type AppError,
  type Sale,
  type SalesQuery,
  type SalesReport,
} from '@/shared/api'
import { useT } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { sales } from '@/shared/locales/sales'
import { formatMoney } from '@/shared/money'
import { hhmm } from '@/shared/time'

/**
 * 銷售記錄。
 *
 * # 跟「帳單退款」那一頁的分工
 *
 * 退款那一頁是**為了退款而找單**：客人拿著收據回來，收銀員照末幾碼搜。
 * 這一頁是**翻帳**：老闆想看昨天賣了什麼、上禮拜三那筆一千二是誰結的、
 * 這個月外送佔多少。所以查詢條件是日期區間，而不是單號。
 *
 * # 合計那一行放最上面
 *
 * 因為它是唯一一個「不用捲就想知道」的數字。而且它是**獨立查出來的**，
 * 不是拿列表那幾百筆加的 —— 一份「只加總到前 500 筆」的營業額比沒有更糟，
 * 因為它看起來是完整的。
 */
export default function SalesPanel() {
  const t = useT()
  const [data, setData] = useState<SalesReport | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [open, setOpen] = useState<string | null>(null)

  const [range, setRange] = useState<{ from: string; to: string } | null>(null)
  const [channel, setChannel] = useState<string | null>(null)
  const [refundedOnly, setRefundedOnly] = useState(false)
  const [keyword, setKeyword] = useState('')

  const query: SalesQuery = {
    from: range?.from ?? null,
    to: range?.to ?? null,
    channel,
    refundedOnly,
    keyword: keyword.trim() || null,
  }

  const load = useCallback(async (q: SalesQuery) => {
    try {
      setData(await salesApi.history(q))
      setError(null)
    } catch (e) {
      setData(null)
      setError((e as AppError).message ?? String(e))
    }
  }, [])

  // 打字時等一下再查：邊看收據邊按，每一鍵都打一次 DB 沒有意義。
  useEffect(() => {
    const id = setTimeout(() => void load(query), 250)
    return () => clearTimeout(id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [load, range?.from, range?.to, channel, refundedOnly, keyword])

  const exportXlsx = async () => {
    const dir = await dialogApi.pickFolder().catch(() => null)
    if (!dir) return
    setBusy(true)
    try {
      const path = await salesApi.exportSalesXlsx(query, dir)
      setNote(t(sales.exported, { path }))
      setError(null)
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <section className="mb-3 flex flex-wrap items-end gap-3 rounded border border-slate-800 bg-slate-900/40 px-4 py-3">
        <div className="flex gap-1">
          <Preset label={t(sales.today)} active={range === null} onClick={() => setRange(null)} />
          <Preset
            label={t(sales.last7Days)}
            active={!!range && days(range) === 6}
            onClick={() => setRange(lastDays(7))}
          />
          <Preset
            label={t(sales.last30Days)}
            active={!!range && days(range) === 29}
            onClick={() => setRange(lastDays(30))}
          />
        </div>
        <label>
          <span className="mb-1 block text-xs text-slate-500">{t(sales.dateFrom)}</span>
          <input
            type="date"
            className="rounded bg-slate-800 px-2 py-1.5 text-sm"
            value={range?.from ?? data?.from ?? ''}
            onChange={(e) => setRange((r) => ({ from: e.target.value, to: r?.to ?? e.target.value }))}
          />
        </label>
        <label>
          <span className="mb-1 block text-xs text-slate-500">{t(sales.dateTo)}</span>
          <input
            type="date"
            className="rounded bg-slate-800 px-2 py-1.5 text-sm"
            value={range?.to ?? data?.to ?? ''}
            onChange={(e) => setRange((r) => ({ from: r?.from ?? e.target.value, to: e.target.value }))}
          />
        </label>
        <input
          className="w-40 rounded bg-slate-800 px-2 py-1.5 text-sm"
          placeholder={t(sales.billNoPlaceholder)}
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
        />
        <div className="flex gap-1">
          {/* key 用通路代碼而不是標籤：標籤會跟著語言變，拿它當 key
              等於每換一次語言就把這四顆按鈕全部拆掉重建。 */}
          {(
            [
              [null, sales.channelAll],
              ['dine_in', sales.channelDineIn],
              ['takeout', sales.channelTakeout],
              ['delivery', sales.channelDelivery],
            ] as const
          ).map(([v, label]) => (
            <Preset
              key={v ?? 'all'}
              label={t(label)}
              active={channel === v}
              onClick={() => setChannel(v)}
            />
          ))}
        </div>
        <label className="flex items-center gap-2 pb-1.5 text-sm text-slate-300">
          <input
            type="checkbox"
            checked={refundedOnly}
            onChange={(e) => setRefundedOnly(e.target.checked)}
          />
          {t(sales.refundedOnly)}
        </label>

        <div className="ml-auto flex items-end gap-4">
          {data && (
            <>
              <Stat label={t(sales.statCount)} value={String(data.count)} />
              <Stat label={t(sales.statRevenue)} value={formatMoney(data.total)} big />
              {data.refundedTotal > 0 && (
                <Stat label={t(sales.statRefunded)} value={formatMoney(-data.refundedTotal)} warn />
              )}
            </>
          )}
          <button
            className="rounded bg-emerald-800 px-4 py-2 text-sm hover:bg-emerald-700 disabled:opacity-40"
            disabled={busy || !data || data.count === 0}
            title={t(sales.exportHint)}
            onClick={() => void exportXlsx()}
          >
            {t(sales.exportExcel)}
          </button>
        </div>
      </section>

      {error && (
        <p className="mb-3 rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}
      {note && (
        <p className="mb-3 flex items-start gap-3 rounded border border-emerald-800 bg-emerald-950/50 px-3 py-2 text-sm text-emerald-200">
          <span className="flex-1 break-all">{note}</span>
          <button className="shrink-0 text-emerald-400" onClick={() => setNote(null)}>
            ✕
          </button>
        </p>
      )}

      {data && data.byMethod.length > 0 && (
        <div className="mb-3 flex flex-wrap gap-2 text-sm">
          {data.byMethod.map((m) => (
            <span key={m.method} className="rounded bg-slate-900 px-3 py-1.5">
              {m.method}
              <span className="ml-2 font-mono text-slate-300">{formatMoney(m.amount)}</span>
              {m.refunded > 0 && (
                <span className="ml-1 text-xs text-amber-400">
                  {t(sales.refundShort, { amount: formatMoney(m.refunded) })}
                </span>
              )}
            </span>
          ))}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {!data ? (
          <p className="py-10 text-center text-sm text-slate-600">{t(ui.loading)}</p>
        ) : data.sales.length === 0 ? (
          <p className="py-10 text-center text-sm text-slate-600">{t(sales.noSalesInRange)}</p>
        ) : (
          <>
            {data.truncated && (
              <p className="mb-2 text-xs text-amber-400">{t(sales.truncated)}</p>
            )}
            {data.sales.map((s) => (
              <SaleRow
                key={s.billId}
                sale={s}
                expanded={open === s.billId}
                onToggle={() => setOpen((o) => (o === s.billId ? null : s.billId))}
              />
            ))}
          </>
        )}
      </div>
    </div>
  )
}

function SaleRow({
  sale,
  expanded,
  onToggle,
}: {
  sale: Sale
  expanded: boolean
  onToggle: () => void
}) {
  const t = useT()
  return (
    <div className="mb-1 rounded bg-slate-900/50">
      <button
        className="flex w-full items-baseline gap-3 px-3 py-2 text-left text-sm hover:bg-slate-800/60"
        onClick={onToggle}
      >
        <span className="w-4 shrink-0 text-slate-600">{expanded ? '▾' : '▸'}</span>
        <span className="w-20 shrink-0 font-mono text-xs text-slate-500">
          {sale.businessDate.slice(5)} {hhmm(sale.settledAt)}
        </span>
        <span className="w-16 shrink-0 font-mono text-xs text-slate-400">
          {sale.billNo.split('-').pop()}
        </span>
        <span className="w-10 shrink-0 text-xs text-slate-500">{sale.channelLabel}</span>
        {sale.tableLabel && (
          <span className="shrink-0 text-xs text-sky-300">{sale.tableLabel}</span>
        )}
        {sale.splitLabel && (
          <span className="shrink-0 rounded bg-slate-800 px-1 text-xs text-slate-400">
            {t(sales.splitLabel, { label: sale.splitLabel })}
          </span>
        )}
        <span className="min-w-0 flex-1 truncate text-xs text-slate-500">
          {sale.lines
            .filter((l) => !l.voided)
            .map((l) => l.name)
            .join(t(sales.listSeparator))}
        </span>
        {sale.refundedTotal > 0 && (
          <span className="shrink-0 text-xs text-amber-400">
            {t(sales.refundedAmount, { amount: formatMoney(sale.refundedTotal) })}
          </span>
        )}
        <span className="w-20 shrink-0 text-right font-mono">{formatMoney(sale.grandTotal)}</span>
      </button>

      {expanded && (
        <div className="border-t border-slate-800 px-3 py-2 pl-11 text-sm">
          {sale.lines.map((l, i) => (
            <div
              key={i}
              className={`flex items-start gap-3 py-0.5 ${
                l.voided ? 'text-slate-600 line-through' : ''
              }`}
            >
              <span className="w-8 shrink-0 text-right text-slate-500">
                {l.qtyMilli % 1000 === 0 ? l.qtyMilli / 1000 : (l.qtyMilli / 1000).toFixed(1)}
              </span>
              <span className="min-w-0 flex-1">
                {l.name}
                {l.variantName && (
                  <span className="text-slate-500">
                    {t(sales.variantSuffix, { name: l.variantName })}
                  </span>
                )}
                {/* 當時點的選項也要留著 —— 「這杯到底是不是半糖」是客訴時
                    唯一能回答問題的東西。 */}
                {l.options.length > 0 && (
                  <span className="block text-xs text-amber-300/80">
                    {l.options.join(t(sales.listSeparator))}
                  </span>
                )}
                {l.note && <span className="block text-xs text-sky-300/80">※ {l.note}</span>}
              </span>
              <span className="w-16 shrink-0 text-right font-mono text-slate-400">
                {formatMoney(l.amount)}
              </span>
            </div>
          ))}

          <div className="mt-2 flex flex-wrap gap-x-6 gap-y-1 border-t border-slate-800/60 pt-2 text-xs text-slate-500">
            <span>{t(sales.subtotal, { amount: formatMoney(sale.subtotal) })}</span>
            {sale.discountTotal !== 0 && (
              <span>{t(sales.discount, { amount: formatMoney(sale.discountTotal) })}</span>
            )}
            {sale.serviceCharge !== 0 && (
              <span>{t(sales.serviceCharge, { amount: formatMoney(sale.serviceCharge) })}</span>
            )}
            <span>
              {t(sales.taxLine, {
                net: formatMoney(sale.salesAmount),
                tax: formatMoney(sale.taxAmount),
              })}
            </span>
            {sale.payments.map((p, i) => (
              <span key={i} className="text-slate-400">
                {p.method} {formatMoney(p.amount)}
                {p.refunded > 0 && (
                  <span className="ml-1 text-amber-400">
                    {t(sales.refundShort, { amount: formatMoney(p.refunded) })}
                  </span>
                )}
              </span>
            ))}
            {sale.settledBy && <span>{t(sales.settledBy, { name: sale.settledBy })}</span>}
            <span className="font-mono">{sale.orderNo}</span>
          </div>
        </div>
      )}
    </div>
  )
}

function Stat({
  label,
  value,
  big,
  warn,
}: {
  label: string
  value: string
  big?: boolean
  warn?: boolean
}) {
  return (
    <div className="text-right">
      <div className="text-xs text-slate-500">{label}</div>
      <div
        className={`font-mono ${big ? 'text-2xl text-emerald-300' : 'text-lg'} ${
          warn ? 'text-amber-300' : ''
        }`}
      >
        {value}
      </div>
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

/** 最近 n 天（含今天）。用瀏覽器的本地日期 —— UTC 會在凌晨少算一天。 */
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

function days(r: { from: string; to: string }): number {
  return Math.round((Date.parse(r.to) - Date.parse(r.from)) / 86_400_000)
}
