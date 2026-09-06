import { useCallback, useEffect, useRef, useState } from 'react'

import { dequeue, enqueue, queued } from './offline'
import { useBoard, type Connection } from './useBoard'
import { kdsApi, type KdsLine, type KdsStatus, type KdsTicket } from '@/shared/api'

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
 *
 * # 斷線時按的「完成」不會消失
 *
 * 那些點擊在這之前是直接不見的：畫面上打了勾、伺服器沒收到，於是那一項在
 * 收銀端永遠停在製作中。現在它們進本地佇列，連上之後照按下去的順序重放。
 * 重放安全，因為狀態只能往前推 —— 重複的那一次會被靜靜忽略。
 */
export default function App() {
  const { board, connection, silentFor, stale, reconnect } = useBoard()
  const [station, setStation] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const { pending, push, replay } = useOfflineQueue()

  // 樂觀套用還沒送出去的那幾筆。廚師按下去就要看到勾 ——
  // 不然他會以為沒按到，然後再按一次。
  const tickets = (board?.tickets ?? []).map((t) => ({
    ...t,
    lines: t.lines.map((l) => (pending[l.id] ? { ...l, status: pending[l.id] } : l)),
  }))
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
    // 只往前推一格。廚師按的是「這一項好了」，不是選一個狀態。
    const to = line.status === 'ready' ? 'served' : 'ready'
    setBusy(line.id)
    try {
      await kdsApi.advance(line.id, to)
    } catch {
      // ★ 送不出去就存起來，不要讓這一下消失。
      //   下一份快照會把伺服器的真實狀態帶回來，而佇列會在連上之後重放。
      await push(line.id, to)
    } finally {
      setBusy(null)
    }
  }

  // 連上就重放。放在這裡而不是 useBoard 裡，是因為重放是**寫入**——
  // 而 useBoard 只負責讀。
  useEffect(() => {
    if (connection === 'live') void replay()
  }, [connection, replay])

  const pendingCount = Object.keys(pending).length

  return (
    <main className="min-h-screen bg-slate-950 text-slate-100">
      <ConnectionBanner
        state={connection}
        silentFor={silentFor}
        stale={stale}
        pending={pendingCount}
        onRetry={reconnect}
      />

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
  stale,
  pending,
  onRetry,
}: {
  state: Connection
  silentFor: number
  stale: boolean
  pending: number
  onRetry: () => void
}) {
  if (state === 'live' && pending === 0) return null
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
          : stale
            ? '連線中… 現在畫的是上一次的單'
            : '連線中…'}
      </span>
      {/* 還沒送出去的那幾下要說出來。廚師會想知道「我剛剛按的到底算不算」。 */}
      {pending > 0 && (
        <span className="rounded bg-black/30 px-3 py-1 text-base">
          {pending} 個「完成」還沒送出去，連上就會補送
        </span>
      )}
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

/** 超過這個時間還沒送出去的「完成」就丟掉。跨過一個班之後它已經沒有意義。 */
const STALE_ACTION_MS = 8 * 60 * 60 * 1000

/**
 * 斷線期間按的「完成」。
 *
 * `pending` 是 lineId → 目標狀態，畫面拿它做樂觀顯示；真正的順序存在
 * IndexedDB 裡，因為平板重開之後那些點擊還是要送出去。
 */
function useOfflineQueue() {
  const [pending, setPending] = useState<Record<string, KdsStatus>>({})
  const replaying = useRef(false)

  const refresh = useCallback(async () => {
    const items = await queued()
    const map: Record<string, KdsStatus> = {}
    for (const q of items) map[q.lineId] = q.to
    setPending(map)
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const push = useCallback(
    async (lineId: string, to: KdsStatus) => {
      await enqueue(lineId, to)
      await refresh()
    },
    [refresh],
  )

  const replay = useCallback(async () => {
    // 兩個 effect 同時觸發重放會把同一筆送兩次。送兩次本身是安全的
    // （狀態只能往前推），但會讓佇列數字閃來閃去。
    if (replaying.current) return
    replaying.current = true
    try {
      for (const q of await queued()) {
        // ★ 太舊的就丟掉。
        //   隔了一個班之後才送出去的「完成」已經沒有意義，而它會讓橫幅上
        //   那個數字永遠掛著 —— 一個永遠不會歸零的警告等於沒有警告。
        if (Date.now() - q.at > STALE_ACTION_MS) {
          dequeue(q.seq)
          continue
        }
        try {
          await kdsApi.advance(q.lineId, q.to)
          dequeue(q.seq)
        } catch (e) {
          // ★ 「伺服器說不行」與「連不上伺服器」要分開處理。
          //
          //   那一項被退掉了、單被作廢了 —— 伺服器會回一個不可重試的錯誤，
          //   而這一筆永遠不會成功。不丟掉的話它會把整個佇列堵死，
          //   後面每一筆真正該送出去的「完成」都送不出去。
          const err = e as { code?: string; retryable?: boolean }
          if (typeof err?.code === 'string' && !err.retryable) {
            dequeue(q.seq)
            continue
          }
          // 連不上或伺服器暫時有問題：停在這裡保住順序，下次連上再繼續。
          break
        }
      }
      await refresh()
    } finally {
      replaying.current = false
    }
  }, [refresh])

  return { pending, push, replay }
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
