"use client";

import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowUp, Square } from "lucide-react";
import type { EditMode, Message, Part, SearchResult, Suggestion } from "@/lib/types";
import { minimalChange } from "@/lib/suggestions-extension";
import { clockTime } from "@/lib/time";
import { Segmented } from "./Segmented";
import { Activity, Proofread, Scribe } from "./Loaders";
import { ThreadMenu } from "./ThreadMenu";
import type { ThreadMeta } from "@/lib/threads";

interface Props {
  desktop?: boolean;
  /** Re-send a prompt (with its original passage) after a failed run. */
  onRetry?: (prompt: string, quote: string | null) => void;
  onOpenSettings?: () => void;
  messages: Message[];
  suggestions: Suggestion[];
  running: boolean;
  mode: EditMode;
  setMode: (m: EditMode) => void;
  webSearch: boolean;
  setWebSearch: (b: boolean) => void;
  quote: string | null;
  clearQuote: () => void;
  activeId: string | null;
  setActiveId: (id: string | null) => void;
  isStale: (s: Suggestion) => boolean;
  onSend: (text: string) => void;
  onStop: () => void;
  threads: ThreadMeta[];
  threadId: string;
  threadTitle: string;
  onNewThread: () => void;
  onOpenThread: (id: string) => void;
  onDeleteThread: (id: string) => void;
  onAccept: (id: string) => void;
  onReject: (id: string) => void;
  onAcceptMany: (ids: string[]) => void;
  onLocate: (id: string) => void;
}

function activity(messages: Message[], running: boolean) {
  if (!running) return "idle";
  const last = messages[messages.length - 1]?.parts.at(-1);
  if (!last) return "considering";
  if (last.type === "reasoning" && !last.done) return "considering";
  if (last.type === "tool" && last.status === "running") return last.tool === "web_search" ? "searching" : "reading";
  return "writing";
}

export function ChatPanel(p: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [p.messages]);

  const state = activity(p.messages, p.running);

  return (
    <aside className="flex w-[340px] shrink-0 flex-col border-l border-rule bg-margin lg:w-[380px] xl:w-[420px]">
      <header data-tauri-drag-region className="flex h-12 shrink-0 items-center gap-3 whitespace-nowrap border-b border-rule px-5 xl:px-6">
        <ThreadMenu
          threads={p.threads}
          currentId={p.threadId}
          title={p.threadTitle}
          disabled={p.running}
          onOpen={p.onOpenThread}
          onDelete={p.onDeleteThread}
        />
        <span className="num flex shrink-0 items-center gap-1.5 text-[11.5px] text-graphite">
          {p.running && state !== "idle" && <Activity key={`mark-${state}`} kind={state} className="swap-fade" />}
          <span key={state} className="swap-in">{state}</span>
        </span>
        {p.messages.length > 0 && (
          <button onClick={p.onNewThread} disabled={p.running} className="tbtn fade-in ml-auto shrink-0 text-[12.5px]">New thread</button>
        )}
      </header>

      <div
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
        className="min-h-0 flex-1 overflow-y-auto"
      >
        {p.messages.length === 0 ? (
          <EmptyState />
        ) : (
          <div className="flex flex-col gap-9 px-6 pb-10 pt-7">
            {p.messages.map((m) =>
              m.role === "user" ? (
                <UserEntry key={m.id} m={m} />
              ) : (
                <AgentEntry key={m.id} m={m}>
                  {m.parts.map((part, i) => (
                    <PartView key={i} part={part} caret={!!m.streaming && i === m.parts.length - 1} p={p} />
                  ))}
                </AgentEntry>
              ),
            )}
          </div>
        )}
      </div>

      <Composer {...p} />
    </aside>
  );
}

// ---------------------------------------------------------------------------

function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center px-6 text-center">
      <p className="font-serif text-[21px] leading-snug text-ink">The margin is empty.</p>
      <p className="mt-3 max-w-[34ch] text-balance text-[13.5px] leading-relaxed text-ink-2">
        Ask for an edit, a source, or a second opinion. Changes arrive in the manuscript as numbered marks for you to accept or decline.
      </p>
    </div>
  );
}

