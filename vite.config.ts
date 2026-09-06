import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL('./package.json', import.meta.url)), 'utf-8'),
) as { version: string }

const root = fileURLToPath(new URL('.', import.meta.url))

export default defineConfig({
  plugins: [react()],
  clearScreen: false,

  resolve: {
    alias: { '@': resolve(root, 'src') },
  },

  server: {
    port: 1420,
    // strictPort：撞埠就直接失敗，不要偷偷換一個。
    // tauri.conf.json 的 devUrl 是寫死的 127.0.0.1:1420，
    // 偷換埠只會讓 App 開出一個空白視窗，而那很難查。
    strictPort: true,
    // 讓區網的平板 / 手機能連上 dev server 測 kds.html 與 order.html。
    host: true,
    // dev 時前端打 /api 轉給 cargo 起的 axum（GUI dev 也是同一個 port）。
    proxy: { '/api': { target: 'http://127.0.0.1:8129', ws: true } },
  },

  define: {
    // 版號的單一事實來源是 package.json，由 build-installer.ps1 三檔同步。
    __APP_VERSION__: JSON.stringify(pkg.version),
  },

  build: {
    // 三個 entry 而非單一 SPA。四個理由：
    // ① 顧客手機的 bundle 不該包含收銀機（他們常常是用 4G 或擁擠的店內 Wi-Fi 開）
    // ② 執行環境不同：pos 在 Tauri webview 走 IPC、kds/order 在真瀏覽器走 HTTP
    // ③ order.html 是唯一「不受信任的人會開」的頁面，分開才能單獨套嚴格 CSP
    // ④ Tauri 天然支援：視窗載 index.html，其餘由 axum 從同一份 dist/ 服務
    rollupOptions: {
      input: {
        index: resolve(root, 'index.html'),
        kds: resolve(root, 'kds.html'),
        order: resolve(root, 'order.html'),
      },
      output: {
        manualChunks(id: string) {
          if (!id.includes('node_modules')) return undefined
          if (/[\\/](react|react-dom|scheduler)[\\/]/.test(id)) return 'react-vendor'
          return undefined
        },
      },
    },
  },
})
