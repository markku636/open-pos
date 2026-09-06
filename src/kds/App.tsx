import { useEffect, useState } from 'react'

import { api, transport, type AppError, type AppInfo } from '@/shared/api'

/** 廚房顯示 畫面。 */
export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [error, setError] = useState<AppError | null>(null)

  useEffect(() => {
    api.appInfo().then(setInfo).catch(setError)
  }, [])

  return (
    <main className="min-h-screen bg-slate-950 p-8 text-slate-100">
      <h1 className="text-2xl font-semibold">open-pos · 廚房顯示</h1>
      <p className="mt-2 text-slate-400">M1 骨架：v1.1 會接上 SSE 即時推播與離線佇列。</p>

      <dl className="mt-6 space-y-1 font-mono text-sm">
        <Row label="前端版本" value={__APP_VERSION__} />
        <Row label="傳輸層" value={transport.kind} />
        <Row
          label="後端"
          value={
            info
              ? `${info.version}（schema ${info.schemaVersion}）`
              : error
                ? `連線失敗：${error.message}`
                : '查詢中…'
          }
        />
        {info && <Row label="資料目錄" value={info.dataDir} />}
      </dl>
    </main>
  )
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex gap-2">
      <span className="w-24 shrink-0 text-slate-500">{label}</span>
      <span className="break-all">{value}</span>
    </div>
  )
}
