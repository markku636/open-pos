import { useCallback, useEffect, useState } from 'react'

import { tableApi, type AppError, type DiningTable, type TableInput } from '@/shared/api'
import { formatMoney } from '@/shared/money'
import { hhmm } from '@/shared/time'

/**
 * 桌位圖。
 *
 * # 這一頁要回答的只有三個問題
 *
 * 1. 哪幾桌是空的？（帶客人入座）
 * 2. 這一桌坐多久了、吃了多少錢？（客人問「多少錢」時，店員最常被問的）
 * 3. 哪一桌該去收了？（坐很久、已經沒在點東西）
 *
 * 所以每張卡片上只有這三件事，不放桌位屬性那些一年改一次的東西 ——
 * 那些收在「編輯桌位」後面。
 *
 * # 為什麼不做拖拉排版的平面圖
 *
 * 因為它解決的是老闆看的問題，不是店員用的問題。店員在忙的時候要的是
 * 「A3 在哪、多少錢」，一個排序穩定的網格比一張要用眼睛找的平面圖快。
 * 真正的平面圖等有人來提 issue 再說。
 */
export default function TableMap({
  onPick,
}: {
  onPick: (table: DiningTable, guestCount: number) => void
}) {
  const [tables, setTables] = useState<DiningTable[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [editing, setEditing] = useState(false)
  const [form, setForm] = useState<TableInput | null>(null)
  const [seating, setSeating] = useState<DiningTable | null>(null)
  const [busy, setBusy] = useState(false)

  // 讀取自己不設錯誤訊息，而是把錯誤**回傳**給呼叫端。
  //
  // ★ 這一層間接是必要的：背景輪詢每 10 秒跑一次，如果它成功時順手清掉
  // 錯誤訊息，那「這一桌還有 1 張單沒有結帳」這句話最多只會存在十秒 ——
  // 而店員按下「清桌」之後很可能正低頭看客人。被擋下來的操作必須留下話。
  const load = useCallback(async (): Promise<string | null> => {
    try {
      setTables(await tableApi.list())
      return null
    } catch (e) {
      return (e as AppError).message ?? String(e)
    }
  }, [])

  useEffect(() => {
    void load().then((e) => e && setError(e))
    // 桌位金額會被別台終端改動（外場平板加點），所以自己重查。
    // 10 秒是「店員不會覺得數字是舊的」與「不要一直打 DB」的折衷。
    // 輪詢失敗照樣要說，但成功時不動既有的訊息。
    const id = setInterval(() => void load().then((e) => e && setError(e)), 10_000)
    return () => clearInterval(id)
  }, [load])

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await fn()
      setError(await load())
      return true
    } catch (e) {
      setError((e as AppError).message ?? String(e))
      // 失敗之後也要重讀（別人可能剛剛改過），但它的錯誤不能蓋掉上面那句。
      void load()
      return false
    } finally {
      setBusy(false)
    }
  }

  const areas = groupByArea(tables ?? [])
  const seated = (tables ?? []).filter((t) => t.session).length

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="mb-3 flex items-center gap-3">
        <h2 className="text-lg font-semibold">桌位</h2>
        {tables && (
          <span className="text-sm text-slate-500">
            {seated} / {tables.length} 桌使用中
          </span>
        )}
        <span className="ml-auto" />
        <button
          className={`rounded px-3 py-1.5 text-sm ${
            editing ? 'bg-slate-700 text-slate-100' : 'text-slate-400 hover:text-slate-200'
          }`}
          onClick={() => {
            setEditing((v) => !v)
            setForm(null)
          }}
        >
          編輯桌位
        </button>
        {editing && (
          <button
            className="rounded bg-sky-800 px-3 py-1.5 text-sm hover:bg-sky-700"
            onClick={() => setForm({ code: '', seats: 4, areaName: null })}
          >
            ＋ 新增
          </button>
        )}
      </div>

      {error && (
        <div className="mb-3 flex items-start gap-3 rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          <span className="flex-1">{error}</span>
          <button
            className="shrink-0 px-1 text-red-400 hover:text-red-200"
            title="知道了"
            onClick={() => setError(null)}
          >
            ✕
          </button>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {tables === null ? (
          <p className="py-12 text-center text-sm text-slate-600">載入中…</p>
        ) : tables.length === 0 ? (
          <p className="py-12 text-center text-sm text-slate-600">
            還沒有桌位。按「編輯桌位 → 新增」建立第一桌，
            <br />
            或是這家店只做外帶的話，這一頁可以完全不用。
          </p>
        ) : (
          areas.map(([area, list]) => (
            <section key={area} className="mb-5">
              {areas.length > 1 && (
                <h3 className="mb-2 text-sm text-slate-500">{area}</h3>
              )}
              <div className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2">
                {list.map((t) => (
                  <TableCard
                    key={t.id}
                    table={t}
                    editing={editing}
                    busy={busy}
                    // 空桌要先問人數（服務費、翻桌率、報表全靠它），
                    // 已經有客人的桌直接進點餐畫面 —— 加點的時候再問一次人數
                    // 只會被隨便按掉。
                    onOpen={() => (t.session ? onPick(t, t.session.guestCount) : setSeating(t))}
                    onEdit={() =>
                      setForm({
                        id: t.id,
                        code: t.code,
                        name: t.name ?? null,
                        seats: t.seats,
                        areaName: t.areaName ?? null,
                        isActive: t.isActive,
                      })
                    }
                    onClear={() => {
                      if (!confirm(`要清掉 ${t.code} 嗎？\n\n這一桌的帳必須已經結完。`)) return
                      void run(() => tableApi.close(t.id))
                    }}
                  />
                ))}
              </div>
            </section>
          ))
        )}
      </div>

      {seating && (
        <SeatDialog
          table={seating}
          onCancel={() => setSeating(null)}
          onSeat={(guests) => {
            const t = seating
            setSeating(null)
            onPick(t, guests)
          }}
        />
      )}

      {form && (
        <TableForm
          value={form}
          busy={busy}
          onCancel={() => setForm(null)}
          onDelete={
            form.id
              ? () => {
                  if (!confirm(`要刪掉 ${form.code} 嗎？`)) return
                  void run(() => tableApi.remove(form.id!)).then((ok) => {
                    if (ok) setForm(null)
                  })
                }
              : undefined
          }
          onSave={(input) => {
            void run(() => tableApi.upsert(input)).then((ok) => {
              if (ok) setForm(null)
            })
          }}
        />
      )}
    </div>
  )
}

