/**
 * 把譯文裡的 `**粗體**` 畫出來。
 *
 * # 為什麼需要這一個東西
 *
 * 有些句子的重點只在其中一小段：
 *
 * > **已經結帳的單不會變** —— 存的是當時算好的金額……
 *
 * 老闆不會把整段讀完。而如果為了強調而把重點拆成獨立的 `Msg`，
 * 譯者就會拿到兩個沒有上下文的碎片 —— 中文可以拆的地方，
 * 日文與英文的語序常常拆不到同一刀上。所以標記留在句子裡，
 * 由這裡在畫面上還原。
 *
 * # 它刻意不是 markdown
 *
 * 只認 `**`，不認連結、清單、`_斜體_`。收銀機的文案不需要那些，
 * 而每多支援一種語法，就多一種「譯者不小心打了一個星號，
 * 整句話從畫面上消失」的可能。找不到成對的 `**` 就原樣輸出。
 */
export interface Segment {
  text: string
  bold: boolean
}

/**
 * 切成一段一段。分出來當純函式是為了能測 —— 這段邏輯的重點全在
 * 「標記打壞了會怎樣」，而那用渲染測起來遠比用陣列測麻煩。
 */
export function segments(text: string): Segment[] {
  const parts = text.split('**')
  return parts.map((p, i) => ({
    text: p,
    // 奇數段落是被一對 `**` 夾住的部分。若星號數量是奇數（少打一個），
    // 最後一段會落在奇數位卻沒有結尾標記，於是原樣輸出 ——
    // 壞掉的標記不會吃掉文字。
    bold: i % 2 === 1 && i < parts.length - 1,
  }))
}

export function Emphasis({ text }: { text: string }) {
  // 用 split 而不是 replace + dangerouslySetInnerHTML：譯文是資料，
  // 而把資料當 HTML 塞進 DOM 是一個永遠不必要冒的險。
  return (
    <>
      {segments(text).map((s, i) =>
        s.bold ? (
          <strong key={i} className="font-semibold text-slate-200">
            {s.text}
          </strong>
        ) : (
          <span key={i}>{s.text}</span>
        ),
      )}
    </>
  )
}
