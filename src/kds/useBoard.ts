import { useCallback, useEffect, useRef, useState } from 'react'

import { loadBoard, saveBoard } from './offline'
import type { KdsBoard } from '@/shared/api'

/** 多久沒收到任何東西就認定連線死了。伺服器每 16 秒送一次心跳。 */
const WATCHDOG_MS = 45_000
/** 重連的退避。廚房在等單，所以上限抓得很短。 */
const RECONNECT_MIN_MS = 1_000
const RECONNECT_MAX_MS = 10_000

export type Connection = 'connecting' | 'live' | 'lost'

/**
 * 訂閱廚房看板。
 *
 * # 為什麼要自己做看門狗
 *
 * 瀏覽器的 `EventSource` **沒有 read timeout**。爛 AP 與平板省電會靜默切斷
 * 閒置的 TCP 連線，而瀏覽器不會知道 —— 症狀是「看起來還連著但收不到單」。
 *
 * 這是這整個功能最惡劣的失敗模式：**無聲的漏單**。廚師不會發現自己少做了
 * 一份餐，客人會。所以不能依賴內建的重連，要自己數秒。
 *
 * # 為什麼收到的是整份快照
 *
 * 快照是自我修正的：斷線期間發生的任何變化，都會在重連後的第一份快照裡
 * 直接反映。增量重播則會讓錯誤永久累積。
 *
 * # 為什麼要存一份到 IndexedDB
 *
 * 平板被 Android 回收之後重開、或廚房 AP 剛好在重啟時，沒有快取的話廚師會
 * 盯著「連線中…」，而爐子上的東西還在煮。所以先畫最後一份看板（標明是舊的），
 * 連上之後自動蓋掉。
 */
export function useBoard(): {
  board: KdsBoard | null
  connection: Connection
  /** 最後一次收到任何訊息（含心跳）到現在幾秒。 */
  silentFor: number
  /** 畫面上這份是從本地快取來的（還沒連上）。 */
  stale: boolean
  reconnect: () => void
} {
  const [board, setBoard] = useState<KdsBoard | null>(null)
  const [connection, setConnection] = useState<Connection>('connecting')
  const [silentFor, setSilentFor] = useState(0)
  const [stale, setStale] = useState(false)

  // 開場先畫本地那一份。第一份推播進來就會蓋掉它。
  useEffect(() => {
    let live = true
    void loadBoard().then((cached) => {
      if (!live || !cached) return
      setBoard((b) => {
        if (b) return b
        setStale(true)
        return cached.board
      })
    })
    return () => {
      live = false
    }
  }, [])

  const lastMessageAt = useRef(Date.now())
  const source = useRef<EventSource | null>(null)
  const attempt = useRef(0)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const connect = useCallback(() => {
    source.current?.close()
    setConnection((c) => (c === 'live' ? 'live' : 'connecting'))

    const es = new EventSource('/api/events/kds')
    source.current = es

    const touch = () => {
      lastMessageAt.current = Date.now()
      attempt.current = 0
      setConnection('live')
    }

    es.addEventListener('board', (e) => {
      touch()
      try {
        const next = JSON.parse((e as MessageEvent).data) as KdsBoard
        setBoard(next)
        setStale(false)
        saveBoard(next)
      } catch {
        /* 壞掉的一筆略過，下一次快照會蓋掉它 */
      }
    })
    es.addEventListener('heartbeat', touch)
    es.onopen = touch
    es.onerror = () => {
      // 這裡只代表「瀏覽器覺得斷了」。真正的判斷交給看門狗 ——
      // 最危險的情況正是瀏覽器**不覺得**斷了的時候。
      setConnection('lost')
    }
  }, [])

  useEffect(() => {
    connect()
    return () => source.current?.close()
  }, [connect])

  // 看門狗。每秒檢查一次「多久沒收到東西了」。
  useEffect(() => {
    const id = setInterval(() => {
      const silent = Date.now() - lastMessageAt.current
      setSilentFor(Math.floor(silent / 1000))
      if (silent > WATCHDOG_MS) {
        setConnection('lost')
        // 主動關掉再重連。不能等瀏覽器 —— 它根本不知道連線已經死了。
        attempt.current += 1
        const delay = Math.min(
          RECONNECT_MAX_MS,
          RECONNECT_MIN_MS * 2 ** Math.min(attempt.current, 4),
        )
        if (!timer.current) {
          timer.current = setTimeout(() => {
            timer.current = null
            lastMessageAt.current = Date.now()
            connect()
          }, delay)
        }
      }
    }, 1000)
    return () => clearInterval(id)
  }, [connect])

  const reconnect = useCallback(() => {
    attempt.current = 0
    lastMessageAt.current = Date.now()
    connect()
  }, [connect])

  return { board, connection, silentFor, stale, reconnect }
}
