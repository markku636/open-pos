import type { Catalog } from '@/shared/i18n'

/**
 * 系統類畫面：「關於」對話框、區網連線、備份與還原。
 *
 * # 這三頁的字為什麼特別難翻
 *
 * 這裡的文案有一半是**在講原因**，不是在下指令：「還原不會刪掉現在的資料」、
 * 「這一頁上的檢查證明不了防火牆」。這些句子存在的理由，是店家在慌張的時候
 * 需要的是「按下去會發生什麼」，而不是一顆按鈕。所以翻譯時整句一起翻，
 * 不要為了對齊中文的斷句而切成一節一節 —— 切碎之後日文的語序會壞掉。
 *
 * 同理，IP、備份大小、檔名這些會變的值一律走佔位符（`{from}`、`{size}`），
 * 而不是把句子拆成前後兩半再用 JSX 接起來：中文的「從 A 變成 B」在英文是
 * 「from A to B」、日文是「A から B に」，拆開之後沒有一種語言接得回去。
 *
 * # 按鈕要短
 *
 * 「關於」對話框只有 max-w-sm 寬，底下三顆連結按鈕排成一列。日文的
 * 「GitHubリポジトリ」「ツール紹介ページ」照字面翻會直接把那一列撐爆，
 * 所以這裡刻意用「GitHub」「ツール紹介」——寧可短，不要換行。
 *
 * # 刻意沒放進來的東西
 *
 * 後端回傳的字串（`lan.detail`、`f.bucket`、`AppError.message`）不在這裡。
 * 那些是伺服器那邊的責任，前端把它們塞進字典只會得到一份永遠對不上的假翻譯。
 *
 * # 有幾則英文的結尾**故意留一個空格**
 *
 * `ipChangedAction`、`kdsHint`、`sameDiskWarn` 這三則在畫面上都是直接接著
 * 下一則印出來的（中間那一段要換顏色，所以拆成兩個節點）。中文與日文的句號
 * 後面本來就不空格，英文要 —— 而 JSX 會把兩個 `{}` 之間的換行整個吃掉，
 * 補不回來。所以空格寫在英文那一則的字串尾巴，別當成手滑刪掉。
 */
