import { useCallback, useEffect, useState } from 'react'

import { auditApi, type AppError, type AuditGroup, type AuditReport } from '@/shared/api'
import { useT } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { reports } from '@/shared/locales/reports'
import { formatMoney } from '@/shared/money'
import { hhmm } from '@/shared/time'

/**
 * 稽核紀錄。
 *
 * # 這一頁存在的理由
 *
 * `audit_logs` 從第一天就在寫，但在這之前**沒有任何地方讀得到它** ——
 * 一份沒有人看得到的紀錄，防弊效果等於零。
 *
 * # 單看一筆永遠是合理的
 *
 * 「這一單免了 200 元」單看沒有問題，看一整個月的分布才看得出問題。
 * 所以這一頁最上面是**統計**，明細在下面 —— 而不是相反。
 *
 * 統計是另外查的，不是拿明細那幾百筆算的。一份「只統計到前 500 筆」的
 * 防弊報表比沒有更糟，因為它看起來是完整的。
 */
export default function AuditPanel() {
  const t = useT()
  const [report, setReport] = useState<AuditReport | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const [moneyOnly, setMoneyOnly] = useState(true)
  const [action, setAction] = useState<string | null>(null)

  const run = useCallback(async () => {
    try {
      setReport(
        await auditApi.query({
          from: from || null,
          to: to || from || null,
          action,
          moneyOnly,
        }),
      )
      setError(null)
    } catch (e) {
      setReport(null)
      setError((e as AppError).message ?? String(e))
    }
  }, [from, to, action, moneyOnly])

  useEffect(() => {
    void run()
  }, [run])

  // 日期空白＝今天，所以第一次進來就有東西看，不必先設條件。
  useEffect(() => {
    if (report && !from) setFrom(report.from)
  }, [report, from])

  return (
    <div className="h-full space-y-5 overflow-y-auto pr-2">
      <section className="flex flex-wrap items-end gap-3 rounded border border-slate-800 bg-slate-900/40 px-4 py-3">
        <label>
          <span className="mb-1 block text-xs text-slate-500">{t(reports.from)}</span>
          <input
            type="date"
            className="rounded bg-slate-800 px-2 py-1.5 text-sm"
            value={from}
            onChange={(e) => setFrom(e.target.value)}
          />
        </label>
        <label>
          <span className="mb-1 block text-xs text-slate-500">{t(reports.toSameDay)}</span>
          <input
            type="date"
            className="rounded bg-slate-800 px-2 py-1.5 text-sm"
            value={to}
            onChange={(e) => setTo(e.target.value)}
          />
        </label>
        <label className="flex items-center gap-2 pb-1.5 text-sm text-slate-300">
          <input
            type="checkbox"
            checked={moneyOnly}
            onChange={(e) => setMoneyOnly(e.target.checked)}
          />
          {t(reports.moneyOnly)}
        </label>
        {action && (
          <button
            className="rounded bg-sky-900 px-3 py-1.5 text-sm hover:bg-sky-800"
            onClick={() => setAction(null)}
          >
            {t(reports.filterOnly, {
              label: report?.byAction.find((g) => g.key === action)?.label ?? action,
            })}
          </button>
        )}
        <span className="ml-auto pb-1.5 text-right">
          <span className="block text-xs text-slate-500">{t(reports.moneyMoved)}</span>
          <span
            className={`font-mono text-xl ${
              (report?.totalAmount ?? 0) < 0 ? 'text-amber-300' : 'text-slate-300'
            }`}
          >
            {formatMoney(report?.totalAmount ?? 0, { sign: true })}
          </span>
        </span>
      </section>

      {error && (
        <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}

      {report && (
        <>
          <div className="grid gap-4 md:grid-cols-2">
            <Groups
              title={t(reports.byAction)}
              groups={report.byAction}
              active={action}
              onPick={(k) => setAction((a) => (a === k ? null : k))}
            />
            <Groups title={t(reports.byActor)} groups={report.byActor} />
          </div>

          <section>
            <h3 className="mb-2 text-sm text-slate-400">
              {t(reports.details)}
              {report.truncated && (
                <span className="ml-2 text-amber-400">{t(reports.truncated)}</span>
              )}
            </h3>
            {report.rows.length === 0 ? (
              <p className="rounded bg-slate-900/40 px-3 py-8 text-center text-sm text-slate-600">
                {t(reports.noRecords)}
              </p>
            ) : (
              <div className="overflow-x-auto rounded border border-slate-800">
                <table className="w-full text-sm">
                  <thead className="bg-slate-900/60 text-left text-xs text-slate-500">
                    <tr>
                      <th className="px-3 py-2">{t(reports.colTime)}</th>
                      <th className="px-3 py-2">{t(reports.colAction)}</th>
                      <th className="px-3 py-2">{t(reports.colTarget)}</th>
                      <th className="px-3 py-2">{t(reports.colActor)}</th>
                      <th className="px-3 py-2">{t(reports.colReason)}</th>
                      <th className="px-3 py-2">{t(reports.colApproval)}</th>
                      <th className="px-3 py-2 text-right">{t(reports.colAmount)}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {report.rows.map((r) => (
                      <tr key={r.id} className="border-t border-slate-800/60">
                        <td className="px-3 py-1.5 font-mono text-xs text-slate-500">
                          {r.businessDate !== report.from && (
                            <span className="mr-1">{r.businessDate?.slice(5)}</span>
                          )}
                          {hhmm(r.at)}
                        </td>
                        <td className="px-3 py-1.5">{r.actionLabel}</td>
                        <td className="px-3 py-1.5 font-mono text-xs text-slate-400">
                          {r.label ?? r.entityId.slice(-6)}
                        </td>
                        <td className="px-3 py-1.5 text-slate-400">{r.actorName ?? '—'}</td>
                        <td className="px-3 py-1.5 text-slate-400">{r.reasonName ?? ''}</td>
                        {/* 簽核欄有值＝這是一個需要主管授權的動作。
                            它跟金額欄放在一起，因為「誰批准了這筆錢」是同一個問題。 */}
                        <td className="px-3 py-1.5 text-xs text-sky-300">
                          {r.approvedByName ?? ''}
                        </td>
                        <td
                          className={`px-3 py-1.5 text-right font-mono ${
                            (r.amountDelta ?? 0) < 0 ? 'text-amber-300' : 'text-slate-500'
                          }`}
                        >
                          {r.amountDelta ? formatMoney(r.amountDelta, { sign: true }) : ''}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        </>
      )}
    </div>
  )
}

function Groups({
  title,
  groups,
  active,
  onPick,
}: {
  title: string
  groups: AuditGroup[]
  active?: string | null
  onPick?: (key: string) => void
}) {
  const t = useT()
  const worst = Math.max(1, ...groups.map((g) => Math.abs(g.amount)))
  return (
    <section className="rounded border border-slate-800 bg-slate-900/40 p-3">
      <h3 className="mb-2 text-sm text-slate-400">{title}</h3>
      {groups.length === 0 ? (
        <p className="py-4 text-center text-xs text-slate-600">{t(ui.noData)}</p>
      ) : (
        <ul className="space-y-1">
          {groups.map((g) => (
            <li key={g.key}>
              <button
                className={`flex w-full items-baseline gap-2 rounded px-2 py-1 text-sm ${
                  active === g.key ? 'bg-sky-900/60' : onPick ? 'hover:bg-slate-800' : ''
                }`}
                disabled={!onPick}
                onClick={() => onPick?.(g.key)}
              >
                <span className="w-28 shrink-0 truncate text-left">{g.label}</span>
                <span className="w-10 shrink-0 text-right text-xs text-slate-500">
                  {g.count}
                </span>
                {/* 長條圖只是為了讓「哪一項特別多」不必用眼睛比數字。 */}
                <span className="h-2 flex-1 overflow-hidden rounded bg-slate-800">
                  <span
                    className={`block h-full ${g.amount < 0 ? 'bg-amber-600' : 'bg-slate-600'}`}
                    style={{ width: `${(Math.abs(g.amount) / worst) * 100}%` }}
                  />
                </span>
                <span
                  className={`w-20 shrink-0 text-right font-mono text-xs ${
                    g.amount < 0 ? 'text-amber-300' : 'text-slate-500'
                  }`}
                >
                  {g.amount ? formatMoney(g.amount, { sign: true }) : ''}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
