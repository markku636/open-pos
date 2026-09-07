import type { Catalog } from '@/shared/i18n'

/**
 * 頂欄分頁與全域按鈕。
 *
 * # 字典的寫法
 *
 * 三種語言擺在同一行，漏一個 TypeScript 當場就紅（`Msg` 三欄都是必填）。
 * 這是整套多語系唯一真正保證得了的事 —— 翻得好不好保證不了，
 * 但「有沒有翻」可以。
 *
 * 用 `satisfies Catalog` 而不是 `: Catalog`：前者保留每個鍵的字面型別，
 * 所以 `nav.order` 有自動完成、拼錯會紅；後者會把它壓成 `Record<string, Msg>`，
 * 拼錯的鍵要到執行期才發現。
 *
 * # 日文用詞
 *
 * 分頁標籤要**短**。日文與英文都比中文長，而頂欄有十個分頁 —— 一換行，
 * 整個工具列就往下擠掉一整列，點餐區跟著變矮。所以這裡寧可用「伝票」
 * 而不是更完整的「伝票・返金」。
 *
 * 收銀相關的詞用日本餐飲業的慣用語，不是字面直譯：
 * 結帳是「会計」不是「決済」、桌位是「テーブル」不是「席位」、
 * 日結是「日次締め」—— 直譯出來的日文，日本店員讀得懂但會覺得是外國軟體。
 */
export const nav = {
  order: { 'zh-TW': '點餐', en: 'Order', ja: '注文' },
  tables: { 'zh-TW': '桌位', en: 'Tables', ja: 'テーブル' },
  bills: { 'zh-TW': '帳單退款', en: 'Bills', ja: '伝票' },
  sales: { 'zh-TW': '銷售記錄', en: 'Sales', ja: '売上履歴' },
  menu: { 'zh-TW': '商品維護', en: 'Menu', ja: '商品管理' },
  printer: { 'zh-TW': '出單機', en: 'Printers', ja: 'プリンター' },
  gateway: { 'zh-TW': '金流', en: 'Payments', ja: '決済' },
  shift: { 'zh-TW': '班別日結', en: 'Shifts', ja: '日次締め' },
  backup: { 'zh-TW': '備份', en: 'Backup', ja: 'バックアップ' },
  status: { 'zh-TW': '系統狀態', en: 'System', ja: 'システム' },
  about: { 'zh-TW': '關於', en: 'About', ja: 'について' },
  language: {
    'zh-TW': '介面語言',
    en: 'Language',
    ja: '表示言語',
  },
} satisfies Catalog

/** 跨畫面共用的按鈕與短語。放這裡而不是各自複製一份，免得同一顆按鈕四種講法。 */
export const ui = {
  cancel: { 'zh-TW': '取消', en: 'Cancel', ja: 'キャンセル' },
  save: { 'zh-TW': '儲存', en: 'Save', ja: '保存' },
  delete: { 'zh-TW': '刪除', en: 'Delete', ja: '削除' },
  edit: { 'zh-TW': '設定', en: 'Configure', ja: '設定' },
  add: { 'zh-TW': '新增', en: 'Add', ja: '追加' },
  close: { 'zh-TW': '關閉', en: 'Close', ja: '閉じる' },
  retry: { 'zh-TW': '重試', en: 'Retry', ja: '再試行' },
  loading: { 'zh-TW': '查詢中…', en: 'Loading…', ja: '読み込み中…' },
  noData: { 'zh-TW': '沒有資料', en: 'No data', ja: 'データがありません' },
  // 「元」在日文是「円」，而那個字剛好是 Big5 編不出來的字之一 ——
  // 收據要印日文時，出單機那一頁必須選 Shift_JIS，否則它會變成空白。
  currency: { 'zh-TW': '元', en: 'NT$', ja: '円' },
} satisfies Catalog
