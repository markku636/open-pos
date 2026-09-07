/**
 * 唯一的 API 橋接層（對標 db-kit 的 src/api.ts）。
 *
 * 三個 entry（收銀 / KDS / 掃碼點餐）都只 import 這一份，
 * 完全不需要知道自己跑在 Tauri 還是瀏覽器裡。
 *
 * 型別對應後端的 serde 定義（`src-tauri/src/services/app.rs`），
 * 後端統一用 camelCase 輸出，所以兩邊的欄位名一致。
 */
import type { Locale } from '@/shared/i18n'

import { transport } from "./transport";

export interface AppInfo {
  version: string;
  dataDir: string;
  schemaVersion: number;
  startedAt: string;
}

export interface HealthItem {
  name: string;
  ok: boolean;
  /** ok 為 false 時，這裡必須說得出「該怎麼辦」。 */
  detail: string;
}

export interface Health {
  ok: boolean;
  items: HealthItem[];
}

export const api = {
  appInfo: () => transport.call<AppInfo>("app_info"),
  /** 健康檢查。不健康時後端回 503，transport 會丟出 AppError。 */
  health: () => transport.call<Health>("health"),
  /**
   * 用系統預設瀏覽器開啟外部連結。
   *
   * 只有 Tauri 這一側有這個 command —— 區網那邊的呼叫者是顧客手機與廚房平板，
   * 讓他們叫「主機」開瀏覽器是一個真的安全漏洞。所以瀏覽器模式改用 window.open，
   * 開的是「使用者自己那台裝置」的瀏覽器。
   */
  openExternal: (url: string) =>
    transport.kind === "tauri"
      ? transport.call<void>("open_external", { url })
      : Promise.resolve(void window.open(url, "_blank", "noopener,noreferrer")),
};

export { transport };
export type { AppError } from "./transport";

// ---------------------------------------------------------------- 商品

export interface Variant {
  id: string;
  itemId: string;
  code: string;
  name: string;
  /** delta = 跟著品項基本價加減；absolute = 自己一個固定價。 */
  priceMode: "delta" | "absolute";
  price: number;
  priceDelta: number;
  isDefault: boolean;
  sortOrder: number;
  isActive: boolean;
}

export interface Item {
  id: string;
  categoryId: string | null;
  name: string;
  shortName: string | null;
  /** 整數元（見 docs/adr/0001-money-as-integer-dollars.md）。 */
  basePrice: number;
  taxCode: string;
  isOpenPrice: boolean;
  soldOutUntil: string | null;
  sortOrder: number;
  isActive: boolean;
  variants: Variant[];
  /** 這個品項要問哪幾組選項。群組本身在 MenuTree.modifierGroups 裡。 */
  modifierGroupIds: string[];
}

/**
 * 選項群組（甜度 / 冰塊 / 加購）。
 *
 * 分成群組而不是一堆平的選項，是因為「甜度」要必選一個、
 * 「加購」可以選很多個也可以不選 —— 這個差別直接決定點餐畫面長什麼樣子。
 */
export interface ModifierGroup {
  id: string;
  name: string;
  /** single = 單選（甜度）；multiple = 複選（加購）。 */
  selectionType: "single" | "multiple";
  /** 最少要選幾個。1 = 必選。 */
  minSelect: number;
  maxSelect: number;
  sortOrder: number;
  options: Modifier[];
}

export interface Modifier {
  id: string;
  groupId: string;
  name: string;
  /** 加價。0 = 免費選項（半糖、去冰）。 */
  price: number;
  isDefault: boolean;
  soldOutUntil: string | null;
  sortOrder: number;
  isActive: boolean;
}

export interface Category {
  id: string;
  name: string;
  color: string | null;
  sortOrder: number;
  isActive: boolean;
}

export interface CategoryNode extends Category {
  items: Item[];
}

export interface MenuTree {
  categories: CategoryNode[];
  /** 沒有分類的品項。刻意單獨列出來 —— 藏起來的話老闆會以為商品不見了。 */
  uncategorized: Item[];
  /** 店裡所有的選項群組，各一份。品項用 id 指過來。 */
  modifierGroups: ModifierGroup[];
}

export interface CategoryInput {
  id?: string;
  name: string;
  color?: string | null;
  sortOrder?: number;
  isActive?: boolean;
}

export interface ItemInput {
  id?: string;
  categoryId?: string | null;
  name: string;
  shortName?: string | null;
  basePrice: number;
  taxCode?: string;
  isOpenPrice?: boolean;
  soldOutUntil?: string | null;
  sortOrder?: number;
  isActive?: boolean;
}

export interface VariantInput {
  id?: string;
  itemId: string;
  code: string;
  name: string;
  priceMode: "delta" | "absolute";
  price?: number;
  priceDelta?: number;
  isDefault?: boolean;
  sortOrder?: number;
  isActive?: boolean;
}

