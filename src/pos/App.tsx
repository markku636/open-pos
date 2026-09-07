import { useEffect, useState } from 'react'

import AboutDialog from './AboutDialog'
import BackupPanel from './BackupPanel'
import BillsPanel from './BillsPanel'
import LanPanel from './LanPanel'
import MenuManager from './MenuManager'
import OrderScreen, { type Seat } from './OrderScreen'
import TableMap from './TableMap'
import PrinterSettings from './PrinterSettings'
import GatewayPanel from './GatewayPanel'
import DiningPlanPanel from './DiningPlanPanel'
import { LocaleProvider, LOCALE_LABELS, LOCALES, useLocale, useT, type Locale, type Msg } from '@/shared/i18n'
import { nav, ui } from '@/shared/locales/nav'
import { statusbar } from '@/shared/locales/statusbar'
import SalesPanel from './SalesPanel'
import ShiftPanel from './ShiftPanel'
import {
  api,
  diagnosticsApi,
  printerApi,
  transport,
  type AppError,
  type AppInfo,
  type Health,
  type PrintQueueStatus, localeApi} from '@/shared/api'
import { APP_NAME } from '@/shared/brand'

type Tab = 'order' | 'tables' | 'bills' | 'sales' | 'menu' | 'printer' | 'gateway' | 'plan' | 'shift' | 'backup' | 'status'

/**
 * 最外層：把語言設定讀進來，再交給 `LocaleProvider`。
 *
 * 拆成兩層是因為 `useT()` 必須在 Provider **底下**才拿得到語言 ——
 * 同一個元件既提供 context 又消費它，讀到的永遠是預設值。
 *
 * 讀設定失敗就用繁中開下去，不擋畫面：語言讀不到是小事，
 * 收銀機開不起來是大事。
 */
export default function App() {
  const [initial, setInitial] = useState<Locale | null>(null)

  useEffect(() => {
    localeApi
      .get()
      .then(setInitial)
      .catch(() => setInitial('zh-TW'))
  }, [])

  // 還沒讀到之前不要先畫一次中文再跳成日文 —— 那一下閃爍看起來像壞掉。
  if (!initial) return <div className="h-screen bg-slate-950" />

  return (
    <LocaleProvider initial={initial} onChange={(l) => void localeApi.set(l).catch(() => {})}>
      <Shell />
    </LocaleProvider>
  )
}

