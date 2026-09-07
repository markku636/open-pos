import { createContext, useContext, useMemo, useState, type ReactNode } from 'react'

/**
 * 多語系：繁體中文 / English / 日本語。
 *
 * # 為什麼三個語言寫在同一個物件裡，而不是三份字典檔
 *
 * ```ts
 * cancel: { 'zh-TW': '取消', en: 'Cancel', ja: 'キャンセル' }
 * ```
 *
 * 三份分開的檔案（`zh.json` / `en.json` / `ja.json`）最常見的下場是**漂移**：
 * 有人加了一個中文鍵，忘了加日文，而那個鍵要等到某個日本店員按到那顆按鈕
 * 才會被發現 —— 到時候畫面上是一個空白或一串鍵名。
 *
 * 三種語言擺在同一行的話，少寫一個 **TypeScript 當場就會紅**（`Msg` 三個欄位
 * 都是必填），根本進不了 commit。翻譯的品質保證不了，但「有沒有翻」可以。
 *
 * # 語言是店家層級的設定，不是每個使用者各自的偏好
 *
 * 收銀機是共用的機器，交接班不會換語言。所以它存在後端設定裡，
 * 而不是瀏覽器的 localStorage —— 不然同一間店的兩台機器會顯示不同語言，
 * 而且換一台電腦就要重設一次。
 *
 * # 刻意不做自動偵測
 *
 * 不看 `navigator.language`。收銀機的作業系統語言跟店家想顯示的語言沒有關係，
 * 而「它自己變了」是使用者最難自行排除的一種問題。
 */
export const LOCALES = ['zh-TW', 'en', 'ja'] as const

export type Locale = (typeof LOCALES)[number]

/** 一則訊息的三種語言。三個都是必填 —— 漏一個編譯就不會過。 */
export type Msg = { readonly 'zh-TW': string; readonly en: string; readonly ja: string }

/** 一份字典。用 `satisfies Catalog` 宣告，就能同時拿到型別檢查與鍵值的自動完成。 */
export type Catalog = Readonly<Record<string, Msg>>

export const LOCALE_LABELS: Record<Locale, string> = {
  'zh-TW': '繁體中文',
  en: 'English',
  ja: '日本語',
}

/**
 * 把 `{name}` 換成對應的值。
 *
 * 找不到的佔位符**原樣留著**：畫面上看到 `{n}` 至少看得出來是漏了參數，
 * 換成空字串會變成「還有 張單沒結帳」這種讀不懂、又不像壞掉的句子。
 */
function interpolate(template: string, params?: Record<string, string | number>): string {
  if (!params) return template
  let out = template
  for (const [k, v] of Object.entries(params)) {
    // 用 split/join 而不是 replaceAll：後者要 ES2021，改 tsconfig 的 target
    // 只為了一個字串函式不值得。
    out = out.split(`{${k}}`).join(String(v))
  }
  return out
}

/**
 * 取一則訊息的指定語言版本。
 *
 * 該語言是空字串時退回中文 —— 顯示中文比顯示空白好，
 * 空白會讓人以為是畫面壞了。
 */
export function translate(
  msg: Msg,
  locale: Locale,
  params?: Record<string, string | number>,
): string {
  const raw = msg[locale] || msg['zh-TW'] || ''
  return interpolate(raw, params)
}

type LocaleValue = {
  locale: Locale
  setLocale: (l: Locale) => void
}

const LocaleContext = createContext<LocaleValue>({
  locale: 'zh-TW',
  setLocale: () => {},
})

export function LocaleProvider({
  initial = 'zh-TW',
  onChange,
  children,
}: {
  initial?: Locale
  /** 換語言時把它存回後端（店家層級設定）。 */
  onChange?: (l: Locale) => void
  children: ReactNode
}) {
  const [locale, setLocaleState] = useState<Locale>(initial)
  const value = useMemo<LocaleValue>(
    () => ({
      locale,
      setLocale: (l) => {
        setLocaleState(l)
        onChange?.(l)
      },
    }),
    [locale, onChange],
  )
  return <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>
}

export function useLocale(): LocaleValue {
  return useContext(LocaleContext)
}

/**
 * 翻譯函式。
 *
 * ```tsx
 * const t = useT()
 * <button>{t(ui.cancel)}</button>
 * <p>{t(ui.unpaidCount, { n: 3 })}</p>
 * ```
 *
 * 吃的是 `Msg` 物件而不是字串鍵：拼錯鍵名在編譯期就會被抓到，
 * 不必等到執行期才發現畫面上是一串 `order.cart.empty`。
 */
export function useT() {
  const { locale } = useLocale()
  return useMemo(
    () =>
      (msg: Msg, params?: Record<string, string | number>): string =>
        translate(msg, locale, params),
    [locale],
  )
}

/** 判斷一個字串是不是合法的語言代碼（讀後端設定時用）。 */
export function asLocale(s: string | null | undefined): Locale {
  return (LOCALES as readonly string[]).includes(s ?? '') ? (s as Locale) : 'zh-TW'
}
