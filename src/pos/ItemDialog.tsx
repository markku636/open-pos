import { useMemo, useState } from 'react'

import type { Item, MenuTree, Modifier, ModifierGroup, Variant } from '@/shared/api'
import { formatMoney } from '@/shared/money'

/**
 * 選規格與選項。
 *
 * 「珍奶半糖少冰加珍珠」在這之前記不下來 —— 規格建得起來但點餐時選不到，
 * 選項連建都建不了。而那是台灣飲料店每一杯都會發生的事。
 *
 * # 這個對話框只在需要的時候出現
 *
 * 沒有規格也沒有選項的品項（滷肉飯），按下去就直接進購物車。
 * 每一次多按一下的成本乘上一天三百單，是收銀員會抱怨的那種成本。
 *
 * # 為什麼必選的群組要擋住「加入」
 *
 * 因為「忘了選甜度」的下場是廚房自己猜，而猜錯要重做一杯。
 * 擋在這裡的代價是收銀員多按一下，擋不住的代價是一杯飲料。
 */
export interface Chosen {
  variantId: string | null
  modifierIds: string[]
}

export default function ItemDialog({
  item,
  tree,
  onCancel,
  onAdd,
}: {
  item: Item
  tree: MenuTree
  onCancel: () => void
  onAdd: (chosen: Chosen) => void
}) {
  const groups = useMemo(
    () =>
      item.modifierGroupIds
        .map((id) => tree.modifierGroups.find((g) => g.id === id))
        .filter((g): g is ModifierGroup => !!g),
    [item.modifierGroupIds, tree.modifierGroups],
  )

  const variants = item.variants.filter((v) => v.isActive)
  const [variantId, setVariantId] = useState<string | null>(
    variants.find((v) => v.isDefault)?.id ?? variants[0]?.id ?? null,
  )
  // 預設值先勾起來：多數客人不會改甜度，而每一杯都要點一下正常糖很煩。
  const [picked, setPicked] = useState<string[]>(() =>
    groups.flatMap((g) => g.options.filter((o) => o.isDefault && o.isActive).map((o) => o.id)),
  )

  const toggle = (g: ModifierGroup, o: Modifier) => {
    setPicked((p) => {
      if (g.selectionType === 'single') {
        const others = p.filter((id) => !g.options.some((x) => x.id === id))
        // 單選群組再點一次同一個＝取消（除非它是必選的）。
        return p.includes(o.id) && g.minSelect === 0 ? others : [...others, o.id]
      }
      return p.includes(o.id) ? p.filter((id) => id !== o.id) : [...p, o.id]
    })
  }

  const missing = groups.filter(
    (g) => g.minSelect > 0 && g.options.filter((o) => picked.includes(o.id)).length < g.minSelect,
  )

  const extra = groups
    .flatMap((g) => g.options)
    .filter((o) => picked.includes(o.id))
    .reduce((s, o) => s + o.price, 0)
  const variant = variants.find((v) => v.id === variantId) ?? null
  const price = priceOf(item, variant) + extra

  return (
    <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/60 p-4">
      <div className="max-h-full w-full max-w-md overflow-y-auto rounded-lg border border-slate-700 bg-slate-900 p-5">
        <div className="flex items-baseline justify-between">
          <h2 className="text-lg font-semibold">{item.name}</h2>
          <span className="font-mono text-2xl text-sky-300">{formatMoney(price)}</span>
        </div>

        {variants.length > 0 && (
          <section className="mt-4">
            <h3 className="mb-1.5 text-sm text-slate-400">規格</h3>
            <div className="flex flex-wrap gap-1.5">
              {variants.map((v) => (
                <Choice
                  key={v.id}
                  active={variantId === v.id}
                  label={v.name}
                  delta={priceOf(item, v) - item.basePrice}
                  onClick={() => setVariantId(v.id)}
                />
              ))}
            </div>
          </section>
        )}

        {groups.map((g) => (
          <section className="mt-4" key={g.id}>
            <h3 className="mb-1.5 text-sm text-slate-400">
              {g.name}
              {g.minSelect > 0 && <span className="ml-1 text-amber-400">必選</span>}
              {g.selectionType === 'multiple' && (
                <span className="ml-1 text-xs text-slate-600">可複選</span>
              )}
            </h3>
            <div className="flex flex-wrap gap-1.5">
              {g.options
                .filter((o) => o.isActive)
                .map((o) => (
                  <Choice
                    key={o.id}
                    active={picked.includes(o.id)}
                    label={o.name}
                    delta={o.price}
                    soldOut={!!o.soldOutUntil}
                    onClick={() => toggle(g, o)}
                  />
                ))}
            </div>
          </section>
        ))}

        {/* ★ 擋住「忘了選甜度」。擋不住的代價是廚房自己猜，猜錯要重做一杯。 */}
        {missing.length > 0 && (
          <p className="mt-4 rounded bg-amber-950/40 px-3 py-2 text-sm text-amber-300">
            還要選：{missing.map((g) => g.name).join('、')}
          </p>
        )}

        <div className="mt-5 flex gap-2">
          <button
            className="flex-1 rounded bg-slate-800 py-3 hover:bg-slate-700"
            onClick={onCancel}
          >
            取消
          </button>
          <button
            className="flex-[2] rounded bg-sky-700 py-3 text-lg font-semibold hover:bg-sky-600 disabled:opacity-40"
            disabled={missing.length > 0}
            onClick={() => onAdd({ variantId, modifierIds: picked })}
          >
            加入 {formatMoney(price)}
          </button>
        </div>
      </div>
    </div>
  )
}

