/**
 * 檢查 GitHub 上有沒有更新的 Release（抄自 db-kit 的 src/updateCheck.ts）。
 *
 * 純前端直打 GitHub API：它的回應帶 `Access-Control-Allow-Origin: *`，
 * 而 tauri.conf.json 的 `security.csp` 是 null，所以打包後的 webview 也通得過。
 *
 * ★ 對 POS 有一條 db-kit 沒有的硬規則：**任何失敗都必須安靜略過。**
 * 店裡的網路可能整天不通、也可能被防火牆擋在內網。
 * 一個「查不到有沒有新版」的錯誤訊息，絕不能出現在正在收錢的畫面上。
 *
 * 預設**不**在啟動時自動查（與 db-kit 相反）：POS 是地端優先的軟體，
 * 開機就往外打 API 這件事應該由店家自己決定。
 */
import { REPO } from './brand'

const CACHE_KEY = 'open-pos:update'
/** 每天最多打一次 API —— GitHub 匿名額度是 60 次/小時，而店裡可能有多台機器共用同一個對外 IP。 */
const TTL_MS = 24 * 60 * 60 * 1000
const AUTO_KEY = 'open-pos:updateCheck'

/** 啟動時是否自動檢查更新。預設關閉 —— 地端軟體不該自作主張連外。 */
export function autoCheckEnabled(): boolean {
  try {
    return localStorage.getItem(AUTO_KEY) === '1'
  } catch {
    return false
  }
}

export function setAutoCheckEnabled(on: boolean) {
  try {
    localStorage.setItem(AUTO_KEY, on ? '1' : '0')
  } catch {
    /* 無痕視窗或停用 storage：忽略 */
  }
}

interface Cache {
  checkedAt: number
  version: string
  url: string
}

export interface UpdateInfo {
  version: string
  url: string
}

/**
 * 語意化版本比較：latest 是否比 current 新。
 *
 * 逐段做**數值**比較而不是字典序 —— `0.2.10` 必須大於 `0.2.9`。
 * 這是版本比較最常見的一個 bug，而且要到第十個版本才會浮出來。
 */
export function isNewer(latest: string, current: string): boolean {
  const parse = (v: string): number[] =>
    String(v)
      .trim()
      .replace(/^v/i, '')
      .split(/[-+]/)[0]
      .split('.')
      .map((s) => {
        const n = parseInt(s, 10)
        return Number.isFinite(n) ? n : 0
      })
  const a = parse(latest)
  const b = parse(current)
  const len = Math.max(a.length, b.length)
  for (let i = 0; i < len; i++) {
    const d = (a[i] ?? 0) - (b[i] ?? 0)
    if (d !== 0) return d > 0
  }
  return false
}

function readCache(): Cache | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY)
    if (!raw) return null
    const c = JSON.parse(raw) as Cache
    if (
      typeof c?.checkedAt === 'number' &&
      typeof c?.version === 'string' &&
      typeof c?.url === 'string'
    ) {
      return c
    }
  } catch {
    /* 快取毀損：當作沒有 */
  }
  return null
}

/**
 * 取得最新 Release。
 *
 * 只負責「查最新版是幾號」，「有沒有比較新」由呼叫端用 `isNewer` 判斷。
 * 離線、rate limit、還沒發過 release —— 一律回既有快取或 null，不丟例外。
 *
 * `force`：略過 TTL 直接打 API（使用者主動按「檢查更新」就該真的去查）。
 */
export async function checkForUpdate(opts?: { force?: boolean }): Promise<UpdateInfo | null> {
  const cached = readCache()
  const fallback = (): UpdateInfo | null =>
    cached ? { version: cached.version, url: cached.url } : null
  if (!opts?.force && cached && Date.now() - cached.checkedAt < TTL_MS) return fallback()
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
      headers: { Accept: 'application/vnd.github+json' },
    })
    if (!res.ok) return fallback()
    const data = (await res.json()) as { tag_name?: string; html_url?: string }
    const version = (data.tag_name ?? '').trim().replace(/^v/i, '')
    if (!version) return fallback()
    const url = data.html_url || `https://github.com/${REPO}/releases/latest`
    try {
      localStorage.setItem(
        CACHE_KEY,
        JSON.stringify({ checkedAt: Date.now(), version, url } satisfies Cache),
      )
    } catch {
      /* 忽略寫入失敗 */
    }
    return { version, url }
  } catch {
    return fallback()
  }
}
