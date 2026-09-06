import { useEffect, useState } from 'react'

import MenuManager from './MenuManager'
import OrderScreen from './OrderScreen'
import { api, transport, type AppError, type AppInfo, type Health } from '@/shared/api'

type Tab = 'order' | 'menu' | 'status'

/** 收銀機主畫面。 */
export default function App() {
  const [tab, setTab] = useState<Tab>('order')

  return (
    <div className="flex h-screen flex-col bg-slate-950 text-slate-100">
      <header className="flex shrink-0 items-center gap-1 border-b border-slate-800 px-4 py-2">
        <span className="mr-4 font-semibold">open-pos</span>
        <TabButton active={tab === 'order'} onClick={() => setTab('order')}>
          點餐
        </TabButton>
        <TabButton active={tab === 'menu'} onClick={() => setTab('menu')}>
          商品維護
        </TabButton>
        <TabButton active={tab === 'status'} onClick={() => setTab('status')}>
          系統狀態
        </TabButton>
        <span className="ml-auto text-xs text-slate-600">
          出單機在 M5、班別日結在 M6
        </span>
      </header>

      <main className="min-h-0 flex-1 overflow-hidden p-4">
        {tab === 'order' && <OrderScreen />}
        {tab === 'menu' && <MenuManager />}
        {tab === 'status' && <StatusPanel />}
      </main>
    </div>
  )
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      className={`rounded px-3 py-1.5 text-sm ${
        active ? 'bg-slate-800 text-slate-100' : 'text-slate-400 hover:text-slate-200'
      }`}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

/**
 * 系統狀態。
 *
 * 這一頁存在的理由：地端 + 離線 + 非技術使用者三件事疊起來，代表維護者永遠
 * 無法重現「今天中午印不出來、現在又好了」這種回報。所以要讓店家在打電話之前
 * 就能自己看出是哪一段斷了。
 */
function StatusPanel() {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [health, setHealth] = useState<Health | null>(null)
  const [error, setError] = useState<AppError | null>(null)

  useEffect(() => {
    api.appInfo().then(setInfo).catch(setError)
    // 不健康時後端回 503，transport 會丟出 AppError —— 但那是
    // 「檢查結果是壞的」而不是「檢查失敗了」，所以不併進 error 顯示。
    api
      .health()
      .then(setHealth)
      .catch(() => setHealth(null))
  }, [])

  return (
    <div className="max-w-2xl space-y-6">
      <section>
        <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
          版本
        </h2>
        <dl className="space-y-1 font-mono text-sm">
          <Row label="前端" value={__APP_VERSION__} />
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
      </section>

      {health && (
        <section>
          <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
            健康檢查
          </h2>
          <ul className="space-y-1 text-sm">
            {health.items.map((i) => (
              <li key={i.name} className="flex gap-2">
                <span className={i.ok ? 'text-emerald-400' : 'text-amber-400'}>
                  {i.ok ? '●' : '▲'}
                </span>
                <span className="w-16 shrink-0 text-slate-400">{i.name}</span>
                <span className={i.ok ? 'text-slate-300' : 'text-amber-200'}>
                  {i.detail}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  )
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex gap-2">
      <span className="w-20 shrink-0 text-slate-500">{label}</span>
      <span className="break-all">{value}</span>
    </div>
  )
}
