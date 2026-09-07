import { useCallback, useEffect, useState } from 'react'

import {
  printerApi,
  type AppError,
  type BindingMode,
  type PrintJob,
  type Printer,
  type PrinterInput,
  type Station,
  type Transport,
} from '@/shared/api'
import { useT, type Msg } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { printer } from '@/shared/locales/printer'
import { dateTime } from '@/shared/time'

/** 翻譯函式的型別。模組層的小工具（`describe` 等）拿不到 hook，只能用參數傳。 */
type Translate = ReturnType<typeof useT>

/**
 * 出單機設定。
 *
 * 這一頁的目標不是「把所有欄位攤出來」，而是讓一位不懂電腦的店家能自己走完
 * 「接上機器 → 測試連線 → 印一張測試單 → 決定哪一區印哪一台」。
 * 每一個失敗都必須說出下一步該做什麼，不能只說「失敗」。
 */
export default function PrinterSettings() {
  const t = useT()
  const [printers, setPrinters] = useState<Printer[]>([])
  const [stations, setStations] = useState<Station[]>([])
  const [jobs, setJobs] = useState<PrintJob[]>([])
  const [editing, setEditing] = useState<PrinterInput | null>(null)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null)

  const reload = useCallback(async () => {
    try {
      const [p, s, j] = await Promise.all([
        printerApi.list(),
        printerApi.stations(),
        printerApi.jobs(30),
      ])
      setPrinters(p)
      setStations(s)
      setJobs(j)
    } catch (e) {
      setMessage({ ok: false, text: (e as AppError).message ?? String(e) })
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const run = async (fn: () => Promise<unknown>, okText?: string) => {
    setBusy(true)
    setMessage(null)
    try {
      await fn()
      if (okText) setMessage({ ok: true, text: okText })
      await reload()
    } catch (e) {
      setMessage({ ok: false, text: (e as AppError).message ?? String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="h-full space-y-8 overflow-y-auto pr-2">
      {message && (
        <p
          className={`rounded px-3 py-2 text-sm ${
            message.ok
              ? 'border border-emerald-800 bg-emerald-950/60 text-emerald-200'
              : 'border border-red-800 bg-red-950/60 text-red-200'
          }`}
        >
          {message.text}
        </p>
      )}

      {/* ─── 印表機 ───────────────────────────────────── */}
      <section>
        <header className="mb-3 flex items-center gap-3">
          <h2 className="text-sm font-semibold uppercase tracking-wide text-slate-400">
            {t(printer.sectionPrinters)}
          </h2>
          <button
            className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700"
            onClick={() => setEditing(blankPrinter())}
          >
            {t(ui.add)}
          </button>
        </header>

        {printers.length === 0 && !editing && (
          <p className="rounded border border-slate-800 bg-slate-900/40 px-4 py-6 text-sm text-slate-400">
            {t(printer.emptyPrinters)}
            <span className="mt-1 block text-xs text-slate-500">
              {t(printer.emptyPrintersHint)}
            </span>
          </p>
        )}

        <div className="space-y-2">
          {printers.map((p) => (
            <div
              key={p.id}
              className="flex flex-wrap items-center gap-3 rounded border border-slate-800 bg-slate-900/40 px-4 py-3"
            >
              <span className="font-medium">{p.name}</span>
              <span className="font-mono text-xs text-slate-500">{describe(p.transport, t)}</span>
              <span className="rounded bg-slate-800 px-2 py-0.5 text-xs text-slate-400">
                {p.caps.paper === 'mm58' ? '58mm' : '80mm'}
              </span>
              {p.lastProbeOk === true && (
                <span className="text-xs text-emerald-400">{t(printer.probeOk)}</span>
              )}
              {p.lastProbeOk === false && (
                <span className="text-xs text-amber-400" title={p.lastError ?? ''}>
                  {t(printer.probeFail)}
                </span>
              )}

              <div className="ml-auto flex gap-1">
                <button
                  className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 hover:text-slate-200"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      const r = await printerApi.probe(p.id)
                      setMessage({ ok: r.ok, text: r.detail })
                    })
                  }
                >
                  {t(printer.testConnection)}
                </button>
                <button
                  className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 hover:text-slate-200"
                  disabled={busy}
                  title={t(printer.testPrintHint)}
                  onClick={() =>
                    void run(
                      () => printerApi.testPrint(p.id),
                      t(printer.testPrintSent, { name: p.name }),
                    )
                  }
                >
                  {t(printer.testPrint)}
                </button>
                <button
                  className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 hover:text-slate-200"
                  onClick={() => setEditing(toInput(p))}
                >
                  {t(printer.edit)}
                </button>
                <button
                  className="rounded px-2 py-1 text-xs text-slate-500 hover:bg-slate-800 hover:text-red-400"
                  disabled={busy}
                  onClick={() =>
                    void run(() => printerApi.remove(p.id), t(printer.removed, { name: p.name }))
                  }
                >
                  {t(printer.remove)}
                </button>
              </div>
            </div>
          ))}
        </div>

        {editing && (
          <PrinterForm
            value={editing}
            busy={busy}
            onCancel={() => setEditing(null)}
            onSave={(input) =>
              void run(async () => {
                await printerApi.upsert(input)
                setEditing(null)
              }, t(printer.saved))
            }
          />
        )}
      </section>

      {/* ─── 出單分區 ─────────────────────────────────── */}
      <section>
        <header className="mb-1 flex items-center gap-3">
          <h2 className="text-sm font-semibold uppercase tracking-wide text-slate-400">
            {t(printer.sectionStations)}
          </h2>
          <button
            className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700"
            disabled={busy}
            onClick={() => {
              const name = prompt(t(printer.stationNamePrompt))?.trim()
              if (name)
                void run(
                  () => printerApi.upsertStation({ name }),
                  t(printer.stationAdded, { name }),
                )
            }}
          >
            {t(ui.add)}
          </button>
        </header>
        <p className="mb-3 text-xs text-slate-500">{t(printer.stationsHint)}</p>

        {stations.length === 0 ? (
          <p className="rounded border border-slate-800 bg-slate-900/40 px-4 py-4 text-sm text-slate-400">
            {t(printer.emptyStations)}
          </p>
        ) : (
          <div className="space-y-2">
            {stations.map((s) => (
              <div
                key={s.id}
                className="rounded border border-slate-800 bg-slate-900/40 px-4 py-3"
              >
                <div className="flex flex-wrap items-center gap-3">
                  <span className="font-medium">{s.name}</span>
                  <span className="text-xs text-slate-500">
                    {s.printers.length === 0
                      ? t(printer.stationUnbound)
                      : t(printer.printerCount, { n: s.printers.length })}
                  </span>
                  <button
                    className="ml-auto rounded px-2 py-1 text-xs text-slate-500 hover:bg-slate-800 hover:text-red-400"
                    disabled={busy}
                    onClick={() =>
                      void run(
                        () => printerApi.removeStation(s.id),
                        t(printer.stationDeleted, { name: s.name }),
                      )
                    }
                  >
                    {t(ui.delete)}
                  </button>
                </div>

                <div className="mt-2 flex flex-wrap gap-1">
                  {printers.map((p) => {
                    const bound = s.printers.find((b) => b.printerId === p.id)
                    return (
                      <button
                        key={p.id}
                        disabled={busy}
                        className={`rounded px-2.5 py-1 text-xs ${
                          bound
                            ? 'bg-sky-800 text-sky-50'
                            : 'bg-slate-800 text-slate-400 hover:bg-slate-700'
                        }`}
                        title={
                          bound
                            ? bound.mode === 'always'
                              ? t(printer.bindAlwaysHint)
                              : t(printer.bindPrimaryHint)
                            : t(printer.bindHint)
                        }
                        onClick={() =>
                          void run(() =>
                            printerApi.upsertStation({
                              id: s.id,
                              name: s.name,
                              printers: nextBindings(s.printers, p.id),
                            }),
                          )
                        }
                      >
                        {p.name}
                        {bound &&
                          ` · ${t(bound.mode === 'always' ? printer.bindAlways : printer.bindPrimary)}`}
                      </button>
                    )
                  })}
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* ─── 列印紀錄 ─────────────────────────────────── */}
      <section>
        <h2 className="mb-1 text-sm font-semibold uppercase tracking-wide text-slate-400">
          {t(printer.sectionJobs)}
        </h2>
        <p className="mb-3 text-xs text-slate-500">{t(printer.jobsHint)}</p>

        {jobs.length === 0 ? (
          <p className="text-sm text-slate-600">{t(printer.emptyJobs)}</p>
        ) : (
          <div className="space-y-1">
            {jobs.map((j) => (
              <div
                key={j.id}
                className="flex flex-wrap items-center gap-3 rounded bg-slate-900/40 px-3 py-2 text-sm"
              >
                <span className={statusColor(j.status)}>{statusLabel(j.status, t)}</span>
                <span className="text-slate-400">{j.printerName}</span>
                {j.stationName && <span className="text-xs text-slate-500">{j.stationName}</span>}
                <span className="text-xs text-slate-600">{reasonLabel(j.reason, t)}</span>
                <span className="font-mono text-xs text-slate-600">
                  {dateTime(j.createdAt)}
                </span>
                {j.lastError && (
                  <span className="min-w-0 flex-1 truncate text-xs text-amber-300" title={j.lastError}>
                    {j.lastError}
                  </span>
                )}
                {(j.status === 'dead' || j.status === 'failed') && (
                  <button
                    className="ml-auto rounded bg-slate-800 px-2 py-1 text-xs hover:bg-slate-700"
                    disabled={busy}
                    onClick={() => void run(() => printerApi.retryJob(j.id), t(printer.requeued))}
                  >
                    {t(printer.reprint)}
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

/** 點一下在「沒綁 → 主要 → 每次都印 → 沒綁」之間輪替。 */
function nextBindings(
  current: Station['printers'],
  printerId: string,
): Station['printers'] {
  const bound = current.find((b) => b.printerId === printerId)
  const others = current.filter((b) => b.printerId !== printerId)
  if (!bound) {
    return [...others, { printerId, priority: others.length, mode: 'failover' as BindingMode }]
  }
  if (bound.mode === 'failover') {
    return [...others, { ...bound, mode: 'always' as BindingMode }]
  }
  return others
}

function blankPrinter(): PrinterInput {
  return {
    name: '',
    // 網路型是預設：它是唯一不需要裝驅動程式的接法。
    transport: { kind: 'network', host: '', port: 9100 },
    paper: 'mm80',
    encoding: 'big5',
    cutter: true,
    renderMode: 'text',
    isActive: true,
  }
}

function toInput(p: Printer): PrinterInput {
  return {
    id: p.id,
    name: p.name,
    transport: p.transport,
    paper: p.caps.paper,
    encoding: p.caps.encoding,
    cutter: p.caps.cutter,
    drawer: p.caps.drawer,
    statusQuery: p.caps.statusQuery,
    renderMode: p.renderMode,
    isActive: p.isActive,
  }
}

function describe(transport: Transport, t: Translate): string {
  switch (transport.kind) {
    case 'network':
      return `${transport.host}:${transport.port}`
    case 'file':
      return transport.path
    case 'usb':
      return `USB ${transport.vid.toString(16)}:${transport.pid.toString(16)}`
    case 'bluetooth':
      return t(printer.bluetooth, { addr: transport.addr })
  }
}

function statusLabel(s: PrintJob['status'], t: Translate): string {
  return t(
    (
      {
        pending: printer.jobPending,
        printing: printer.jobPrinting,
        done: printer.jobDone,
        failed: printer.jobFailed,
        dead: printer.jobDead,
        cancelled: printer.jobCancelled,
      } as const
    )[s],
  )
}

function statusColor(s: PrintJob['status']): string {
  if (s === 'done') return 'text-emerald-400'
  if (s === 'dead') return 'text-red-400'
  if (s === 'failed') return 'text-amber-400'
  if (s === 'cancelled') return 'text-slate-600'
  return 'text-sky-400'
}

function reasonLabel(r: string, t: Translate): string {
  const msg = (
    {
      new_order: printer.reasonNewOrder,
      add_items: printer.reasonAddItems,
      void: printer.reasonVoid,
      reprint: printer.reasonReprint,
      settle: printer.reasonSettle,
    } as Record<string, Msg | undefined>
  )[r]
  // 認不得的原因原樣印出來：那是後端新加的類型，顯示英文代碼也比顯示空白好查。
  return msg ? t(msg) : r
}

function PrinterForm({
  value,
  busy,
  onCancel,
  onSave,
}: {
  value: PrinterInput
  busy: boolean
  onCancel: () => void
  onSave: (input: PrinterInput) => void
}) {
  const t = useT()
  const [form, setForm] = useState<PrinterInput>(value)
  const net = form.transport.kind === 'network' ? form.transport : null

  return (
    <div className="mt-3 space-y-3 rounded border border-slate-700 bg-slate-900 p-4">
      <div className="flex flex-wrap gap-3">
        <label className="flex flex-col gap-1 text-xs text-slate-400">
          {t(printer.fieldName)}
          <input
            className="w-48 rounded bg-slate-800 px-3 py-2 text-sm text-slate-100"
            placeholder={t(printer.namePlaceholder)}
            autoFocus
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
          />
        </label>

        <label className="flex flex-col gap-1 text-xs text-slate-400">
          {t(printer.fieldHost)}
          <input
            className="w-40 rounded bg-slate-800 px-3 py-2 font-mono text-sm text-slate-100"
            placeholder="192.168.1.23"
            value={net?.host ?? ''}
            onChange={(e) =>
              setForm({
                ...form,
                transport: { kind: 'network', host: e.target.value, port: net?.port ?? 9100 },
              })
            }
          />
        </label>

        <label className="flex flex-col gap-1 text-xs text-slate-400">
          {t(printer.fieldPort)}
          <input
            className="w-24 rounded bg-slate-800 px-3 py-2 font-mono text-sm text-slate-100"
            inputMode="numeric"
            value={net?.port ?? 9100}
            onChange={(e) =>
              setForm({
                ...form,
                transport: {
                  kind: 'network',
                  host: net?.host ?? '',
                  port: Number(e.target.value) || 9100,
                },
              })
            }
          />
        </label>

        <label className="flex flex-col gap-1 text-xs text-slate-400">
          {t(printer.fieldPaper)}
          <select
            className="rounded bg-slate-800 px-3 py-2 text-sm text-slate-100"
            value={form.paper}
            onChange={(e) => setForm({ ...form, paper: e.target.value as PrinterInput['paper'] })}
          >
            <option value="mm80">{t(printer.paper80)}</option>
            <option value="mm58">{t(printer.paper58)}</option>
          </select>
        </label>

        <label className="flex flex-col gap-1 text-xs text-slate-400">
          {t(printer.fieldEncoding)}
          <select
            className="rounded bg-slate-800 px-3 py-2 text-sm text-slate-100"
            value={form.encoding}
            onChange={(e) =>
              setForm({ ...form, encoding: e.target.value as PrinterInput['encoding'] })
            }
          >
            <option value="big5">{t(printer.encodingBig5)}</option>
            <option value="gb18030">{t(printer.encodingGb18030)}</option>
            <option value="utf8">{t(printer.encodingUtf8)}</option>
          </select>
        </label>
      </div>

      <p className="text-xs text-slate-500">
        {t(printer.formHint)}
        <span className="mt-1 block">{t(printer.formHintEncoding)}</span>
      </p>

      <div className="flex gap-2">
        <button
          className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700"
          onClick={onCancel}
        >
          {t(ui.cancel)}
        </button>
        <button
          className="rounded bg-emerald-700 px-4 py-2 text-sm font-medium hover:bg-emerald-600 disabled:opacity-40"
          disabled={busy || !form.name.trim()}
          onClick={() => onSave(form)}
        >
          {t(ui.save)}
        </button>
      </div>
    </div>
  )
}