/** 沒有區域的桌歸到「未分區」，而不是消失。 */
function groupByArea(tables: DiningTable[]): [string, DiningTable[]][] {
  const map = new Map<string, DiningTable[]>()
  for (const t of tables) {
    const key = t.areaName ?? '未分區'
    const list = map.get(key)
    if (list) list.push(t)
    else map.set(key, [t])
  }
  return [...map.entries()]
}

function TableCard({
  table,
  editing,
  busy,
  onOpen,
  onEdit,
  onClear,
}: {
  table: DiningTable
  editing: boolean
  busy: boolean
  onOpen: () => void
  onEdit: () => void
  onClear: () => void
}) {
  const s = table.session
  const mins = s ? Math.floor(s.seatedSeconds / 60) : 0
  // 顏色只表達一件事：這桌坐多久了。九十分鐘以上通常代表該去看一下 ——
  // 不是催客人，是「這桌可能已經吃完但還沒結帳」。
  const tone = !s
    ? 'border-slate-800 bg-slate-900/50 hover:border-slate-600'
    : mins >= 120
      ? 'border-red-700 bg-red-950/30 hover:border-red-500'
      : mins >= 90
        ? 'border-amber-700 bg-amber-950/20 hover:border-amber-500'
        : 'border-emerald-800 bg-emerald-950/20 hover:border-emerald-600'

  return (
    <div className={`rounded-lg border-2 ${tone} transition`}>
      <button
        className="w-full p-3 text-left disabled:opacity-40"
        disabled={busy || (editing && !s)}
        onClick={editing ? onEdit : onOpen}
      >
        <div className="flex items-baseline gap-2">
          <span className="text-xl font-semibold">{table.code}</span>
          {table.name && <span className="text-sm text-slate-400">{table.name}</span>}
          <span className="ml-auto text-xs text-slate-600">{table.seats} 人桌</span>
        </div>

        {s ? (
          <div className="mt-2">
            <div className="flex items-baseline justify-between">
              <span className="text-lg font-semibold text-sky-300">
                {formatMoney(s.total)}
              </span>
              <span className="font-mono text-sm text-slate-400">{mins} 分</span>
            </div>
            <div className="mt-0.5 text-xs text-slate-500">
              {s.guestCount} 位 · {hhmm(s.openedAt)} 入座
              {s.orderCount > 1 && ` · ${s.orderCount} 張單`}
            </div>
          </div>
        ) : (
          <p className="mt-2 text-sm text-slate-600">
            {table.isActive ? '空桌' : '停用中'}
          </p>
        )}
      </button>

      {s && (
        <div className="flex gap-1 border-t border-slate-800/80 px-2 py-1.5">
          <button
            className="flex-1 rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 hover:text-slate-200 disabled:opacity-40"
            disabled={busy}
            onClick={onOpen}
          >
            加點 / 結帳
          </button>
          <button
            className="rounded px-2 py-1 text-xs text-slate-500 hover:bg-slate-800 hover:text-slate-300 disabled:opacity-40"
            disabled={busy}
            title="把這一桌清空。必須先結完帳"
            onClick={onClear}
          >
            清桌
          </button>
        </div>
      )}

      {editing && !s && (
        <div className="border-t border-slate-800/80 px-2 py-1.5">
          <button
            className="w-full rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 hover:text-slate-200 disabled:opacity-40"
            disabled={busy}
            onClick={onEdit}
          >
            編輯
          </button>
        </div>
      )}
    </div>
  )
}

