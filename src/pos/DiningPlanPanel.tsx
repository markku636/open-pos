import { useCallback, useEffect, useState } from 'react'

import {
  diningApi,
  menuApi,
  type AppError,
  type DiningPlan,
  type DiningPlanInput,
  type MenuTree,
} from '@/shared/api'
import { useT } from '@/shared/i18n'
import { formatMoney } from '@/shared/money'
import { ui } from '@/shared/locales/nav'
import { plan as tp } from '@/shared/locales/plan'

/**
 * 吃到飽 / 無限暢飲方案。
 *
 * # 這一頁在設定什麼
 *
 * 三件事：**收多少**（方案商品）、**吃什麼不另外收錢**（成員）、**坐多久**（時限）。
 *
 * # 方案本身就是一個商品
 *
 * 不是另一種東西 —— 這是查過的 25 套產品的共識（見 docs/dining-modes.md）。
 * 所以這裡選的是既有的商品，而人頭費就是**那個商品點 N 份**。
 *
 * 好處直接寫在畫面上讓老闆看得懂：想做「大人 599 / 小孩 299」，
 * 就去那個商品加規格，不必在這裡再設一套人頭分級。
 *
 * # 時限只提醒
 *
 * 查過的產品**沒有一套**會在時間到的時候自動加價或擋單。所以這裡的文案要
 * 講清楚它只會變顏色與提示，否則老闆會以為設了就會自動收超時費。
 */
export default function DiningPlanPanel() {
  const t = useT()
  const [plans, setPlans] = useState<DiningPlan[] | null>(null)
  const [menu, setMenu] = useState<MenuTree | null>(null)
  const [editing, setEditing] = useState<DiningPlan | 'new' | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const [p, m] = await Promise.all([diningApi.list(), menuApi.tree()])
      setPlans(p)
      setMenu(m)
      setError(null)
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const remove = async (p: DiningPlan) => {
    if (!confirm(t(tp.confirmDelete, { name: p.name }))) return
    try {
      await diningApi.remove(p.id)
      await load()
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    }
  }

  return (
    <div className="h-full space-y-4 overflow-y-auto pr-2">
      <section className="rounded border border-slate-800 bg-slate-900/40 px-4 py-3 text-sm text-slate-400">
        <p className="text-slate-300">{t(tp.intro)}</p>
        <p className="mt-1">{t(tp.introTiers)}</p>
      </section>

      {error && (
        <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}

      <div className="flex items-center gap-3">
        <h2 className="text-sm text-slate-400">{t(tp.title)}</h2>
        <button
          className="rounded bg-emerald-800 px-3 py-1.5 text-sm hover:bg-emerald-700"
          onClick={() => setEditing('new')}
        >
          ＋ {t(ui.add)}
        </button>
      </div>

      {plans && plans.length === 0 && (
        <p className="rounded bg-slate-900/40 px-3 py-10 text-center text-sm text-slate-600">
          {t(tp.empty)}
          <br />
          {t(tp.emptyHint)}
        </p>
      )}

      <div className="grid gap-3 md:grid-cols-2">
        {(plans ?? []).map((p) => (
          <Card
            key={p.id}
            p={p}
            menu={menu}
            onEdit={() => setEditing(p)}
            onRemove={() => void remove(p)}
          />
        ))}
      </div>

      {editing && menu && (
        <Editor
          key={editing === 'new' ? 'new' : editing.id}
          plan={editing === 'new' ? null : editing}
          menu={menu}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null)
            void load()
          }}
        />
      )}
    </div>
  )
}

function allItems(menu: MenuTree) {
  return [...menu.categories.flatMap((c) => c.items), ...menu.uncategorized]
}