/** 商品維護。
 *
 * 讀（menuTree）同時開在區網端點上 —— KDS 與顧客手機都要看得到品名與價格。
 * 寫（upsert / delete）**只走 Tauri IPC**，所以就算 LAN server 有漏洞，
 * 也改不到菜單與價格。
 */
export const menuApi = {
  tree: () => transport.call<MenuTree>("menu_tree"),
  upsertCategory: (input: CategoryInput) =>
    transport.call<Category>("upsert_category", { input }),
  deleteCategory: (id: string) =>
    transport.call<void>("delete_category", { id }),
  upsertItem: (input: ItemInput) =>
    transport.call<Item>("upsert_item", { input }),
  deleteItem: (id: string) => transport.call<void>("delete_item", { id }),
  upsertVariant: (input: VariantInput) =>
    transport.call<Variant>("upsert_variant", { input }),
  deleteVariant: (id: string) => transport.call<void>("delete_variant", { id }),

  upsertModifierGroup: (input: {
    id?: string | null;
    name: string;
    selectionType?: "single" | "multiple";
    minSelect?: number;
    maxSelect?: number;
    sortOrder?: number;
  }) => transport.call<ModifierGroup>("upsert_modifier_group", { input }),
  deleteModifierGroup: (id: string) =>
    transport.call<void>("delete_modifier_group", { id }),
  upsertModifier: (input: {
    id?: string | null;
    groupId: string;
    name: string;
    price?: number;
    isDefault?: boolean;
    sortOrder?: number;
    isActive?: boolean;
  }) => transport.call<Modifier>("upsert_modifier", { input }),
  deleteModifier: (id: string) =>
    transport.call<void>("delete_modifier", { id }),
  /** 這個品項要問哪幾組選項。整批送 —— 畫面上是一排勾選框。 */
  setItemModifierGroups: (itemId: string, groupIds: string[]) =>
    transport.call<void>("set_item_modifier_groups", { itemId, groupIds }),
};

// ---------------------------------------------------------------- 點餐與結帳

export type Channel = "dine_in" | "takeout" | "delivery";

export interface OrderLine {
  id: string;
  lineNo: number;
  name: string;
  variantName: string | null;
  options: string[];
  note: string | null;
  /** 數量 × 1000。半份是 500。 */
  qtyMilli: number;
  unitPrice: number;
  amount: number;
  /** 分項分帳時，這一行已經被誰結掉了。 */
  paid: boolean;
}

export interface Order {
  id: string;
  orderNo: string;
  status:
    | "draft"
    | "placed"
    | "in_progress"
    | "ready"
    | "served"
    | "settled"
    | "voided";
  /** 樂觀鎖版本。寫入時必須帶回去，不符會回 409。 */
  rev: number;
  channel: Channel;
  tableId: string | null;
  tableLabel: string | null;
  guestCount: number;
  businessDate: string;
  lines: OrderLine[];
  subtotal: number;
  serviceCharge: number;
  roundingAdjustment: number;
  grandTotal: number;
  salesAmount: number;
  taxAmount: number;
  paidTotal: number;
  changeTotal: number;
  /** 已經開出去的帳單收了多少。分帳到一半時 < grandTotal。 */
  billedTotal: number;
  /** 已經結了幾份。 */
  billCount: number;
  /** even / by_item / by_amount。還沒分過是 null。 */
  splitMode: string | null;
  /** 平分時說好要分幾份。 */
  splitCount: number | null;
}

export interface SettleResult {
  order: Order;
  billNo: string;
  change: number;
  /** 這張單還有多少沒結。> 0 代表**還不能讓客人走**。 */
  remaining: number;
  /** 這是第幾份。 */
  splitIndex: number;
}

/**
 * 分帳的一份。
 *
 * 三個模式對應櫃檯真的會聽到的三句話：
 * 「我們四個平分」/「我先出 500」/「我的只有那碗麵」。
 */
export type SplitReq =
  | { mode: "even"; parts: number }
  | { mode: "amount"; amount: number }
  | { mode: "items"; lineIds: string[] };

export interface SplitPreview {
  /** 這一份應收多少。**由 Rust 算，前端只負責顯示。** */
  due: number;
  index: number;
  count: number | null;
  orderTotal: number;
  billed: number;
  remainingAfter: number;
}

export interface PaymentMethod {
  /** 內部 ULID。結帳畫面用不到，金流設定拿它掛外鍵。 */
  id: string;
  code: string;
  name: string;
  kind: string;
  /** 只有現金這類方式能找零。刷卡「找零」是不存在的東西。 */
  allowsChange: boolean;
  opensDrawer: boolean;
}

export interface NewLine {
  itemId: string;
  variantId?: string | null;
  qtyMilli?: number;
  modifierIds?: string[];
  note?: string | null;
}