export const system = {
  // ─── 關於對話框 ───────────────────────────────
  tagline: {
    'zh-TW': '地端優先的開源餐飲 POS：點餐、結帳、出單、日結',
    en: 'Local-first open source restaurant POS: orders, checkout, tickets, day-end',
    ja: 'ローカル優先のオープンソース飲食店POS：注文・会計・伝票印刷・日次締め',
  },
  version: { 'zh-TW': '版本 {v}', en: 'Version {v}', ja: 'バージョン {v}' },
  copyVersion: {
    'zh-TW': '複製版本資訊（回報問題時附上）',
    en: 'Copy version info (attach it when reporting an issue)',
    ja: 'バージョン情報をコピー（不具合報告に添付）',
  },
  copied: { 'zh-TW': '已複製', en: 'Copied', ja: 'コピーしました' },
  copy: { 'zh-TW': '複製', en: 'Copy', ja: 'コピー' },
  // 版本資訊裡「連不到後端」的那一行。它會被複製進 issue，所以三種語言都要有 ——
  // 店家貼上來的第一行如果是中文，維護者反而看得出他用的是哪一種介面語言。
  backendOffline: {
    'zh-TW': 'backend 未連線',
    en: 'backend not connected',
    ja: 'backend 未接続',
  },
  updateAvailable: {
    'zh-TW': '有新版 v{v}，點擊前往下載',
    en: 'v{v} is available, click to download',
    ja: '新バージョン v{v} があります。クリックでダウンロード',
  },
  upToDate: { 'zh-TW': '已是最新版本', en: 'Up to date', ja: '最新バージョンです' },
  checkFailed: {
    'zh-TW': '查不到（離線或已達 GitHub 上限），稍後再試',
    en: 'Cannot check (offline or GitHub rate limit), try again later',
    ja: '確認できません（オフラインまたはGitHubの制限）。後でもう一度お試しください',
  },
  checking: { 'zh-TW': '檢查中…', en: 'Checking…', ja: '確認中…' },
  checkUpdate: { 'zh-TW': '檢查更新', en: 'Check for updates', ja: '更新を確認' },
  autoCheck: {
    'zh-TW': '開機時自動檢查更新',
    en: 'Check for updates at startup',
    ja: '起動時に更新を確認する',
  },
  // 三顆連結按鈕排成一列，日文照字面翻會撐爆 max-w-sm 的對話框。
  linkRepo: { 'zh-TW': 'GitHub 專案', en: 'GitHub', ja: 'GitHub' },
  linkToolPage: { 'zh-TW': '工具介紹頁', en: 'Tool page', ja: 'ツール紹介' },
  linkIssue: { 'zh-TW': '回報問題', en: 'Report issue', ja: '不具合報告' },
  license: {
    'zh-TW': 'MIT 授權 · Tauri + React 打造',
    en: 'MIT licensed · Built with Tauri + React',
    ja: 'MITライセンス · Tauri + React 製',
  },

  // ─── 區網連線 ─────────────────────────────────
  lanFailed: {
    'zh-TW': '查不到區網狀態：{msg}',
    en: 'Cannot read LAN status: {msg}',
    ja: 'LANの状態を取得できません：{msg}',
  },
  ipChanged: {
    'zh-TW': '主機的 IP 從 {from} 變成 {to}。',
    en: 'The host IP changed from {from} to {to}.',
    ja: 'このパソコンのIPが {from} から {to} に変わりました。',
  },
  ipChangedAction: {
    'zh-TW': '平板上存的網址要改，貼在桌上的 QR 也要重印。',
    en: 'Update the address saved on the tablets, and reprint the QR codes on the tables. ',
    ja: 'タブレットに保存したURLの変更と、テーブルのQRコードの再印刷が必要です。',
  },
  ipChangedFix: {
    'zh-TW': '要避免它再發生，請到路由器把這台主機設成固定 IP 或 DHCP 保留。',
    en: 'To stop it happening again, give this computer a static IP or a DHCP reservation on the router.',
    ja: '再発を防ぐには、ルーターでこのパソコンを固定IPまたはDHCP予約に設定してください。',
  },
  kdsUrl: {
    'zh-TW': '廚房平板要開的網址',
    en: 'Open this address on the kitchen tablet',
    ja: 'キッチンのタブレットで開くURL',
  },
  kdsHint: {
    'zh-TW': '用平板的瀏覽器打開這個網址就是廚房畫面。開得起來就代表整條路都通了 ——',
    en: 'Open it in the tablet browser and you get the kitchen screen. If it loads, the whole path works. ',
    ja: 'タブレットのブラウザで開くとキッチン画面が出ます。開ければ経路はすべて通っています。',
  },
  kdsHintFirewall: {
    'zh-TW': '這一頁上的檢查證明不了防火牆，只有另一台裝置能證明。',
    en: 'The checks on this page cannot prove the firewall is open. Only another device can.',
    ja: 'このページの確認ではファイアウォールは検証できません。別の端末でしか確かめられません。',
  },
  noLanAddress: {
    'zh-TW': '找不到可用的區網位址。主機可能沒有連上網路，或只剩下虛擬網卡。',
    en: 'No usable LAN address. The computer may be offline, or only virtual adapters are left.',
    ja: '利用できるLANアドレスがありません。ネットワーク未接続か、仮想アダプターしか残っていない可能性があります。',
  },
  interfaces: {
    'zh-TW': '網卡（{n}）',
    en: 'Adapters ({n})',
    ja: 'ネットワークアダプター（{n}）',
  },
  inUse: { 'zh-TW': '目前使用', en: 'in use', ja: '使用中' },
  troubleshoot: {
    'zh-TW': '連不上的時候',
    en: 'If it will not connect',
    ja: 'つながらないとき',
  },
  tipSameWifi: {
    'zh-TW': '平板跟收銀機是不是連到同一個 Wi-Fi？（訪客網路通常是隔離的）',
    en: 'Are the tablet and the register on the same Wi-Fi? (Guest networks are usually isolated.)',
    ja: 'タブレットとレジは同じWi-Fiにつながっていますか？（ゲスト用ネットワークは通常分離されています）',
  },
  tipApIsolation: {
    'zh-TW': 'AP 有沒有開「用戶端隔離 / AP Isolation」？開著的話同網段也連不到彼此。',
    en: 'Is client isolation (AP Isolation) on? With it on, devices on the same subnet still cannot reach each other.',
    ja: 'APの「クライアント分離 / AP Isolation」が有効になっていませんか？有効だと同じネットワークでも互いに通信できません。',
  },
  tipFirewall: {
    'zh-TW': 'Windows 防火牆有沒有放行？第一次啟動時跳出來的對話框如果按了「取消」，就要用下面這一行補回來。',
    en: 'Is the Windows firewall allowing it? If the dialog on first launch was cancelled, run the line below to add the rule back.',
    ja: 'Windowsファイアウォールで許可されていますか？初回起動時のダイアログで「キャンセル」を押した場合は、下のコマンドで許可を追加してください。',
  },
  firewallNote: {
    'zh-TW': '在「命令提示字元（系統管理員）」貼上執行。{app} 不會自己動你的防火牆設定。',
    en: 'Paste it into Command Prompt (Administrator) and run it. {app} never touches your firewall settings on its own.',
    ja: '「コマンドプロンプト（管理者）」に貼り付けて実行してください。{app} がファイアウォール設定を勝手に変更することはありません。',
  },
  recheck: { 'zh-TW': '重新檢查', en: 'Check again', ja: '再確認' },

  // ─── 備份與還原 ───────────────────────────────
  saved: { 'zh-TW': '已儲存', en: 'Saved', ja: '保存しました' },
  pendingRestore: {
    'zh-TW': '有一個還原在等待重新啟動',
    en: 'A restore is waiting for a restart',
    ja: '復元が再起動待ちです',
  },
  restoreSource: { 'zh-TW': '來源：{source}', en: 'From: {source}', ja: '復元元：{source}' },
  restorePendingHint: {
    'zh-TW': '關掉 {app} 再重新開啟就會完成還原。現在的資料不會被刪除，會改名成 pre-restore 留在資料夾裡。',
    en: 'Close {app} and open it again to finish the restore. The current data is not deleted, it is renamed to pre-restore and kept in the folder.',
    ja: '{app} を一度終了して開き直すと復元が完了します。現在のデータは削除されず、pre-restore にリネームしてフォルダーに残ります。',
  },
  // 日文的「取消」是作廢／void（出單機那一頁的 reasonVoid 就是它），
  // 這裡是「不要做這個動作了」，所以用キャンセル。
  cancelRestore: { 'zh-TW': '取消還原', en: 'Cancel restore', ja: '復元をキャンセル' },
  restoreCancelled: {
    'zh-TW': '已取消還原',
    en: 'Restore cancelled',
    ja: '復元を取り消しました',
  },
  backupSettings: { 'zh-TW': '備份設定', en: 'Backup settings', ja: 'バックアップ設定' },
  // 這三句在畫面上是同一段話，中間那句用比較亮的顏色標出來 ——
  // 所以三種語言都要能照這個順序接起來讀得通。
  sameDiskWarn: {
    'zh-TW': '備份跟資料庫放在同一顆硬碟上，只防「誤刪」不防「硬碟壞掉」。',
    en: 'Backups sit on the same disk as the database, so they cover an accidental delete but not a dead disk. ',
    ja: 'バックアップはデータベースと同じディスク上にあるため、誤削除には有効ですがディスク故障には無力です。',
  },
  setSecondLocation: {
    'zh-TW': '請指定第二個位置',
    en: 'Set a second location',
    ja: '2つ目の保存先を指定してください',
  },
  usbEnough: {
    'zh-TW': '—— 一支常插著的 USB 隨身碟就夠了。',
    en: ', a USB stick left plugged in is enough.',
    ja: '。挿しっぱなしのUSBメモリで十分です。',
  },
  secondLocation: {
    'zh-TW': '第二個備份位置',
    en: 'Second backup location',
    ja: '2つ目の保存先',
  },
  dirPlaceholder: {
    'zh-TW': 'E:\\ 或 E:\\pos-backup',
    en: 'E:\\ or E:\\pos-backup',
    ja: 'E:\\ または E:\\pos-backup',
  },
  saveLocation: { 'zh-TW': '儲存位置', en: 'Save path', ja: 'パスを保存' },
  hourly: {
    'zh-TW': '每小時自動備份',
    en: 'Auto backup every hour',
    ja: '1時間ごとに自動バックアップ',
  },
  onClose: {
    'zh-TW': '關班與日結時各備一次',
    en: 'Back up at shift close and day-end',
    // 關班固定譯「シフト締め」（見 shift.ts），不是「レジ締め」——
    // 同一件事在班別頁與備份頁講成兩個詞，店家會以為備的是不同時機。
    ja: 'シフト締めと日次締めのたびにバックアップ',
  },
  backupNow: { 'zh-TW': '立刻備份一次', en: 'Back up now', ja: '今すぐバックアップ' },
  backupDone: {
    'zh-TW': '備份完成（{size}，{ms}ms）',
    en: 'Backup done ({size}, {ms}ms)',
    ja: 'バックアップ完了（{size}、{ms}ms）',
  },
  backupDoneExternal: {
    'zh-TW': '備份完成（{size}，{ms}ms），外接位置也有一份',
    en: 'Backup done ({size}, {ms}ms), the second location has a copy too',
    ja: 'バックアップ完了（{size}、{ms}ms）。2つ目の保存先にもコピーしました',
  },
  backupExternalFailed: {
    'zh-TW': '本機備份完成（{size}），但外接位置失敗：{error}',
    en: 'Local backup done ({size}), but the second location failed: {error}',
    ja: 'ローカルのバックアップは完了しました（{size}）が、2つ目の保存先で失敗しました：{error}',
  },
  backupFiles: { 'zh-TW': '備份檔', en: 'Backup files', ja: 'バックアップファイル' },
  restoreSafeNote: {
    'zh-TW': '還原不會刪掉現在的資料 —— 它會被改名成 pre-restore 留在資料夾裡。還原是人在慌張的時候做的事，所以不能是不可逆的。',
    en: 'Restoring does not delete the current data, it is renamed to pre-restore and kept in the folder. People restore while they are panicking, so it must not be irreversible.',
    ja: '復元しても現在のデータは削除されません。pre-restore にリネームしてフォルダーに残ります。復元は慌てているときに行う操作なので、取り返しがつかない作りにはしていません。',
  },
  noBackups: {
    'zh-TW': '還沒有任何備份。按上面的「立刻備份一次」試一下 —— 確認過自己會用，備份才有意義。',
    en: 'No backups yet. Try the "Back up now" button above. A backup only counts once you know you can use it.',
    ja: 'バックアップはまだありません。上の「今すぐバックアップ」を一度試してください。自分で使えると確認できて初めて意味があります。',
  },
  fileExternal: {
    'zh-TW': '在第二個位置（隨身碟）上',
    en: 'On the second location (USB)',
    ja: '2つ目の保存先（USBメモリ）にあります',
  },
  fileLocal: { 'zh-TW': '在本機', en: 'On this computer', ja: 'このパソコン内' },
  restore: { 'zh-TW': '還原', en: 'Restore', ja: '復元' },
  restoreConfirm: {
    'zh-TW':
      '要用這份備份還原嗎？\n\n{name}\n\n現在的資料會被改名保留（pre-restore），不會刪除。\n還原會在下次啟動 {app} 時完成。',
    en: 'Restore from this backup?\n\n{name}\n\nThe current data is renamed and kept (pre-restore), not deleted.\nThe restore finishes the next time {app} starts.',
    ja: 'このバックアップから復元しますか？\n\n{name}\n\n現在のデータはリネームして保持され（pre-restore）、削除されません。\n復元は次回 {app} を起動したときに完了します。',
  },

  /**
   * 區網探測結果與網卡不能用的原因。後端回代碼，句子在這裡組。
   *
   * `lanBound` 那一句要特別小心：從自己連自己**證明不了防火牆有放行**，
   * 只證明 server 綁對了網卡。這一頁不可以印一個假的綠燈 ——
   * 唯一能證明的是另一台裝置真的開得起來。
   */
  lanNoAddress: {
    'zh-TW': '找不到可用的區網位址。主機可能沒有連上網路，或只剩下虛擬網卡。',
    en: 'No usable LAN address. The host may be offline, or only virtual adapters are left.',
    ja: '利用可能なLANアドレスがありません。ホストがネットワークに繋がっていないか、仮想アダプターしか残っていない可能性があります。',
  },
  lanBound: {
    'zh-TW': 'server 有綁在這張網卡上。能不能從平板連進來，要用平板實際開一次才知道。',
    en: 'The server is bound to this adapter. Whether a tablet can reach it is only proven by opening it on the tablet.',
    ja: 'サーバーはこのアダプターにバインドされています。タブレットから繋がるかどうかは、実際にタブレットで開いてみないと分かりません。',
  },
  lanResolveFailed: {
    'zh-TW': '{addr} 解析不出位址。{error}',
    en: 'Could not resolve {addr}. {error}',
    ja: '{addr} を解決できません。{error}',
  },
  lanConnectFailed: {
    'zh-TW': '連不到 {addr}（{error}）。server 可能只綁在本機位址上。',
    en: 'Cannot reach {addr} ({error}). The server may be bound to localhost only.',
    ja: '{addr} に接続できません（{error}）。サーバーがローカルホストのみにバインドされている可能性があります。',
  },
  nicLoopback: {
    'zh-TW': '本機位址，平板連不到',
    en: 'Loopback address, tablets cannot reach it',
    ja: 'ループバックアドレスのため、タブレットからは繋がりません',
  },
  nicLinkLocal: {
    'zh-TW': 'DHCP 沒拿到位址（169.254.x.x），這個位址明天就會變',
    en: 'DHCP did not assign an address (169.254.x.x); this one will change tomorrow',
    ja: 'DHCPからアドレスを取得できていません（169.254.x.x）。このアドレスは明日には変わります',
  },
  nicNotPrivate: {
    'zh-TW': '不是區網位址',
    en: 'Not a private LAN address',
    ja: 'LANのアドレスではありません',
  },
  nicVirtual: {
    'zh-TW': '{vendor} 的虛擬網卡，不是店裡的網路',
    en: '{vendor} virtual adapter, not the shop network',
    ja: '{vendor} の仮想アダプターで、店舗のネットワークではありません',
  },
} satisfies Catalog
