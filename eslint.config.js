import js from '@eslint/js'
import reactHooks from 'eslint-plugin-react-hooks'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  { ignores: ['dist', 'src-tauri', 'node_modules'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    plugins: { 'react-hooks': reactHooks },
    rules: {
      // ★ 這兩條要裝了外掛才存在。
      //
      //   在這之前，`SalesPanel.tsx` 裡有一行
      //   `// eslint-disable-next-line react-hooks/exhaustive-deps`，
      //   而那條規則根本沒被載入 —— eslint 10 對「關掉一條不存在的規則」
      //   是報錯的，於是 CI 的 Frontend job 一直是紅的。
      //   註解看起來在做事，其實兩件事都沒做到：規則沒開，CI 也過不了。
      //
      // `rules-of-hooks` 是 error：條件式呼叫 hook 是真的會壞。
      // `exhaustive-deps` 只給 warn：它的建議有時候是錯的（例如刻意
      //   只在某幾個值變動時重跑），而讓一條會誤判的規則擋住 CI，
      //   結果會是大家開始亂加 disable 註解 —— 那比沒有規則更糟。
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',

      // 全形空格（U+3000）在中文排版裡是正當的字元 —— 用來分隔
      // 「未稅 199　稅 10」這種並列的資訊，比兩個半形空格好看得多。
      // 但它在程式碼裡確實可能是誤貼進來的，所以只在 JSX 文字與字串裡放行。
      'no-irregular-whitespace': ['error', { skipJSXText: true, skipStrings: true, skipTemplates: true }],
    },
    languageOptions: {
      globals: {
        window: 'readonly',
        document: 'readonly',
        fetch: 'readonly',
        console: 'readonly',
        __APP_VERSION__: 'readonly',
      },
    },
  },
)