export interface PaymentInput {
  methodCode: string;
  /** 這一筆要沖銷帳單多少。 */
  amount: number;
  /** 客人實際遞出來的錢。只有現金會與 amount 不同。 */
  tendered?: number | null;
  refNo?: string | null;
}

/** 前端產生的冪等鍵。Wi-Fi 抖動時重送不會重複收款。 */
export function newIdemKey(): string {
  return crypto.randomUUID();
}

export const orderApi = {
  open: (channel: Channel, tableId?: string | null, guestCount?: number) =>
    transport.call<Order>("open_order", {
      req: {
        channel,
        tableId: tableId ?? null,
        guestCount: guestCount ?? 1,
        clientId: newIdemKey(),
      },
    }),
  get: (id: string) => transport.call<Order>("get_order", { id }),
  listOpen: () => transport.call<Order[]>("list_open_orders"),
  addLines: (orderId: string, expectedRev: number, lines: NewLine[]) =>
    transport.call<Order>("add_lines", {
      req: { orderId, expectedRev, lines },
    }),
  voidLine: (orderId: string, expectedRev: number, lineId: string) =>
    transport.call<Order>("void_line", {
      orderId,
      expectedRev,
      lineId,
      reasonId: null,
    }),
  settle: (
    orderId: string,
    expectedRev: number,
    payments: PaymentInput[],
    split?: SplitReq,
  ) =>
    transport.call<SettleResult>("settle", {
      req: {
        orderId,
        expectedRev,
        payments,
        idemKey: newIdemKey(),
        split: split ?? null,
      },
    }),
  /**
   * 分帳試算。
   *
   * 「四個人分 101 元」的答案是 26/25/25/25 —— 最大餘數法。在這裡再實作一次
   * 同一套進位規則，就是在等兩邊哪天不一樣，而不一樣的那天螢幕與資料庫會
   * 差一元、沒有人找得到原因。本機 IPC 往返不到 1ms。
   */
  previewSplit: (orderId: string, split: SplitReq | null) =>
    transport.call<SplitPreview>("preview_split", { orderId, split }),
  paymentMethods: () => transport.call<PaymentMethod[]>("payment_methods"),
};

// ---------------------------------------------------------------- 出單機

/**
 * 傳輸方式。後端的 serde tag 是 `kind`（見 infra::printer::Transport）。
 *
 * v1 只實作網路型與檔案；USB 與藍牙的形狀先留著，之後補實作時
 * 前端不必再動一次型別。
 */
export type Transport =
  | { kind: "network"; host: string; port: number }
  | { kind: "file"; path: string; append: boolean }
  | { kind: "usb"; vid: number; pid: number; serial?: string | null }
  | { kind: "bluetooth"; addr: string; channel: number };

export type PaperWidth = "mm58" | "mm80";
export type CjkEncoding = "big5" | "gb18030" | "utf8";

export interface PrinterCaps {
  paper: PaperWidth;
  raster: boolean;
  cutter: boolean;
  drawer: boolean;
  statusQuery: boolean;
  encoding: CjkEncoding;
}

export interface Printer {
  id: string;
  name: string;
  transport: Transport;
  caps: PrinterCaps;
  renderMode: "raster" | "text";
  isActive: boolean;
  lastProbeAt?: string | null;
  lastProbeOk?: boolean | null;
  lastError?: string | null;
}

export interface PrinterInput {
  id?: string | null;
  name: string;
  transport: Transport;
  paper: PaperWidth;
  encoding?: CjkEncoding;
  cutter?: boolean;
  drawer?: boolean;
  statusQuery?: boolean;
  renderMode?: "raster" | "text";
  isActive?: boolean;
}

export interface ProbeResult {
  ok: boolean;
  detail: string;
}

export type BindingMode = "failover" | "always";

export interface StationPrinter {
  printerId: string;
  priority: number;
  mode: BindingMode;
}

export interface Station {
  id: string;
  name: string;
  template: "kitchen" | "drink" | "receipt" | "label";
  splitPerItem: boolean;
  sortOrder: number;
  isActive: boolean;
  printers: StationPrinter[];
}

export interface StationInput {
  id?: string | null;
  name: string;
  template?: Station["template"];
  splitPerItem?: boolean;
  sortOrder?: number;
  isActive?: boolean;
  /** 省略 = 不動綁定；給了 = 整組取代。 */
  printers?: StationPrinter[];
}

export interface PrintJob {
  id: string;
  printerId: string;
  printerName: string;
  stationName?: string | null;
  orderId?: string | null;
  docType: string;
  reason: string;
  status: "pending" | "printing" | "done" | "failed" | "dead" | "cancelled";
  attempts: number;
  lastError?: string | null;
  /** transient（網路斷）/ needs_attention（缺紙）/ permanent（設定錯）。 */
  lastErrorClass?: "transient" | "needs_attention" | "permanent" | null;
  createdAt: string;
  doneAt?: string | null;
}