function TableForm({
  value,
  busy,
  onSave,
  onDelete,
  onCancel,
}: {
  value: TableInput
  busy: boolean
  onSave: (input: TableInput) => void
  onDelete?: () => void
  onCancel: () => void
}) {
  const [draft, setDraft] = useState<TableInput>(value)

  return (
    <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/60 p-4">
      <div className="w-full max-w-sm rounded-lg border border-slate-700 bg-slate-900 p-5">
        <h3 className="mb-4 text-lg font-semibold">{value.id ? '編輯桌位' : '新增桌位'}</h3>

        <label className="mb-3 block">
          <span className="mb-1 block text-sm text-slate-400">桌號</span>
          <input
            autoFocus
            className="w-full rounded bg-slate-800 px-3 py-2"
            placeholder="A1"
            value={draft.code}
            onChange={(e) => setDraft({ ...draft, code: e.target.value })}
          />
          {/* 桌號是店員用喊的，所以短。這句提示省下很多「桌號要打什麼」的猶豫。 */}
          <span className="mt-1 block text-xs text-slate-600">店員報號用的，越短越好</span>
        </label>

        <label className="mb-3 block">
          <span className="mb-1 block text-sm text-slate-400">名稱（選填）</span>
          <input
            className="w-full rounded bg-slate-800 px-3 py-2"
            placeholder="靠窗"
            value={draft.name ?? ''}
            onChange={(e) => setDraft({ ...draft, name: e.target.value || null })}
          />
        </label>

        <div className="mb-3 flex gap-3">
          <label className="flex-1">
            <span className="mb-1 block text-sm text-slate-400">座位數</span>
            <input
              type="number"
              min={1}
              className="w-full rounded bg-slate-800 px-3 py-2"
              value={draft.seats ?? 4}
              onChange={(e) => setDraft({ ...draft, seats: Number(e.target.value) || 1 })}
            />
          </label>
          <label className="flex-1">
            <span className="mb-1 block text-sm text-slate-400">區域（選填）</span>
            <input
              className="w-full rounded bg-slate-800 px-3 py-2"
              placeholder="大廳"
              value={draft.areaName ?? ''}
              onChange={(e) => setDraft({ ...draft, areaName: e.target.value || null })}
            />
          </label>
        </div>

        <label className="mb-4 flex items-center gap-2 text-sm text-slate-400">
          <input
            type="checkbox"
            checked={draft.isActive ?? true}
            onChange={(e) => setDraft({ ...draft, isActive: e.target.checked })}
          />
          啟用（停用的桌還會留在圖上，但不能開檯）
        </label>

        <div className="flex gap-2">
          {onDelete && (
            <button
              className="rounded px-3 py-2 text-sm text-slate-500 hover:text-red-300 disabled:opacity-40"
              disabled={busy}
              onClick={onDelete}
            >
              刪除
            </button>
          )}
          <span className="ml-auto" />
          <button
            className="rounded px-4 py-2 text-sm text-slate-400 hover:text-slate-200"
            onClick={onCancel}
          >
            取消
          </button>
          <button
            className="rounded bg-sky-700 px-4 py-2 text-sm font-semibold hover:bg-sky-600 disabled:opacity-40"
            disabled={busy || !draft.code.trim()}
            onClick={() => onSave(draft)}
          >
            儲存
          </button>
        </div>
      </div>
    </div>
  )
}