function Card({
  p,
  menu,
  onEdit,
  onRemove,
}: {
  p: DiningPlan
  menu: MenuTree | null
  onEdit: () => void
  onRemove: () => void
}) {
  const t = useT()
  const item = menu ? allItems(menu).find((i) => i.id === p.itemId) : undefined
  const catNames = menu
    ? p.memberCategories
        .map((id) => menu.categories.find((c) => c.id === id)?.name)
        .filter(Boolean)
    : []

  return (
    <section className="rounded border border-slate-800 bg-slate-900/40 p-3">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="truncate font-medium">{p.name}</h3>
            {!p.isActive && (
              <span className="shrink-0 rounded bg-slate-800 px-1.5 py-0.5 text-xs text-slate-500">
                {t(tp.inactive)}
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs text-slate-500">
            {p.itemName}
            {item && ` · ${formatMoney(item.basePrice)}`}
            {p.limitMinutes > 0 && ` · ${t(tp.minutes, { n: p.limitMinutes })}`}
          </p>
        </div>
        <button className="shrink-0 text-xs text-slate-400 hover:text-slate-200" onClick={onEdit}>
          {t(ui.edit)}
        </button>
        <button className="shrink-0 text-xs text-red-400/70 hover:text-red-300" onClick={onRemove}>
          {t(ui.delete)}
        </button>
      </div>

      <p className="mt-2 border-t border-slate-800 pt-2 text-xs text-slate-500">
        {catNames.length === 0 && p.memberItems.length === 0
          ? t(tp.noMembers)
          : t(tp.members, {
              cats: catNames.join('/') || '—',
              n: p.memberItems.length,
            })}
      </p>
    </section>
  )
}

function Editor({
  plan,
  menu,
  onClose,
  onSaved,
}: {
  plan: DiningPlan | null
  menu: MenuTree
  onClose: () => void
  onSaved: () => void
}) {
  const t = useT()
  const items = allItems(menu)
  const [itemId, setItemId] = useState(plan?.itemId ?? items[0]?.id ?? '')
  const [name, setName] = useState(plan?.name ?? '')
  const [limit, setLimit] = useState(String(plan?.limitMinutes ?? 0))
  const [notice, setNotice] = useState(String(plan?.noticeMinutes ?? 0))
  const [cats, setCats] = useState<string[]>(plan?.memberCategories ?? [])
  const [memberItems, setMemberItems] = useState<string[]>(plan?.memberItems ?? [])
  const [active, setActive] = useState(plan?.isActive ?? true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const toggle = (list: string[], set: (v: string[]) => void, id: string) =>
    set(list.includes(id) ? list.filter((x) => x !== id) : [...list, id])

  const save = async () => {
    setBusy(true)
    try {
      const input: DiningPlanInput = {
        id: plan?.id ?? null,
        itemId,
        name: name.trim(),
        limitMinutes: Number(limit) || 0,
        noticeMinutes: Number(notice) || 0,
        isActive: active,
        memberCategories: cats,
        memberItems,
      }
      await diningApi.upsert(input)
      onSaved()
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-4"
      onClick={onClose}
    >
      <div
        className="max-h-full w-full max-w-2xl overflow-y-auto rounded-lg border border-slate-700 bg-slate-900 p-5"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="mb-4 text-lg">{plan ? t(tp.editTitle) : t(tp.newTitle)}</h2>

        <div className="space-y-4">
          <label className="block">
            <span className="mb-1 block text-xs text-slate-500">{t(tp.name)}</span>
            <input
              className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t(tp.namePlaceholder)}
            />
          </label>

          <label className="block">
            <span className="mb-1 block text-xs text-slate-500">{t(tp.chargeItem)}</span>
            <select
              className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
              value={itemId}
              onChange={(e) => setItemId(e.target.value)}
            >
              {items.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name}（{formatMoney(i.basePrice)}）
                </option>
              ))}
            </select>
            {/* 這一段直接教老闆怎麼做「大人 / 小孩不同價」——
                他不會知道那要去改商品規格。 */}
            <p className="mt-1 text-xs text-slate-500">{t(tp.chargeItemHint)}</p>
          </label>

          <fieldset className="rounded border border-slate-800 p-3">
            <legend className="px-1 text-xs text-slate-500">{t(tp.membersTitle)}</legend>
            <p className="mb-2 text-xs text-slate-600">{t(tp.membersHint)}</p>
            <div className="flex flex-wrap gap-1.5">
              {menu.categories.map((c) => (
                <button
                  key={c.id}
                  className={`rounded px-2 py-1 text-xs ${
                    cats.includes(c.id)
                      ? 'bg-emerald-800 text-emerald-100'
                      : 'bg-slate-800 text-slate-400'
                  }`}
                  onClick={() => toggle(cats, setCats, c.id)}
                >
                  {c.name}
                </button>
              ))}
            </div>
            {/* 個別品項只在「整個分類不對」的時候才用得到，所以收在後面。 */}
            <details className="mt-3">
              <summary className="cursor-pointer text-xs text-slate-500">
                {t(tp.pickItems, { n: memberItems.length })}
              </summary>
              <div className="mt-2 flex max-h-40 flex-wrap gap-1.5 overflow-y-auto">
                {items
                  .filter((i) => i.id !== itemId)
                  .map((i) => (
                    <button
                      key={i.id}
                      className={`rounded px-2 py-1 text-xs ${
                        memberItems.includes(i.id)
                          ? 'bg-emerald-800 text-emerald-100'
                          : 'bg-slate-800 text-slate-400'
                      }`}
                      onClick={() => toggle(memberItems, setMemberItems, i.id)}
                    >
                      {i.name}
                    </button>
                  ))}
              </div>
            </details>
          </fieldset>

          <div className="grid grid-cols-2 gap-3">
            <label className="block">
              <span className="mb-1 block text-xs text-slate-500">{t(tp.limit)}</span>
              <input
                className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
                inputMode="numeric"
                value={limit}
                onChange={(e) => setLimit(e.target.value)}
              />
            </label>
            <label className="block">
              <span className="mb-1 block text-xs text-slate-500">{t(tp.notice)}</span>
              <input
                className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
                inputMode="numeric"
                value={notice}
                onChange={(e) => setNotice(e.target.value)}
              />
            </label>
          </div>
          {/* ★ 這一句很重要：查過的產品沒有一套會自動加價或擋單，
              而老闆很容易以為設了時限就會自動收超時費。 */}
          <p className="text-xs text-amber-400/80">{t(tp.limitWarning)}</p>

          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={active}
              onChange={(e) => setActive(e.target.checked)}
            />
            <span>{t(tp.enabled)}</span>
          </label>

          {error && (
            <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
              {error}
            </p>
          )}
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <button className="rounded bg-slate-800 px-4 py-2 text-sm" onClick={onClose}>
            {t(ui.cancel)}
          </button>
          <button
            className="rounded bg-emerald-700 px-4 py-2 text-sm hover:bg-emerald-600 disabled:opacity-40"
            disabled={busy}
            onClick={() => void save()}
          >
            {t(ui.save)}
          </button>
        </div>
      </div>
    </div>
  )
}