export interface PrintQueueStatus {
  pending: number;
  dead: number;
  /** 有意圖但展不開（通常是還沒設定任何印表機）。 */
  unrouted: number;
  needsAttention: boolean;
  /**
   * 現在是哪一種狀況。**後端回代碼，句子由前端組。**
   *
   * 這一行在收銀機頂欄上最顯眼，如果句子在 Rust 裡就 format! 好了，
   * 它永遠是中文的 —— 後端不知道看的人要哪一種語言。
   */
  state: "dead" | "unrouted" | "pending" | "ok";
}

/**
 * 出單機相關的指令**只存在於 Tauri 這一側**。
 *
 * 區網那邊的呼叫者是顧客手機與廚房平板：就算區網服務有漏洞，
 * 攻擊面也只到「亂送單」，到不了「改設定」或「看失敗的單」。
 * 所以在瀏覽器裡開這一頁會拿到 404，那是刻意的。
 */
export const printerApi = {
  list: () => transport.call<Printer[]>("list_printers"),
  upsert: (input: PrinterInput) =>
    transport.call<Printer>("upsert_printer", { input }),
  remove: (id: string) => transport.call<void>("delete_printer", { id }),
  probe: (id: string) => transport.call<ProbeResult>("probe_printer", { id }),
  testPrint: (id: string) => transport.call<void>("test_print", { id }),

  stations: () => transport.call<Station[]>("list_stations"),
  upsertStation: (input: StationInput) =>
    transport.call<Station>("upsert_station", { input }),
  removeStation: (id: string) => transport.call<void>("delete_station", { id }),

  queueStatus: () => transport.call<PrintQueueStatus>("print_queue_status"),
  jobs: (limit?: number) =>
    transport.call<PrintJob[]>("list_print_jobs", { limit: limit ?? 50 }),
  retryJob: (id: string) => transport.call<void>("retry_print_job", { id }),
  cancelJob: (id: string) => transport.call<void>("cancel_print_job", { id }),
};

// ---------------------------------------------------------------- 班別與日結

export interface DenomCount {
  /** 面額，以元為單位（1000 = 一千元鈔）。 */
  denomination: number;
  count: number;
}

export interface Shift {
  id: string;
  shiftNo: string;
  businessDate: string;
  status: "open" | "closing" | "closed" | "reviewed";
  openedAt: string;
  openedBy: string;
  openingFloat: number;
  closedAt?: string | null;
  /**
   * ★ 關班之前一定是 null。
   *
   * 盲盤的整個重點：先看到應有金額的話，短少的人會直接照抄，
   * 而那正是「現金差異永遠是零」的原因。
   */
  expectedCash?: number | null;
  countedCash?: number | null;
  cashVariance?: number | null;
  note?: string | null;
}

export interface SalesTotals {
  bills: number;
  subtotal: number;
  discount: number;
  serviceCharge: number;
  rounding: number;
  sales: number;
  tax: number;
  total: number;
}

export interface PaymentTotal {
  code: string;
  name: string;
  count: number;
  amount: number;
}

export interface CashSummary {
  openingFloat: number;
  cashSales: number;
  paidIn: number;
  paidOut: number;
  /** 現金退款。錢是從抽屜拿出去的，所以要扣。 */
  cashRefunds: number;
  expected: number;
  counted?: number | null;
  /** 正數 = 溢收，負數 = 短少。 */
  variance?: number | null;
}

export interface VoidTotals {
  voidedLines: number;
  voidedAmount: number;
}

/**
 * 退款統計。
 *
 * 不併進 sales 而是獨立一欄：作廢是「這筆生意沒發生」，退款是「發生了、
 * 然後退回來」。營業額要看得到原來賣了多少，也要看得到退了多少。
 */
export interface RefundTotals {
  count: number;
  amount: number;
  /** 其中用現金退的。關班的應有現金要扣掉它。 */
  cashAmount: number;
}

export interface ShiftReport {
  shiftNo: string;
  businessDate: string;
  openedAt: string;
  closedAt?: string | null;
  sales: SalesTotals;
  payments: PaymentTotal[];
  cash: CashSummary;
  voids: VoidTotals;
  refunds: RefundTotals;
}

export interface ItemLine {
  name: string;
  qtyMilli: number;
  amount: number;
}

export interface DayReport {
  businessDate: string;
  zReportNo: string;
  closedAt: string;
  sales: SalesTotals;
  payments: PaymentTotal[];
  voids: VoidTotals;
  refunds: RefundTotals;
  shifts: Shift[];
  topItems: ItemLine[];
}

