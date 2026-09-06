/**
 * 兩種 transport 的最小共同介面。
 *
 * 收銀機跑在 Tauri webview 裡（走 IPC），KDS 與顧客手機跑在真瀏覽器裡（走 HTTP）。
 * 如果不抽象，每個 API 呼叫都會變成 `if (isTauri) invoke() else fetch()`，
 * 這種條件會滲透到整個 codebase。
 *
 * 關鍵設計是後端把 HTTP 做成 **RPC over HTTP**（`POST /api/rpc/{name}`，
 * name 與 Tauri command 名稱 1:1），兩個實作才能長得一模一樣。
 *
 * 回應形狀也刻意對齊：成功時直接回 T 的 JSON（**不包 envelope**），
 * 失敗時回非 2xx + `{ error: AppError }`。因為 Tauri 的 invoke 成功會 resolve 出 T、
 * 失敗會 reject 出 AppError —— HTTP 端對齊這個形狀，api.ts 才能只有一份。
 */

/** 後端 AppError 的序列化形狀（見 src-tauri/src/error.rs）。 */
export interface AppError {
  kind: string
  /** 機器可讀、與顯示語言無關。前端據此分支，不要去比對 message。 */
  code: string
  message: string
  retryable: boolean
}

export interface Transport {
  /** 呼叫一個具名指令。名稱與 Rust 的 #[tauri::command] 函式名 1:1。 */
  call<T>(name: string, args?: Record<string, unknown>): Promise<T>
  /** 這個 transport 的名字，只用於診斷顯示。 */
  readonly kind: 'tauri' | 'http'
}

/** Tauri 2 會在 webview 注入 __TAURI_INTERNALS__。 */
export function isTauri(): boolean {
  return (
    typeof window !== 'undefined' &&
    ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)
  )
}

const tauriTransport: Transport = {
  kind: 'tauri',
  async call<T>(name: string, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<T>(name, args ?? {})
  },
}

const httpTransport: Transport = {
  kind: 'http',
  async call<T>(name: string, args?: Record<string, unknown>): Promise<T> {
    const res = await fetch(`/api/rpc/${name}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(args ?? {}),
    })
    if (!res.ok) {
      // 與 invoke 的 reject 同形：拋出 AppError 本身，不是 Error 包裝。
      const body = (await res.json().catch(() => null)) as { error?: AppError } | null
      throw (
        body?.error ?? {
          kind: 'internal',
          code: 'ERR_INTERNAL',
          message: `HTTP ${res.status}`,
          retryable: res.status >= 500,
        }
      )
    }
    return res.status === 204 ? (undefined as T) : ((await res.json()) as T)
  },
}

export const transport: Transport = isTauri() ? tauriTransport : httpTransport
