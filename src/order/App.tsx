import { useEffect, useState } from 'react'

import { api, transport, type AppError } from '@/shared/api'

/** 顧客點餐 畫面。 */
export default function App() {
  const [version, setVersion] = useState<string | null>(null)
  const [error, setError] = useState<AppError | null>(null)

  useEffect(() => {
    api
      .appInfo()
      .then((info) => setVersion(info.version))
      .catch((e: AppError) => setError(e))
  }, [])

  return (
    <main className="min-h-screen bg-slate-950 p-8 text-slate-100">
      <h1 className="text-2xl font-semibold">open-pos · 顧客點餐</h1>
      <p className="mt-2 text-slate-400">M0 骨架：v1.3 會接上桌卡 QR 與雙層授權。</p>
      <dl className="mt-6 space-y-1 text-sm">
        <div>
          <span className="text-slate-500">前端版本：</span>
          <span className="font-mono">{__APP_VERSION__}</span>
        </div>
        <div>
          <span className="text-slate-500">傳輸層：</span>
          <span className="font-mono">{transport.kind}</span>
        </div>
        <div>
          <span className="text-slate-500">後端版本：</span>
          <span className="font-mono">
            {version ?? (error ? `（尚未連線：${error.code}）` : '查詢中…')}
          </span>
        </div>
      </dl>
    </main>
  )
}
