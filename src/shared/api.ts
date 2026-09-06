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

// ---------------------------------------------------------------- 點餐與結帳

export type Channel = 'dine_in' | 'takeout' | 'delivery'

export interface OrderLine {
  id: string
  lineNo: number
  name: string
  variantName: string | null
  options: string[]
  note: string | null
  /** 數量 × 1000。半份是 500。 */
  qtyMilli: number
  unitPrice: number
  amount: number
}

export interface Order {
  id: string
  orderNo: string
  status: 'draft' | 'placed' | 'in_progress' | 'ready' | 'served' | 'settled' | 'voided'
  /** 樂觀鎖版本。寫入時必須帶回去，不符會回 409。 */
  rev: number
  channel: Channel
  tableId: string | null
  tableLabel: string | null
  guestCount: number
  businessDate: string
  lines: OrderLine[]
  subtotal: number
  serviceCharge: number
  roundingAdjustment: number
  grandTotal: number
  salesAmount: number
  taxAmount: number
  paidTotal: number
  changeTotal: number
}

export interface SettleResult {
  order: Order
  billNo: string
  change: number
}

export interface PaymentMethod {
  code: string
  name: string
  kind: string
  /** 只有現金這類方式能找零。刷卡「找零」是不存在的東西。 */
  allowsChange: boolean
  opensDrawer: boolean
}

export interface NewLine {
  itemId: string
  variantId?: string | null
  qtyMilli?: number
  modifierIds?: string[]
  note?: string | null
}

export interface PaymentInput {
  methodCode: string
  /** 這一筆要沖銷帳單多少。 */
  amount: number
  /** 客人實際遞出來的錢。只有現金會與 amount 不同。 */
  tendered?: number | null
  refNo?: string | null
}

/** 前端產生的冪等鍵。Wi-Fi 抖動時重送不會重複收款。 */
export function newIdemKey(): string {
  return crypto.randomUUID()
}

export const orderApi = {
  open: (channel: Channel, tableId?: string | null, guestCount?: number) =>
    transport.call<Order>('open_order', {
      req: { channel, tableId: tableId ?? null, guestCount: guestCount ?? 1, clientId: newIdemKey() },
    }),
  get: (id: string) => transport.call<Order>('get_order', { id }),
  listOpen: () => transport.call<Order[]>('list_open_orders'),
  addLines: (orderId: string, expectedRev: number, lines: NewLine[]) =>
    transport.call<Order>('add_lines', { req: { orderId, expectedRev, lines } }),
  voidLine: (orderId: string, expectedRev: number, lineId: string) =>
    transport.call<Order>('void_line', { orderId, expectedRev, lineId, reasonId: null }),
  settle: (orderId: string, expectedRev: number, payments: PaymentInput[]) =>
    transport.call<SettleResult>('settle', {
      req: { orderId, expectedRev, payments, idemKey: newIdemKey() },
    }),
  paymentMethods: () => transport.call<PaymentMethod[]>('payment_methods'),
}
