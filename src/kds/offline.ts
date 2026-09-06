import type { KdsBoard, KdsStatus } from '@/shared/api'

/**
 * 廚房平板的離線儲存。
 *
 * # 為什麼是 IndexedDB 而不是 Service Worker
 *
 * 區網走的是 HTTP（掃 QR 開私有 IP 不跳警告，但**不是** secure context），
 * 而 Service Worker 只在 secure context 下能用。IndexedDB 沒有這個限制。
 *
 * # 它解決兩個不同的問題
 *
 * 1. **白畫面。** 平板被 Android 回收之後重開，或是廚房 AP 剛好在重啟 ——
 *    沒有快取的話廚師會盯著「連線中…」，而爐子上的東西還在煮。
 *    存一份最後的看板，重開時先畫出來（標明是舊的），連上之後自動蓋掉。
 * 2. **斷線期間按的「完成」。** 那些點擊在這之前是直接消失的：
 *    畫面上打了勾，伺服器沒收到，於是那一項在收銀端永遠停在「製作中」。
 *    改成寫進本地佇列，連上之後照順序重放。
 *
 * # 重放為什麼安全
 *
 * `kds_advance` 是**只能往前推**的：把已經 ready 的行再推一次 ready 會被
 * 靜靜忽略。所以「送出了但沒收到回應」的那一筆重放不會造成任何傷害 ——
 * 寧可重放，不可漏掉。
 *
 * # 為什麼不用 idb / dexie
 *
 * 這裡只有兩個 object store 與五個操作。一個 3KB 的相依會讓 KDS 的
 * bundle 變大，而廚房平板往往是三年前的 Android。
 */

const DB_NAME = 'openpos-kds'
const DB_VERSION = 1
const STORE_BOARD = 'board'
const STORE_QUEUE = 'queue'

/** 斷線期間按的一次「完成」。 */
export interface QueuedAdvance {
  /** 前端產生的序號。重放的順序就是按下去的順序。 */
  seq: number
  lineId: string
  to: KdsStatus
  at: number
}

let dbPromise: Promise<IDBDatabase | null> | null = null

function open(): Promise<IDBDatabase | null> {
  if (dbPromise) return dbPromise
  dbPromise = new Promise((resolve) => {
    // 無痕視窗、關掉網站資料、或某些 kiosk 瀏覽器會讓它整個不存在。
    // 那不是錯誤 —— 只是沒有離線能力，其餘功能照常。
    if (typeof indexedDB === 'undefined') return resolve(null)
    let req: IDBOpenDBRequest
    try {
      req = indexedDB.open(DB_NAME, DB_VERSION)
    } catch {
      return resolve(null)
    }
    req.onupgradeneeded = () => {
      const db = req.result
      if (!db.objectStoreNames.contains(STORE_BOARD)) db.createObjectStore(STORE_BOARD)
      if (!db.objectStoreNames.contains(STORE_QUEUE)) {
        db.createObjectStore(STORE_QUEUE, { keyPath: 'seq' })
      }
    }
    req.onsuccess = () => resolve(req.result)
    req.onerror = () => resolve(null)
    req.onblocked = () => resolve(null)
  })
  return dbPromise
}

function run<T>(
  store: string,
  mode: IDBTransactionMode,
  fn: (s: IDBObjectStore) => IDBRequest,
): Promise<T | null> {
  return open().then(
    (db) =>
      new Promise<T | null>((resolve) => {
        if (!db) return resolve(null)
        try {
          const tx = db.transaction(store, mode)
          const req = fn(tx.objectStore(store))
          req.onsuccess = () => resolve(req.result as T)
          req.onerror = () => resolve(null)
          tx.onabort = () => resolve(null)
        } catch {
          resolve(null)
        }
      }),
  )
}

/** 最後一份看板。重開時先畫它，免得廚師盯著空畫面。 */
export function saveBoard(board: KdsBoard): void {
  void run(STORE_BOARD, 'readwrite', (s) => s.put({ board, at: Date.now() }, 'last'))
}

export async function loadBoard(): Promise<{ board: KdsBoard; at: number } | null> {
  return (await run<{ board: KdsBoard; at: number }>(STORE_BOARD, 'readonly', (s) =>
    s.get('last'),
  )) as { board: KdsBoard; at: number } | null
}

/** 把一次「完成」排進佇列。回傳排進去的那一筆。 */
export async function enqueue(lineId: string, to: KdsStatus): Promise<QueuedAdvance> {
  const pending = await queued()
  const item: QueuedAdvance = {
    // 用「目前最大 + 1」而不是時間戳：平板的時鐘會跳（NTP、手動改），
    // 而重放的順序必須等於按下去的順序。
    seq: pending.reduce((m, q) => Math.max(m, q.seq), 0) + 1,
    lineId,
    to,
    at: Date.now(),
  }
  await run(STORE_QUEUE, 'readwrite', (s) => s.put(item))
  return item
}

export async function queued(): Promise<QueuedAdvance[]> {
  const all = (await run<QueuedAdvance[]>(STORE_QUEUE, 'readonly', (s) => s.getAll())) ?? []
  return all.sort((a, b) => a.seq - b.seq)
}

export function dequeue(seq: number): void {
  void run(STORE_QUEUE, 'readwrite', (s) => s.delete(seq))
}
