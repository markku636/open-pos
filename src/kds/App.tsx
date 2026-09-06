import { useState } from 'react'

import { useBoard, type Connection } from './useBoard'
import { kdsApi, type KdsLine, type KdsTicket } from '@/shared/api'

/**
 * 廚房顯示。
 *
 * # 這一頁的設計前提
 *
 * 看的人正在做菜：手是濕的、油的，而且離螢幕一公尺遠。所以：
 *
 * * 字大、按鈕大，一張單就是一個大方塊
 * * 顏色只表達一件事：**這張等多久了**
 * * 一整項就是一個按鈕，不必瞄準小圖示
 *
 * # 紙本才是第一真相
 *
 * 這面板是**輔助顯示**。主機故障時，紙本出單機是唯一還會出單的東西；
 * 純 KDS、沒有出單機的店家沒有安全的降級路徑 —— 這句話寫在 README 裡，
 * 而不是藏起來。
 */
export default function App() {
  const { board, connection, silentFor, reconnect } = useBoard()
  const [station, setStation] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)

  const tickets = board?.tickets ?? []
  const stations = Array.from(
    new Set(
      tickets.flatMap((t) => t.lines.map((l) => l.stationName)).filter((s): s is string => !!s),
    ),
  )

  const shown = station
    ? tickets
        .map((t) => ({ ...t, lines: t.lines.filter((l) => l.stationName === station) }))
        .filter((t) => t.lines.length > 0)
    : tickets

  const advance = async (line: KdsLine) => {
    setBusy(line.id)
    try {
      // 只往前推一格。廚師按的是「這一項好了」，不是選一個狀態。
      await kdsApi.advance(line.id, line.status === 'ready' ? 'served' : 'ready')
    } catch {
      /* 下一份快照會把真實狀態帶回來 —— 不必自己補救 */
    } finally {
      setBusy(null)
    }
  }

  return (
    <main className="min-h-screen bg-slate-950 text-slate-100">
      <ConnectionBanner state={connection} silentFor={silentFor} onRetry={reconnect} />

      <header className="flex flex-wrap items-center gap-2 px-4 py-3">
        <h1 className="mr-2 text-xl font-semibold">廚房</h1>
        {stations.length > 1 && (
          <>
            <StationTab active={station === null} onClick={() => setStation(null)}>
              全部
            </StationTab>
            {stations.map((s) => (
              <StationTab key={s} active={station === s} onClick={() => setStation(s)}>
                {s}
              </StationTab>
            ))}
          </>
        )}
        <span className="ml-auto text-sm text-slate-500">{shown.length} 張單</span>
      </header>

      {shown.length === 0 ? (
        <p className="px-4 py-24 text-center text-2xl text-slate-700">
          {board ? '沒有待做的單' : '連線中…'}
        </p>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3 px-4 pb-8">
          {shown.map((t) => (
            <Ticket key={t.orderId} ticket={t} busy={busy} onAdvance={advance} />
          ))}
        </div>
      )}
    </main>
  )
}

/**
 * 連線狀態。
 *
 * ★ 這條橫幅是整個 KDS 最重要的一個元件。
 *
 * 「看起來還連著但收不到單」是最惡劣的失敗模式：廚師不會發現自己少做了
 * 一份餐，客人會。所以斷線的時候要**很難不注意到**。
 */
function ConnectionBanner({
  state,
  silentFor,
  onRetry,
}: {
  state: Connection
  silentFor: number
  onRetry: () => void
}) {
  if (state === 'live') return null
  const lost = state === 'lost'
  return (
    <div
      className={`flex items-center gap-3 px-4 py-3 text-lg ${
        lost ? 'animate-pulse bg-red-800 text-red-50' : 'bg-slate-800 text-slate-300'
      }`}
    >
      <span className="text-2xl">{lost ? '⚠' : '…'}</span>
      <span>
        {lost
          ? `跟收銀機斷線了 —— 現在看到的單可能是舊的（已經 ${silentFor} 秒沒有訊息）`
          : '連線中…'}
      </span>
      {lost && (
        <button
          className="ml-auto rounded bg-red-950/60 px-4 py-2 text-base hover:bg-red-950"
          onClick={onRetry}
        >
          重新連線
        </button>
      )}
    </div>
  )
}

function StationTab({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      className={`rounded px-4 py-2 text-base ${
        active ? 'bg-sky-800 text-sky-50' : 'bg-slate-900 text-slate-400'
      }`}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

function Ticket({
  ticket,
  busy,
  onAdvance,
}: {
  ticket: KdsTicket
  busy: string | null
  onAdvance: (line: KdsLine) => void
}) {
  // 顏色只表達一件事：等多久了。五分鐘內是正常的，十分鐘以上要有人注意。
  const mins = Math.floor(ticket.waitingSeconds / 60)
  const urgency = mins >= 10 ? 'late' : mins >= 5 ? 'slow' : 'ok'
  const tone = {
    late: 'border-red-600 bg-red-950/40',
    slow: 'border-amber-600 bg-amber-950/30',
    ok: 'border-slate-700 bg-slate-900',
  }[urgency]
  const clockTone = { late: 'text-red-300', slow: 'text-amber-300', ok: 'text-slate-500' }[urgency]

  return (
    <section className={`rounded-lg border-2 ${tone} p-3`}>
      <header className="flex items-baseline gap-2 border-b border-slate-700/60 pb-2">
        <span className="font-mono text-lg">{ticket.orderNo.split('-').pop()}</span>
        <span className="text-sm text-slate-400">{ticket.channelLabel}</span>
        {ticket.tableLabel && <span className="text-sm text-sky-300">{ticket.tableLabel}</span>}
        <span className={`ml-auto font-mono text-lg ${clockTone}`}>{mins}分</span>
      </header>

      <ul className="mt-2 space-y-1">
        {ticket.lines.map((l) => (
          <li key={l.id}>
            <LineRow line={l} busy={busy === l.id} onAdvance={() => onAdvance(l)} />
          </li>
        ))}
      </ul>
    </section>
  )
}

/** 一整項就是一個按鈕：廚師的手是濕的，不該需要瞄準小圖示。 */
function LineRow({
  line,
  busy,
  onAdvance,
}: {
  line: KdsLine
  busy: boolean
  onAdvance: () => void
}) {
  const done = line.status === 'ready'
  const qty = line.qtyMilli % 1000 === 0 ? line.qtyMilli / 1000 : (line.qtyMilli / 1000).toFixed(1)

  return (
    <button
      className={`flex w-full items-start gap-3 rounded px-2 py-3 text-left transition disabled:opacity-50 ${
        done ? 'bg-emerald-900/40 text-emerald-200 line-through' : 'hover:bg-slate-800'
      }`}
      disabled={busy}
      onClick={onAdvance}
    >
      <span className="w-8 shrink-0 text-right font-mono text-xl">{qty}</span>
      <span className="min-w-0 flex-1">
        <span className="text-xl leading-snug">
          {line.name}
          {line.variantName && <span className="text-slate-400">（{line.variantName}）</span>}
        </span>
        {/* 加購與備註要用不同顏色：它們是廚師最容易漏掉的兩件事。 */}
        {line.options.length > 0 && (
          <span className="block text-base text-amber-300">{line.options.join('、')}</span>
        )}
        {line.note && <span className="block text-base text-sky-300">※ {line.note}</span>}
      </span>
      {done && <span className="shrink-0 text-2xl">✓</span>}
    </button>
  )
}
