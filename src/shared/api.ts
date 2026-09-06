/**
 * 唯一的 API 橋接層（對標 db-kit 的 src/api.ts）。
 *
 * 三個 entry（收銀 / KDS / 掃碼點餐）都只 import 這一份，
 * 完全不需要知道自己跑在 Tauri 還是瀏覽器裡。
 */
import { transport } from './transport'

export interface AppInfo {
  version: string
  dataDir: string
  transport: 'tauri' | 'http'
}

export const api = {
  /** M0 佔位：證明兩種 transport 都通得了。 */
  appInfo: () => transport.call<AppInfo>('app_info'),
}

export { transport }
export type { AppError } from './transport'