export interface DayStatus {
  businessDate: string;
  /** not_started / open / closing / closed / locked */
  status: string;
  shift: Shift | null;
  closedShifts: number;
}

export const shiftApi = {
  dayStatus: () => transport.call<DayStatus>("day_status"),
  open: (openingFloat: number, counts?: DenomCount[], note?: string) =>
    transport.call<Shift>("open_shift", {
      req: { openingFloat, counts, note },
    }),
  close: (counts: DenomCount[], note?: string) =>
    transport.call<ShiftReport>("close_shift", { req: { counts, note } }),
  cashMovement: (
    kind: "paid_in" | "paid_out" | "drop",
    amount: number,
    note?: string,
  ) =>
    transport.call<void>("record_cash_movement", {
      req: { kind, amount, note },
    }),
  /** X 報表。需要 report.daily —— 收銀員預設拿不到，那正是盲盤的前提。 */
  xReport: () => transport.call<ShiftReport>("x_report"),
  closeDay: () => transport.call<DayReport>("close_business_day"),
};

// ---------------------------------------------------------------- 備份與還原

export interface BackupSettings {
  /** 第二個實體媒體（建議是常插著的隨身碟）。備份跟資料庫同一顆硬碟只防誤刪，不防壞掉。 */
  externalDir?: string | null;
  hourly: boolean;
  onClose: boolean;
}

export interface AppSettings {
  backup: BackupSettings;
}

export interface BackupFile {
  path: string;
  name: string;
  bucket: string;
  sizeBytes: number;
  modifiedAt?: string | null;
  external: boolean;
}

export interface BackupRunResult {
  localPath: string;
  externalPath?: string | null;
  sizeBytes: number;
  tookMs: number;
  /** 外接位置寫不進去時的說明。不是錯誤 —— 本機那一份已經好了。 */
  externalError?: string | null;
}

export interface PendingRestore {
  source: string;
  stagedAt: string;
}

export const backupApi = {
  settings: () => transport.call<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) =>
    transport.call<AppSettings>("save_settings", { settings }),
  run: () => transport.call<BackupRunResult>("run_backup"),
  list: () => transport.call<BackupFile[]>("list_backups"),
  /** 準備還原。真正的替換發生在下一次啟動 —— 資料庫在程式跑的時候是開著的。 */
  stageRestore: (path: string) =>
    transport.call<string>("stage_restore", { path }),
  cancelRestore: () => transport.call<void>("cancel_restore"),
  pendingRestore: () =>
    transport.call<PendingRestore | null>("pending_restore"),
};

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
  report: () => transport.call<string>("diagnostics_report"),
  /** 存成一個 .txt（含 BOM，記事本開起來中文才不會變亂碼）。 */
  export: (dir: string) =>
    transport.call<string>("export_diagnostics", { dir }),
};

/**
 * 把某一天的日結匯出成 CSV。
 *
 * 讀的是日結當下存下來的快照，不是重算 —— 三個月後叫出來的數字必須跟
 * 當時印出來的那張紙一模一樣。
 */
export const reportApi = {
  exportDayCsv: (businessDate: string, dir: string) =>
    transport.call<string>("export_day_csv", { businessDate, dir }),
};

// ---------------------------------------------------------------- 折扣與作廢

export interface DiscountInput {
  orderId: string;
  expectedRev: number;
  /** 省略 = 整單折扣。 */
  lineId?: string | null;
  /** percent（basis point，8500 = 85 折）/ amount（整數元）/ comp（招待）。 */
  kind: "percent" | "amount" | "comp";
  value: number;
  reasonId?: string | null;
  note?: string | null;
  /** 收銀員權限不足時的主管授權。 */
  approverId?: string | null;
}

export const discountApi = {
  apply: (req: DiscountInput) =>
    transport.call<Order>("apply_discount", { req }),
  voidOrder: (
    orderId: string,
    expectedRev: number,
    reasonId?: string | null,
    note?: string | null,
  ) =>
    transport.call<Order>("void_order", {
      req: { orderId, expectedRev, reasonId, note },
    }),
};

// ---------------------------------------------------------------- 廚房顯示

/** 廚房能推到的狀態。往回推會被伺服器靜靜忽略。 */
export type KdsStatus = "cooking" | "ready" | "served";

export interface KdsLine {
  id: string;
  name: string;
  variantName?: string | null;
  options: string[];
  note?: string | null;
  qtyMilli: number;
  /** pending / fired / cooking / ready */
  status: string;
  stationId?: string | null;
  stationName?: string | null;
}

export interface KdsTicket {
  orderId: string;
  orderNo: string;
  /** 通路代碼。要顯示的字在 `locales/kds.ts` —— 後端不知道這台平板講哪一國話。 */
  channel: Channel;
  tableLabel?: string | null;
  /** 這張單開了多久。廚房看的是「等最久的那一張」而不是時間點。 */
  waitingSeconds: number;
  placedAt: string;
  lines: KdsLine[];
}

