import { useState } from 'react'

import { menuApi, type MenuTree, type Modifier, type ModifierGroup } from '@/shared/api'
import { parseMoney } from '@/shared/money'

/**
 * 選項群組維護（甜度 / 冰塊 / 加購）。
 *
 * # 為什麼群組是「店裡的」而不是「品項的」
 *
 * 因為一份飲料單上二十杯飲料的甜度選項一模一樣。做成品項的附屬品，
 * 老闆要打二十次；做成店裡共用的一組，改一次全部跟著改。
 *
 * # 單選與複選的差別要在建立時就講清楚
 *
 * 「甜度」必選一個、「加購」可以不選也可以選很多個 —— 這個差別直接決定
 * 點餐畫面長什麼樣子，而它是建群組時唯一真的需要想一下的決定。
 * 所以那兩顆按鈕放在最上面，而且寫著它們的實際效果。
 */
export default function ModifierPane({
  tree,
  busy,
  run,
}: {
  tree: MenuTree
  busy: boolean
  run: (fn: () => Promise<unknown>) => Promise<void>
}) {
  const [name, setName] = useState('')
  const [multiple, setMultiple] = useState(false)
  const [open, setOpen] = useState<string | null>(null)

  const add = () => {
    if (!name.trim()) return
    void run(() =>
      menuApi.upsertModifierGroup({
        name: name.trim(),
        selectionType: multiple ? 'multiple' : 'single',
        // 單選預設必選一個（甜度不選就是廚房自己猜）；複選預設可以不選。
        minSelect: multiple ? 0 : 1,
        maxSelect: multiple ? 0 : 1,
        sortOrder: tree.modifierGroups.length * 10,
      }),
    ).then(() => setName(''))
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <h2 className="mb-2 text-sm text-slate-400">
        選項群組
        <span className="ml-2 text-xs text-slate-600">
          建好之後到品項那邊勾選要問哪幾組
        </span>
      </h2>

      <div className="mb-3 flex flex-wrap items-center gap-2 rounded bg-slate-900/60 p-2">
        <input
          className="min-w-32 flex-1 rounded bg-slate-800 px-3 py-2 text-sm"
          placeholder="群組名稱（甜度 / 加購）"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && add()}
        />
        <div className="flex gap-1">
          <Toggle active={!multiple} onClick={() => setMultiple(false)}>
            單選（必選一個）
          </Toggle>
          <Toggle active={multiple} onClick={() => setMultiple(true)}>
            複選（可不選）
          </Toggle>
        </div>
        <button
          className="rounded bg-sky-800 px-4 py-2 text-sm hover:bg-sky-700 disabled:opacity-40"
          disabled={busy || !name.trim()}
          onClick={add}
        >
          新增群組
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {tree.modifierGroups.length === 0 ? (
          <p className="py-10 text-center text-sm text-slate-600">
            還沒有選項群組。
            <br />
            建一組「甜度」（單選）跟一組「加購」（複選），
            <br />
            點餐時就問得出「珍奶半糖少冰加珍珠」。
          </p>
        ) : (
          tree.modifierGroups.map((g) => (
            <GroupRow
              key={g.id}
              group={g}
              busy={busy}
              expanded={open === g.id}
              onToggle={() => setOpen((o) => (o === g.id ? null : g.id))}
              run={run}
            />
          ))
        )}
      </div>
    </div>
  )
}

