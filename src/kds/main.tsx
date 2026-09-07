import React, { useEffect, useState } from 'react'
import ReactDOM from 'react-dom/client'

import { localeApi } from '@/shared/api'
import { LocaleProvider, type Locale } from '@/shared/i18n'

import App from './App'
import '../styles.css'

/**
 * 這一頁跟收銀機是**不同的 Vite entry**，所以不會繼承收銀機的 LocaleProvider ——
 * 少了這一層，整份字典查出來永遠是中文，而且沒有任何錯誤訊息。
 *
 * 語言跟著店家的設定走（`get_locale` 在區網上是唯讀開放的），不是跟著平板的
 * 作業系統語言：店家在收銀機上把語言改成日文之後，廚房那台也要跟著變，
 * 否則他會完全不知道為什麼只有一半的畫面換了語言。
 *
 * 讀不到就用中文開下去 —— 廚房看板不能因為查不到語言就開不起來。
 */
function Root() {
  const [locale, setLocale] = useState<Locale | null>(null)

  useEffect(() => {
    localeApi
      .get()
      .then(setLocale)
      .catch(() => setLocale('zh-TW'))
  }, [])

  if (!locale) return null

  return (
    <LocaleProvider initial={locale}>
      <App />
    </LocaleProvider>
  )
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
)
