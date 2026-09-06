/**
 * 產品品牌的單一真相（對標 db-kit 的 src/brand.ts）。
 *
 * 不讓 "open-pos" 這幾個字散落在 JSX 裡的理由，跟 db-kit 一樣：
 * 品牌名永不翻譯，集中成常數之後，未來做 i18n 時 codemod 不會誤包 t()，
 * 文案則以參數化的方式帶入。另外，有店家想掛自己招牌時，
 * 只要改這一行，而不是去 grep 整個 codebase。
 */
export const APP_NAME = 'open-pos'

/** GitHub 專案（更新檢查與「回報問題」都指到這裡）。 */
export const REPO = 'markku636/open-pos'

/** 作者部落格。 */
export const BLOG_URL = 'https://blog.markkulab.net/'

/**
 * 部落格上的工具介紹頁（免費工具那一欄）。
 *
 * 「有新版，點擊前往下載」導到這裡而不是 GitHub Release：
 * 那一頁有安裝說明、硬體需求與截圖，而 GitHub 的 Release 頁對一位
 * 開早餐店的老闆只是一串看不懂的檔名。下載連結本來就掛在那一頁上。
 */
export const TOOL_PAGE_URL = 'https://blog.markkulab.net/tools/open-pos'

/** 一句話介紹。「關於」對話框與 README 第一行講的是同一件事。 */
export const TAGLINE = '地端優先的開源餐飲 POS：點餐、結帳、出單、日結'
