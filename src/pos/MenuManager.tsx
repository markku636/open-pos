import { useCallback, useEffect, useState } from 'react'

import {
  menuApi,
  type AppError,
  type Category,
  type CategoryNode,
  type Item,
  type MenuTree,
  type Variant,
} from '@/shared/api'
import { formatMoney, parseMoney } from '@/shared/money'

/**
 * 商品維護。
 *
 * 版面刻意是「左分類、右品項」的兩欄：店家改菜單時的心智模型就是
 * 「先找到那一類，再改裡面那一項」。做成一張大表格反而要一直捲。
 *
 * 所有欄位都是直接編輯後按儲存，沒有彈窗 —— 收銀機是觸控螢幕，
 * 疊層的彈窗在手指操作下很容易誤觸到後面那一層。
 */
export default function MenuManager() {
  const [tree, setTree] = useState<MenuTree | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const reload = useCallback(async () => {
    try {
      const t = await menuApi.tree()
      setTree(t)
      setError(null)
      setSelected((cur) => {
        if (cur === 'uncategorized') return cur
        if (cur && t.categories.some((c) => c.id === cur)) return cur
        return t.categories[0]?.id ?? (t.uncategorized.length ? 'uncategorized' : null)
      })
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  /** 統一的動作包裝：擋重複點擊、把後端錯誤顯示出來、成功後重讀。 */
  const run = useCallback(
    async (fn: () => Promise<unknown>) => {
      setBusy(true)
      try {
        await fn()
        await reload()
        setError(null)
      } catch (e) {
        setError((e as AppError).message ?? String(e))
      } finally {
        setBusy(false)
      }
    },
    [reload],
  )

  if (!tree) {
    return (
      <div className="p-6 text-slate-400">
        {error ? <ErrorBar message={error} /> : '載入中…'}
      </div>
    )
  }

  const current: CategoryNode | null =
    selected === 'uncategorized'
      ? null
      : (tree.categories.find((c) => c.id === selected) ?? null)
  const items = selected === 'uncategorized' ? tree.uncategorized : (current?.items ?? [])

  return (
    <div className="flex h-full min-h-0 flex-col">
      {error && <ErrorBar message={error} onDismiss={() => setError(null)} />}

      <div className="flex min-h-0 flex-1 gap-4">
        <CategoryPane
          tree={tree}
          selected={selected}
          busy={busy}
          onSelect={setSelected}
          onSave={(input) => run(() => menuApi.upsertCategory(input))}
          onDelete={(id) => run(() => menuApi.deleteCategory(id))}
        />
        <ItemPane
          categoryId={selected === 'uncategorized' ? null : (current?.id ?? null)}
          title={selected === 'uncategorized' ? '未分類' : (current?.name ?? '')}
          items={items}
          busy={busy}
          onSave={(input) => run(() => menuApi.upsertItem(input))}
          onDelete={(id) => run(() => menuApi.deleteItem(id))}
          onSaveVariant={(input) => run(() => menuApi.upsertVariant(input))}
          onDeleteVariant={(id) => run(() => menuApi.deleteVariant(id))}
        />
      </div>
    </div>
  )
}

function ErrorBar({ message, onDismiss }: { message: string; onDismiss?: () => void }) {
  return (
    <div className="mb-3 flex items-start gap-3 rounded border border-red-800 bg-red-950/60 px-4 py-3 text-sm text-red-200">
      <span className="flex-1 whitespace-pre-wrap">{message}</span>
      {onDismiss && (
        <button className="shrink-0 text-red-400 hover:text-red-200" onClick={onDismiss}>
          關閉
        </button>
      )}
    </div>
  )
}

// ---------------------------------------------------------------- 分類

function CategoryPane({
  tree,
  selected,
  busy,
  onSelect,
  onSave,
  onDelete,
}: {
  tree: MenuTree
  selected: string | null
  busy: boolean
  onSelect: (id: string) => void
  onSave: (input: { id?: string; name: string }) => void
  onDelete: (id: string) => void
}) {
  const [adding, setAdding] = useState('')
  const [editing, setEditing] = useState<Category | null>(null)

  return (
    <aside className="flex w-64 shrink-0 flex-col gap-2 overflow-y-auto">
      <h2 className="text-xs font-semibold uppercase tracking-wide text-slate-500">分類</h2>

      {tree.categories.map((c) =>
        editing?.id === c.id ? (
          <div key={c.id} className="flex gap-1">
            <input
              className="min-w-0 flex-1 rounded bg-slate-800 px-2 py-2 text-sm"
              value={editing.name}
              autoFocus
              onChange={(e) => setEditing({ ...editing, name: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  onSave({ id: editing.id, name: editing.name })
                  setEditing(null)
                }
                if (e.key === 'Escape') setEditing(null)
              }}
            />
            <button
              className="rounded bg-slate-700 px-2 text-xs"
              onClick={() => {
                onSave({ id: editing.id, name: editing.name })
                setEditing(null)
              }}
            >
              存
            </button>
          </div>
        ) : (
          <div key={c.id} className="group flex items-center gap-1">
            <button
              className={`min-w-0 flex-1 truncate rounded px-3 py-2 text-left text-sm ${
                selected === c.id
                  ? 'bg-sky-900 text-sky-100'
                  : 'bg-slate-900 text-slate-300 hover:bg-slate-800'
              }`}
              onClick={() => onSelect(c.id)}
              onDoubleClick={() => setEditing(c)}
              title="雙擊可改名"
            >
              {c.name}
              <span className="ml-2 text-xs text-slate-500">{c.items.length}</span>
            </button>
            <button
              className="shrink-0 px-1 text-xs text-slate-600 opacity-0 transition group-hover:opacity-100 hover:text-red-400"
              disabled={busy}
              title="刪除分類（裡面的品項會移到「未分類」，不會一起刪掉）"
              onClick={() => {
                if (
                  confirm(
                    `刪除分類「${c.name}」？\n\n裡面的 ${c.items.length} 個品項會移到「未分類」，不會被刪掉。`,
                  )
                ) {
                  onDelete(c.id)
                }
              }}
            >
              ✕
            </button>
          </div>
        ),
      )}

      {tree.uncategorized.length > 0 && (
        <button
          className={`truncate rounded px-3 py-2 text-left text-sm ${
            selected === 'uncategorized'
              ? 'bg-amber-900 text-amber-100'
              : 'bg-slate-900 text-amber-300/80 hover:bg-slate-800'
          }`}
          onClick={() => onSelect('uncategorized')}
        >
          未分類
          <span className="ml-2 text-xs text-slate-500">{tree.uncategorized.length}</span>
        </button>
      )}

      <div className="mt-2 flex gap-1">
        <input
          className="min-w-0 flex-1 rounded bg-slate-800 px-2 py-2 text-sm placeholder:text-slate-600"
          placeholder="新增分類…"
          value={adding}
          disabled={busy}
          onChange={(e) => setAdding(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && adding.trim()) {
              onSave({ name: adding.trim() })
              setAdding('')
            }
          }}
        />
        <button
          className="shrink-0 rounded bg-slate-700 px-3 text-sm disabled:opacity-40"
          disabled={busy || !adding.trim()}
          onClick={() => {
            onSave({ name: adding.trim() })
            setAdding('')
          }}
        >
          ＋
        </button>
      </div>
    </aside>
  )
}

// ---------------------------------------------------------------- 品項

const EMPTY_ITEM = { name: '', price: '' }

function ItemPane({
  categoryId,
  title,
  items,
  busy,
  onSave,
  onDelete,
  onSaveVariant,
  onDeleteVariant,
}: {
  categoryId: string | null
  title: string
  items: Item[]
  busy: boolean
  onSave: (input: {
    id?: string
    categoryId?: string | null
    name: string
    basePrice: number
    isActive?: boolean
  }) => void
  onDelete: (id: string) => void
  onSaveVariant: (input: {
    id?: string
    itemId: string
    code: string
    name: string
    priceMode: 'delta' | 'absolute'
    priceDelta?: number
    price?: number
  }) => void
  onDeleteVariant: (id: string) => void
}) {
  const [draft, setDraft] = useState(EMPTY_ITEM)
  const [expanded, setExpanded] = useState<string | null>(null)

  const addItem = () => {
    const price = parseMoney(draft.price)
    if (!draft.name.trim() || price === null) return
    onSave({ categoryId, name: draft.name.trim(), basePrice: price })
    setDraft(EMPTY_ITEM)
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
        {title || '請先建立一個分類'}
      </h2>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {items.length === 0 && (
          <p className="py-8 text-center text-sm text-slate-600">這一類還沒有商品</p>
        )}

        {items.map((it) => (
          <ItemRow
            key={it.id}
            item={it}
            busy={busy}
            expanded={expanded === it.id}
            onToggle={() => setExpanded(expanded === it.id ? null : it.id)}
            onSave={onSave}
            onDelete={onDelete}
            onSaveVariant={onSaveVariant}
            onDeleteVariant={onDeleteVariant}
          />
        ))}
      </div>

      <div className="mt-3 flex gap-2 border-t border-slate-800 pt-3">
        <input
          className="min-w-0 flex-1 rounded bg-slate-800 px-3 py-2 text-sm placeholder:text-slate-600"
          placeholder="品名"
          value={draft.name}
          disabled={busy}
          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          onKeyDown={(e) => e.key === 'Enter' && addItem()}
        />
        <input
          className="w-28 rounded bg-slate-800 px-3 py-2 text-right text-sm placeholder:text-slate-600"
          placeholder="價格"
          inputMode="numeric"
          value={draft.price}
          disabled={busy}
          onChange={(e) => setDraft({ ...draft, price: e.target.value })}
          onKeyDown={(e) => e.key === 'Enter' && addItem()}
        />
        <button
          className="shrink-0 rounded bg-sky-800 px-4 text-sm hover:bg-sky-700 disabled:opacity-40"
          disabled={busy || !draft.name.trim() || parseMoney(draft.price) === null}
          onClick={addItem}
        >
          新增
        </button>
      </div>
    </section>
  )
}

function ItemRow({
  item,
  busy,
  expanded,
  onToggle,
  onSave,
  onDelete,
  onSaveVariant,
  onDeleteVariant,
}: {
  item: Item
  busy: boolean
  expanded: boolean
  onToggle: () => void
  onSave: (input: {
    id?: string
    name: string
    basePrice: number
    isActive?: boolean
  }) => void
  onDelete: (id: string) => void
  onSaveVariant: (input: {
    itemId: string
    code: string
    name: string
    priceMode: 'delta' | 'absolute'
    priceDelta?: number
  }) => void
  onDeleteVariant: (id: string) => void
}) {
  const [price, setPrice] = useState(String(item.basePrice))
  const [name, setName] = useState(item.name)

  // 外部重讀之後要跟著更新，否則儲存完會看到舊值。
  useEffect(() => {
    setPrice(String(item.basePrice))
    setName(item.name)
  }, [item.basePrice, item.name])

  const dirty = name !== item.name || parseMoney(price) !== item.basePrice

  return (
    <div className="mb-1 rounded bg-slate-900">
      <div className="flex items-center gap-2 px-3 py-2">
        <button
          className="w-6 shrink-0 text-slate-600 hover:text-slate-300"
          onClick={onToggle}
          title="規格（大 / 中 / 小）"
        >
          {expanded ? '▾' : '▸'}
        </button>

        <input
          className="min-w-0 flex-1 rounded bg-transparent px-1 py-1 text-sm hover:bg-slate-800 focus:bg-slate-800"
          value={name}
          disabled={busy}
          onChange={(e) => setName(e.target.value)}
        />

        <input
          className="w-24 rounded bg-transparent px-1 py-1 text-right text-sm hover:bg-slate-800 focus:bg-slate-800"
          value={price}
          inputMode="numeric"
          disabled={busy}
          onChange={(e) => setPrice(e.target.value)}
        />

        {dirty ? (
          <button
            className="shrink-0 rounded bg-emerald-800 px-3 py-1 text-xs hover:bg-emerald-700 disabled:opacity-40"
            disabled={busy || !name.trim() || parseMoney(price) === null}
            onClick={() =>
              onSave({ id: item.id, name: name.trim(), basePrice: parseMoney(price)! })
            }
          >
            儲存
          </button>
        ) : (
          <span className="w-[52px] shrink-0 text-right text-xs text-slate-600">
            {item.variants.length > 0 ? `${item.variants.length} 規格` : ''}
          </span>
        )}

        <button
          className="shrink-0 px-1 text-xs text-slate-600 hover:text-red-400"
          disabled={busy}
          title="刪除商品（歷史訂單不受影響）"
          onClick={() => {
            if (confirm(`刪除「${item.name}」？\n\n歷史訂單裡的紀錄不會受影響。`)) {
              onDelete(item.id)
            }
          }}
        >
          ✕
        </button>
      </div>

      {expanded && (
        <VariantEditor
          item={item}
          busy={busy}
          onSave={onSaveVariant}
          onDelete={onDeleteVariant}
        />
      )}
    </div>
  )
}

function VariantEditor({
  item,
  busy,
  onSave,
  onDelete,
}: {
  item: Item
  busy: boolean
  onSave: (input: {
    itemId: string
    code: string
    name: string
    priceMode: 'delta' | 'absolute'
    priceDelta?: number
  }) => void
  onDelete: (id: string) => void
}) {
  const [name, setName] = useState('')
  const [delta, setDelta] = useState('')

  const add = () => {
    const d = parseMoney(delta || '0')
    if (!name.trim() || d === null) return
    onSave({
      itemId: item.id,
      // code 給機器看（未來對外系統會用），name 給人看。
      code: name.trim(),
      name: name.trim(),
      priceMode: 'delta',
      priceDelta: d,
    })
    setName('')
    setDelta('')
  }

  return (
    <div className="border-t border-slate-800 px-3 py-2 pl-11">
      {item.variants.length === 0 && (
        <p className="mb-2 text-xs text-slate-600">
          還沒有規格。加了之後點餐時會先問客人要哪一種（例如大杯 +10）。
        </p>
      )}

      {item.variants.map((v) => (
        <div key={v.id} className="flex items-center gap-2 py-1 text-sm">
          <span className="flex-1">{v.name}</span>
          <span className="w-24 text-right text-slate-400">{variantLabel(v, item)}</span>
          <button
            className="px-1 text-xs text-slate-600 hover:text-red-400"
            disabled={busy}
            onClick={() => onDelete(v.id)}
          >
            ✕
          </button>
        </div>
      ))}

      <div className="mt-2 flex gap-2">
        <input
          className="min-w-0 flex-1 rounded bg-slate-800 px-2 py-1 text-sm placeholder:text-slate-600"
          placeholder="規格名稱（大杯）"
          value={name}
          disabled={busy}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && add()}
        />
        <input
          className="w-20 rounded bg-slate-800 px-2 py-1 text-right text-sm placeholder:text-slate-600"
          placeholder="+10"
          inputMode="numeric"
          value={delta}
          disabled={busy}
          onChange={(e) => setDelta(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && add()}
        />
        <button
          className="shrink-0 rounded bg-slate-700 px-3 text-sm disabled:opacity-40"
          disabled={busy || !name.trim()}
          onClick={add}
        >
          加規格
        </button>
      </div>
    </div>
  )
}

function variantLabel(v: Variant, item: Item): string {
  if (v.priceMode === 'absolute') return formatMoney(v.price)
  const total = item.basePrice + v.priceDelta
  const sign = v.priceDelta === 0 ? '' : ` (${formatMoney(v.priceDelta, { sign: true })})`
  return `${formatMoney(total)}${sign}`
}