export interface KdsBoard {
  generatedAt: string;
  tickets: KdsTicket[];
}

/**
 * 廚房顯示。
 *
 * 讀取走 SSE（見 src/kds/useBoard.ts），寫入走一般的 POST ——
 * KDS 需要的雙向只有「這一項做好了」，為它扛一整套 WebSocket 的狀態機
 * 並不划算，而樂觀更新與錯誤處理都是熟悉的請求／回應模型。
 */
export const kdsApi = {
  board: () => transport.call<KdsBoard>("kds_board"),
  /** 只能往前推：pending → cooking → ready → served。 */
  advance: (lineId: string, to: KdsStatus) =>
    transport.call<KdsBoard>("kds_advance", { lineId, to }),
};

// ---------------------------------------------------------------- 桌位

export interface TableSession {
  id: string;
  guestCount: number;
  openedAt: string;
  /** 這一桌目前累積多少錢。店員最常被問的問題。 */
  total: number;
  orderCount: number;
  seatedSeconds: number;
}

export interface DiningTable {
  id: string;
  code: string;
  name?: string | null;
  areaName?: string | null;
  seats: number;
  isActive: boolean;
  /** 有值代表這一桌現在有人。 */
  session: TableSession | null;
}

export interface TableInput {
  id?: string | null;
  code: string;
  name?: string | null;
  seats?: number;
  areaName?: string | null;
  isActive?: boolean;
}

export const tableApi = {
  list: () => transport.call<DiningTable[]>("list_tables"),
  upsert: (input: TableInput) =>
    transport.call<DiningTable>("upsert_table", { input }),
  remove: (id: string) => transport.call<void>("delete_table", { id }),
  /** 清桌。只有在沒有未結帳的單時才允許 —— 否則它會變成一個把帳丟掉的按鈕。 */
  close: (tableId: string) => transport.call<void>("close_table", { tableId }),
};

// ---------------------------------------------------------------- 示範資料

export interface DemoResult {
  /** false = 已經有商品，什麼都沒動。 */
  created: boolean;
  categories: number;
  items: number;
  tables: number;
}

export const demoApi = {
  /** 一鍵示範資料。裝起來看到一片空白的人多半不會先建十個品項才試用。 */
  seed: () => transport.call<DemoResult>("seed_demo"),
};

// ---------------------------------------------------------------- 原因代碼

export interface ReasonCode {
  id: string;
  code: string;
  name: string;
  /** 選了這個原因就一定要補一句說明。 */
  requiresNote: boolean;
}

/** kind：void / discount / comp / refund / cash_in / cash_out。 */
export const reasonApi = {
  list: (kind: string) =>
    transport.call<ReasonCode[]>("list_reasons", { kind }),
};

// ---------------------------------------------------------------- 退款

export interface BillPayment {
  id: string;
  methodCode: string;
  methodName: string;
  amount: number;
  refunded: number;
  refundable: number;
}

export interface Bill {
  id: string;
  billNo: string;
  orderNo: string;
  businessDate: string;
  settledAt: string | null;
  status: string;
  grandTotal: number;
  refundedTotal: number;
  refundable: number;
  /** 分帳的那一份：「2／4」。整單結帳是 null。 */
  splitLabel: string | null;
  payments: BillPayment[];
}

export interface RefundResult {
  billNo: string;
  amount: number;
  methodName: string;
  refundedTotal: number;
  refundable: number;
  billStatus: string;
}

export const refundApi = {
  /** 找帳單。給單號片段就跨日找 —— 客人拿著三天前的收據回來是常態。 */
  findBills: (opts?: {
    businessDate?: string | null;
    billNo?: string | null;
  }) =>
    transport.call<Bill[]>("find_bills", {
      req: {
        businessDate: opts?.businessDate ?? null,
        billNo: opts?.billNo ?? null,
      },
    }),
  /**
   * 退款。一定要選原因，而且**原路退回** ——
   * 刷卡收的錢用現金退是最經典的一種內神通外鬼。
   */
  refund: (input: {
    billId: string;
    paymentId?: string | null;
    amount: number;
    reasonId: string;
    note?: string | null;
  }) =>
    transport.call<RefundResult>("refund", {
      req: {
        billId: input.billId,
        paymentId: input.paymentId ?? null,
        amount: input.amount,
        reasonId: input.reasonId,
        note: input.note ?? null,
        approverId: null,
        idemKey: newIdemKey(),
      },
    }),
};

/** 補印收據。重送的是當初那一張的快照，並印上「※ 補印 第 N 次 ※」。 */
export const reprintApi = {
  receipt: (billId: string) =>
    transport.call<void>("reprint_receipt", { billId }),
};

