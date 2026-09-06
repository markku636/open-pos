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
  /**
   * 用系統預設瀏覽器開啟外部連結。
   *
   * 只有 Tauri 這一側有這個 command —— 區網那邊的呼叫者是顧客手機與廚房平板，
   * 讓他們叫「主機」開瀏覽器是一個真的安全漏洞。所以瀏覽器模式改用 window.open，
   * 開的是「使用者自己那台裝置」的瀏覽器。
   */
  openExternal: (url: string) =>
    transport.kind === 'tauri'
      ? transport.call<void>('open_external', { url })
      : Promise.resolve(void window.open(url, '_blank', 'noopener,noreferrer')),
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

// ---------------------------------------------------------------- 出單機

/**
 * 傳輸方式。後端的 serde tag 是 `kind`（見 infra::printer::Transport）。
 *
 * v1 只實作網路型與檔案；USB 與藍牙的形狀先留著，之後補實作時
 * 前端不必再動一次型別。
 */
export type Transport =
  | { kind: 'network'; host: string; port: number }
  | { kind: 'file'; path: string; append: boolean }
  | { kind: 'usb'; vid: number; pid: number; serial?: string | null }
  | { kind: 'bluetooth'; addr: string; channel: number }

export type PaperWidth = 'mm58' | 'mm80'
export type CjkEncoding = 'big5' | 'gb18030' | 'utf8'

export interface PrinterCaps {
  paper: PaperWidth
  raster: boolean
  cutter: boolean
  drawer: boolean
  statusQuery: boolean
  encoding: CjkEncoding
}

export interface Printer {
  id: string
  name: string
  transport: Transport
  caps: PrinterCaps
  renderMode: 'raster' | 'text'
  isActive: boolean
  lastProbeAt?: string | null
  lastProbeOk?: boolean | null
  lastError?: string | null
}

export interface PrinterInput {
  id?: string | null
  name: string
  transport: Transport
  paper: PaperWidth
  encoding?: CjkEncoding
  cutter?: boolean
  drawer?: boolean
  statusQuery?: boolean
  renderMode?: 'raster' | 'text'
  isActive?: boolean
}

export interface ProbeResult {
  ok: boolean
  detail: string
}

export type BindingMode = 'failover' | 'always'

export interface StationPrinter {
  printerId: string
  priority: number
  mode: BindingMode
}

export interface Station {
  id: string
  name: string
  template: 'kitchen' | 'drink' | 'receipt' | 'label'
  splitPerItem: boolean
  sortOrder: number
  isActive: boolean
  printers: StationPrinter[]
}

export interface StationInput {
  id?: string | null
  name: string
  template?: Station['template']
  splitPerItem?: boolean
  sortOrder?: number
  isActive?: boolean
  /** 省略 = 不動綁定；給了 = 整組取代。 */
  printers?: StationPrinter[]
}

export interface PrintJob {
  id: string
  printerId: string
  printerName: string
  stationName?: string | null
  orderId?: string | null
  docType: string
  reason: string
  status: 'pending' | 'printing' | 'done' | 'failed' | 'dead' | 'cancelled'
  attempts: number
  lastError?: string | null
  /** transient（網路斷）/ needs_attention（缺紙）/ permanent（設定錯）。 */
  lastErrorClass?: 'transient' | 'needs_attention' | 'permanent' | null
  createdAt: string
  doneAt?: string | null
}

export interface PrintQueueStatus {
  pending: number
  dead: number
  /** 有意圖但展不開（通常是還沒設定任何印表機）。 */
  unrouted: number
  needsAttention: boolean
  detail: string
}

/**
 * 出單機相關的指令**只存在於 Tauri 這一側**。
 *
 * 區網那邊的呼叫者是顧客手機與廚房平板：就算區網服務有漏洞，
 * 攻擊面也只到「亂送單」，到不了「改設定」或「看失敗的單」。
 * 所以在瀏覽器裡開這一頁會拿到 404，那是刻意的。
 */
export const printerApi = {
  list: () => transport.call<Printer[]>('list_printers'),
  upsert: (input: PrinterInput) => transport.call<Printer>('upsert_printer', { input }),
  remove: (id: string) => transport.call<void>('delete_printer', { id }),
  probe: (id: string) => transport.call<ProbeResult>('probe_printer', { id }),
  testPrint: (id: string) => transport.call<void>('test_print', { id }),

  stations: () => transport.call<Station[]>('list_stations'),
  upsertStation: (input: StationInput) => transport.call<Station>('upsert_station', { input }),
  removeStation: (id: string) => transport.call<void>('delete_station', { id }),

  queueStatus: () => transport.call<PrintQueueStatus>('print_queue_status'),
  jobs: (limit?: number) => transport.call<PrintJob[]>('list_print_jobs', { limit: limit ?? 50 }),
  retryJob: (id: string) => transport.call<void>('retry_print_job', { id }),
  cancelJob: (id: string) => transport.call<void>('cancel_print_job', { id }),
}

// ---------------------------------------------------------------- 班別與日結

export interface DenomCount {
  /** 面額，以元為單位（1000 = 一千元鈔）。 */
  denomination: number
  count: number
}

export interface Shift {
  id: string
  shiftNo: string
  businessDate: string
  status: 'open' | 'closing' | 'closed' | 'reviewed'
  openedAt: string
  openedBy: string
  openingFloat: number
  closedAt?: string | null
  /**
   * ★ 關班之前一定是 null。
   *
   * 盲盤的整個重點：先看到應有金額的話，短少的人會直接照抄，
   * 而那正是「現金差異永遠是零」的原因。
   */
  expectedCash?: number | null
  countedCash?: number | null
  cashVariance?: number | null
  note?: string | null
}

