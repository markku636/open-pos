import { useEffect, useState } from 'react'

import AboutDialog from './AboutDialog'
import MenuManager from './MenuManager'
import OrderScreen from './OrderScreen'
import PrinterSettings from './PrinterSettings'
import {
  api,
  printerApi,
  transport,
  type AppError,
  type AppInfo,
  type Health,
  type PrintQueueStatus,
} from '@/shared/api'
import { APP_NAME } from '@/shared/brand'

type Tab = 'order' | 'menu' | 'printer' | 'status'

/** 收銀機主畫面。 */
export default function App() {
  const [tab, setTab] = useState<Tab>('order')
  const [about, setAbout] = useState(false)
  // 版本資訊在「系統狀態」與「關於」兩處都要用，所以在最上層抓一次。
  // 兩邊各抓一次不只是浪費，還會出現「兩個畫面顯示不同版本」這種
  // 讓人完全無法診斷的狀況。
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [infoError, setInfoError] = useState<AppError | null>(null)

  // 出單狀態。**每 5 秒重查一次，而且常駐在畫面上。**
  // POS 最常見的客訴是「廚房沒收到單」，技術根因幾乎都是
  // 「系統知道印失敗了，但沒有告訴任何人」。
  const [queue, setQueue] = useState<PrintQueueStatus | null>(null)

  useEffect(() => {
    api.appInfo().then(setInfo).catch(setInfoError)
  }, [])

  useEffect(() => {
    // 區網端沒有這個指令（設定類指令刻意不掛在區網上），查不到就不顯示。
    const poll = () => printerApi.queueStatus().then(setQueue).catch(() => setQueue(null))
    void poll()
    const id = setInterval(poll, 5000)
    return () => clearInterval(id)
  }, [])

  return (
    <div className="flex h-screen flex-col bg-slate-950 text-slate-100">
      <header className="flex shrink-0 items-center gap-1 border-b border-slate-800 px-4 py-2">
        <img src="/app-icon.png" alt="" className="mr-2 h-5 w-5 rounded" draggable={false} />
        <span className="mr-4 font-semibold">{APP_NAME}</span>
        <TabButton active={tab === 'order'} onClick={() => setTab('order')}>
          點餐
        </TabButton>
        <TabButton active={tab === 'menu'} onClick={() => setTab('menu')}>
          商品維護
        </TabButton>
        <TabButton active={tab === 'printer'} onClick={() => setTab('printer')}>
          出單機
        </TabButton>
        <TabButton active={tab === 'status'} onClick={() => setTab('status')}>
          系統狀態
        </TabButton>
        <span className="ml-auto" />
        {queue?.needsAttention && (
          <button
            className="mr-2 flex items-center gap-1.5 rounded bg-red-950/70 px-3 py-1.5 text-sm text-red-200 hover:bg-red-900/70"
            title="點一下看出單狀況"
            onClick={() => setTab('printer')}
          >
            <span className="h-2 w-2 animate-pulse rounded-full bg-red-400" />
            {queue.detail}
          </button>
        )}
        <button
          className="rounded px-3 py-1.5 text-sm text-slate-400 hover:text-slate-200"
          onClick={() => setAbout(true)}
        >
          關於
        </button>
      </header>

      <main className="min-h-0 flex-1 overflow-hidden p-4">
        {tab === 'order' && <OrderScreen />}
        {tab === 'menu' && <MenuManager />}
        {tab === 'printer' && <PrinterSettings />}
        {tab === 'status' && <StatusPanel info={info} error={infoError} />}
      </main>

      {about && <AboutDialog info={info} onClose={() => setAbout(false)} />}
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
function StatusPanel({ info, error }: { info: AppInfo | null; error: AppError | null }) {
  const [health, setHealth] = useState<Health | null>(null)

  useEffect(() => {
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

      <section>
        <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
          目前進度
        </h2>
        {/*
          把路線圖放在這裡而不是頂欄：頂欄是收銀員整天盯著的地方，
          任何不影響「現在這一單」的字都是雜訊。想知道進度的人會自己來看這頁。
        */}
        <ul className="space-y-1 text-sm text-slate-400">
          <li>● 點餐、結帳、商品維護：可以用了</li>
          <li>● 出單機（ESC/POS 網路型）：可以用了</li>
          <li>○ 班別交接與日結、備份還原：規劃中</li>
        </ul>
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
