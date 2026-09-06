/**
 * 顧客掃碼點餐頁的體積守門。
 *
 * 客人是在店裡用 4G 或擁擠的店內 Wi-Fi 開這一頁的，每 100KB 都有感。
 * 多 entry 的設計就是為了讓收銀機那一大包（表格、數字鍵盤、報表、設定）
 * 不會被下載到客人手機上 —— 但沒有一道機械化的守門，這個優勢會在幾個月內被侵蝕掉。
 */
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { gzipSync } from 'node:zlib'
import { join } from 'node:path'

const DIST = 'dist/assets'
const LIMIT_KB = 150

/** order entry 會載入的 chunk：它自己 + 共用的 vendor 與樣式。 */
const PREFIXES = ['order-', 'react-vendor-', 'styles-']

let total = 0
const rows = []
for (const f of readdirSync(DIST)) {
  if (!PREFIXES.some((p) => f.startsWith(p))) continue
  if (!f.endsWith('.js') && !f.endsWith('.css')) continue
  const bytes = gzipSync(readFileSync(join(DIST, f))).length
  total += bytes
  rows.push([f, bytes])
}

if (rows.length === 0) {
  console.error(`找不到 ${DIST} 下的 order entry 產物，請先跑 npm run build`)
  process.exit(1)
}

for (const [f, b] of rows.sort((a, b) => b[1] - a[1])) {
  console.log(`  ${(b / 1024).toFixed(1).padStart(7)} KB gz  ${f}`)
}
const kb = total / 1024
console.log(`order entry 合計：${kb.toFixed(1)} KB gz（上限 ${LIMIT_KB} KB）`)

if (kb > LIMIT_KB) {
  console.error(
    `\n超出上限。顧客手機的 bundle 不該包含收銀機的程式碼 —— ` +
      `請檢查 src/order/ 是否 import 到 src/shared/ 以外的東西。`,
  )
  process.exit(1)
}