// ---------------------------------------------------------------- 稽核

export interface AuditRow {
  id: string;
  at: string;
  businessDate: string | null;
  actorName: string | null;
  /** 動作代碼（`refund`、`void_after_settle`…）。查 `auditActions` 翻成人話。 */
  action: string;
  entityType: string;
  entityId: string;
  /** 人看得懂的對象（單號、帳單號）。 */
  label: string | null;
  amountDelta: number | null;
  reasonName: string | null;
  /** 誰簽的核。有值代表這是一個需要授權的動作。 */
  approvedByName: string | null;
}

export interface AuditGroup {
  /** 依動作分組時是動作代碼（要查字典），依操作者分組時是人名（原樣顯示）。 */
  key: string;
  count: number;
  amount: number;
}

export interface AuditReport {
  from: string;
  to: string;
  rows: AuditRow[];
  totalAmount: number;
  byAction: AuditGroup[];
  byActor: AuditGroup[];
  truncated: boolean;
}

export interface AuditQuery {
  from?: string | null;
  to?: string | null;
  action?: string | null;
  actorId?: string | null;
  /** 只看動到錢的。**這是最常按的一個開關。** */
  moneyOnly?: boolean;
}

export const auditApi = {
  query: (query: AuditQuery) =>
    transport.call<AuditReport>("audit_query", { query }),
};

// ---------------------------------------------------------------- 營運分析

export interface HourBucket {
  /** 0–23，店家時區。 */
  hour: number;
  bills: number;
  total: number;
}

export interface NamedTotal {
  /** 這一列屬於哪一種（`comp` / `void_item` / `dine_in`…）。查 `insightCodes`。 */
  code: string | null;
  /** 店家自己打的字（折扣名、原因名、品名）。不翻譯。 */
  name: string | null;
  count: number;
  amount: number;
}

export interface Insight {
  from: string;
  to: string;
  bills: number;
  total: number;
  /** 平均客單價（營業額 ÷ 帳單數）。 */
  averageBill: number;
  hours: HourBucket[];
  discounts: NamedTotal[];
  voids: NamedTotal[];
  items: NamedTotal[];
  channels: NamedTotal[];
}

/**
 * 一段期間的營運分析。
 *
 * 跟 Z 報表的分工：Z 報表是當天算好、永不重算的快照（稅務與交接的問題），
 * 這一支是現算的（老闆的問題：哪個時段最忙、這個月折掉多少）。
 */
export const insightApi = {
  query: (query: { from?: string | null; to?: string | null }) =>
    transport.call<Insight>("insight", { query }),
};

// ---------------------------------------------------------------- 區網

export interface NetInterface {
  name: string;
  ip: string;
  /** 這一張是不是挑來當區網位址的那一張。 */
  chosen: boolean;
  usable: boolean;
  /**
   * 這張網卡為什麼不能用。`null` = 可以用。
   *
   * 後端回代碼不回句子 —— 它不知道看的人要哪一種語言。
   * `vendor` 是廠商專有名詞（Docker / WSL…），三種語言都一樣。
   */
  note: { code: string; vendor: string | null } | null;
}

export interface LanStatus {
  port: number;
  ip: string | null;
  /** 廚房平板要開的網址。 */
  kdsUrl: string | null;
  orderUrl: string | null;
  /** server 真的綁在區網介面上。**這不等於防火牆有放行。** */
  bound: boolean;
  /** 探測結果。代碼 + OS 給的原始錯誤字串（那個不翻譯）。 */
  detail:
    | { code: "noAddress" }
    | { code: "bound" }
    | { code: "resolveFailed"; addr: string; error: string }
    | { code: "connectFailed"; addr: string; error: string };
  interfaces: NetInterface[];
  previousIp: string | null;
  /** IP 換過了 —— 桌卡 QR 全部要重印。 */
  ipChanged: boolean;
  firewallCommand: string;
}

export const lanApi = {
  status: () => transport.call<LanStatus>("lan_status"),
};

// ---------------------------------------------------------------- 銷售記錄

export interface SaleLine {
  name: string;
  variantName: string | null;
  /** 加了什麼選項（半糖、少冰、加珍珠）。 */
  options: string[];
  note: string | null;
  qtyMilli: number;
  unitPrice: number;
  amount: number;
  /** 這一行被退掉了。 */
  voided: boolean;
}

export interface SalePayment {
  method: string;
  amount: number;
  refunded: number;
}

