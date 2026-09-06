import { useState } from 'react'

import { api, transport, type AppInfo } from '@/shared/api'
import { APP_NAME, REPO, TAGLINE, TOOL_PAGE_URL } from '@/shared/brand'
import {
  autoCheckEnabled,
  checkForUpdate,
  isNewer,
  setAutoCheckEnabled,
  type UpdateInfo,
} from '@/shared/updateCheck'

/** 手動檢查更新的狀態機：idle（還沒查）→ checking → latest / update / failed。 */
type CheckState =
  | { phase: 'idle' }
  | { phase: 'checking' }
  | { phase: 'latest' }
  | { phase: 'update'; info: UpdateInfo }
  | { phase: 'failed' }

/**
 * 「關於 open-pos」對話框（版面抄自 db-kit 的 AboutDialog）。
 *
 * 它看起來只是一頁作者資訊，實際上是**支援成本的第一道防線**。
 * 地端 + 離線 + 非技術使用者三件事疊起來，維護者永遠無法重現
 * 「今天中午印不出來、現在又好了」這種回報。所以這一頁要能讓店家在
 * 打電話之前，先把「哪一版、裝在哪、schema 幾號」一鍵複製貼進 issue。
 *
 * 更新檢查沿用 updateCheck，但帶 force 略過每日快取 ——
 * 使用者主動點的，就該真的去查一次。
 */
