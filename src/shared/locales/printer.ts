import type { Catalog } from '@/shared/i18n'

/**
 * 出單機設定頁。
 *
 * # 這一頁的字為什麼特別多
 *
 * 出單機是整套系統裡唯一「插了線還是可能不動」的硬體，而按下去的人多半不懂
 * 什麼是連接埠。所以這裡的每一句提示都在回答「那我現在該做什麼」——
 * 翻成別的語言時，要跟著翻的是那個**動作**，不是字面。
 *
 * # 按鈕要短
 *
 * 每一台印表機那一列有四顆按鈕（測試連線 / 測試列印 / 編輯 / 移除），
 * 日文與英文都比中文長。「測試連線」翻成「接続テスト」會把整列擠成兩行，
 * 所以用「接続確認」；英文用 Test 而不是 Test connection。
 *
 * # 日文用詞
 *
 * 用日本餐飲業的慣用語而不是字面直譯：結帳是「会計」不是「決済」、
 * 加點是「追加注文」、補印是「再印刷」、單據是「伝票」。
 * 「出單分區」沒有完全對應的日文詞，用「出力エリア」——
 * 它同時說得出「印到哪」與「這是一個區」兩件事。
 *
 * 取消 / 儲存 / 新增 / 刪除這幾顆共用按鈕不放這裡，直接用 `ui`，
 * 免得同一顆按鈕在不同頁面出現兩種講法。
 */