/** 收銀機主畫面。 */
function Shell() {
  const t = useT()
  const [tab, setTab] = useState<Tab>('order')
  const [about, setAbout] = useState(false)
  // 目前正在服務的那一桌。放在最上層是因為它跨兩個分頁：
  // 在「桌位」點一桌，動作發生在「點餐」。
  const [seat, setSeat] = useState<Seat | null>(null)
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
      <header className="flex shrink-0 flex-wrap items-center gap-1 border-b border-slate-800 px-4 py-2">
        {/* 招牌就該看得見。收銀機整天開著，這是店員唯一會一直看到的品牌。 */}
        <img src="/app-icon.png" alt="" className="mr-2 h-9 w-9 rounded-lg" draggable={false} />
        <span className="mr-4 text-lg font-semibold tracking-tight">{APP_NAME}</span>
        <TabButton active={tab === 'order'} onClick={() => setTab('order')}>
          {t(nav.order)}
          {seat && <span className="ml-1.5 text-emerald-300">{seat.table.code}</span>}
        </TabButton>
        <TabButton active={tab === 'tables'} onClick={() => setTab('tables')}>
          {t(nav.tables)}
        </TabButton>
        <TabButton active={tab === 'bills'} onClick={() => setTab('bills')}>
          {t(nav.bills)}
        </TabButton>
        <TabButton active={tab === 'sales'} onClick={() => setTab('sales')}>
          {t(nav.sales)}
        </TabButton>
        <TabButton active={tab === 'menu'} onClick={() => setTab('menu')}>
          {t(nav.menu)}
        </TabButton>
        <TabButton active={tab === 'printer'} onClick={() => setTab('printer')}>
          {t(nav.printer)}
        </TabButton>
        <TabButton active={tab === 'gateway'} onClick={() => setTab('gateway')}>
          {t(nav.gateway)}
        </TabButton>
        <TabButton active={tab === 'plan'} onClick={() => setTab('plan')}>
          {t(nav.plan)}
        </TabButton>
        <TabButton active={tab === 'shift'} onClick={() => setTab('shift')}>
          {t(nav.shift)}
        </TabButton>
        <TabButton active={tab === 'backup'} onClick={() => setTab('backup')}>
          {t(nav.backup)}
        </TabButton>
        <TabButton active={tab === 'status'} onClick={() => setTab('status')}>
          {t(nav.status)}
        </TabButton>
        <span className="ml-auto" />
        {queue?.needsAttention && (
          <button
            className="mr-2 flex items-center gap-1.5 rounded bg-red-950/70 px-3 py-1.5 text-sm text-red-200 hover:bg-red-900/70"
            title={t(statusbar.queueBadgeTitle)}
            onClick={() => setTab('printer')}
          >
            <span className="h-2 w-2 animate-pulse rounded-full bg-red-400" />
            {queueText(t, queue)}
          </button>
        )}
        <LocalePicker />
        <button
          className="rounded px-3 py-1.5 text-sm text-slate-400 hover:text-slate-200"
          onClick={() => setAbout(true)}
        >
          {t(nav.about)}
        </button>
      </header>

      <main className="min-h-0 flex-1 overflow-hidden p-4">
        {/* 兩個分頁都保持掛載：切到「桌位」看一眼再切回來，購物車不能不見。
            內用點到一半跑去查別桌多少錢是每天都在發生的事。 */}
        <div className={tab === 'order' ? 'h-full' : 'hidden'}>
          <OrderScreen seat={seat} onLeaveSeat={() => setSeat(null)} />
        </div>
        {tab === 'tables' && (
          <TableMap
            onPick={(table, guestCount) => {
              setSeat({ table, guestCount })
              setTab('order')
            }}
          />
        )}
        {tab === 'bills' && <BillsPanel />}
        {tab === 'sales' && <SalesPanel />}
        {tab === 'menu' && <MenuManager />}
        {tab === 'printer' && <PrinterSettings />}
        {tab === 'gateway' && <GatewayPanel />}
        {tab === 'plan' && <DiningPlanPanel />}
        {tab === 'shift' && <ShiftPanel />}
        {tab === 'backup' && <BackupPanel />}
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
  const t = useT()
  const [health, setHealth] = useState<Health | null>(null)

  useEffect(() => {
    // 不健康時後端回 503，transport 會丟出 AppError —— 但那是
    // 「檢查結果是壞的」而不是「檢查失敗了」，所以不併進 error 顯示。
    api
      .health()
      .then(setHealth)
      .catch(() => setHealth(null))
  }, [])

  // 後端那一欄有三種狀態：查到了、連不上、還在查。三種都要說得出來 ——
  // 空白會被當成「後端沒裝」，而那正是最不想讓人誤會的一件事。
  const backendValue = info
    ? t(statusbar.backendVersion, { v: info.version, s: info.schemaVersion })
    : error
      ? t(statusbar.backendFailed, { msg: error.message })
      : t(ui.loading)

  return (
    <div className="max-w-3xl space-y-6">
      <section>
        <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
          {t(statusbar.sectionVersion)}
        </h2>
        <dl className="space-y-1 font-mono text-sm">
          <Row label={t(statusbar.frontend)} value={__APP_VERSION__} />
          <Row label={t(statusbar.transport)} value={transport.kind} />
          <Row label={t(statusbar.backend)} value={backendValue} />
          {info && <Row label={t(statusbar.dataDir)} value={info.dataDir} />}
        </dl>
      </section>

      <section>
        <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
          {t(statusbar.sectionProgress)}
        </h2>
        {/*
          把路線圖放在這裡而不是頂欄：頂欄是收銀員整天盯著的地方，
          任何不影響「現在這一單」的字都是雜訊。想知道進度的人會自己來看這頁。
        */}
        {/* 句型只翻一次（`statusbar.ready`），會變的只有功能名稱：多一項功能
            就多一個短語，不必再翻一次整句。 */}
        <ul className="space-y-1 text-sm text-slate-400">
          {[
            statusbar.capOrdering,
            statusbar.capPrinter,
            statusbar.capShift,
            statusbar.capBackup,
            statusbar.capTables,
            statusbar.capKds,
            statusbar.capReports,
          ].map((cap) => (
            <li key={cap.en}>● {t(statusbar.ready, { what: t(cap) })}</li>
          ))}
        </ul>
      </section>

      {/* 區網放在診斷之前：裝機第一天會用到的是它，而診斷是事後才需要的。 */}
      <section>
        <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
          {t(statusbar.sectionLan)}
        </h2>
        <LanPanel />
      </section>

      <section>
        <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
          {t(statusbar.sectionReport)}
        </h2>
        <DiagnosticsBox />
      </section>

      {health && (
        <section>
          <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
            {t(statusbar.sectionHealth)}
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

/**
 * 診斷資訊。
 *
 * 「今天中午印不出來，現在又好了」是最常見的回報，而地端 + 離線 +
 * 非技術使用者三件事疊起來，代表維護者沒有任何辦法重現。
 * 所以要讓店家能一鍵拿到一份**貼得進 issue** 的純文字。
 */
function DiagnosticsBox() {
  const t = useT()
  const [text, setText] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [copied, setCopied] = useState(false)

  return (
    <div>
      {/* 三個節點是同一段話，中間那句換顏色。英文的第一則結尾留了一個空格 ——
          JSX 會把 {} 之間的換行整個吃掉，補不回來。 */}
      <p className="mb-2 text-xs text-slate-500">
        {t(statusbar.diagIncludes)}
        <span className="text-slate-400">{t(statusbar.diagExcludes)}</span>
        {t(statusbar.diagNoUpload)}
      </p>
      <div className="flex flex-wrap gap-2">
        <button
          className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-40"
          disabled={busy}
          onClick={() => {
            setBusy(true)
            diagnosticsApi
              .report()
              .then(setText)
              .catch((e: AppError) => setText(t(statusbar.diagFailed, { msg: e.message })))
              .finally(() => setBusy(false))
          }}
        >
          {t(statusbar.diagGenerate)}
        </button>
        {text && (
          <button
            className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700"
            onClick={() => {
              navigator.clipboard
                ?.writeText(text)
                .then(() => setCopied(true))
                .catch(() => {})
              setTimeout(() => setCopied(false), 2000)
            }}
          >
            {t(copied ? statusbar.copied : statusbar.copyAll)}
          </button>
        )}
      </div>
      {text && (
        <pre className="mt-3 max-h-80 overflow-auto rounded bg-slate-950/70 p-3 text-[11px] leading-relaxed text-slate-400">
          {text}
        </pre>
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

/**
 * 語言切換器。
 *
 * 用 select 而不是三顆按鈕：頂欄的橫向空間要留給分頁，而語言是裝機時設一次、
 * 之後幾乎不再碰的東西 —— 不值得長期佔著三個按鈕的寬度。
 *
 * 每個選項用**該語言自己的寫法**（English 不寫成「英文」），
 * 因為會去點它的人，多半正是看不懂目前這個語言的人。
 */
function LocalePicker() {
  const { locale, setLocale } = useLocale()
  return (
    <select
      className="rounded bg-slate-900 px-2 py-1.5 text-sm text-slate-300 hover:text-slate-100"
      value={locale}
      onChange={(e) => setLocale(e.target.value as Locale)}
      title={`${LOCALE_LABELS[locale]} · Language`}
    >
      {LOCALES.map((l) => (
        <option key={l} value={l}>
          {LOCALE_LABELS[l]}
        </option>
      ))}
    </select>
  )
}

/**
 * 出單佇列橫幅的句子。
 *
 * 後端只回 state 與三個數字，句子在這裡組 —— 這樣它才會跟著介面語言走。
 * 放在元件外面是因為 PrinterSettings 之後也要用同一句，
 * 而同一個狀況在兩個地方講不一樣的話，店家會以為是兩件事。
 */
function queueText(
  t: (m: Msg, p?: Record<string, string | number>) => string,
  q: PrintQueueStatus,
): string {
  switch (q.state) {
    case 'dead':
      return t(statusbar.queueDead, { n: q.dead })
    case 'unrouted':
      return t(statusbar.queueUnrouted, { n: q.unrouted })
    case 'pending':
      return t(statusbar.queuePending, { n: q.pending })
    default:
      return t(statusbar.queueOk)
  }
}
