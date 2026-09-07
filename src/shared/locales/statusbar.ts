import type { Catalog } from '@/shared/i18n'

/**
 * 頂欄的出單警示，以及「系統狀態」那一頁。
 *
 * # 為什麼不併進 `system.ts`
 *
 * 那一份是「關於／區網／備份」三個對話框的字。這一份是收銀員與店長會看的
 * 診斷畫面 —— 兩邊改動的頻率與改動的人都不一樣，混在一起只會讓兩邊互相踩。
 *
 * # 「可以用了」那一串用樣板，不要每則各寫一次
 *
 * 七則功能都是「X：可以用了」，所以只有 `ready` 一則帶 `{what}`，
 * 功能名各自是一個短語。這在三種語言都成立（`X：可以用了` / `X: ready` /
 * `X：利用できます`），而且加一項功能只要多一個短語，不必再翻一次整句。
 *
 * 注意這跟「把句子拆成兩半再用 JSX 接起來」不是同一件事：這裡整句仍然在
 * 字典裡，語序由各語言自己決定，只是把會變的名詞抽成佔位符。
 *
 * # 診斷那一段的英文結尾**故意留一個空格**
 *
 * `diagIncludes` 在畫面上直接接著 `diagExcludes` 印出來（中間那一段要換顏色，
 * 所以拆成兩個節點）。中文與日文的句號後面本來就不空格，英文要 ——
 * 而 JSX 會把兩個 `{}` 之間的換行整個吃掉，補不回來。
 *
 * # 版本表的欄名要短
 *
 * 左欄寬度是寫死的 `w-20`（80px）。日文的「フロントエンド」「データ保存先」
 * 照字面翻會直接壓到右邊的值，所以用「フロント」「保存先」——
 * 那一欄旁邊就是值，看得懂比翻得完整重要。
 */
