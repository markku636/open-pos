/**
 * 唯一的 API 橋接層（對標 db-kit 的 src/api.ts）。
 *
 * 三個 entry（收銀 / KDS / 掃碼點餐）都只 import 這一份，
 * 完全不需要知道自己跑在 Tauri 還是瀏覽器裡。
 *
 * 型別對應後端的 serde 定義（`src-tauri/src/services/app.rs`），
 * 後端統一用 camelCase 輸出，所以兩邊的欄位名一致。
 */
import { transport } from './transport'

export interface AppInfo {
  version: string
  dataDir: string
  schemaVersion: number
  startedAt: string
}

export interface HealthItem {
  name: string
  ok: boolean
  /** ok 為 false 時，這裡必須說得出「該怎麼辦」。 */
  detail: string
}

export interface Health {
  ok: boolean
  items: HealthItem[]
}

export const api = {
  appInfo: () => transport.call<AppInfo>('app_info'),
  /** 健康檢查。不健康時後端回 503，transport 會丟出 AppError。 */
  health: () => transport.call<Health>('health'),
}

export { transport }
export type { AppError } from './transport'