export const printer = {
  // ─── 印表機 ─────────────────────────────────────
  sectionPrinters: { 'zh-TW': '出單機', en: 'Printers', ja: 'プリンター' },
  emptyPrinters: {
    'zh-TW': '還沒有設定任何出單機。',
    en: 'No printers set up yet.',
    ja: 'プリンターがまだ登録されていません。',
  },
  emptyPrintersHint: {
    'zh-TW':
      '最常見的接法是網路型：把印表機接上店裡的網路，在它印出來的自我測試頁上找到 IP，連接埠填 9100。',
    en: 'The usual setup is network: put the printer on the shop network, read its IP off the self-test page it prints, and use port 9100.',
    ja: '一般的なのはネットワーク接続です。プリンターを店内のネットワークにつなぎ、セルフテスト印字に出る IP を入力し、ポートは 9100 にします。',
  },
  probeOk: { 'zh-TW': '● 連得上', en: '● Online', ja: '● 接続OK' },
  probeFail: { 'zh-TW': '▲ 連不上', en: '▲ Offline', ja: '▲ 接続不可' },
  testConnection: { 'zh-TW': '測試連線', en: 'Test', ja: '接続確認' },
  testPrint: { 'zh-TW': '測試列印', en: 'Test print', ja: 'テスト印刷' },
  testPrintHint: {
    'zh-TW': '印一張看得懂就代表設定正確的紙',
    en: 'Prints one slip; if you can read it, the setup is right',
    ja: 'テスト用紙を1枚印刷します。文字が読めれば設定は正しいです',
  },
  testPrintSent: {
    'zh-TW': '已送出測試單到「{name}」',
    en: 'Test print sent to "{name}"',
    ja: '「{name}」にテスト印刷を送信しました',
  },
  edit: { 'zh-TW': '編輯', en: 'Edit', ja: '編集' },
  remove: { 'zh-TW': '移除', en: 'Remove', ja: '削除' },
  removed: {
    'zh-TW': '已移除「{name}」',
    en: 'Removed "{name}"',
    ja: '「{name}」を削除しました',
  },
  saved: { 'zh-TW': '已儲存', en: 'Saved', ja: '保存しました' },

  // 傳輸方式。網路型顯示 IP:port、USB 顯示 vid:pid，都不必翻；只有藍牙有中文。
  bluetooth: { 'zh-TW': '藍牙 {addr}', en: 'Bluetooth {addr}', ja: 'Bluetooth {addr}' },

  // ─── 出單分區 ───────────────────────────────────
  sectionStations: { 'zh-TW': '出單分區', en: 'Print stations', ja: '出力エリア' },
  stationNamePrompt: {
    'zh-TW': '分區名稱（例如：飲料吧、熱炒區）',
    en: 'Station name (e.g. Bar, Wok station)',
    ja: 'エリア名（例：ドリンクバー、厨房）',
  },
  stationAdded: {
    'zh-TW': '已新增「{name}」',
    en: 'Added "{name}"',
    ja: '「{name}」を追加しました',
  },
  stationsHint: {
    'zh-TW':
      '分區是「飲料吧」這種穩定的概念，底下綁哪一台機器是設定。品項直接綁印表機的話，換一台機器就要去改幾百筆菜單。',
    en: 'A station is a stable idea like "the bar"; which machine sits under it is just configuration. Bind items straight to a printer and swapping the machine means editing hundreds of menu rows.',
    ja: 'エリアは「ドリンクバー」のような変わらない概念で、どの機械を割り当てるかは設定にすぎません。商品を直接プリンターに紐付けると、機械を1台替えるだけで何百件もの商品を直すことになります。',
  },
  emptyStations: {
    'zh-TW': '還沒有分區。只有一台機器的店家不需要設，所有單都會印到那一台。',
    en: 'No stations yet. A shop with a single printer needs none; everything prints there.',
    ja: 'エリアはまだありません。プリンターが1台だけの店では不要で、すべてそこに印刷されます。',
  },
  stationUnbound: {
    'zh-TW': '⚠ 還沒綁機器 —— 這一區的單會印到櫃檯並標成代印',
    en: '⚠ No printer bound; these tickets print at the counter, marked as fallback',
    ja: '⚠ プリンター未割当。伝票はレジで代理印刷として出力されます',
  },
  // 英文不寫 "{n} printers"，只有一台時會變成 "1 printers"。
  printerCount: { 'zh-TW': '{n} 台', en: '{n} bound', ja: '{n} 台' },
  stationDeleted: {
    'zh-TW': '已刪除「{name}」',
    en: 'Deleted "{name}"',
    ja: '「{name}」を削除しました',
  },
  // 接在機器名稱後面的小字（「櫃檯 · 每次」），所以要比按鈕更短。
  bindAlways: { 'zh-TW': '每次', en: 'Always', ja: '毎回' },
  bindPrimary: { 'zh-TW': '主要', en: 'Primary', ja: 'メイン' },
  bindAlwaysHint: {
    'zh-TW': '每一次都印（例如櫃檯留底）',
    en: 'Prints every time (e.g. the counter copy)',
    ja: '毎回印刷します（レジの控えなど）',
  },
  bindPrimaryHint: {
    'zh-TW': '這一區的主要機器',
    en: 'Main printer for this station',
    ja: 'このエリアのメイン機',
  },
  bindHint: { 'zh-TW': '點一下綁上去', en: 'Click to bind', ja: 'クリックで割り当て' },

  // ─── 列印紀錄 ───────────────────────────────────
  sectionJobs: { 'zh-TW': '最近的單', en: 'Recent jobs', ja: '最近の印刷' },
  jobsHint: {
    'zh-TW':
      '印失敗的單會留在這裡。POS 最常見的客訴是「廚房沒收到單」，所以這一頁存在的意義就是讓失敗看得見。',
    en: 'Failed tickets stay here. The most common POS complaint is "the kitchen never got the order", so this page exists to make failures visible.',
    ja: '印刷に失敗した伝票はここに残ります。POS で最も多い苦情は「厨房に伝票が届いていない」なので、この画面は失敗を見えるようにするためにあります。',
  },
  emptyJobs: {
    'zh-TW': '還沒有列印紀錄。',
    en: 'No print jobs yet.',
    ja: '印刷履歴はまだありません。',
  },
  requeued: { 'zh-TW': '已放回佇列', en: 'Back in the queue', ja: 'キューに戻しました' },
  reprint: { 'zh-TW': '補印', en: 'Reprint', ja: '再印刷' },

  // 前面的符號不隨語言變，所以留在字串裡，免得每個呼叫端各自拼一次。
  jobPending: { 'zh-TW': '● 排隊中', en: '● Queued', ja: '● 待機中' },
  jobPrinting: { 'zh-TW': '● 列印中', en: '● Printing', ja: '● 印刷中' },
  jobDone: { 'zh-TW': '● 已印出', en: '● Printed', ja: '● 印刷済' },
  jobFailed: { 'zh-TW': '▲ 失敗', en: '▲ Failed', ja: '▲ 失敗' },
  jobDead: { 'zh-TW': '■ 印不出來', en: '■ Gave up', ja: '■ 印刷不可' },
  // 「取消」在日文是作廢／void 的意思，被下面的 reasonVoid 佔走了；
  // 這一列的「已取消」是指這張單沒印就被中止，所以用「中止」。
  jobCancelled: { 'zh-TW': '— 已取消', en: '— Cancelled', ja: '— 中止' },

  reasonNewOrder: { 'zh-TW': '新單', en: 'New order', ja: '新規注文' },
  reasonAddItems: { 'zh-TW': '加點', en: 'Add-on', ja: '追加注文' },
  reasonVoid: { 'zh-TW': '取消', en: 'Void', ja: '取消' },
  reasonReprint: { 'zh-TW': '補印', en: 'Reprint', ja: '再印刷' },
  reasonSettle: { 'zh-TW': '結帳', en: 'Checkout', ja: '会計' },

  // ─── 新增／編輯表單 ─────────────────────────────
  fieldName: { 'zh-TW': '名稱', en: 'Name', ja: '名称' },
  namePlaceholder: { 'zh-TW': '櫃檯、飲料吧…', en: 'Counter, Bar…', ja: 'レジ、ドリンクバー…' },
  fieldHost: { 'zh-TW': 'IP 位址', en: 'IP address', ja: 'IPアドレス' },
  fieldPort: { 'zh-TW': '連接埠', en: 'Port', ja: 'ポート' },
  fieldPaper: { 'zh-TW': '紙寬', en: 'Paper', ja: '用紙幅' },
  paper80: { 'zh-TW': '80mm（48 欄）', en: '80mm (48 cols)', ja: '80mm（48桁）' },
  paper58: { 'zh-TW': '58mm（32 欄）', en: '58mm (32 cols)', ja: '58mm（32桁）' },
  fieldEncoding: { 'zh-TW': '中文編碼', en: 'Encoding', ja: '文字コード' },
  encodingBig5: {
    'zh-TW': 'Big5（台灣機常見）',
    en: 'Big5 (most Taiwan units)',
    ja: 'Big5（台湾製に多い）',
  },
  encodingGb18030: {
    'zh-TW': 'GB18030（陸製機常見）',
    en: 'GB18030 (most China-made units)',
    ja: 'GB18030（中国製に多い）',
  },
  encodingUtf8: {
    'zh-TW': 'UTF-8（少數新款）',
    en: 'UTF-8 (a few newer units)',
    ja: 'UTF-8（一部の新型）',
  },
  formHint: {
    'zh-TW':
      '大部分出單機的連接埠都是 9100。IP 可以在機器的自我測試頁上找到（多數機型是按住走紙鍵再開機會印出來）。',
    en: 'Port 9100 works for most printers. The IP is on the self-test page (on most models, hold the feed button while switching the printer on).',
    ja: 'ほとんどのプリンターはポート 9100 です。IP はセルフテスト印字で確認できます（多くの機種は紙送りボタンを押しながら電源を入れると印字されます）。',
  },
  formHintEncoding: {
    'zh-TW': '印出來變成問號或亂碼，通常是編碼選錯了 —— 換一個再按「測試列印」。',
    en: 'Question marks or garbled characters usually mean the wrong encoding. Pick another one and hit Test print.',
    ja: '「?」や文字化けになる場合は、たいてい文字コードの選択ミスです。別のものに変えて「テスト印刷」を押してください。',
  },
} satisfies Catalog