/**
 * 「幾位？」
 *
 * 開檯必問，因為服務費、翻桌率、每人平均消費全部從這個數字長出來，
 * 而事後補問是補不回來的。
 *
 * 做成一排大按鈕而不是輸入框：帶位的時候店員一手拿菜單、眼睛在看客人，
 * 一戳就好。座位數是預設的那一顆 —— 多數情況下它就是對的。
 */
function SeatDialog({
  table,
  onSeat,
  onCancel,
}: {
  table: DiningTable
  onSeat: (guests: number) => void
  onCancel: () => void
}) {
  const [custom, setCustom] = useState('')
  const quick = [1, 2, 3, 4, 5, 6, 8, 10]

  return (
    <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/60 p-4">
      <div className="w-full max-w-sm rounded-lg border border-slate-700 bg-slate-900 p-5">
        <h3 className="mb-1 text-lg font-semibold">{table.code} 開檯</h3>
        <p className="mb-4 text-sm text-slate-500">幾位？</p>

        <div className="grid grid-cols-4 gap-2">
          {quick.map((n) => (
            <button
              key={n}
              className={`rounded py-4 text-xl font-semibold ${
                n === table.seats
                  ? 'bg-sky-700 hover:bg-sky-600'
                  : 'bg-slate-800 hover:bg-slate-700'
              }`}
              onClick={() => onSeat(n)}
            >
              {n}
            </button>
          ))}
        </div>

        <div className="mt-3 flex gap-2">
          <input
            type="number"
            min={1}
            className="w-full rounded bg-slate-800 px-3 py-2"
            placeholder="其他人數"
            value={custom}
            onChange={(e) => setCustom(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && Number(custom) > 0) onSeat(Number(custom))
            }}
          />
          <button
            className="shrink-0 rounded bg-slate-800 px-4 text-sm hover:bg-slate-700 disabled:opacity-40"
            disabled={!(Number(custom) > 0)}
            onClick={() => onSeat(Number(custom))}
          >
            確定
          </button>
        </div>

        <button
          className="mt-4 w-full rounded py-2 text-sm text-slate-400 hover:text-slate-200"
          onClick={onCancel}
        >
          取消
        </button>
      </div>
    </div>
  )
}
