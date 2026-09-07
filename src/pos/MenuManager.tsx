import { useCallback, useEffect, useState } from 'react'

import ModifierPane from './ModifierPane'

import {
  menuApi,
  type AppError,
  type Category,
  type CategoryNode,
  type Item,
  type MenuTree,
  type ModifierGroup,
  type Variant,
} from '@/shared/api'
import { useT } from '@/shared/i18n'
import { menu } from '@/shared/locales/menu'
import { ui } from '@/shared/locales/nav'
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
/** 左邊那一欄選到「選項群組」時的哨兵值。 */
const MODIFIERS = '__modifiers__'

export default function MenuManager() {
  const t = useT()
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
        {error ? <ErrorBar message={error} /> : t(menu.loading)}
      </div>
    )
  }

  const current: CategoryNode | null =
    selected === 'uncategorized' || selected === MODIFIERS
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
        {selected === MODIFIERS ? (
          <ModifierPane tree={tree} busy={busy} run={run} />
        ) : (
          <ItemPane
            categoryId={selected === 'uncategorized' ? null : (current?.id ?? null)}
            title={
              selected === 'uncategorized' ? t(menu.uncategorized) : (current?.name ?? '')
            }
            items={items}
            groups={tree.modifierGroups}
            busy={busy}
            onSave={(input) => run(() => menuApi.upsertItem(input))}
            onDelete={(id) => run(() => menuApi.deleteItem(id))}
            onSaveVariant={(input) => run(() => menuApi.upsertVariant(input))}
            onDeleteVariant={(id) => run(() => menuApi.deleteVariant(id))}
            onSetGroups={(itemId, ids) =>
              run(() => menuApi.setItemModifierGroups(itemId, ids))
            }
          />
        )}
      </div>
    </div>
  )
}