function GroupRow({
  group,
  busy,
  expanded,
  onToggle,
  run,
}: {
  group: ModifierGroup
  busy: boolean
  expanded: boolean
  onToggle: () => void
  run: (fn: () => Promise<unknown>) => Promise<void>
}) {
  const [name, setName] = useState('')
  const [price, setPrice] = useState('')

  const add = () => {
    const p = parseMoney(price || '0')
    if (!name.trim() || p === null) return
    void run(() =>
      menuApi.upsertModifier({
        groupId: group.id,
        name: name.trim(),
        price: p,
        sortOrder: group.options.length * 10,
        isActive: true,
      }),
    ).then(() => {
      setName('')
      setPrice('')
    })
  }

  return (
    <div className="mb-1 rounded bg-slate-900/50">
      <div className="flex items-center gap-2 px-3 py-2">
        <button className="w-5 text-slate-600" onClick={onToggle}>
          {expanded ? '▾' : '▸'}
        </button>
        <span className="flex-1 font-medium">{group.name}</span>
        <span className="text-xs text-slate-500">
          {group.selectionType === 'single' ? '單選' : '複選'}
          {group.minSelect > 0 && <span className="ml-1 text-amber-400">必選</span>}
        </span>
        <span className="w-16 text-right text-xs text-slate-600">
          {group.options.length} 個選項
        </span>
        <button
          className="px-1 text-xs text-slate-600 hover:text-red-400"
          disabled={busy}
          title="刪掉整組。已經賣出去的訂單不受影響（存的是當時的名稱與價格）"
          onClick={() => {
            if (!confirm(`要刪掉「${group.name}」整組嗎？\n\n掛著它的品項會一起解除。`)) return
            void run(() => menuApi.deleteModifierGroup(group.id))
          }}
        >
          刪除
        </button>
      </div>

      {expanded && (
        <div className="border-t border-slate-800 px-3 py-2 pl-10">
          {group.options.length === 0 && (
            <p className="mb-2 text-xs text-slate-600">
              還沒有選項。加價填 0 就是免費選項（半糖、去冰）。
            </p>
          )}

          {group.options.map((o) => (
            <OptionRow key={o.id} option={o} group={group} busy={busy} run={run} />
          ))}

          <div className="mt-2 flex gap-2">
            <input
              className="flex-1 rounded bg-slate-800 px-2 py-1.5 text-sm"
              placeholder="選項名稱（半糖 / 加珍珠）"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && add()}
            />
            <input
              className="w-24 rounded bg-slate-800 px-2 py-1.5 text-right text-sm"
              placeholder="加價"
              inputMode="numeric"
              value={price}
              onChange={(e) => setPrice(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && add()}
            />
            <button
              className="rounded bg-slate-800 px-3 py-1.5 text-sm hover:bg-slate-700 disabled:opacity-40"
              disabled={busy || !name.trim()}
              onClick={add}
            >
              加選項
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

function OptionRow({
  option,
  group,
  busy,
  run,
}: {
  option: Modifier
  group: ModifierGroup
  busy: boolean
  run: (fn: () => Promise<unknown>) => Promise<void>
}) {
  const setDefault = () =>
    void run(() =>
      menuApi.upsertModifier({
        id: option.id,
        groupId: group.id,
        name: option.name,
        price: option.price,
        isDefault: !option.isDefault,
        sortOrder: option.sortOrder,
        isActive: option.isActive,
      }),
    )

  return (
    <div className="flex items-center gap-2 py-1 text-sm">
      <span className="flex-1">{option.name}</span>
      <span className="w-16 text-right text-slate-400">
        {option.price === 0 ? '免費' : `+${option.price}`}
      </span>
      {/* 預設值會在點餐畫面上先勾起來。多數客人不改甜度，
          而每一杯都要點一下「正常糖」很煩。 */}
      <button
        className={`rounded px-2 py-0.5 text-xs ${
          option.isDefault ? 'bg-sky-800 text-sky-100' : 'text-slate-600 hover:text-slate-300'
        }`}
        disabled={busy}
        title="點餐時先勾起來"
        onClick={setDefault}
      >
        預設
      </button>
      <button
        className="px-1 text-xs text-slate-600 hover:text-red-400"
        disabled={busy}
        onClick={() => void run(() => menuApi.deleteModifier(option.id))}
      >
        ✕
      </button>
    </div>
  )
}

function Toggle({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      className={`rounded px-3 py-2 text-xs ${
        active ? 'bg-sky-800 text-sky-50' : 'bg-slate-800 text-slate-400 hover:bg-slate-700'
      }`}
      onClick={onClick}
    >
      {children}
    </button>
  )
}
