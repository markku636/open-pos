import { useCallback, useEffect, useState } from 'react'

import {
  backupApi,
  type AppError,
  type AppSettings,
  type BackupFile,
  type PendingRestore,
} from '@/shared/api'
import { APP_NAME } from '@/shared/brand'
import { useT } from '@/shared/i18n'
import { system } from '@/shared/locales/system'
import { dateTime } from '@/shared/time'

/**
 * 備份與還原。
 *
 * 這一頁的存在理由只有一句話：**使用者第一個月掉資料，這個專案就結束了**
 * —— 而且他不會回來告訴你原因。
 *
 * 所以備份預設開啟、自己跑、而且還原按鈕就放在同一頁。
 * 一個「有備份但沒有人知道怎麼還原」的系統，等於沒有備份。
 */
export default function BackupPanel() {
  const t = useT()
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [files, setFiles] = useState<BackupFile[]>([])
  const [pending, setPending] = useState<PendingRestore | null>(null)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null)
  const [dir, setDir] = useState('')

  const reload = useCallback(async () => {
    try {
      const [s, f, p] = await Promise.all([
        backupApi.settings(),
        backupApi.list(),
        backupApi.pendingRestore(),
      ])
      setSettings(s)
      setDir(s.backup.externalDir ?? '')
      setFiles(f)
      setPending(p)
    } catch (e) {
      setMessage({ ok: false, text: (e as AppError).message ?? String(e) })
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const run = async (fn: () => Promise<unknown>, okText?: string) => {
    setBusy(true)
    setMessage(null)
    try {
      await fn()
      if (okText) setMessage({ ok: true, text: okText })
      await reload()
    } catch (e) {
      setMessage({ ok: false, text: (e as AppError).message ?? String(e) })
    } finally {
      setBusy(false)
    }
  }

  const patch = (p: Partial<AppSettings['backup']>) => {
    if (!settings) return
    void run(
      () => backupApi.saveSettings({ ...settings, backup: { ...settings.backup, ...p } }),
      t(system.saved),
    )
  }

  return (
    <div className="h-full space-y-6 overflow-y-auto pr-2">
      {message && (
        <p
          className={`whitespace-pre-line rounded px-3 py-2 text-sm ${
            message.ok
              ? 'border border-emerald-800 bg-emerald-950/60 text-emerald-200'
              : 'border border-red-800 bg-red-950/60 text-red-200'
          }`}
        >
          {message.text}
        </p>
      )}

      {pending && (
        <section className="rounded border border-amber-700 bg-amber-950/40 p-4">
          <h2 className="text-sm font-semibold text-amber-200">{t(system.pendingRestore)}</h2>
          <p className="mt-1 whitespace-pre-line text-sm text-amber-100/80">
            {t(system.restoreSource, { source: pending.source })}
            {'\n'}
            {t(system.restorePendingHint, { app: APP_NAME })}
          </p>
          <button
            className="mt-3 rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700"
            disabled={busy}
            onClick={() =>
              void run(() => backupApi.cancelRestore(), t(system.restoreCancelled))
            }
          >
            {t(system.cancelRestore)}
          </button>
        </section>
      )}

      {/* ─── 設定 ─────────────────────────────────────── */}
      <section className="rounded border border-slate-800 bg-slate-900/40 p-4">
        <h2 className="mb-1 text-sm font-semibold text-slate-300">{t(system.backupSettings)}</h2>
        <p className="mb-3 text-xs text-slate-500">
          {t(system.sameDiskWarn)}
          <span className="text-slate-400">{t(system.setSecondLocation)}</span>
          {t(system.usbEnough)}
        </p>

        <div className="flex flex-wrap items-end gap-3">
          <label className="flex min-w-[20rem] flex-1 flex-col gap-1 text-xs text-slate-400">
            {t(system.secondLocation)}
            <input
              className="rounded bg-slate-800 px-3 py-2 font-mono text-sm"
              placeholder={t(system.dirPlaceholder)}
              value={dir}
              onChange={(e) => setDir(e.target.value)}
            />
          </label>
          <button
            className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-40"
            disabled={busy || !settings}
            onClick={() => patch({ externalDir: dir.trim() || null })}
          >
            {t(system.saveLocation)}
          </button>
        </div>

        {settings && (
          <div className="mt-3 flex flex-wrap gap-4 text-sm text-slate-400">
            <label className="flex cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                checked={settings.backup.hourly}
                disabled={busy}
                onChange={(e) => patch({ hourly: e.target.checked })}
              />
              {t(system.hourly)}
            </label>
            <label className="flex cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                checked={settings.backup.onClose}
                disabled={busy}
                onChange={(e) => patch({ onClose: e.target.checked })}
              />
              {t(system.onClose)}
            </label>
          </div>
        )}

        <button
          className="mt-4 rounded bg-emerald-700 px-5 py-2.5 font-medium hover:bg-emerald-600 disabled:opacity-40"
          disabled={busy}
          onClick={() =>
            void run(async () => {
              const r = await backupApi.run()
              setMessage({
                ok: !r.externalError,
                // 「有沒有備到第二個位置」寫成兩則完整的句子，而不是在後面接一段
                // 「，外接位置也有一份」——中文接得起來，英日文的標點與語序接不起來。
                text: r.externalError
                  ? t(system.backupExternalFailed, {
                      size: kb(r.sizeBytes),
                      error: r.externalError,
                    })
                  : t(r.externalPath ? system.backupDoneExternal : system.backupDone, {
                      size: kb(r.sizeBytes),
                      ms: r.tookMs,
                    }),
              })
            })
          }
        >
          {t(system.backupNow)}
        </button>
      </section>

      {/* ─── 備份列表與還原 ─────────────────────────── */}
      <section>
        <h2 className="mb-1 text-sm font-semibold text-slate-300">{t(system.backupFiles)}</h2>
        <p className="mb-3 text-xs text-slate-500">{t(system.restoreSafeNote)}</p>

        {files.length === 0 ? (
          <p className="rounded border border-slate-800 bg-slate-900/40 px-4 py-4 text-sm text-slate-400">
            {t(system.noBackups)}
          </p>
        ) : (
          <div className="space-y-1">
            {files.map((f) => (
              <div
                key={f.path}
                className="flex flex-wrap items-center gap-3 rounded bg-slate-900/40 px-3 py-2 text-sm"
              >
                <span
                  className={`rounded px-2 py-0.5 text-xs ${
                    f.external ? 'bg-emerald-900/60 text-emerald-300' : 'bg-slate-800 text-slate-400'
                  }`}
                  title={f.external ? t(system.fileExternal) : t(system.fileLocal)}
                >
                  {f.bucket}
                </span>
                <span className="font-mono text-xs text-slate-400">{f.name}</span>
                <span className="text-xs text-slate-600">{kb(f.sizeBytes)}</span>
                <span className="text-xs text-slate-600">{dateTime(f.modifiedAt)}</span>
                <button
                  className="ml-auto rounded bg-slate-800 px-3 py-1 text-xs hover:bg-amber-800"
                  disabled={busy}
                  onClick={() => {
                    if (!confirm(t(system.restoreConfirm, { name: f.name, app: APP_NAME })))
                      return
                    void run(async () => {
                      const msg = await backupApi.stageRestore(f.path)
                      setMessage({ ok: true, text: msg })
                    })
                  }}
                >
                  {t(system.restore)}
                </button>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

function kb(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}