function ErrorBar({ message, onDismiss }: { message: string; onDismiss?: () => void }) {
  const t = useT()
  return (
    <div className="mb-3 flex items-start gap-3 rounded border border-red-800 bg-red-950/60 px-4 py-3 text-sm text-red-200">
      {/* 錯誤內容來自後端，語系由後端決定，前端不翻。 */}
      <span className="flex-1 whitespace-pre-wrap">{message}</span>
      {onDismiss && (
        <button className="shrink-0 text-red-400 hover:text-red-200" onClick={onDismiss}>
          {t(ui.close)}
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
  const t = useT()
  const [adding, setAdding] = useState('')
  const [editing, setEditing] = useState<Category | null>(null)

  return (
    <aside className="flex w-64 shrink-0 flex-col gap-2 overflow-y-auto">
      <h2 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
        {t(menu.categories)}
      </h2>

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
              {t(menu.saveShort)}
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
              title={t(menu.renameHint)}
            >
              {c.name}
              <span className="ml-2 text-xs text-slate-500">{c.items.length}</span>
            </button>
            <button
              className="shrink-0 px-1 text-xs text-slate-600 opacity-0 transition group-hover:opacity-100 hover:text-red-400"
              disabled={busy}
              title={t(menu.deleteCategoryHint)}
              onClick={() => {
                if (
                  confirm(
                    t(menu.deleteCategoryConfirm, { name: c.name, n: c.items.length }),
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
          {t(menu.uncategorized)}
          <span className="ml-2 text-xs text-slate-500">{tree.uncategorized.length}</span>
        </button>
      )}

      {/* 選項群組是**店裡共用**的，不屬於任何一個分類 ——
          所以它跟分類並排，而不是藏在某個品項底下。 */}
      <button
        className={`mt-2 truncate rounded px-3 py-2 text-left text-sm ${
          selected === MODIFIERS
            ? 'bg-sky-900 text-sky-100'
            : 'bg-slate-900 text-slate-300 hover:bg-slate-800'
        }`}
        onClick={() => onSelect(MODIFIERS)}
      >
        {t(menu.modifierGroups)}
        <span className="ml-2 text-xs text-slate-500">{tree.modifierGroups.length}</span>
      </button>

      <div className="mt-2 flex gap-1">
        <input
          className="min-w-0 flex-1 rounded bg-slate-800 px-2 py-2 text-sm placeholder:text-slate-600"
          placeholder={t(menu.addCategory)}
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
  groups,
  busy,
  onSave,
  onDelete,
  onSaveVariant,
  onDeleteVariant,
  onSetGroups,
}: {
  categoryId: string | null
  title: string
  items: Item[]
  groups: ModifierGroup[]
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
  onSetGroups: (itemId: string, groupIds: string[]) => void
}) {
  const t = useT()
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
        {title || t(menu.pickCategory)}
      </h2>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {items.length === 0 && (
          <p className="py-8 text-center text-sm text-slate-600">{t(menu.noItems)}</p>
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
            groups={groups}
            onSetGroups={onSetGroups}
          />
        ))}
      </div>

      <div className="mt-3 flex gap-2 border-t border-slate-800 pt-3">
        <input
          className="min-w-0 flex-1 rounded bg-slate-800 px-3 py-2 text-sm placeholder:text-slate-600"
          placeholder={t(menu.itemName)}
          value={draft.name}
          disabled={busy}
          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          onKeyDown={(e) => e.key === 'Enter' && addItem()}
        />
        <input
          className="w-28 rounded bg-slate-800 px-3 py-2 text-right text-sm placeholder:text-slate-600"
          placeholder={t(menu.itemPrice)}
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
          {t(ui.add)}
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
  groups,
  onSetGroups,
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
  groups: ModifierGroup[]
  onSetGroups: (itemId: string, groupIds: string[]) => void
}) {
  const t = useT()
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
          title={t(menu.variantsHint)}
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
            {t(ui.save)}
          </button>
        ) : (
          <span className="w-[52px] shrink-0 text-right text-xs text-slate-600">
            {item.variants.length > 0
              ? t(menu.variantCount, { n: item.variants.length })
              : ''}
          </span>
        )}

        <button
          className="shrink-0 px-1 text-xs text-slate-600 hover:text-red-400"
          disabled={busy}
          title={t(menu.deleteItemHint)}
          onClick={() => {
            if (confirm(t(menu.deleteItemConfirm, { name: item.name }))) {
              onDelete(item.id)
            }
          }}
        >
          ✕
        </button>
      </div>

      {expanded && (
        <>
          <VariantEditor
            item={item}
            busy={busy}
            onSave={onSaveVariant}
            onDelete={onDeleteVariant}
          />
          <GroupPicker
            item={item}
            groups={groups}
            busy={busy}
            onSetGroups={onSetGroups}
          />
        </>
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
  const t = useT()
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
        <p className="mb-2 text-xs text-slate-600">{t(menu.noVariants)}</p>
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
          placeholder={t(menu.variantName)}
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
          {t(menu.addVariant)}
        </button>
      </div>
    </div>
  )
}

/**
 * 這個品項要問哪幾組選項。
 *
 * 整批送（不是加一個／刪一個）：畫面上是一排勾選框，整批送比較不會出現
 * 「勾了但沒存到」那種狀態 —— 而那是使用者最不會發現的一種錯。
 */
function GroupPicker({
  item,
  groups,
  busy,
  onSetGroups,
}: {
  item: Item
  groups: ModifierGroup[]
  busy: boolean
  onSetGroups: (itemId: string, groupIds: string[]) => void
}) {
  const t = useT()
  if (groups.length === 0) {
    return (
      <div className="border-t border-slate-800 px-3 py-2 pl-11 text-xs text-slate-600">
        {t(menu.noGroups)}
      </div>
    )
  }
  const on = new Set(item.modifierGroupIds)
  return (
    <div className="border-t border-slate-800 px-3 py-2 pl-11">
      <p className="mb-1.5 text-xs text-slate-600">{t(menu.askOnOrder)}</p>
      <div className="flex flex-wrap gap-1.5">
        {groups.map((g) => (
          <button
            key={g.id}
            className={`rounded px-3 py-1.5 text-sm ${
              on.has(g.id) ? 'bg-sky-800 text-sky-50' : 'bg-slate-800 text-slate-400'
            }`}
            disabled={busy}
            onClick={() =>
              onSetGroups(
                item.id,
                on.has(g.id)
                  ? item.modifierGroupIds.filter((x) => x !== g.id)
                  : [...item.modifierGroupIds, g.id],
              )
            }
          >
            {g.name}
            <span className="ml-1 text-xs opacity-60">
              {g.selectionType === 'single' ? t(menu.selectSingle) : t(menu.selectMulti)}
            </span>
          </button>
        ))}
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