function EntryHead({ who, at }: { who: string; at: number }) {
  return (
    <div className="label mb-2.5 flex gap-2">
      <span className={who === "Forge" ? "text-pencil" : ""}>{who}</span>
      <span className="text-faint">{clockTime(at)}</span>
    </div>
  );
}

function UserEntry({ m }: { m: Message }) {
  const text = m.parts[0]?.type === "text" ? m.parts[0].text : "";
  return (
    <div className="fade-in">
      <EntryHead who="You" at={m.createdAt} />
      {m.quote && <Quotation text={m.quote} />}
      <p className="whitespace-pre-wrap font-serif text-[15.5px] leading-[1.55] text-ink">{text}</p>
    </div>
  );
}

function Quotation({ text, clamp = 3 }: { text: string; clamp?: number }) {
  return (
    <div className="relative mb-2.5 pl-4">
      <span aria-hidden className="absolute left-0 top-[-3px] font-serif text-[20px] leading-none text-pencil">“</span>
      <p className="font-serif text-[13.5px] italic leading-relaxed text-graphite" style={{ display: "-webkit-box", WebkitLineClamp: clamp, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
        {text}
      </p>
    </div>
  );
}

function AgentEntry({ m, children }: { m: Message; children: ReactNode }) {
  return (
    <div className="fade-in">
      <EntryHead who="Forge" at={m.createdAt} />
      <div className="flex flex-col gap-3.5">
        {children}
        {m.streaming && m.parts.length === 0 && (
          <span className="flex items-center gap-2.5 text-[12.5px] text-graphite"><Scribe /> Considering</span>
        )}
      </div>
    </div>
  );
}

function PartView({ part, caret, p }: { part: Part; caret: boolean; p: Props }) {
  switch (part.type) {
    case "reasoning":
      return <Reasoning part={part} />;
    case "text":
      return <Prose text={part.text} caret={caret} />;
    case "tool":
      return part.tool === "web_search" ? (
        <SearchLine part={part} />
      ) : part.tool === "fetch_url" ? (
        <PageLine part={part} />
      ) : part.tool === "history" ? (
        <HistoryLine part={part} />
      ) : (
        <ReadLine part={part} />
      );
    case "error":
      return (
        <div className="swap-fade flex flex-wrap items-baseline gap-x-4 gap-y-1 text-[12.5px]">
          <span className="text-pencil-deep">{part.message}</span>
          {part.retry && p.onRetry && (
            <button onClick={() => p.onRetry!(part.retry!.prompt, part.retry!.quote)} disabled={p.running} className="tbtn-ink disabled:opacity-35">
              Try again
            </button>
          )}
        </div>
      );
    case "setup":
      return (
        <div className="swap-fade flex flex-wrap items-baseline gap-x-4 gap-y-1 text-[13px] text-ink-2">
          <span>{part.message}</span>
          {p.onOpenSettings && (
            <button onClick={p.onOpenSettings} className="tbtn-ink">
              Open Settings
            </button>
          )}
        </div>
      );
    case "suggestions": {
      const items = part.ids.map((id) => p.suggestions.find((s) => s.id === id)).filter(Boolean) as Suggestion[];
      return <Marks items={items} p={p} />;
    }
  }
}

// ---- Process ---------------------------------------------------------------

function Reasoning({ part }: { part: Extract<Part, { type: "reasoning" }> }) {
  const [open, setOpen] = useState(false);
  if (!part.done)
    return (
      <span className="flex items-center gap-2.5 text-[12.5px] text-graphite"><Scribe /> Considering</span>
    );
  return (
    <div>
      <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="tbtn swap-fade flex items-center gap-1.5 text-[12.5px]">
        Considered for <span className="num">{part.seconds}s</span>
        <span className="num inline-block w-2 text-[11px] transition-transform duration-200 ease-[var(--ease-out)]" style={{ transform: open ? "rotate(180deg)" : "none" }}>
          {open ? "−" : "+"}
        </span>
      </button>
      <div className="reveal" data-open={open}>
        <div>
          <p className="pt-2 font-serif text-[13.5px] italic leading-relaxed text-graphite">{part.text}</p>
        </div>
      </div>
    </div>
  );
}

function ToolRow({ verb, running, children }: { verb: string; running: boolean; children: ReactNode }) {
  return (
    <div className="flex items-baseline gap-3 text-[12.5px]">
      <span className="label w-[52px] shrink-0">{verb}</span>
      <span className="flex min-w-0 items-center gap-2 text-ink-2">
        {running && <Proofread className="shrink-0" />}
        {children}
      </span>
    </div>
  );
}

/** A web page Forge read (fetch_url). */
function PageLine({ part }: { part: Extract<Part, { type: "tool" }> }) {
  const running = part.status === "running";
  const src = part.results?.[0];
  let host = part.query ?? "";
  try {
    host = new URL(part.query ?? "").hostname.replace(/^www\./, "");
  } catch {}
  return (
    <div>
      <ToolRow verb="Read" running={running}>
        <span className="truncate">{src?.title ?? host}</span>
        {!running && (
          <span className={`num shrink-0 text-[11.5px] ${part.ok === false ? "text-pencil-deep" : "text-graphite"}`}>
            · {part.ok === false ? part.summary : host}
          </span>
        )}
      </ToolRow>
    </div>
  );
}

/** Forge looking through saved versions (list_versions / read_version). */
function HistoryLine({ part }: { part: Extract<Part, { type: "tool" }> }) {
  const running = part.status === "running";
  return (
    <ToolRow verb="History" running={running}>
      <span className="truncate">{part.query}</span>
      {!running && part.summary && (
        <span className={`num shrink-0 text-[11.5px] ${part.ok === false ? "text-pencil-deep" : "text-graphite"}`}>· {part.summary}</span>
      )}
    </ToolRow>
  );
}

function ReadLine({ part }: { part: Extract<Part, { type: "tool" }> }) {
  const running = part.status === "running";
  return (
    <ToolRow verb="Read" running={running}>
      {running ? "The manuscript" : <>The manuscript <span className="num text-[11.5px] text-graphite">· {part.summary}</span></>}
    </ToolRow>
  );
}

function SearchLine({ part }: { part: Extract<Part, { type: "tool" }> }) {
  const running = part.status === "running";
  return (
    <div>
      <ToolRow verb="Search" running={running}>
        <span className="truncate">“{part.query}”</span>
        {!running && part.ok === false && <span className="num shrink-0 text-[11.5px] text-pencil-deep">· {part.summary}</span>}
      </ToolRow>
      {part.results && part.results.length > 0 && (
        <ol className="mt-2 ml-[64px] flex flex-col">
          {part.results.map((r, i) => (
            <Source key={r.url} r={r} n={r.n ?? i + 1} />
          ))}
        </ol>
      )}
    </div>
  );
}

function Source({ r, n }: { r: SearchResult; n: number }) {
  const [open, setOpen] = useState(false);
  return (
    <li className="fade-in border-t border-rule first:border-t-0" style={{ animationDelay: `${n * 60}ms` }}>
      <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="press group flex w-full origin-left items-baseline gap-2 py-1.5 text-left">
        <span className="num w-3 shrink-0 text-[10.5px] text-graphite">{n}</span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[12.5px] text-ink-2 group-hover:text-ink">{r.title}</span>
          <span className="num block text-[10.5px] text-graphite">{r.domain}</span>
          <span className="reveal" data-open={open}>
            <span className="block">
              <span className="mt-1 block font-serif text-[13px] leading-relaxed text-graphite">{r.snippet}</span>
            </span>
          </span>
        </span>
      </button>
    </li>
  );
}

// ---- Marks (proposed edits) ------------------------------------------------

function Marks({ items, p }: { items: Suggestion[]; p: Props }) {
  const open = items.filter((s) => s.status === "pending" && !p.isStale(s));
  return (
    <div className="mt-1">
      <ol className="stagger border-t border-rule">
        {items.map((s) => (
          <Mark key={s.id} s={s} n={p.suggestions.indexOf(s) + 1} p={p} />
        ))}
      </ol>
      {open.length > 1 && (
        <div className="fade-in flex items-center justify-end gap-5 pt-3 text-[12.5px]">
          <button onClick={() => open.forEach((s) => p.onReject(s.id))} className="tbtn">Decline all</button>
          <button onClick={() => p.onAcceptMany(open.map((s) => s.id))} className="tbtn-pencil">
            Accept all <span className="num">{open.length}</span>
          </button>
        </div>
      )}
    </div>
  );
}

const STATUS: Record<string, string> = {
  accepted: "Accepted",
  applied: "Applied directly",
  rejected: "Declined",
  stale: "Passage has changed",
};

function Mark({ s, n, p }: { s: Suggestion; n: number; p: Props }) {
  const stale = p.isStale(s);
  const live = s.status === "pending" && !stale;
  const active = live && p.activeId === s.id;
  const { deleted, inserted, prefix, suffix } = minimalChange(s.find, s.replace);
  const lead = s.find.slice(0, prefix);
  const tail = s.find.slice(s.find.length - suffix);
  const clip = (t: string, from: "start" | "end") =>
    t.length <= 36 ? t : from === "start" ? "…" + t.slice(t.length - 36) : t.slice(0, 36) + "…";
  const status = stale ? "stale" : s.status;

  return (
    <li
      onMouseEnter={() => live && p.setActiveId(s.id)}
      onMouseLeave={() => p.activeId === s.id && p.setActiveId(null)}
      className={`-mx-3 flex gap-3 border-b border-rule px-3 py-3.5 transition-colors duration-200 ${active ? "bg-paper" : ""}`}
    >
      <span className={`num w-4 shrink-0 pt-px text-[11px] font-medium transition-colors duration-300 ${live ? "text-pencil" : "text-faint"}`}>{n}</span>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-3">
          <span className={`min-w-0 flex-1 text-[12.5px] transition-colors duration-300 ${live ? "text-ink-2" : "text-graphite"}`}>{s.reason}</span>
          {live && (
            <button onClick={() => p.onLocate(s.id)} className="tbtn shrink-0 text-[11.5px]">Find</button>
          )}
        </div>
        {s.kind === "insert" ? (
          <div className={`mt-1.5 font-serif text-[14.5px] leading-[1.6] transition-opacity duration-300 ${live ? "" : "opacity-60"}`}>
            <p className="text-[12.5px] text-graphite">
              <span className="font-sans">New paragraph after</span> “{clip(s.find, "start")}”
            </p>
            {s.replace
              .split(/\n{2,}/)
              .filter((t) => t.trim())
              .map((t, i) => (
                <p key={i} className="mt-1.5">
                  <ins className="diff-ins">{t}</ins>
                </p>
              ))}
          </div>
        ) : (
          <p className={`mt-1.5 font-serif text-[14.5px] leading-[1.6] transition-opacity duration-300 ${live ? "" : "opacity-60"}`}>
            {lead && <span className="text-graphite">{clip(lead, "start")}</span>}
            {deleted && <del className="diff-del">{deleted}</del>}
            {deleted && inserted && " "}
            {inserted && <ins className="diff-ins">{inserted}</ins>}
            {tail && <span className="text-graphite">{clip(tail, "end")}</span>}
          </p>
        )}
        <div key={live ? "live" : status} className="swap-fade mt-2.5 flex items-center gap-5 text-[12.5px]">
          {live ? (
            <>
              <button onClick={() => p.onAccept(s.id)} className="tbtn-pencil">Accept</button>
              <button onClick={() => p.onReject(s.id)} className="tbtn">Decline</button>
            </>
          ) : (
            <span className="label">{STATUS[status]}</span>
          )}
        </div>
      </div>
    </li>
  );
}

// ---- Prose -----------------------------------------------------------------

function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(\*\*[^*]+\*\*|\*[^*]+\*|\s?\[\d+\])/g;
  let last = 0;
  let k = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const t = m[0];
    if (t.startsWith("**")) out.push(<strong key={k++} className="font-medium text-ink">{t.slice(2, -2)}</strong>);
    else if (t.trim().startsWith("[")) out.push(<sup key={k++} className="cite">{t.trim().slice(1, -1)}</sup>);
    else out.push(<em key={k++} className="font-serif italic">{t.slice(1, -1)}</em>);
    last = m.index + t.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

function Prose({ text, caret }: { text: string; caret: boolean }) {
  const blocks = text.split(/\n\n+/);
  return (
    <div className="flex flex-col gap-2.5 text-[13.5px] leading-[1.65] text-ink-2">
      {blocks.map((b, i) => {
        const lines = b.split("\n");
        const tail = caret && i === blocks.length - 1 ? <span className="caret" /> : null;
        if (lines.every((l) => l.startsWith("- ")))
          return (
            <ul key={i} className="flex flex-col gap-1.5">
              {lines.map((l, j) => (
                <li key={j} className="relative pl-4">
                  <span className="absolute left-0 top-[0.8em] h-px w-2 bg-graphite" />
                  {inline(l.slice(2))}
                  {j === lines.length - 1 && tail}
                </li>
              ))}
            </ul>
          );
        return (
          <p key={i}>
            {lines.map((l, j) => (
              <Fragment key={j}>
                {j > 0 && <br />}
                {inline(l)}
              </Fragment>
            ))}
            {tail}
          </p>
        );
      })}
    </div>
  );
}

// ---- Composer --------------------------------------------------------------

function Composer(p: Props) {
  const [text, setText] = useState("");
  const ta = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = Math.min(el.scrollHeight, 200) + "px";
  }, [text]);

  const submit = () => {
    if (!text.trim() || p.running) return;
    p.onSend(text.trim());
    setText("");
  };

  return (
    <div className="shrink-0 border-t border-rule bg-margin px-6 pb-4 pt-4 transition-colors duration-200 focus-within:border-rule-2 focus-within:bg-paper">
      {p.quote && (
        <div className="fade-in flex items-start gap-3">
          <div className="min-w-0 flex-1"><Quotation text={p.quote} clamp={2} /></div>
          <button onClick={p.clearQuote} className="tbtn text-[11.5px]">Remove</button>
        </div>
      )}
      <textarea
        id="composer"
        ref={ta}
        rows={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            submit();
          }
        }}
        placeholder={p.quote ? "What should change in this passage?" : "Write a note to the editor"}
        className="block w-full resize-none bg-transparent font-serif text-[15.5px] leading-[1.55] text-ink outline-none placeholder:italic placeholder:text-faint focus-visible:outline-none"
      />
      <div className="mt-3 flex items-center gap-4 whitespace-nowrap text-[12px] xl:gap-5">
        <Segmented
          label="Edit mode"
          value={p.mode}
          onChange={p.setMode}
          options={[
            { value: "suggest", label: "Suggest" },
            { value: "auto", label: "Apply directly" },
          ]}
        />
        <span className="h-3.5 w-px bg-rule-2" />
        <button
          role="switch"
          aria-checked={p.webSearch}
          onClick={() => p.setWebSearch(!p.webSearch)}
          className="press flex items-center gap-1.5 text-graphite hover:text-ink"
        >
          <span className={`grid size-3 place-items-center rounded-[2px] border transition-colors duration-150 ${p.webSearch ? "border-ink bg-ink" : "border-rule-2"}`}>
            <span className="check-dot size-1 bg-paper" data-on={p.webSearch} />
          </span>
          Web search
        </button>
        <button
          onClick={p.running ? p.onStop : submit}
          disabled={!p.running && !text.trim()}
          aria-label={p.running ? "Stop" : "Send"}
          className="press morph ml-auto size-7 place-items-center rounded-[3px] bg-ink text-paper hover:opacity-85 disabled:opacity-20"
        >
          <span className="grid place-items-center" data-on={!p.running}>
            <ArrowUp size={14} strokeWidth={2} />
          </span>
          <span className="grid place-items-center" data-on={p.running}>
            <Square size={9} fill="currentColor" />
          </span>
        </button>
      </div>
    </div>
  );
}
