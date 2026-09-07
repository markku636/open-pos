import { useCallback, useEffect, useState } from 'react'

import { lanApi, type AppError, type LanStatus } from '@/shared/api'
import { APP_NAME } from '@/shared/brand'
import { useT } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { system } from '@/shared/locales/system'

/**
 * 區網連線。
 *
 * # 為什麼這一段值得佔一整個區塊
 *
 * **裝機第一天的頭號故障就是「平板連不上收銀機」**，而它的三個成因都不會
 * 出現在任何錯誤訊息裡：防火牆的對話框被按了取消、路由器重開之後 IP 變了、
 * 主機有好幾張網卡而系統挑了錯的那張。
 *
 * 三個都是「東西看起來都正常，就是連不上」。所以這裡把平板要開的網址、
 * 所有候選網卡、上一次的 IP 全部攤開來 —— 讓店家在打電話之前就看得出
 * 是哪一段斷了。
 *
 * # 不印一個假的綠燈
 *
 * 從自己的區網 IP 連自己，只能證明 server 綁在對的網卡上；**證明不了防火牆**
 * （Windows 對自己連自己多半在核心裡就短路了）。唯一能證明的是另一台裝置，
 * 所以畫面上直接這樣寫，並且把那台裝置該開的網址放大。
 */
export default function LanPanel() {
  const t = useT()
  const [lan, setLan] = useState<LanStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState<string | null>(null)

  const load = useCallback(() => {
    lanApi
      .status()
      .then((s) => {
        setLan(s)
        setError(null)
      })
      .catch((e: AppError) => setError(e.message ?? String(e)))
  }, [])

  useEffect(load, [load])

  const copy = (what: string, text: string) => {
    void navigator.clipboard?.writeText(text).then(() => {
      setCopied(what)
      setTimeout(() => setCopied(null), 2000)
    })
  }

  if (error) {
    return (
      <p className="rounded border border-amber-800 bg-amber-950/40 px-3 py-2 text-sm text-amber-200">
        {t(system.lanFailed, { msg: error })}
      </p>
    )
  }
  if (!lan) return <p className="text-sm text-slate-600">{t(ui.loading)}</p>

  return (
    <div className="space-y-4">
      {/* ★ IP 換掉是最容易被忽略、代價又最高的一種故障：
          全店印好的桌卡 QR 會在同一秒集體失效，而畫面上什麼事都沒有。 */}
      {lan.ipChanged && (
        <p className="rounded border border-amber-700 bg-amber-950/50 px-3 py-2 text-sm text-amber-200">
          {/* IP 走佔位符而不是 <span className="font-mono">：中文的「從 A 變成 B」、
              英文的 from A to B、日文的 A から B に，語序各不相同 ——
              把句子拆成前後兩半再用 JSX 接回去，三種語言只有一種會對。 */}
          ⚠ {t(system.ipChanged, { from: lan.previousIp ?? '', to: lan.ip ?? '' })}
          <br />
          {t(system.ipChangedAction)}
          <span className="text-amber-300">{t(system.ipChangedFix)}</span>
        </p>
      )}

      {lan.kdsUrl ? (
        <div className="rounded border border-slate-700 bg-slate-900/60 p-4">
          <div className="text-xs text-slate-500">{t(system.kdsUrl)}</div>
          <div className="mt-1 flex items-center gap-3">
            <span className="select-all font-mono text-xl text-sky-300">{lan.kdsUrl}</span>
            <button
              className="rounded bg-slate-800 px-3 py-1.5 text-xs hover:bg-slate-700"
              onClick={() => copy('kds', lan.kdsUrl!)}
            >
              {copied === 'kds' ? t(system.copied) : t(system.copy)}
            </button>
          </div>
          <p className="mt-2 text-xs text-slate-500">
            {t(system.kdsHint)}
            <span className="text-slate-400">{t(system.kdsHintFirewall)}</span>
          </p>
        </div>
      ) : (
        <p className="rounded border border-amber-800 bg-amber-950/40 px-3 py-2 text-sm text-amber-200">
          {t(system.noLanAddress)}
        </p>
      )}

      <div className="flex items-start gap-2 text-sm">
        <span className={lan.bound ? 'text-emerald-400' : 'text-amber-400'}>
          {lan.bound ? '●' : '▲'}
        </span>
        <span className="text-slate-300">{lan.detail}</span>
      </div>

      <section>
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
          {t(system.interfaces, { n: lan.interfaces.length })}
        </h3>
        <ul className="space-y-1 text-sm">
          {lan.interfaces.map((i) => (
            <li key={`${i.name}-${i.ip}`} className="flex flex-wrap items-baseline gap-2">
              <span className={i.chosen ? 'text-emerald-400' : 'text-slate-700'}>
                {i.chosen ? '●' : '○'}
              </span>
              <span className="w-40 shrink-0 truncate text-slate-400" title={i.name}>
                {i.name}
              </span>
              <span className="font-mono text-slate-300">{i.ip}</span>
              {i.note && <span className="text-xs text-slate-600">{i.note}</span>}
              {i.chosen && (
                <span className="text-xs text-emerald-400">← {t(system.inUse)}</span>
              )}
            </li>
          ))}
        </ul>
      </section>

      <section>
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
          {t(system.troubleshoot)}
        </h3>
        <ol className="ml-4 list-decimal space-y-1 text-sm text-slate-400">
          {/* 原本「同一個」是用一個亮色 span 標出來的，翻譯之後整句一起翻 ——
              強調的位置在三種語言裡都不一樣，硬切開只會讓語序壞掉。 */}
          <li>{t(system.tipSameWifi)}</li>
          <li>{t(system.tipApIsolation)}</li>
          <li>{t(system.tipFirewall)}</li>
        </ol>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <code className="flex-1 select-all rounded bg-slate-950 px-3 py-2 font-mono text-xs text-slate-300">
            {lan.firewallCommand}
          </code>
          <button
            className="rounded bg-slate-800 px-3 py-2 text-xs hover:bg-slate-700"
            onClick={() => copy('fw', lan.firewallCommand)}
          >
            {copied === 'fw' ? t(system.copied) : t(system.copy)}
          </button>
        </div>
        {/* 不自己偷偷提權改防火牆：開源專案這樣做會被質疑，而且使用者
            也該知道自己的機器被改了什麼。 */}
        <p className="mt-1 text-xs text-slate-600">
          {t(system.firewallNote, { app: APP_NAME })}
        </p>
      </section>

      <button
        className="rounded bg-slate-800 px-4 py-2 text-sm hover:bg-slate-700"
        onClick={load}
      >
        {t(system.recheck)}
      </button>
    </div>
  )
}
