import { useCallback, useEffect, useState } from "react";

import {
  gatewayApi,
  orderApi,
  type AppError,
  type Gateway,
  type GatewayInput,
  type PaymentMethod,
  type ProviderDef,
} from "@/shared/api";
import { useT } from "@/shared/i18n";
// 字典取名 gw 而不是 gateway：Editor 的 prop 就叫 gateway，同名會把字典遮掉。
import { gateway as gw } from "@/shared/locales/gateway";
import { ui } from "@/shared/locales/nav";

/**
 * 金流設定。
 *
 * # 這一頁最重要的一句話寫在畫面上，不是寫在這裡
 *
 * 「憑證存在資料庫裡，沒有加密」—— 直接印給店家看。
 *
 * 假裝它被加密了比誠實地說出來更危險：店家會以為備份檔可以隨便丟雲端。
 * 而一個知道「這個 .db 等於我的收款權限」的老闆，會自己把它收好。
 *
 * # 憑證只進不出
 *
 * 欄位裡永遠不會有明文 —— 已經設定過的顯示末四碼當 placeholder，
 * 沒有動它就送空字串，後端沿用原本存的值。所以「改個測試環境的勾勾」
 * 不需要把 secret 重打一次，而 secret 也不會躺在 webview 的記憶體裡。
 */
export default function GatewayPanel() {
  const t = useT();
  const [rows, setRows] = useState<Gateway[] | null>(null);
  const [methods, setMethods] = useState<PaymentMethod[]>([]);
  const [providers, setProviders] = useState<ProviderDef[]>([]);
  const [editing, setEditing] = useState<Gateway | "new" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [g, m, p] = await Promise.all([
        gatewayApi.list(),
        orderApi.paymentMethods(),
        gatewayApi.providers(),
      ]);
      setRows(g);
      setMethods(m);
      setProviders(p);
      setError(null);
    } catch (e) {
      setError((e as AppError).message ?? String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const remove = async (g: Gateway) => {
    if (!confirm(t(gw.deleteConfirm, { name: g.displayName }))) return;
    try {
      await gatewayApi.remove(g.id);
      await load();
    } catch (e) {
      setError((e as AppError).message ?? String(e));
    }
  };

  return (
    <div className="h-full space-y-4 overflow-y-auto pr-2">
      <section className="rounded border border-amber-800/60 bg-amber-950/30 px-4 py-3 text-sm text-amber-200/90">
        <p className="font-medium">{t(gw.warnTitle)}</p>
        <p className="mt-1 text-amber-200/70">
          {t(gw.warnLead)}
          <span className="text-amber-100">{t(gw.warnBackup)}</span>
          {t(gw.warnTail)}
        </p>
      </section>

      {error && (
        <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}

      <div className="flex items-center gap-3">
        <h2 className="text-sm text-slate-400">{t(gw.configured)}</h2>
        <button
          className="rounded bg-emerald-800 px-3 py-1.5 text-sm hover:bg-emerald-700"
          onClick={() => setEditing("new")}
        >
          ＋ {t(ui.add)}
        </button>
      </div>

      {rows && rows.length === 0 && (
        <p className="rounded bg-slate-900/40 px-3 py-10 text-center text-sm text-slate-600">
          {t(gw.emptyTitle)}
          <br />
          {t(gw.emptyHint)}
        </p>
      )}

      <div className="grid gap-3 md:grid-cols-2">
        {(rows ?? []).map((g) => (
          <Card
            key={g.id}
            g={g}
            onEdit={() => setEditing(g)}
            onRemove={() => void remove(g)}
          />
        ))}
      </div>

      {editing && (
        <Editor
          key={editing === "new" ? "new" : editing.id}
          gateway={editing === "new" ? null : editing}
          methods={methods}
          providers={providers}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            void load();
          }}
        />
      )}
    </div>
  );
}