export default function AboutDialog({
  info,
  onClose,
}: {
  info: AppInfo | null
  onClose: () => void
}) {
  const [check, setCheck] = useState<CheckState>({ phase: 'idle' })
  const [copied, setCopied] = useState(false)
  const [auto, setAuto] = useState(autoCheckEnabled)

  const openUrl = (url: string) => {
    void api.openExternal(url).catch(() => {})
  }

  const runCheck = async () => {
    setCheck({ phase: 'checking' })
    const r = await checkForUpdate({ force: true }).catch(() => null)
    if (!r) setCheck({ phase: 'failed' })
    else if (isNewer(r.version, __APP_VERSION__)) setCheck({ phase: 'update', info: r })
    else setCheck({ phase: 'latest' })
  }

  // 回報問題時要附的東西。**刻意包含 schema 版本與資料目錄** ——
  // 這兩項是「升級後打不開」與「資料存到 OneDrive 裡」這兩類問題的第一線索。
  const versionInfo = [
    `${APP_NAME} v${__APP_VERSION__}`,
    info ? `backend ${info.version} / schema ${info.schemaVersion}` : 'backend 未連線',
    info ? `data ${info.dataDir}` : null,
    `transport ${transport.kind}`,
    navigator.userAgent,
  ]
    .filter(Boolean)
    .join('\n')

  const copyVersion = async () => {
    try {
      await navigator.clipboard.writeText(versionInfo)
      setCopied(true)
      setTimeout(() => setCopied(false), 1600)
    } catch {
      /* 沒有剪貼簿權限：使用者仍看得到畫面上的版本號 */
    }
  }

  const links = [
    { icon: IconExternal, label: 'GitHub 專案', url: `https://github.com/${REPO}` },
    { icon: IconBook, label: '工具介紹頁', url: TOOL_PAGE_URL },
    { icon: IconBug, label: '回報問題', url: `https://github.com/${REPO}/issues/new` },
  ]

  return (
    <div
      className="fixed inset-0 z-20 flex items-center justify-center bg-black/60 p-6"
      onClick={onClose}
    >
      <div
        className="w-full max-w-sm rounded-lg bg-slate-900 p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex flex-col items-center gap-1 text-center">
          <img
            src="/app-icon.png"
            alt={APP_NAME}
            className="mb-2 h-16 w-16 rounded-2xl"
            draggable={false}
          />
          <div className="text-lg font-semibold">{APP_NAME}</div>

          <div className="flex items-center gap-1 font-mono text-xs text-slate-500 tabular-nums">
            <span>版本 {__APP_VERSION__}</span>
            <button
              type="button"
              onClick={() => void copyVersion()}
              title="複製版本資訊（回報問題時附上）"
              className="grid h-5 w-5 place-items-center rounded text-slate-500 hover:bg-slate-800 hover:text-slate-200"
            >
              <IconCopy />
            </button>
            {copied && <span className="text-emerald-400">已複製</span>}
          </div>

          <p className="mt-1 text-sm text-slate-400">{TAGLINE}</p>

          {info && (
            // 資料目錄要完整顯示、不能截斷：「資料存到 OneDrive 資料夾裡」
            // 是這套系統最容易靜默壞掉的一種裝法，而店家只有在這裡看得到它。
            <div className="mt-2 w-full rounded bg-slate-950/60 px-2 py-1.5 text-left font-mono text-[11px] leading-relaxed text-slate-500">
              <div>schema {info.schemaVersion}　·　{transport.kind}</div>
              <div className="break-all text-slate-600">{info.dataDir}</div>
            </div>
          )}

          {/* 更新檢查 */}
          <div className="mt-3 flex min-h-[52px] flex-col items-center gap-2">
            {check.phase === 'update' && (
              <button
                type="button"
                // 導到部落格的工具頁，不是 GitHub Release ——
                // 那一頁有安裝說明與硬體需求，Release 頁對店家只是一串檔名。
                onClick={() => openUrl(TOOL_PAGE_URL)}
                className="inline-flex items-center gap-1.5 text-sm font-medium text-sky-400 hover:underline"
              >
                <span className="h-1.5 w-1.5 rounded-full bg-sky-400" aria-hidden />
                有新版 v{check.info.version}，點擊前往下載
              </button>
            )}
            {check.phase === 'latest' && (
              <div className="text-sm text-emerald-400">已是最新版本</div>
            )}
            {check.phase === 'failed' && (
              <div className="text-sm text-slate-500">
                查不到（離線或已達 GitHub 上限），稍後再試
              </div>
            )}
            <button
              type="button"
              disabled={check.phase === 'checking'}
              onClick={() => void runCheck()}
              className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700 disabled:opacity-50"
            >
              {check.phase === 'checking' ? '檢查中…' : '檢查更新'}
            </button>
          </div>

          {/*
            預設關閉自動檢查。地端優先的 POS 不該自作主張連外 ——
            而且店裡的網路一整天不通是常態，不是異常。
          */}
          <label className="mt-1 flex cursor-pointer items-center gap-2 text-xs text-slate-500">
            <input
              type="checkbox"
              checked={auto}
              onChange={(e) => {
                setAuto(e.target.checked)
                setAutoCheckEnabled(e.target.checked)
              }}
            />
            開機時自動檢查更新
          </label>

          <div className="mt-3 flex items-center gap-1">
            {links.map((l) => (
              <button
                type="button"
                key={l.label}
                onClick={() => openUrl(l.url)}
                className="inline-flex items-center gap-1.5 rounded px-2 py-1 text-[13px] text-slate-400 hover:bg-slate-800 hover:text-slate-200"
              >
                <l.icon />
                {l.label}
              </button>
            ))}
          </div>

          <div className="mt-3 text-[11px] text-slate-600">
            MIT 授權 · Tauri + React 打造
          </div>
        </div>

        <button
          type="button"
          className="mt-5 w-full rounded bg-slate-800 py-2 text-sm hover:bg-slate-700"
          onClick={onClose}
        >
          關閉
        </button>
      </div>
    </div>
  )
}

/*
 * 圖示用內嵌 SVG 而不是拉一個 icon 套件進來。
 * 掃碼點餐那個 entry 的 gzip 預算是 150KB，而 icon 套件很容易一路長到那個量級；
 * 這裡只需要四個圖形，手寫的成本遠低於再多一個相依。
 */
function svg(path: React.ReactNode) {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {path}
    </svg>
  )
}

function IconCopy() {
  return svg(
    <>
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </>,
  )
}

function IconExternal() {
  return svg(
    <>
      <path d="M15 3h6v6" />
      <path d="M10 14 21 3" />
      <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
    </>,
  )
}

function IconBook() {
  return svg(
    <>
      <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
    </>,
  )
}

function IconBug() {
  return svg(
    <>
      <path d="M8 2v2m8-2v2M5 10h14M5 14h14" />
      <rect x="8" y="6" width="8" height="14" rx="4" />
      <path d="M3 10h2m14 0h2M3 17h2m14 0h2" />
    </>,
  )
}