/** 這個品項按下去要不要先問。沒有規格也沒有選項就直接進購物車。 */
export function needsDialog(item: Item, tree: MenuTree): boolean {
  return (
    item.variants.some((v) => v.isActive) ||
    item.modifierGroupIds.some((id) => tree.modifierGroups.some((g) => g.id === id))
  )
}

/** 沒有規格也沒有選項時，預設要送什麼。 */
export function defaultChoice(item: Item, tree: MenuTree): Chosen {
  const variants = item.variants.filter((v) => v.isActive)
  return {
    variantId: variants.find((v) => v.isDefault)?.id ?? null,
    modifierIds: item.modifierGroupIds
      .map((id) => tree.modifierGroups.find((g) => g.id === id))
      .filter((g): g is ModifierGroup => !!g)
      .flatMap((g) => g.options.filter((o) => o.isDefault && o.isActive).map((o) => o.id)),
  }
}

function priceOf(item: Item, variant: Variant | null): number {
  if (!variant) return item.basePrice
  // delta = 跟著基本價加減；absolute = 自己一個固定價。
  // 兩種都要支援，因為老闆改基本價時的預期行為不同。
  return variant.priceMode === 'absolute' ? variant.price : item.basePrice + variant.priceDelta
}

function Choice({
  active,
  label,
  delta,
  soldOut,
  onClick,
}: {
  active: boolean
  label: string
  delta: number
  soldOut?: boolean
  onClick: () => void
}) {
  return (
    <button
      // 選項按鈕也要夠大 —— 收銀員是站著用食指戳的。
      className={`min-h-11 rounded px-4 py-2 text-base disabled:opacity-30 ${
        active ? 'bg-sky-700 text-sky-50' : 'bg-slate-800 text-slate-300 hover:bg-slate-700'
      }`}
      disabled={soldOut}
      onClick={onClick}
    >
      {label}
      {delta !== 0 && (
        <span className={`ml-1 text-xs ${active ? 'text-sky-200' : 'text-slate-500'}`}>
          {delta > 0 ? `+${delta}` : delta}
        </span>
      )}
      {soldOut && <span className="ml-1 text-xs text-amber-400">售完</span>}
    </button>
  )
}
