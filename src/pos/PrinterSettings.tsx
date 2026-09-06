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

/**
 * 出單機設定。
 *
 * 這一頁的目標不是「把所有欄位攤出來」，而是讓一位不懂電腦的店家能自己走完
 * 「接上機器 → 測試連線 → 印一張測試單 → 決定哪一區印哪一台」。
 * 每一個失敗都必須說出下一步該做什麼，不能只說「失敗」。
 */
export default function PrinterSettings() {
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
          <h2 className="text-sm font-semibold uppercase tracking-wide text-slate-400">出單機</h2>
          <button
            className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700"
            onClick={() => setEditing(blankPrinter())}
          >
            新增
          </button>
        </header>

        {printers.length === 0 && !editing && (
          <p className="rounded border border-slate-800 bg-slate-900/40 px-4 py-6 text-sm text-slate-400">
            還沒有設定任何出單機。
            <span className="mt-1 block text-xs text-slate-500">
              最常見的接法是網路型：把印表機接上店裡的網路，在它印出來的自我測試頁上找到
              IP，連接埠填 9100。
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
              <span className="font-mono text-xs text-slate-500">{describe(p.transport)}</span>
              <span className="rounded bg-slate-800 px-2 py-0.5 text-xs text-slate-400">
                {p.caps.paper === 'mm58' ? '58mm' : '80mm'}
              </span>
              {p.lastProbeOk === true && <span className="text-xs text-emerald-400">● 連得上</span>}
              {p.lastProbeOk === false && (
                <span className="text-xs text-amber-400" title={p.lastError ?? ''}>
                  ▲ 連不上
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
                  測試連線
                </button>
                <button
                  className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 hover:text-slate-200"
                  disabled={busy}
                  title="印一張看得懂就代表設定正確的紙"
                  onClick={() => void run(() => printerApi.testPrint(p.id), `已送出測試單到「${p.name}」`)}
                >
                  測試列印
                </button>
                <button
                  className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 hover:text-slate-200"
                  onClick={() => setEditing(toInput(p))}
                >
                  編輯
                </button>
                <button
                  className="rounded px-2 py-1 text-xs text-slate-500 hover:bg-slate-800 hover:text-red-400"
                  disabled={busy}
                  onClick={() => void run(() => printerApi.remove(p.id), `已移除「${p.name}」`)}
                >
                  移除
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
              }, '已儲存')
            }
          />
        )}
      </section>

      {/* ─── 出單分區 ─────────────────────────────────── */}
      <section>
        <header className="mb-1 flex items-center gap-3">
          <h2 className="text-sm font-semibold uppercase tracking-wide text-slate-400">出單分區</h2>
          <button
            className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700"
            disabled={busy}
            onClick={() => {
              const name = prompt('分區名稱（例如：飲料吧、熱炒區）')?.trim()
              if (name) void run(() => printerApi.upsertStation({ name }), `已新增「${name}」`)
            }}
          >
            新增
          </button>
        </header>
        <p className="mb-3 text-xs text-slate-500">
          分區是「飲料吧」這種穩定的概念，底下綁哪一台機器是設定。
          品項直接綁印表機的話，換一台機器就要去改幾百筆菜單。
        </p>

        {stations.length === 0 ? (
          <p className="rounded border border-slate-800 bg-slate-900/40 px-4 py-4 text-sm text-slate-400">
            還沒有分區。只有一台機器的店家不需要設，所有單都會印到那一台。
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
                      ? '⚠ 還沒綁機器 —— 這一區的單會印到櫃檯並標成代印'
                      : `${s.printers.length} 台`}
                  </span>
                  <button
                    className="ml-auto rounded px-2 py-1 text-xs text-slate-500 hover:bg-slate-800 hover:text-red-400"
                    disabled={busy}
                    onClick={() => void run(() => printerApi.removeStation(s.id), `已刪除「${s.name}」`)}
                  >
                    刪除
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
                              ? '每一次都印（例如櫃檯留底）'
                              : '這一區的主要機器'
                            : '點一下綁上去'
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
                        {bound && (bound.mode === 'always' ? ' · 每次' : ' · 主要')}
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
          最近的單
        </h2>
        <p className="mb-3 text-xs text-slate-500">
          印失敗的單會留在這裡。POS 最常見的客訴是「廚房沒收到單」，
          所以這一頁存在的意義就是讓失敗看得見。
        </p>

        {jobs.length === 0 ? (
          <p className="text-sm text-slate-600">還沒有列印紀錄。</p>
        ) : (
          <div className="space-y-1">
            {jobs.map((j) => (
              <div
                key={j.id}
                className="flex flex-wrap items-center gap-3 rounded bg-slate-900/40 px-3 py-2 text-sm"
              >
                <span className={statusColor(j.status)}>{statusLabel(j.status)}</span>
                <span className="text-slate-400">{j.printerName}</span>
                {j.stationName && <span className="text-xs text-slate-500">{j.stationName}</span>}
                <span className="text-xs text-slate-600">{reasonLabel(j.reason)}</span>
                <span className="font-mono text-xs text-slate-600">
                  {j.createdAt.slice(11, 16)}
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
                    onClick={() => void run(() => printerApi.retryJob(j.id), '已放回佇列')}
                  >
                    補印
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

function describe(t: Transport): string {
  switch (t.kind) {
    case 'network':
      return `${t.host}:${t.port}`
    case 'file':
      return t.path
    case 'usb':
      return `USB ${t.vid.toString(16)}:${t.pid.toString(16)}`
    case 'bluetooth':
      return `藍牙 ${t.addr}`
  }
}

function statusLabel(s: PrintJob['status']): string {
  return (
    {
      pending: '● 排隊中',
      printing: '● 列印中',
      done: '● 已印出',
      failed: '▲ 失敗',
      dead: '■ 印不出來',
      cancelled: '— 已取消',
    } as const
  )[s]
}

function statusColor(s: PrintJob['status']): string {
  if (s === 'done') return 'text-emerald-400'
  if (s === 'dead') return 'text-red-400'
  if (s === 'failed') return 'text-amber-400'
  if (s === 'cancelled') return 'text-slate-600'
  return 'text-sky-400'
}

function reasonLabel(r: string): string {
  return (
    ({
      new_order: '新單',
      add_items: '加點',
      void: '取消',
      reprint: '補印',
      settle: '結帳',
    } as Record<string, string>)[r] ?? r
  )
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
  const [form, setForm] = useState<PrinterInput>(value)
  const net = form.transport.kind === 'network' ? form.transport : null

  return (
    <div className="mt-3 space-y-3 rounded border border-slate-700 bg-slate-900 p-4">
      <div className="flex flex-wrap gap-3">
        <label className="flex flex-col gap-1 text-xs text-slate-400">
          名稱
          <input
            className="w-48 rounded bg-slate-800 px-3 py-2 text-sm text-slate-100"
            placeholder="櫃檯、飲料吧…"
            autoFocus
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
          />
        </label>

        <label className="flex flex-col gap-1 text-xs text-slate-400">
          IP 位址
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
          連接埠
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
          紙寬
          <select
            className="rounded bg-slate-800 px-3 py-2 text-sm text-slate-100"
            value={form.paper}
            onChange={(e) => setForm({ ...form, paper: e.target.value as PrinterInput['paper'] })}
          >
            <option value="mm80">80mm（48 欄）</option>
            <option value="mm58">58mm（32 欄）</option>
          </select>
        </label>

        <label className="flex flex-col gap-1 text-xs text-slate-400">
          中文編碼
          <select
            className="rounded bg-slate-800 px-3 py-2 text-sm text-slate-100"
            value={form.encoding}
            onChange={(e) =>
              setForm({ ...form, encoding: e.target.value as PrinterInput['encoding'] })
            }
          >
            <option value="big5">Big5（台灣機常見）</option>
            <option value="gb18030">GB18030（陸製機常見）</option>
            <option value="utf8">UTF-8（少數新款）</option>
          </select>
        </label>
      </div>

      <p className="text-xs text-slate-500">
        大部分出單機的連接埠都是 9100。IP 可以在機器的自我測試頁上找到（多數機型是
        按住走紙鍵再開機會印出來）。
        <span className="mt-1 block">
          印出來變成問號或亂碼，通常是編碼選錯了 —— 換一個再按「測試列印」。
        </span>
      </p>

      <div className="flex gap-2">
        <button
          className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700"
          onClick={onCancel}
        >
          取消
        </button>
        <button
          className="rounded bg-emerald-700 px-4 py-2 text-sm font-medium hover:bg-emerald-600 disabled:opacity-40"
          disabled={busy || !form.name.trim()}
          onClick={() => onSave(form)}
        >
          儲存
        </button>
      </div>
    </div>
  )
}
