import type { Catalog } from '@/shared/i18n'

/**
 * 廚房顯示（KDS）。
 *
 * # 為什麼這一頁的字要特別短
 *
 * 看的人手是濕的、離螢幕一公尺遠，所以字級是收銀機那邊的兩倍大。
 * 同一句話日文與英文都比中文長，而這裡每一個字都在跟「單」搶寬度 ——
 * 站別分頁一換行，第一排的單就被擠到摺線以下，而那幾張正是等最久的。
 *
 * # 日文用詞
 *
 * 用日本廚房現場的講法，不是字面直譯：單是「伝票」、完成是「完了」、
 * 收銀機是「レジ」。張數用「件」而不是「枚」—— 螢幕上的是資料不是紙，
 * 「枚」是清點紙本才會用的量詞。
 *
 * # 佔位符三種語言要一致
 *
 * `{n}` 在三行裡都要出現。少寫一個不會編譯失敗（型別上它就是個字串），
 * 但畫面上會變成「已經 秒沒有訊息」這種讀不懂、又不像壞掉的句子。
 */
export const kds = {
  title: { 'zh-TW': '廚房', en: 'Kitchen', ja: 'キッチン' },
  allStations: { 'zh-TW': '全部', en: 'All', ja: 'すべて' },
  ticketCount: { 'zh-TW': '{n} 張單', en: '{n} tickets', ja: '{n} 件' },
  empty: {
    'zh-TW': '沒有待做的單',
    en: 'No open tickets',
    ja: '調理待ちの伝票はありません',
  },
  connecting: { 'zh-TW': '連線中…', en: 'Connecting…', ja: '接続中…' },
  connectingStale: {
    'zh-TW': '連線中… 現在畫的是上一次的單',
    en: 'Connecting… showing the last tickets',
    ja: '接続中… 表示は前回の伝票です',
  },
  // 這一句是整個 KDS 最重要的字：斷線時要讓人很難不注意到，
  // 所以把「多久沒訊息」也寫進去 —— 只講「斷線了」，廚師無從判斷嚴重程度。
  disconnected: {
    'zh-TW': '跟收銀機斷線了 —— 現在看到的單可能是舊的（已經 {n} 秒沒有訊息）',
    en: 'Disconnected from the register — tickets may be out of date ({n}s with no messages)',
    ja: 'レジと切断されました。表示中の伝票は古い可能性があります（{n} 秒間 通信なし）',
  },
  // 這一則印在橫幅右邊的小方塊裡，跟斷線那一句擠同一列。英文照中文的
  // 長度寫會把方塊撐出橫幅，所以只留「幾筆沒送出去、會補送」這兩件事 ——
  // 廚師要知道的就是這樣。
  pendingActions: {
    'zh-TW': '{n} 個「完成」還沒送出去，連上就會補送',
    en: '{n} "done" queued, will resend',
    ja: '未送信の「完了」が {n} 件。接続後に再送します',
  },
  reconnect: { 'zh-TW': '重新連線', en: 'Reconnect', ja: '再接続' },
  // 等待時間印在單的右上角，跟訂單號一樣是等寬字 —— 只留一個字的位置。
  waitMinutes: { 'zh-TW': '{n}分', en: '{n}m', ja: '{n}分' },

  // ★ 標點也要翻。
  //
  //   加購用「、」串起來、規格用全形括號包起來，這在中文與日文都對，
  //   但英文螢幕上會變成 `Iced tea、Less ice` 與 `（Large）` —— 半形字之間
  //   夾一個全形符號，看起來像是編碼壞掉，而不像是一份看得懂的單。
  //
  //   英文那一版括號前面留一個空格：`Latte (Large)`。少了它會黏成一團。
  listSeparator: { 'zh-TW': '、', en: ', ', ja: '、' },
  variantParen: { 'zh-TW': '（{name}）', en: ' ({name})', ja: '（{name}）' },
} satisfies Catalog
