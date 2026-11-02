"use client";

import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { Check as CheckIcon, ChevronDown, Search } from "lucide-react";
import { ai, formatContext, type AiView, type Check, type ModelInfo, type ProviderView, type RoleChoice, type RoleName, type SearchView, type TestResult } from "@/lib/ai";
import { isDesktop } from "@/lib/desktop";
import { Proofread, Scribe } from "./Loaders";
import { Heading, Row } from "./SettingsParts";

/** Settings → Models: which model does which job, and the providers behind them. */
export function ModelsSettings() {
  const [view, setView] = useState<AiView | null>(null);
  const [checking, setChecking] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const desktop = isDesktop();

  const refresh = useCallback(async () => {
    try {
      setView(await ai.state());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const runCheck = useCallback(
    async (id: string) => {
      setChecking((s) => new Set(s).add(id));
      try {
        await ai.check(id);
      } finally {
        setChecking((s) => {
          const n = new Set(s);
          n.delete(id);
          return n;
        });
        await refresh();
      }
    },
    [refresh],
  );

  // Load, then verify every provider that could be live, in parallel.
  useEffect(() => {
    if (!desktop) return;
    let cancelled = false;
    (async () => {
      const v = await ai.state().catch((e) => {
        setError(String(e));
        return null;
      });
      if (!v || cancelled) return;
      setView(v);
      v.providers
        .filter((p) => p.keySource !== "none" || !p.info.needsKey)
        .filter((p) => p.baseUrl)
        .forEach((p) => void runCheck(p.id));
    })();
    return () => {
      cancelled = true;
    };
  }, [desktop, runCheck]);

  if (!desktop) {
    return (
      <>
        <Heading title="Models" sub="Forge runs on your own provider keys." />
        <p className="border-t border-rule pt-5 text-[13px] leading-relaxed text-ink-2">
          Providers and keys are configured in the desktop app, where keys are kept in a private file on this Mac.
        </p>
      </>
    );
  }

  return (
    <>
      <Heading
        title="Models"
        sub="Forge runs on your own provider keys. Keys stay in a private file on this Mac and are only sent to the provider they belong to."
      />
      {error && <p className="mb-4 text-[12.5px] text-pencil-deep">{error}</p>}
      {view ? (
        <>
          <Roles view={view} onChange={refresh} />
          <div className="label mb-1 mt-9">Providers</div>
          <ul className="border-t border-rule">
            {view.providers.map((p) => (
              <ProviderRow key={p.id} p={p} checking={checking.has(p.id)} onCheck={() => runCheck(p.id)} onChange={refresh} />
            ))}
          </ul>
          <AddCustom
            onAdded={async (id) => {
              await refresh();
              void runCheck(id);
            }}
          />
          <div className="label mb-1 mt-10">Web search</div>
          <p className="mb-2 text-[12px] leading-snug text-graphite">
            Used when Web search is on in the margin. Without a search key, Forge can still read pages you link to.
          </p>
          <ul className="border-t border-rule">
            {view.search.map((s) => (
              <SearchRow key={s.kind} s={s} onChange={refresh} />
            ))}
          </ul>
        </>
      ) : (
        <div className="flex items-center gap-2 border-t border-rule pt-5 text-[12.5px] text-graphite">
          <Proofread /> Loading providers
        </div>
      )}
    </>
  );
}

// ---- Roles --------------------------------------------------------------------

function Roles({ view, onChange }: { view: AiView; onChange: () => void }) {
  const groups = useMemo(
    () =>
      view.providers
        .filter((p) => p.check?.status === "connected" && p.check.models.length)
        .map((p) => ({ provider: p, models: p.check!.models })),
    [view],
  );
  const lookup = (c: RoleChoice | null) => {
    if (!c) return null;
    const provider = view.providers.find((p) => p.id === c.provider);
    const model = provider?.check?.models.find((m) => m.id === c.model);
    return { provider, model, choice: c };
  };
  const set = async (role: RoleName, c: RoleChoice | null) => {
    await ai.setRole(role, c);
    onChange();
  };
  const editor = lookup(view.roles.editor);

  return (
    <>
      <Row label="Editor" hint="Drafts, edits and research. Needs tool use.">
        <ModelPicker role="editor" value={view.roles.editor} groups={groups} providers={view.providers} onChange={(c) => set("editor", c)} />
        {editor?.model && editor.model.tools === false && (
          <p className="swap-fade mt-2 text-[12px] text-pencil-deep">{"This model can’t use tools, so Forge won’t be able to edit with it."}</p>
        )}
      </Row>
      <Row label="Quick" hint="Titles and short summaries. Uses the Editor model unless you pick one.">
        <ModelPicker role="quick" value={view.roles.quick} groups={groups} providers={view.providers} inheritLabel="Same as Editor" onChange={(c) => set("quick", c)} />
      </Row>
      <Row label="Test" hint="A short round trip through the Editor model.">
        <TestLine choice={view.roles.editor} />
      </Row>
    </>
  );
}

function TestLine({ choice }: { choice: RoleChoice | null }) {
  const [state, setState] = useState<{ running: boolean; result: TestResult | null; error: string | null }>({ running: false, result: null, error: null });
  const key = choice ? `${choice.provider}/${choice.model}` : "";
  const [lastKey, setLastKey] = useState(key);
  if (key !== lastKey) {
    setLastKey(key);
    setState({ running: false, result: null, error: null });
  }
  const run = async () => {
    if (!choice) return;
    setState({ running: true, result: null, error: null });
    try {
      setState({ running: false, result: await ai.test(choice.provider, choice.model), error: null });
    } catch (e) {
      setState({ running: false, result: null, error: String(e) });
    }
  };
  const r = state.result;
  return (
    <div className="flex min-h-7 flex-wrap items-center gap-x-4 gap-y-1.5 text-[12.5px]">
      <button onClick={run} disabled={!choice || state.running} className="tbtn-ink disabled:opacity-35">
        Send a test message
      </button>
      {state.running && (
        <span className="flex items-center gap-2 text-graphite">
          <Scribe /> Waiting for a reply
        </span>
      )}
      {r && r.status === "connected" && (
        <span key={r.latencyMs} className="swap-fade flex flex-wrap items-baseline gap-x-2 text-ink-2">
          <span className="font-serif text-[14px] text-ink">“{r.reply.length > 60 ? r.reply.slice(0, 60) + "…" : r.reply}”</span>
          <span className="num text-[11px] text-graphite">
            {(r.latencyMs / 1000).toFixed(1)} s
            {r.usage.outputTokens != null && ` · ${r.usage.outputTokens} tokens`}
            {r.usage.reasoningTokens ? `, ${r.usage.reasoningTokens} reasoning` : ""}
          </span>
        </span>
      )}
      {r && r.status !== "connected" && <span className="swap-fade text-pencil-deep">{r.message}</span>}
      {state.error && <span className="swap-fade text-pencil-deep">{state.error}</span>}
      {!choice && <span className="text-graphite">Choose an Editor model first.</span>}
    </div>
  );
}

// ---- Model picker -------------------------------------------------------------

interface Group {
  provider: ProviderView;
  models: ModelInfo[];
}

function ModelPicker({
  role,
  value,
  groups,
  providers,
  inheritLabel,
  onChange,
}: {
  role: RoleName;
  value: RoleChoice | null;
  groups: Group[];
  providers: ProviderView[];
  inheritLabel?: string;
  onChange: (c: RoleChoice | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [cursor, setCursor] = useState(0);
  const wrap = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const listId = useId();

  const flat = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const items: { provider: ProviderView; model: ModelInfo | null }[] = [];
    if (inheritLabel && !needle) items.push({ provider: providers[0], model: null });
    for (const g of groups)
      for (const m of g.models) if (!needle || m.name.toLowerCase().includes(needle) || m.id.toLowerCase().includes(needle)) items.push({ provider: g.provider, model: m });
    return items;
  }, [groups, q, inheritLabel, providers]);

  // Move focus into the search field on open (autoFocus is unreliable inside a modal dialog).
  useEffect(() => {
    if (open) input.current?.focus();
  }, [open]);
  // Every close starts the next search fresh.
  const [wasOpen, setWasOpen] = useState(open);
  if (wasOpen !== open) {
    setWasOpen(open);
    if (!open) {
      setQ("");
      setCursor(0);
    }
  }

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !wrap.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  const current = value ? groups.find((g) => g.provider.id === value.provider)?.models.find((m) => m.id === value.model) : null;
  const currentProvider = value ? providers.find((p) => p.id === value.provider) : null;
  const choose = (i: number) => {
    const it = flat[i];
    if (!it) return;
    onChange(it.model ? { provider: it.provider.id, model: it.model.id } : null);
    setOpen(false);
    setQ("");
    trigger.current?.focus();
  };

  return (
    <div ref={wrap} className="relative">
      <button
        ref={trigger}
        onClick={() => {
          setOpen((o) => !o);
          setCursor(0);
        }}
        aria-haspopup="listbox"
        aria-expanded={open}
        className="press flex h-9 w-full items-center gap-2.5 rounded-[4px] border border-rule bg-paper px-3 text-left transition-colors duration-150 hover:border-rule-2"
      >
        {value ? (
          <>
            <span className="num min-w-0 truncate text-[12.5px] text-ink">{current?.name ?? value.model.split("/").pop()}</span>
            <span className="shrink-0 text-[11.5px] text-graphite">{currentProvider?.name}</span>
            {current && <Caps m={current} />}
          </>
        ) : (
          <span className="text-[12.5px] text-graphite">{inheritLabel ?? "Choose a model"}</span>
        )}
        <ChevronDown size={14} className={`ml-auto shrink-0 text-graphite transition-transform duration-200 ${open ? "rotate-180" : ""}`} />
      </button>

      {open && (
        <div className="picker-pop absolute left-0 right-0 top-[calc(100%+4px)] z-20 overflow-hidden rounded-[5px] border border-rule bg-paper shadow-[var(--shadow-pop)]">
          <div className="flex items-center gap-2 border-b border-rule px-3">
            <Search size={13} className="text-graphite" />
            <input
              ref={input}
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                setCursor(0);
              }}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setCursor((c) => Math.min(flat.length - 1, c + 1));
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setCursor((c) => Math.max(0, c - 1));
                } else if (e.key === "Enter") {
                  e.preventDefault();
                  choose(cursor);
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  setOpen(false);
                  trigger.current?.focus();
                }
              }}
              placeholder="Search models"
              aria-controls={listId}
              aria-label={`Search ${role} models`}
              className="h-9 min-w-0 flex-1 bg-transparent text-[12.5px] text-ink outline-none placeholder:text-faint"
            />
          </div>
          <ul id={listId} role="listbox" className="max-h-[300px] overflow-y-auto py-1">
            {groups.length === 0 && <li className="px-3 py-3 text-[12.5px] text-graphite">Connect a provider below to choose a model.</li>}
            {groups.length > 0 && flat.length === 0 && <li className="px-3 py-3 text-[12.5px] text-graphite">No models match.</li>}
            {flat.map((it, i) => {
              const prev = flat[i - 1];
              const header = it.model && (!prev || !prev.model || prev.provider.id !== it.provider.id) && groups.length > 1;
              const selected = it.model ? value?.provider === it.provider.id && value?.model === it.model.id : !value;
              return (
                <li key={it.model ? `${it.provider.id}:${it.model.id}` : "inherit"}>
                  {header && <div className="label px-3 pb-1 pt-2.5 text-[9.5px]">{it.provider.name}</div>}
                  <button
                    role="option"
                    aria-selected={selected}
                    onMouseEnter={() => setCursor(i)}
                    onClick={() => choose(i)}
                    className={`flex w-full items-center gap-2.5 px-3 py-1.5 text-left ${cursor === i ? "bg-well" : ""}`}
                  >
                    <span className="grid w-3 shrink-0 place-items-center">{selected && <CheckIcon size={12} className="text-ink" />}</span>
                    {it.model ? (
                      <>
                        <span className="num min-w-0 truncate text-[12.5px] text-ink">{it.model.name}</span>
                        <Caps m={it.model} />
                      </>
                    ) : (
                      <span className="text-[12.5px] text-ink-2">{inheritLabel}</span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </div>
  );
}

/** Capability marks: context window, tools, vision, router. Only what the provider reports. */
function Caps({ m }: { m: ModelInfo }) {
  const ctx = formatContext(m.contextLength);
  return (
    <span className="ml-auto flex shrink-0 items-center gap-1.5">
      {m.router && <Cap>router</Cap>}
      {m.vision && <Cap>vision</Cap>}
      {m.tools === true && <Cap>tools</Cap>}
      {m.tools === false && <Cap muted>no tools</Cap>}
      {ctx && <Cap>{ctx}</Cap>}
    </span>
  );
}

function Cap({ children, muted }: { children: ReactNode; muted?: boolean }) {
  return (
    <span className={`num rounded-[3px] border px-1 py-px text-[9.5px] leading-tight ${muted ? "border-pencil-soft text-pencil-deep" : "border-rule text-graphite"}`}>
      {children}
    </span>
  );
}

// ---- Providers ------------------------------------------------------------------

function statusLine(p: ProviderView, checking: boolean): { tone: "ok" | "bad" | "none"; text: string } {
  if (checking) return { tone: "none", text: "Checking" };
  const c = p.check;
  if (c?.status === "connected") return { tone: "ok", text: `Connected · ${c.models.length} models · ${c.latencyMs} ms` };
  if (c?.status === "unauthorized") return { tone: p.keySource === "none" ? "none" : "bad", text: p.keySource === "none" ? "No key" : "Key rejected" };
  if (c?.status === "unreachable") return { tone: p.info.local ? "none" : "bad", text: p.info.local ? "Not running" : "Not reachable" };
  if (c?.status === "error") return { tone: "bad", text: c.message };
  if (p.info.needsKey && p.keySource === "none") return { tone: "none", text: "No key" };
  return { tone: "none", text: "Not checked" };
}

function ProviderRow({ p, checking, onCheck, onChange }: { p: ProviderView; checking: boolean; onCheck: () => void; onChange: () => void }) {
  const [open, setOpen] = useState(false);
  const s = statusLine(p, checking);
  const keyLine =
    p.keySource === "stored" ? `Key ${p.keyHint ?? ""} saved` : p.keySource === "environment" ? `Key ${p.keyHint ?? ""} from ${p.info.envVar}` : null;

  return (
    <li className="border-b border-rule">
      <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="group flex w-full items-center gap-3 py-3.5 text-left">
        <span
          aria-hidden
          className={`size-[7px] shrink-0 rounded-full transition-colors duration-300 ${s.tone === "ok" ? "bg-add" : s.tone === "bad" ? "bg-pencil" : "bg-rule-2"}`}
        />
        <span className="w-[112px] shrink-0 truncate text-[13px] font-medium text-ink">{p.name}</span>
        <span className="flex min-w-0 flex-1 items-center gap-2 text-[12px] text-graphite">
          {checking && <Proofread />}
          <span key={s.text} className={`swap-fade truncate ${s.tone === "bad" ? "text-pencil-deep" : ""}`}>
            {s.text}
          </span>
        </span>
        {keyLine && <span className="num hidden shrink-0 text-[10.5px] text-faint min-[760px]:inline">{keyLine}</span>}
        <span className="shrink-0 text-[12px] text-graphite group-hover:text-ink">{open ? "Close" : p.keySource === "none" && p.info.needsKey ? "Add key" : "Edit"}</span>
      </button>
      <div className="reveal" data-open={open}>
        <div>{open && <ProviderEditor p={p} onCheck={onCheck} onChange={onChange} onDone={() => setOpen(false)} />}</div>
      </div>
    </li>
  );
}

function ProviderEditor({ p, onCheck, onChange, onDone }: { p: ProviderView; onCheck: () => void; onChange: () => void; onDone: () => void }) {
  const [key, setKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ tone: "ok" | "bad"; text: string } | null>(null);
  const [base, setBase] = useState(p.baseUrl);
  const takesKey = p.info.needsKey || p.kind === "custom";

  const save = async () => {
    setSaving(true);
    setMsg(null);
    try {
      const c: Check = await ai.saveKey(p.id, key);
      if (c.status === "connected") {
        setKey("");
        setMsg({ tone: "ok", text: `Saved. ${c.models.length} models available.` });
        onChange();
      } else {
        setMsg({ tone: "bad", text: c.message + " The key was not saved." });
      }
    } catch (e) {
      setMsg({ tone: "bad", text: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const saveBase = async (value: string) => {
    if (value.trim().replace(/\/+$/, "") === p.baseUrl) return;
    try {
      await ai.setBaseUrl(p.id, value);
      onChange();
      onCheck();
    } catch (e) {
      setMsg({ tone: "bad", text: String(e) });
    }
  };

  return (
    <div className="flex flex-col gap-4 pb-5 pl-[19px]">
      {takesKey && (
        <div>
          <div className="flex items-center gap-2">
            <input
              type="password"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && key.trim() && void save()}
              placeholder={p.keySource === "none" ? p.info.keyPlaceholder || "API key" : "Paste a new key to replace it"}
              aria-label={`${p.name} API key`}
              autoComplete="off"
              spellCheck={false}
              className="num h-8 min-w-0 flex-1 rounded-[4px] border border-rule bg-paper px-2.5 text-[12px] text-ink outline-none transition-colors placeholder:text-faint focus:border-ink-2"
            />
            <button
              onClick={save}
              disabled={!key.trim() || saving}
              className="press h-8 shrink-0 rounded-[3px] bg-ink px-3 text-[12px] font-medium text-paper hover:opacity-85 disabled:opacity-25"
            >
              Save & verify
            </button>
          </div>
          <div className="mt-2 flex min-h-5 flex-wrap items-center gap-x-4 gap-y-1 text-[12px]">
            {saving && (
              <span className="flex items-center gap-2 text-graphite">
                <Proofread /> Verifying with {p.name}
              </span>
            )}
            {msg && <span className={`swap-fade ${msg.tone === "ok" ? "text-ink-2" : "text-pencil-deep"}`}>{msg.text}</span>}
            {!saving && !msg && p.keySource === "environment" && (
              <span className="text-graphite">Using {p.info.envVar} from the environment. A key saved here takes precedence.</span>
            )}
            {p.info.keyUrl && (
              <button onClick={() => void ai.openUrl(p.info.keyUrl!)} className="tbtn">
                Get a key ↗
              </button>
            )}
            {p.keySource === "stored" && (
              <button
                onClick={async () => {
                  await ai.forgetKey(p.id);
                  onChange();
                  onCheck();
                }}
                className="tbtn"
              >
                Forget key
              </button>
            )}
          </div>
        </div>
      )}

      <label className="flex flex-col gap-1.5">
        <span className="text-[11.5px] text-graphite">Address{p.info.local ? " (running on this Mac)" : ""}</span>
        <span className="flex items-center gap-2">
          <input
            value={base}
            onChange={(e) => setBase(e.target.value)}
            onBlur={() => void saveBase(base)}
            onKeyDown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
            spellCheck={false}
            className="num h-8 min-w-0 flex-1 rounded-[4px] border border-rule bg-paper px-2.5 text-[12px] text-ink-2 outline-none transition-colors focus:border-ink-2"
          />
          {p.info.defaultBaseUrl && p.baseUrl !== p.info.defaultBaseUrl && (
            <button
              onClick={() => {
                setBase(p.info.defaultBaseUrl);
                void saveBase("");
              }}
              className="tbtn shrink-0 text-[12px]"
            >
              Reset
            </button>
          )}
        </span>
      </label>

      <div className="flex items-center gap-5 text-[12px]">
        <button onClick={onCheck} className="tbtn-ink">
          Check connection
        </button>
        {p.removable && (
          <button
            onClick={async () => {
              await ai.removeProvider(p.id);
              onDone();
              onChange();
            }}
            className="tbtn"
          >
            Remove provider
          </button>
        )}
      </div>
    </div>
  );
}

function AddCustom({ onAdded }: { onAdded: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const add = async () => {
    try {
      const id = await ai.addCustom(name, url);
      setOpen(false);
      setName("");
      setUrl("");
      setErr(null);
      onAdded(id);
    } catch (e) {
      setErr(String(e));
    }
  };
  return (
    <div className="pt-3">
      {!open ? (
        <button onClick={() => setOpen(true)} className="tbtn text-[12.5px]">
          + Add an OpenAI-compatible provider
        </button>
      ) : (
        <div className="swap-fade flex flex-col gap-2.5">
          <div className="flex items-center gap-2">
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Name"
              aria-label="Provider name"
              className="h-8 w-[140px] rounded-[4px] border border-rule bg-paper px-2.5 text-[12px] text-ink outline-none placeholder:text-faint focus:border-ink-2"
            />
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void add()}
              placeholder="https://gateway.example.com/v1"
              aria-label="Provider address"
              spellCheck={false}
              className="num h-8 min-w-0 flex-1 rounded-[4px] border border-rule bg-paper px-2.5 text-[12px] text-ink outline-none placeholder:text-faint focus:border-ink-2"
            />
            <button onClick={add} disabled={!url.trim()} className="press h-8 rounded-[3px] bg-ink px-3 text-[12px] font-medium text-paper hover:opacity-85 disabled:opacity-25">
              Add
            </button>
            <button onClick={() => setOpen(false)} className="tbtn text-[12px]">
              Cancel
            </button>
          </div>
          {err && <span className="text-[12px] text-pencil-deep">{err}</span>}
        </div>
      )}
    </div>
  );
}

function SearchRow({ s, onChange }: { s: SearchView; onChange: () => void }) {
  const [open, setOpen] = useState(false);
  const [key, setKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ tone: "ok" | "bad"; text: string } | null>(null);
  const [signing, setSigning] = useState(false);
  const has = s.keySource !== "none" || s.signedIn;
  const keyLine =
    s.keySource === "stored"
      ? `Key ${s.keyHint ?? ""} saved`
      : s.keySource === "environment"
        ? `Key ${s.keyHint ?? ""} from ${s.envVar}`
        : null;

  const signIn = async () => {
    setSigning(true);
    setMsg(null);
    try {
      const n = await ai.searchSignIn(s.kind);
      setMsg({ tone: "ok", text: `Signed in. A test search returned ${n} result${n === 1 ? "" : "s"}.` });
      onChange();
    } catch (e) {
      setMsg({ tone: "bad", text: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setSigning(false);
    }
  };

  const save = async () => {
    setSaving(true);
    setMsg(null);
    try {
      const n = await ai.saveSearchKey(s.kind, key);
      setKey("");
      setMsg({ tone: "ok", text: `Saved. A test search returned ${n} result${n === 1 ? "" : "s"}.` });
      onChange();
    } catch (e) {
      setMsg({ tone: "bad", text: `${String(e).replace(/^Error:\s*/, "")} The key was not saved.` });
    } finally {
      setSaving(false);
    }
  };

  return (
    <li className="border-b border-rule">
      <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="group flex w-full items-center gap-3 py-3.5 text-left">
        <span aria-hidden className={`size-[7px] shrink-0 rounded-full transition-colors duration-300 ${has ? "bg-add" : "bg-rule-2"}`} />
        <span className="w-[112px] shrink-0 truncate text-[13px] font-medium text-ink">{s.label}</span>
        <span className="min-w-0 flex-1 truncate text-[12px] text-graphite">{s.signedIn && s.keySource === "none" ? "Signed in" : has ? "Ready" : "No key"}</span>
        {keyLine && <span className="num hidden shrink-0 text-[10.5px] text-faint min-[760px]:inline">{keyLine}</span>}
        <span className="shrink-0 text-[12px] text-graphite group-hover:text-ink">{open ? "Close" : has ? "Edit" : s.signIn ? "Connect" : "Add key"}</span>
      </button>
      <div className="reveal" data-open={open}>
        <div>
          {open && (
            <div className="pb-5 pl-[19px]">
              {s.signIn && (
                <div className="mb-3 flex flex-wrap items-center gap-x-4 gap-y-2">
                  {s.signedIn ? (
                    <>
                      <span className="text-[12px] text-ink-2">Signed in through the browser.</span>
                      <button
                        onClick={async () => {
                          await ai.searchSignOut(s.kind);
                          setMsg(null);
                          onChange();
                        }}
                        className="tbtn"
                      >
                        Sign out
                      </button>
                    </>
                  ) : signing ? (
                    <>
                      <span className="flex items-center gap-2 text-[12px] text-graphite">
                        <Proofread /> Finish signing in in your browser
                      </span>
                      <button onClick={() => void ai.cancelSignIn()} className="tbtn">
                        Cancel
                      </button>
                    </>
                  ) : (
                    <>
                      <button
                        onClick={signIn}
                        className="press h-8 shrink-0 rounded-[3px] bg-ink px-3 text-[12px] font-medium text-paper hover:opacity-85"
                      >
                        Sign in with {s.label}
                      </button>
                      <span className="text-[12px] text-graphite">or paste an API key</span>
                    </>
                  )}
                </div>
              )}
              <div className="flex items-center gap-2">
                <input
                  type="password"
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && key.trim() && void save()}
                  placeholder={s.keySource !== "none" ? "Paste a new key to replace it" : s.keyPlaceholder}
                  aria-label={`${s.label} API key`}
                  autoComplete="off"
                  spellCheck={false}
                  className="num h-8 min-w-0 flex-1 rounded-[4px] border border-rule bg-paper px-2.5 text-[12px] text-ink outline-none transition-colors placeholder:text-faint focus:border-ink-2"
                />
                <button
                  onClick={save}
                  disabled={!key.trim() || saving}
                  className="press h-8 shrink-0 rounded-[3px] bg-ink px-3 text-[12px] font-medium text-paper hover:opacity-85 disabled:opacity-25"
                >
                  Save & verify
                </button>
              </div>
              <div className="mt-2 flex min-h-5 flex-wrap items-center gap-x-4 gap-y-1 text-[12px]">
                {saving && (
                  <span className="flex items-center gap-2 text-graphite">
                    <Proofread /> Running a test search
                  </span>
                )}
                {msg && <span className={`swap-fade ${msg.tone === "ok" ? "text-ink-2" : "text-pencil-deep"}`}>{msg.text}</span>}
                <button onClick={() => void ai.openUrl(s.keyUrl)} className="tbtn">
                  Get a key ↗
                </button>
                {s.keySource === "stored" && (
                  <button
                    onClick={async () => {
                      await ai.forgetSearchKey(s.kind);
                      onChange();
                    }}
                    className="tbtn"
                  >
                    Forget key
                  </button>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </li>
  );
}