export interface Sale {
  billId: string;
  billNo: string;
  orderId: string;
  orderNo: string;
  businessDate: string;
  settledAt: string | null;
  /** 通路代碼（`dine_in` / `takeout` / `delivery`）。標籤查 `sales.ts` 的字典。 */
  channel: string;
  tableLabel: string | null;
  guestCount: number;
  status: string;
  splitLabel: string | null;
  subtotal: number;
  discountTotal: number;
  serviceCharge: number;
  grandTotal: number;
  salesAmount: number;
  taxAmount: number;
  refundedTotal: number;
  /** 誰結的帳。 */
  settledBy: string | null;
  payments: SalePayment[];
  lines: SaleLine[];
}

export interface SalesReport {
  from: string;
  to: string;
  count: number;
  total: number;
  salesAmount: number;
  taxAmount: number;
  refundedTotal: number;
  byMethod: SalePayment[];
  sales: Sale[];
  /** 超過上限 —— 畫面要說「還有更多」，不是假裝就這些。 */
  truncated: boolean;
}

export interface SalesQuery {
  from?: string | null;
  to?: string | null;
  channel?: string | null;
  refundedOnly?: boolean;
  keyword?: string | null;
}

/**
 * 選一個資料夾。取消時回 null。
 *
 * 用原生對話框而不是叫使用者打字：最常見的錯路徑是「已經拔掉的隨身碟」
 * 與「打錯一個字的桌面路徑」，而那兩個都是選單能直接消滅的問題。
 * （`window.prompt` 在 Tauri 的 webview 裡根本不會出現。）
 */
export const dialogApi = {
  pickFolder: () => transport.call<string | null>("pick_folder"),
};

export const salesApi = {
  /** 翻帳：日期區間 + 通路 + 單號片段，每一筆帶明細與付款。 */
  history: (query: SalesQuery) =>
    transport.call<SalesReport>("sales_history", { query }),
  /** 過去某一天的 Z 報表（日結當下算好的那份快照）。 */
  dayReport: (businessDate: string) =>
    transport.call<DayReport>("day_report", { businessDate }),
  /** 有日結報表的營業日清單（新到舊）。 */
  closedDays: () => transport.call<string[]>("closed_days"),
  /** 匯出 Excel。回傳寫出去的檔案路徑。 */
  exportDayXlsx: (businessDate: string, dir: string) =>
    transport.call<string>("export_day_xlsx", { businessDate, dir }),
  exportSalesXlsx: (query: SalesQuery, dir: string) =>
    transport.call<string>("export_sales_xlsx", { query, dir }),
};

/** 一個金流商需要的一個憑證欄位。**永遠不含明文。** */
export interface CredentialField {
  /**
   * 欄位鍵（`hash_key`、`channel_secret`…）。
   *
   * **標籤與說明不在這裡** —— 它們是文案，用這個 key 去
   * `@/shared/locales/gatewayFields` 查（`gwField` / `gwFieldHint`）。
   */
  key: string;
  /** 已經填過了。畫面上顯示末四碼，永遠不回明文。 */
  isSet: boolean;
  tail: string | null;
}

export interface Gateway {
  id: string;
  provider: string;
  displayName: string;
  paymentMethodId: string | null;
  paymentMethodName: string | null;
  isSandbox: boolean;
  isActive: boolean;
  fields: CredentialField[];
  /** 還缺的必填欄位。有東西就代表這條線還不能啟用。 */
  missing: string[];
}

export interface GatewayInput {
  id?: string | null;
  provider: string;
  displayName: string;
  paymentMethodId?: string | null;
  isSandbox?: boolean;
  isActive?: boolean;
  /** 只送有改動的欄位。沒送的沿用已存的值。 */
  credentials?: Record<string, string>;
}

/** 一個可以選的金流商，以及它要哪些憑證欄位。 */
export interface ProviderDef {
  /** `manual` / `linepay` / `newebpay`。名稱與說明在 gatewayFields 字典裡。 */
  code: string;
  fields: CredentialField[];
}

export const gatewayApi = {
  /**
   * 有哪些金流商可以選。
   *
   * 欄位定義故意不寫在 TS 這一側 —— 新增一筆設定時後端還沒有列可以回，
   * 而在前端另抄一份的話，改了後端卻忘了改前端的症狀是「畫面上都填好了、
   * 存進去卻說缺欄位」，且只在新增時發生。所以那張表只有後端一份。
   */
  providers: () => transport.call<ProviderDef[]>("gateway_providers"),
  list: () => transport.call<Gateway[]>("list_gateways"),
  upsert: (input: GatewayInput) =>
    transport.call<Gateway>("upsert_gateway", { input }),
  remove: (id: string) => transport.call<void>("delete_gateway", { id }),
};

/**
 * 介面語言。存在後端而不是瀏覽器 —— 收銀機是共用機器，
 * 換一台電腦或重灌瀏覽器不該要重設一次，同店兩台也不該顯示不同語言。
 */
export const localeApi = {
  get: () => transport.call<Locale>('get_locale'),
  set: (locale: Locale) => transport.call<void>('set_locale', { locale }),
}
