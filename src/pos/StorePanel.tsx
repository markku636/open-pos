import { useCallback, useEffect, useState } from 'react'

import {
  menuApi,
  storeApi,
  type AppError,
  type Item,
  type Store,
  type StoreInput,
} from '@/shared/api'
import { useT } from '@/shared/i18n'
import { ui } from '@/shared/locales/nav'
import { store as ts } from '@/shared/locales/store'
import { Emphasis } from '@/shared/ui/Emphasis'

/**
 * 店家設定。
 *
 * # basis point 不外露
 *
 * 資料庫存的是 basis point（10% = 1000），但老闆想的是「收 10%」。
 * 所以輸入框收百分比，換算藏在這裡 —— 讓使用者算 ×100 是把實作細節
 * 當成介面。
 *
 * 用字串存輸入值而不是 number：受控的 number input 在打到一半
 * （「1.」「0.0」）時會被 React 吃掉字元，而輸入費率正好會經過那些中間狀態。
 */
export default function StorePanel() {
  const t = useT()
  const [s, setS] = useState<Store | null>(null)
  // 開桌費要從菜單裡挑一個商品，所以這一頁需要菜單。抓不到就讓下拉選單
  // 只剩「不收」——菜單讀不到不該讓整個設定頁打不開。
  const [items, setItems] = useState<Item[]>([])
  const [form, setForm] = useState<Record<string, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try {
      const v = await storeApi.get()
      setS(v)
      setForm({
        name: v.name,
        taxId: v.taxId ?? '',
        phone: v.phone ?? '',
        address: v.address ?? '',
        cutoff: v.businessDayCutoff,
        tax: String(v.taxRateBp / 100),
        service: String(v.serviceChargeRateBp / 100),
        rounding: v.roundingPolicy,
        minCharge: String(v.minChargePerHead),
        cover: v.coverChargeItemId ?? '',
      })
      setError(null)
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    }
  }, [])

  useEffect(() => {
    void load()
    menuApi
      .tree()
      .then((t) =>
        setItems([...t.categories.flatMap((c) => c.items), ...t.uncategorized]),
      )
      .catch(() => setItems([]))
  }, [load])

  const set = (k: string, v: string) => {
    setForm((f) => ({ ...f, [k]: v }))
    setSaved(false)
  }

  const save = async () => {
    setBusy(true)
    try {
      const input: StoreInput = {
        name: form.name ?? '',
        taxId: form.taxId || null,
        phone: form.phone || null,
        address: form.address || null,
        businessDayCutoff: form.cutoff ?? '',
        // 百分比 → basis point。Math.round 是必要的：
        // 5.1% × 100 在浮點數下是 509.99999999999994。
        taxRateBp: Math.round(Number(form.tax || 0) * 100),
        serviceChargeRateBp: Math.round(Number(form.service || 0) * 100),
        roundingPolicy: form.rounding ?? 'none',
        minChargePerHead: Math.round(Number(form.minCharge || 0)),
        // 空字串 = 不收。HTML 的 select 沒有 null，後端把空字串當成 None。
        coverChargeItemId: form.cover || null,
      }
      setS(await storeApi.update(input))
      setSaved(true)
      setError(null)
    } catch (e) {
      setError((e as AppError).message ?? String(e))
    } finally {
      setBusy(false)
    }
  }

  if (!s) {
    return (
      <p className="px-3 py-10 text-center text-sm text-slate-600">
        {error ?? t(ui.loading)}
      </p>
    )
  }

  return (
    <div className="h-full space-y-4 overflow-y-auto pr-2">
      {/* ★ 這一段必須在最上面：老闆最容易誤會的就是「改了之後今天的報表會跟著變」。 */}
      <section className="rounded border-l-2 border-amber-700/70 bg-amber-950/20 px-4 py-3 text-sm leading-relaxed text-slate-400">
        <Emphasis text={t(ts.notRetroactive)} />
      </section>

      {error && (
        <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}

      <div className="grid gap-4 md:grid-cols-2">
        <Box title={t(ts.basics)}>
          <Field label={t(ts.name)} value={form.name} onChange={(v) => set('name', v)} />
          <Field
            label={t(ts.taxId)}
            hint={t(ts.taxIdHint)}
            value={form.taxId}
            onChange={(v) => set('taxId', v)}
          />
          <Field label={t(ts.phone)} value={form.phone} onChange={(v) => set('phone', v)} />
          <Field label={t(ts.address)} value={form.address} onChange={(v) => set('address', v)} />
        </Box>

        <Box title={t(ts.money)}>
          <Field
            label={t(ts.taxRate)}
            hint={t(ts.taxRateHint)}
            value={form.tax}
            onChange={(v) => set('tax', v)}
            numeric
          />
          <Field
            label={t(ts.serviceCharge)}
            hint={t(ts.serviceChargeHint)}
            value={form.service}
            onChange={(v) => set('service', v)}
            numeric
          />
          <label className="block">
            <span className="mb-1 block text-xs text-slate-500">{t(ts.rounding)}</span>
            <select
              className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
              value={form.rounding}
              onChange={(e) => set('rounding', e.target.value)}
            >
              <option value="none">{t(ts.roundNone)}</option>
              <option value="to_five">{t(ts.roundToFive)}</option>
              <option value="floor_five">{t(ts.roundFloorFive)}</option>
              <option value="floor_ten">{t(ts.roundFloorTen)}</option>
            </select>
            <p className="mt-1 text-xs text-slate-600">{t(ts.roundingHint)}</p>
          </label>
        </Box>

        <Box title={t(ts.ops)} wide>
          <div className="grid gap-4 md:grid-cols-2">
            <Field
              label={t(ts.cutoff)}
              hint={t(ts.cutoffHint)}
              value={form.cutoff}
              onChange={(v) => set('cutoff', v)}
            />
            <Field
              label={t(ts.minCharge)}
              hint={t(ts.minChargeHint)}
              value={form.minCharge}
              onChange={(v) => set('minCharge', v)}
              numeric
            />
            <label className="block md:col-span-2">
              <span className="mb-1 block text-xs text-slate-500">{t(ts.coverCharge)}</span>
              <select
                className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
                value={form.cover ?? ''}
                onChange={(e) => set('cover', e.target.value)}
              >
                <option value="">{t(ts.coverChargeNone)}</option>
                {items.map((i) => (
                  <option key={i.id} value={i.id}>
                    {i.name}　${i.basePrice}
                  </option>
                ))}
              </select>
              {/* 設定的商品已經下架時要說出來 —— 否則畫面看起來一切正常，
                  而開檯時默默不收錢，兩件事之間隔著好幾個小時。 */}
              {form.cover && !items.some((i) => i.id === form.cover) && (
                <p className="mt-1 text-xs text-amber-300">{t(ts.coverChargeMissing)}</p>
              )}
              <p className="mt-1 text-xs leading-relaxed text-slate-600">
                <Emphasis text={t(ts.coverChargeHint)} />
              </p>
            </label>
          </div>
        </Box>
      </div>

      <div className="flex items-center gap-3">
        <button
          className="rounded bg-emerald-700 px-5 py-2 text-sm hover:bg-emerald-600 disabled:opacity-40"
          disabled={busy}
          onClick={() => void save()}
        >
          {t(ui.save)}
        </button>
        {saved && <span className="text-sm text-emerald-400">{t(ts.saved)}</span>}
      </div>
    </div>
  )
}

function Box({
  title,
  wide,
  children,
}: {
  title: string
  wide?: boolean
  children: React.ReactNode
}) {
  return (
    <section
      className={`space-y-3 rounded border border-slate-800 bg-slate-900/40 p-4 ${
        wide ? 'md:col-span-2' : ''
      }`}
    >
      <h3 className="text-sm text-slate-400">{title}</h3>
      {children}
    </section>
  )
}

function Field({
  label,
  hint,
  value,
  onChange,
  numeric,
}: {
  label: string
  hint?: string
  value: string | undefined
  onChange: (v: string) => void
  numeric?: boolean
}) {
  return (
    <label className="block">
      <span className="mb-1 block text-xs text-slate-500">{label}</span>
      <input
        className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
        // inputMode 而不是 type="number"：後者在手機上會出現上下箭頭，
        // 而且輸入「1.」這種中間狀態時 value 會變成空字串。
        inputMode={numeric ? 'decimal' : undefined}
        value={value ?? ''}
        onChange={(e) => onChange(e.target.value)}
      />
      {hint && (
        <p className="mt-1 text-xs leading-relaxed text-slate-600">
          <Emphasis text={hint} />
        </p>
      )}
    </label>
  )
}
