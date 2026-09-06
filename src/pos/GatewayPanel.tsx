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
    if (
      !confirm(
        `刪除「${g.displayName}」？\n已經收過的款不會受影響，之後不能再用這條線收款。`,
      )
    )
      return;
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
        <p className="font-medium">
          金流憑證存在這台電腦的資料庫裡，沒有加密。
        </p>
        <p className="mt-1 text-amber-200/70">
          也就是說：
          <span className="text-amber-100">
            備份出去的 .db 檔等於你的收款權限
          </span>
          。 請把它當成跟保險箱鑰匙一樣的東西保管，不要丟到公開的雲端資料夾。
        </p>
      </section>

      {error && (
        <p className="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}

      <div className="flex items-center gap-3">
        <h2 className="text-sm text-slate-400">已設定的金流</h2>
        <button
          className="rounded bg-emerald-800 px-3 py-1.5 text-sm hover:bg-emerald-700"
          onClick={() => setEditing("new")}
        >
          ＋ 新增
        </button>
      </div>

      {rows && rows.length === 0 && (
        <p className="rounded bg-slate-900/40 px-3 py-10 text-center text-sm text-slate-600">
          還沒有設定任何金流。
          <br />
          沒有設定也能營業 —— 現金與「自己抄授權碼的刷卡機」本來就不需要串接。
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
  return (
    <section className="rounded border border-slate-800 bg-slate-900/40 p-3">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="truncate font-medium">{g.displayName}</h3>
            {g.isActive ? (
              <span className="shrink-0 rounded bg-emerald-900/70 px-1.5 py-0.5 text-xs text-emerald-300">
                啟用中
              </span>
            ) : (
              <span className="shrink-0 rounded bg-slate-800 px-1.5 py-0.5 text-xs text-slate-500">
                停用
              </span>
            )}
            {g.isSandbox && (
              <span
                className="shrink-0 rounded bg-amber-900/60 px-1.5 py-0.5 text-xs text-amber-300"
                title="測試環境不會真的收到錢"
              >
                測試
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs text-slate-500">
            {g.providerLabel}
            {g.paymentMethodName && ` · 收款方式：${g.paymentMethodName}`}
          </p>
        </div>
        <button
          className="shrink-0 text-xs text-slate-400 hover:text-slate-200"
          onClick={onEdit}
        >
          設定
        </button>
        <button
          className="shrink-0 text-xs text-red-400/70 hover:text-red-300"
          onClick={onRemove}
        >
          刪除
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
                {f.isSet ? `••••${f.tail}` : "未設定"}
              </dd>
            </div>
          ))}
        </dl>
      )}

      {g.missing.length > 0 && (
        <p className="mt-2 text-xs text-amber-400/80">
          還缺 {g.missing.join("、")}，填完才能啟用。
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
        <h2 className="mb-4 text-lg">{gateway ? "設定金流" : "新增金流"}</h2>

        <div className="space-y-4">
          <label className="block">
            <span className="mb-1 block text-xs text-slate-500">金流商</span>
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
            <span className="mb-1 block text-xs text-slate-500">顯示名稱</span>
            <input
              className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="收銀員在結帳畫面上看到的就是這個"
            />
          </label>

          <label className="block">
            <span className="mb-1 block text-xs text-slate-500">
              對應的收款方式
            </span>
            <select
              className="w-full rounded bg-slate-800 px-3 py-2 text-sm"
              value={methodId}
              onChange={(e) => setMethodId(e.target.value)}
            >
              <option value="">（不綁定）</option>
              {methods.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
            <p className="mt-1 text-xs text-slate-500">
              綁定之後，結帳選這個收款方式就會走這條線。日結報表上也照這個分類。
            </p>
          </label>

          {fields.length > 0 && (
            <fieldset className="space-y-3 rounded border border-slate-800 p-3">
              <legend className="px-1 text-xs text-slate-500">憑證</legend>
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
                        ? `已設定 ••••${f.tail}（留空不修改）`
                        : "尚未設定"
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
                測試環境
                <span className="ml-1 text-xs text-slate-500">
                  （不會真的收到錢）
                </span>
              </span>
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={active}
                onChange={(e) => setActive(e.target.checked)}
              />
              <span>啟用</span>
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
            取消
          </button>
          <button
            className="rounded bg-emerald-700 px-4 py-2 text-sm hover:bg-emerald-600 disabled:opacity-40"
            disabled={busy}
            onClick={() => void save()}
          >
            儲存
          </button>
        </div>
      </div>
    </div>
  );
}
