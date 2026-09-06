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

// ---------------------------------------------------------------- 商品

export interface Variant {
  id: string
  itemId: string
  code: string
  name: string
  /** delta = 跟著品項基本價加減；absolute = 自己一個固定價。 */
  priceMode: 'delta' | 'absolute'
  price: number
  priceDelta: number
  isDefault: boolean
  sortOrder: number
  isActive: boolean
}

export interface Item {
  id: string
  categoryId: string | null
  name: string
  shortName: string | null
  /** 整數元（見 docs/adr/0001-money-as-integer-dollars.md）。 */
  basePrice: number
  taxCode: string
  isOpenPrice: boolean
  soldOutUntil: string | null
  sortOrder: number
  isActive: boolean
  variants: Variant[]
}

export interface Category {
  id: string
  name: string
  color: string | null
  sortOrder: number
  isActive: boolean
}

export interface CategoryNode extends Category {
  items: Item[]
}

export interface MenuTree {
  categories: CategoryNode[]
  /** 沒有分類的品項。刻意單獨列出來 —— 藏起來的話老闆會以為商品不見了。 */
  uncategorized: Item[]
}

export interface CategoryInput {
  id?: string
  name: string
  color?: string | null
  sortOrder?: number
  isActive?: boolean
}

export interface ItemInput {
  id?: string
  categoryId?: string | null
  name: string
  shortName?: string | null
  basePrice: number
  taxCode?: string
  isOpenPrice?: boolean
  soldOutUntil?: string | null
  sortOrder?: number
  isActive?: boolean
}

export interface VariantInput {
  id?: string
  itemId: string
  code: string
  name: string
  priceMode: 'delta' | 'absolute'
  price?: number
  priceDelta?: number
  isDefault?: boolean
  sortOrder?: number
  isActive?: boolean
}

/** 商品維護。
 *
 * 讀（menuTree）同時開在區網端點上 —— KDS 與顧客手機都要看得到品名與價格。
 * 寫（upsert / delete）**只走 Tauri IPC**，所以就算 LAN server 有漏洞，
 * 也改不到菜單與價格。
 */
export const menuApi = {
  tree: () => transport.call<MenuTree>('menu_tree'),
  upsertCategory: (input: CategoryInput) =>
    transport.call<Category>('upsert_category', { input }),
  deleteCategory: (id: string) => transport.call<void>('delete_category', { id }),
  upsertItem: (input: ItemInput) => transport.call<Item>('upsert_item', { input }),
  deleteItem: (id: string) => transport.call<void>('delete_item', { id }),
  upsertVariant: (input: VariantInput) =>
    transport.call<Variant>('upsert_variant', { input }),
  deleteVariant: (id: string) => transport.call<void>('delete_variant', { id }),
}