export const statusbar = {
  // ─── 頂欄的出單警示 ───────────────────────────
  // 這顆按鈕只在出單機出事時才出現，點下去會跳到出單機那一頁。
  queueBadgeTitle: {
    'zh-TW': '點一下看出單狀況',
    en: 'Click to see the print queue',
    ja: 'クリックで印刷状況を表示',
  },

  // ─── 版本 ─────────────────────────────────────
  sectionVersion: { 'zh-TW': '版本', en: 'Version', ja: 'バージョン' },
  frontend: { 'zh-TW': '前端', en: 'Frontend', ja: 'フロント' },
  transport: { 'zh-TW': '傳輸層', en: 'Transport', ja: '通信方式' },
  backend: { 'zh-TW': '後端', en: 'Backend', ja: 'サーバー' },
  dataDir: { 'zh-TW': '資料目錄', en: 'Data dir', ja: '保存先' },
  backendVersion: {
    'zh-TW': '{v}（schema {s}）',
    en: '{v} (schema {s})',
    ja: '{v}（schema {s}）',
  },
  backendFailed: {
    'zh-TW': '連線失敗：{msg}',
    en: 'Connection failed: {msg}',
    ja: '接続失敗：{msg}',
  },

  // ─── 目前進度 ─────────────────────────────────
  sectionProgress: { 'zh-TW': '目前進度', en: 'Progress', ja: '現在の進捗' },
  ready: { 'zh-TW': '{what}：可以用了', en: '{what}: ready', ja: '{what}：利用できます' },
  capOrdering: {
    'zh-TW': '點餐、結帳、商品維護',
    en: 'Orders, checkout, menu',
    ja: '注文・会計・商品管理',
  },
  capPrinter: {
    'zh-TW': '出單機（ESC/POS 網路型）',
    en: 'Receipt printers (network ESC/POS)',
    ja: 'プリンター（ESC/POS ネットワーク）',
  },
  capShift: {
    'zh-TW': '班別交接與日結',
    en: 'Shift handover and day-end',
    ja: 'シフト締めと日次締め',
  },
  capBackup: {
    'zh-TW': '備份與還原',
    en: 'Backup and restore',
    ja: 'バックアップと復元',
  },
  capTables: {
    'zh-TW': '桌位、分帳、退款',
    en: 'Tables, split bills, refunds',
    ja: 'テーブル・分割会計・返金',
  },
  capKds: {
    'zh-TW': 'KDS 廚房顯示（含斷線佇列）',
    en: 'KDS kitchen display (with offline queue)',
    ja: 'KDSキッチンディスプレイ（オフライン対応）',
  },
  capReports: {
    'zh-TW': '營運分析與稽核查詢',
    en: 'Analytics and audit log',
    ja: '売上分析と監査ログ',
  },

  // ─── 區網與健康檢查 ───────────────────────────
  sectionLan: {
    'zh-TW': '區網連線（廚房平板）',
    en: 'LAN (kitchen tablet)',
    ja: 'LAN接続（キッチンのタブレット）',
  },
  sectionHealth: { 'zh-TW': '健康檢查', en: 'Health checks', ja: 'ヘルスチェック' },

  // ─── 回報問題 ─────────────────────────────────
  sectionReport: { 'zh-TW': '回報問題', en: 'Report an issue', ja: '不具合報告' },
  // 這三句在畫面上是同一段話，中間那句用比較亮的顏色標出來 ——
  // 所以三種語言都要能照這個順序接起來讀得通。
  diagIncludes: {
    'zh-TW': '內容包含版本、設定、健康檢查、失敗的列印工作與最近的 log。',
    en: 'It includes the version, settings, health checks, failed print jobs and recent logs. ',
    ja: 'バージョン・設定・ヘルスチェック・失敗した印刷ジョブ・最近のログが含まれます。',
  },
  diagExcludes: {
    'zh-TW': '不含品名、客人資訊或帳單明細',
    en: 'It never includes item names, customer information or bill details',
    ja: '商品名・お客様情報・伝票明細は含まれません',
  },
  diagNoUpload: {
    'zh-TW': '，而且不會自動上傳任何東西。',
    en: ', and nothing is ever uploaded automatically.',
    ja: '。また、自動的にアップロードされることもありません。',
  },
  diagGenerate: {
    'zh-TW': '產生診斷資訊',
    en: 'Generate report',
    ja: '診断情報を作成',
  },
  diagFailed: {
    'zh-TW': '讀不到診斷資訊：{msg}',
    en: 'Cannot read the diagnostics: {msg}',
    ja: '診断情報を取得できません：{msg}',
  },
  copyAll: { 'zh-TW': '複製全部', en: 'Copy all', ja: 'すべてコピー' },
  copied: { 'zh-TW': '已複製', en: 'Copied', ja: 'コピーしました' },

  /**
   * 出單佇列的四種狀況。後端只回代碼（見 api.ts 的 PrintQueueStatus.state），
   * 句子在這裡組 —— 數字前端本來就有。
   *
   * dead 是最嚴重的一種：單已經印不出來了。POS 最常見的客訴是「廚房沒收到單」，
   * 而根因幾乎都是系統知道印失敗了卻沒告訴任何人，所以這一句要直接說出
   * 「去哪裡看原因」。
   */
  queueDead: {
    'zh-TW': '有 {n} 張單印不出來 —— 請到出單機設定看原因',
    en: '{n} ticket(s) failed to print - open Printers to see why',
    ja: '{n} 件が印刷できません。プリンター設定で原因を確認してください',
  },
  queueUnrouted: {
    'zh-TW': '有 {n} 張單等著印，但還沒有設定任何出單機',
    en: '{n} ticket(s) waiting, but no printer is set up yet',
    ja: '{n} 件が印刷待ちですが、プリンターが未設定です',
  },
  queuePending: {
    'zh-TW': '{n} 張單排隊中',
    en: '{n} ticket(s) queued',
    ja: '{n} 件を印刷待ち',
  },
  queueOk: { 'zh-TW': '出單正常', en: 'Printing OK', ja: '印刷は正常です' },
} satisfies Catalog