export interface SalesTotals {
  bills: number
  subtotal: number
  discount: number
  serviceCharge: number
  rounding: number
  sales: number
  tax: number
  total: number
}

export interface PaymentTotal {
  code: string
  name: string
  count: number
  amount: number
}

export interface CashSummary {
  openingFloat: number
  cashSales: number
  paidIn: number
  paidOut: number
  expected: number
  counted?: number | null
  /** 正數 = 溢收，負數 = 短少。 */
  variance?: number | null
}

export interface VoidTotals {
  voidedLines: number
  voidedAmount: number
}

export interface ShiftReport {
  shiftNo: string
  businessDate: string
  openedAt: string
  closedAt?: string | null
  sales: SalesTotals
  payments: PaymentTotal[]
  cash: CashSummary
  voids: VoidTotals
}

export interface ItemLine {
  name: string
  qtyMilli: number
  amount: number
}

export interface DayReport {
  businessDate: string
  zReportNo: string
  closedAt: string
  sales: SalesTotals
  payments: PaymentTotal[]
  voids: VoidTotals
  shifts: Shift[]
  topItems: ItemLine[]
}

export interface DayStatus {
  businessDate: string
  /** not_started / open / closing / closed / locked */
  status: string
  shift: Shift | null
  closedShifts: number
}

export const shiftApi = {
  dayStatus: () => transport.call<DayStatus>('day_status'),
  open: (openingFloat: number, counts?: DenomCount[], note?: string) =>
    transport.call<Shift>('open_shift', { req: { openingFloat, counts, note } }),
  close: (counts: DenomCount[], note?: string) =>
    transport.call<ShiftReport>('close_shift', { req: { counts, note } }),
  cashMovement: (kind: 'paid_in' | 'paid_out' | 'drop', amount: number, note?: string) =>
    transport.call<void>('record_cash_movement', { req: { kind, amount, note } }),
  /** X 報表。需要 report.daily —— 收銀員預設拿不到，那正是盲盤的前提。 */
  xReport: () => transport.call<ShiftReport>('x_report'),
  closeDay: () => transport.call<DayReport>('close_business_day'),
}

// ---------------------------------------------------------------- 備份與還原

export interface BackupSettings {
  /** 第二個實體媒體（建議是常插著的隨身碟）。備份跟資料庫同一顆硬碟只防誤刪，不防壞掉。 */
  externalDir?: string | null
  hourly: boolean
  onClose: boolean
}

export interface AppSettings {
  backup: BackupSettings
}

export interface BackupFile {
  path: string
  name: string
  bucket: string
  sizeBytes: number
  modifiedAt?: string | null
  external: boolean
}

export interface BackupRunResult {
  localPath: string
  externalPath?: string | null
  sizeBytes: number
  tookMs: number
  /** 外接位置寫不進去時的說明。不是錯誤 —— 本機那一份已經好了。 */
  externalError?: string | null
}

export interface PendingRestore {
  source: string
  stagedAt: string
}

export const backupApi = {
  settings: () => transport.call<AppSettings>('get_settings'),
  saveSettings: (settings: AppSettings) =>
    transport.call<AppSettings>('save_settings', { settings }),
  run: () => transport.call<BackupRunResult>('run_backup'),
  list: () => transport.call<BackupFile[]>('list_backups'),
  /** 準備還原。真正的替換發生在下一次啟動 —— 資料庫在程式跑的時候是開著的。 */
  stageRestore: (path: string) => transport.call<string>('stage_restore', { path }),
  cancelRestore: () => transport.call<void>('cancel_restore'),
  pendingRestore: () => transport.call<PendingRestore | null>('pending_restore'),
}

// ---------------------------------------------------------------- 診斷

/**
 * 診斷資訊。
 *
 * 「今天中午印不出來，現在又好了」這種回報，維護者沒有任何辦法重現 ——
 * 一人維護的專案通常不是死在寫程式，是死在無法診斷的回報上。
 *
 * 內容不含品名、客人資訊或帳單明細，而且**不會自動上傳任何東西**。
 */
export const diagnosticsApi = {
  report: () => transport.call<string>('diagnostics_report'),
  /** 存成一個 .txt（含 BOM，記事本開起來中文才不會變亂碼）。 */
  export: (dir: string) => transport.call<string>('export_diagnostics', { dir }),
}

/**
 * 把某一天的日結匯出成 CSV。
 *
 * 讀的是日結當下存下來的快照，不是重算 —— 三個月後叫出來的數字必須跟
 * 當時印出來的那張紙一模一樣。
 */
export const reportApi = {
  exportDayCsv: (businessDate: string, dir: string) =>
    transport.call<string>('export_day_csv', { businessDate, dir }),
}

// ---------------------------------------------------------------- 折扣與作廢

export interface DiscountInput {
  orderId: string
  expectedRev: number
  /** 省略 = 整單折扣。 */
  lineId?: string | null
  /** percent（basis point，8500 = 85 折）/ amount（整數元）/ comp（招待）。 */
  kind: 'percent' | 'amount' | 'comp'
  value: number
  reasonId?: string | null
  note?: string | null
  /** 收銀員權限不足時的主管授權。 */
  approverId?: string | null
}

export const discountApi = {
  apply: (req: DiscountInput) => transport.call<Order>('apply_discount', { req }),
  voidOrder: (
    orderId: string,
    expectedRev: number,
    reasonId?: string | null,
    note?: string | null,
  ) =>
    transport.call<Order>('void_order', {
      req: { orderId, expectedRev, reasonId, note },
    }),
}