function Card({
  g,
  onEdit,
  onRemove,
}: {
  g: Gateway;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  return (
    <section className="rounded border border-slate-800 bg-slate-900/40 p-3">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="truncate font-medium">{g.displayName}</h3>
            {g.isActive ? (
              <span className="shrink-0 rounded bg-emerald-900/70 px-1.5 py-0.5 text-xs text-emerald-300">
                {t(gw.active)}
              </span>
            ) : (
              <span className="shrink-0 rounded bg-slate-800 px-1.5 py-0.5 text-xs text-slate-500">
                {t(gw.inactive)}
              </span>
            )}
            {g.isSandbox && (
              <span
                className="shrink-0 rounded bg-amber-900/60 px-1.5 py-0.5 text-xs text-amber-300"
                title={t(gw.sandboxTip)}
              >
                {t(gw.sandboxBadge)}
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs text-slate-500">
            {g.providerLabel}
            {g.paymentMethodName &&
              ` · ${t(gw.methodOf, { name: g.paymentMethodName })}`}
          </p>
        </div>
        <button
          className="shrink-0 text-xs text-slate-400 hover:text-slate-200"
          onClick={onEdit}
        >
          {t(ui.edit)}
        </button>
        <button
          className="shrink-0 text-xs text-red-400/70 hover:text-red-300"
          onClick={onRemove}
        >
          {t(ui.delete)}
        </button>
      </div>

      {g.fields.length > 0 && (
        <dl className="mt-2 space-y-0.5 border-t border-slate-800 pt-2 text-xs">
          {g.fields.map((f) => (
            <div key={f.key} className="flex justify-between">
              <dt className="text-slate-500">{f.label}</dt>
              <dd
                className={
                  f.isSet ? "font-mono text-slate-400" : "text-amber-400/80"
                }
              >
                {f.isSet ? `••••${f.tail}` : t(gw.fieldUnset)}
              </dd>
            </div>
          ))}
        </dl>
      )}

      {g.missing.length > 0 && (
        <p className="mt-2 text-xs text-amber-400/80">
          {/* 缺的欄位名稱是後端給的（憑證欄位定義只有後端一份），這裡不另外翻，
              但串起來的頓號要翻 —— 頓號是中日文的標點，英文清單得用逗號。 */}
          {t(gw.missing, { fields: g.missing.join(t(gw.fieldSeparator)) })}
        </p>
      )}
    </section>
  );
}

function Editor({
  gateway,
  methods,
  providers,
  onClose,
  onSaved,
}: {
  gateway: Gateway | null;
  methods: PaymentMethod[];
  providers: ProviderDef[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const t = useT();
  const [provider, setProvider] = useState(
    gateway?.provider ?? providers[0]?.code ?? "manual",
  );
  const [displayName, setDisplayName] = useState(
    gateway?.displayName ?? providers[0]?.label ?? "",
  );
  const [methodId, setMethodId] = useState(gateway?.paymentMethodId ?? "");
  const [sandbox, setSandbox] = useState(gateway?.isSandbox ?? true);
  const [active, setActive] = useState(gateway?.isActive ?? false);
  const [creds, setCreds] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const def = providers.find((p) => p.code === provider);
  // 已經存在的那一筆用它自己的 fields（帶末四碼）；新增的用後端給的空白定義。
  const fields =
    gateway?.provider === provider ? gateway.fields : (def?.fields ?? []);

  const save = async () => {
    setBusy(true);
    try {
      const input: GatewayInput = {
        id: gateway?.id ?? null,
        provider,
        displayName: displayName.trim(),
        paymentMethodId: methodId || null,
        isSandbox: sandbox,
        isActive: active,
        // 只送真的打了字的欄位。空字串代表「沒動它」，後端沿用原本存的值。
        credentials: Object.fromEntries(
          Object.entries(creds).filter(([, v]) => v.trim() !== ""),
        ),
      };
      await gatewayApi.upsert(input);
      onSaved();
    } catch (e) {
      setError((e as AppError).message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-4"
      onClick={onClose}
    >
      <div
        className="max-h-full w-full max-w-lg overflow-y-auto rounded-lg border border-slate-700 bg-slate-900 p-5"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="mb-4 text-lg">
          {gateway ? t(gw.editTitle) : t(gw.newTitle)}
        </h2>

        <div className="space-y-4">
          <label className="block">
            <span className="mb-1 block text-xs text-slate-500">
              {t(gw.provider)}
            </span>
            <select
              className="w-full rounded bg-slate-800 px-3 py-2 text-sm disabled:opacity-60"
              value={provider}
              // 換金流商等於換一整組憑證欄位。已經存在的那一筆不給改 ——
              // 讓人重新新增一筆，比讓他以為舊的憑證還在裡面安全。
              disabled={!!gateway}
              onChange={(e) => {
                const code = e.target.value;
                setProvider(code);
                setCreds({});
                const p = providers.find((x) => x.code === code);
                if (p) setDisplayName(p.label);
              }}
            >
              {providers.map((p) => (
                <option key={p.code} value={p.code}>
                  {p.label}
                </option>
              ))}
            </select>
            {def && <p className="mt-1 text-xs text-slate-500">{def.note}</p>}
          </label>

          <label className="block">
            <span className="mb-1 block text-xs text-slate-500">
              {t(gw.displayName)}
            </span>
            <input
              className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder={t(gw.displayNameHint)}
            />
          </label>

          <label className="block">
            <span className="mb-1 block text-xs text-slate-500">
              {t(gw.methodField)}
            </span>
            <select
              className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
              value={methodId}
              onChange={(e) => setMethodId(e.target.value)}
            >
              <option value="">{t(gw.methodNone)}</option>
              {methods.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
            <p className="mt-1 text-xs text-slate-500">{t(gw.methodHint)}</p>
          </label>

          {fields.length > 0 && (
            <fieldset className="space-y-3 rounded border border-slate-800 p-3">
              <legend className="px-1 text-xs text-slate-500">
                {t(gw.credentials)}
              </legend>
              {fields.map((f) => (
                <label key={f.key} className="block">
                  <span className="mb-1 block text-xs text-slate-400">
                    {f.label}
                  </span>
                  <input
                    className="w-full rounded bg-slate-800 px-3 py-2 font-mono text-sm"
                    type="password"
                    autoComplete="off"
                    value={creds[f.key] ?? ""}
                    onChange={(e) =>
                      setCreds({ ...creds, [f.key]: e.target.value })
                    }
                    // 已經設定過的顯示末四碼 —— 夠讓人確認「是不是我貼的那一組」，
                    // 又不足以拿去用。留空就是不動它。
                    placeholder={
                      f.isSet
                        ? t(gw.credSet, { tail: f.tail ?? "" })
                        : t(gw.credUnset)
                    }
                  />
                  <p className="mt-1 text-xs text-slate-600">{f.hint}</p>
                </label>
              ))}
            </fieldset>
          )}

          <div className="flex flex-wrap gap-4">
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={sandbox}
                onChange={(e) => setSandbox(e.target.checked)}
              />
              <span>
                {t(gw.sandboxLabel)}
                <span className="ml-1 text-xs text-slate-500">
                  {t(gw.sandboxNote)}
                </span>
              </span>
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={active}
                onChange={(e) => setActive(e.target.checked)}
              />
              <span>{t(gw.enable)}</span>
            </label>
          </div>

          {error && (
            <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
              {error}
            </p>
          )}
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <button
            className="rounded bg-slate-800 px-4 py-2 text-sm"
            onClick={onClose}
          >
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
  );
}
