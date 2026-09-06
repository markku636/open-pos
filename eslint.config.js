import js from '@eslint/js'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  { ignores: ['dist', 'src-tauri', 'node_modules'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    rules: {
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
