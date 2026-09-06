import { useCallback, useEffect, useState } from 'react'

import {
  backupApi,
  type AppError,
  type AppSettings,
  type BackupFile,
  type PendingRestore,
} from '@/shared/api'
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
      '已儲存',
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
          <h2 className="text-sm font-semibold text-amber-200">有一個還原在等待重新啟動</h2>
          <p className="mt-1 whitespace-pre-line text-sm text-amber-100/80">
            來源：{pending.source}
            {'\n'}關掉 open-pos 再重新開啟就會完成還原。現在的資料不會被刪除，
            會改名成 pre-restore 留在資料夾裡。
          </p>
          <button
            className="mt-3 rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700"
            disabled={busy}
            onClick={() => void run(() => backupApi.cancelRestore(), '已取消還原')}
          >
            取消還原
          </button>
        </section>
      )}

      {/* ─── 設定 ─────────────────────────────────────── */}
      <section className="rounded border border-slate-800 bg-slate-900/40 p-4">
        <h2 className="mb-1 text-sm font-semibold text-slate-300">備份設定</h2>
        <p className="mb-3 text-xs text-slate-500">
          備份跟資料庫放在同一顆硬碟上，只防「誤刪」不防「硬碟壞掉」。
          <span className="text-slate-400">請指定第二個位置</span>
          —— 一支常插著的 USB 隨身碟就夠了。
        </p>

        <div className="flex flex-wrap items-end gap-3">
          <label className="flex min-w-[20rem] flex-1 flex-col gap-1 text-xs text-slate-400">
            第二個備份位置
            <input
              className="rounded bg-slate-800 px-3 py-2 font-mono text-sm"
              placeholder="E:\\ 或 E:\\pos-backup"
              value={dir}
              onChange={(e) => setDir(e.target.value)}
            />
          </label>
          <button
            className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700 disabled:opacity-40"
            disabled={busy || !settings}
            onClick={() => patch({ externalDir: dir.trim() || null })}
          >
            儲存位置
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
              每小時自動備份
            </label>
            <label className="flex cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                checked={settings.backup.onClose}
                disabled={busy}
                onChange={(e) => patch({ onClose: e.target.checked })}
              />
              關班與日結時各備一次
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
                text: r.externalError
                  ? `本機備份完成（${kb(r.sizeBytes)}），但外接位置失敗：${r.externalError}`
                  : `備份完成（${kb(r.sizeBytes)}，${r.tookMs}ms）${
                      r.externalPath ? '，外接位置也有一份' : ''
                    }`,
              })
            })
          }
        >
          立刻備份一次
        </button>
      </section>

      {/* ─── 備份列表與還原 ─────────────────────────── */}
      <section>
        <h2 className="mb-1 text-sm font-semibold text-slate-300">備份檔</h2>
        <p className="mb-3 text-xs text-slate-500">
          還原不會刪掉現在的資料 —— 它會被改名成 pre-restore 留在資料夾裡。
          還原是人在慌張的時候做的事，所以不能是不可逆的。
        </p>

        {files.length === 0 ? (
          <p className="rounded border border-slate-800 bg-slate-900/40 px-4 py-4 text-sm text-slate-400">
            還沒有任何備份。按上面的「立刻備份一次」試一下 ——
            確認過自己會用，備份才有意義。
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
                  title={f.external ? '在第二個位置（隨身碟）上' : '在本機'}
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
                    if (
                      !confirm(
                        `要用這份備份還原嗎？\n\n${f.name}\n\n` +
                          '現在的資料會被改名保留（pre-restore），不會刪除。\n' +
                          '還原會在下次啟動 open-pos 時完成。',
                      )
                    )
                      return
                    void run(async () => {
                      const msg = await backupApi.stageRestore(f.path)
                      setMessage({ ok: true, text: msg })
                    })
                  }}
                >
                  還原
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
