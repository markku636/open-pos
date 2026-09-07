import { useState } from 'react'

import { menuApi, type MenuTree, type Modifier, type ModifierGroup } from '@/shared/api'
import { useT } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { order } from '@/shared/locales/order'
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
  const t = useT()
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
        {t(order.groupsTitle)}
        <span className="ml-2 text-xs text-slate-600">{t(order.groupsHint)}</span>
      </h2>

      <div className="mb-3 flex flex-wrap items-center gap-2 rounded bg-slate-900/60 p-2">
        <input
          className="min-w-32 flex-1 rounded bg-slate-800 px-3 py-2 text-sm"
          placeholder={t(order.groupNamePlaceholder)}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && add()}
        />
        <div className="flex gap-1">
          <Toggle active={!multiple} onClick={() => setMultiple(false)}>
            {t(order.singleToggle)}
          </Toggle>
          <Toggle active={multiple} onClick={() => setMultiple(true)}>
            {t(order.multipleToggle)}
          </Toggle>
        </div>
        <button
          className="rounded bg-sky-800 px-4 py-2 text-sm hover:bg-sky-700 disabled:opacity-40"
          disabled={busy || !name.trim()}
          onClick={add}
        >
          {t(order.addGroup)}
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {tree.modifierGroups.length === 0 ? (
          <p className="py-10 text-center text-sm text-slate-600">
            {t(order.noGroups)}
            <br />
            {t(order.noGroupsExample)}
            <br />
            {t(order.noGroupsResult)}
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
  const t = useT()
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
          {group.selectionType === 'single' ? t(order.single) : t(order.multiple)}
          {group.minSelect > 0 && (
            <span className="ml-1 text-amber-400">{t(order.required)}</span>
          )}
        </span>
        <span className="w-16 text-right text-xs text-slate-600">
          {t(order.optionCount, { n: group.options.length })}
        </span>
        <button
          className="px-1 text-xs text-slate-600 hover:text-red-400"
          disabled={busy}
          title={t(order.deleteGroupTitle)}
          onClick={() => {
            if (!confirm(t(order.deleteGroupConfirm, { name: group.name }))) return
            void run(() => menuApi.deleteModifierGroup(group.id))
          }}
        >
          {t(ui.delete)}
        </button>
      </div>

      {expanded && (
        <div className="border-t border-slate-800 px-3 py-2 pl-10">
          {group.options.length === 0 && (
            <p className="mb-2 text-xs text-slate-600">{t(order.noOptions)}</p>
          )}

          {group.options.map((o) => (
            <OptionRow key={o.id} option={o} group={group} busy={busy} run={run} />
          ))}

          <div className="mt-2 flex gap-2">
            <input
              className="flex-1 rounded bg-slate-800 px-2 py-1.5 text-sm"
              placeholder={t(order.optionNamePlaceholder)}
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && add()}
            />
            <input
              className="w-24 rounded bg-slate-800 px-2 py-1.5 text-right text-sm"
              placeholder={t(order.surcharge)}
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
              {t(order.addOption)}
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
  const t = useT()
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
        {option.price === 0 ? t(order.free) : `+${option.price}`}
      </span>
      {/* 預設值會在點餐畫面上先勾起來。多數客人不改甜度，
          而每一杯都要點一下「正常糖」很煩。 */}
      <button
        className={`rounded px-2 py-0.5 text-xs ${
          option.isDefault ? 'bg-sky-800 text-sky-100' : 'text-slate-600 hover:text-slate-300'
        }`}
        disabled={busy}
        title={t(order.defaultTitle)}
        onClick={setDefault}
      >
        {t(order.defaultTag)}
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
