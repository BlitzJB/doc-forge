"use client";

import { useEffect, useRef, useState } from "react";
import { dayLabel } from "@/lib/history";
import type { ThreadMeta } from "@/lib/threads";

interface Props {
  threads: ThreadMeta[];
  currentId: string;
  /** The current thread's title ("" while it's empty). */
  title: string;
  disabled: boolean;
  onOpen: (id: string) => void;
  onDelete: (id: string) => void;
}

function when(ts: number, now: number) {
  const day = dayLabel(ts, now);
  const time = new Date(ts).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  return day === "Today" ? time : day;
}

/** The thread title in the margin's header; opens the document's earlier threads. */
export function ThreadMenu({ threads, currentId, title, disabled, onOpen, onDelete }: Props) {
  const [open, setOpen] = useState(false);
  const [now, setNow] = useState(0);
  const [confirm, setConfirm] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const others = threads.filter((t) => t.id !== currentId);
  const hasMenu = others.length > 0 || threads.some((t) => t.id === currentId);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  // An empty thread is just "Editor": the margin itself.
  const label = title || "Editor";

  return (
    <div ref={root} className="relative min-w-0">
      <button
        type="button"
        disabled={disabled || !hasMenu}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => {
          setNow(Date.now());
          setConfirm(null);
          setOpen((o) => !o);
        }}
        className="group flex min-w-0 max-w-full items-center gap-1.5 text-left disabled:cursor-default"
        title={label}
      >
        <span key={label} className="swap-in truncate text-[13px] font-medium text-ink">
          {label}
        </span>
        {hasMenu && (
          <svg
            aria-hidden
            viewBox="0 0 10 10"
            className={`size-2.5 shrink-0 text-faint transition-transform duration-200 group-hover:text-ink group-disabled:opacity-0 ${open ? "rotate-180" : ""}`}
          >
            <path d="M2 3.75 5 6.75 8 3.75" fill="none" stroke="currentColor" strokeWidth="1.2" />
          </svg>
        )}
      </button>

      {open && (
        <div
          role="menu"
          className="picker-pop absolute left-0 top-[calc(100%+12px)] z-30 w-[300px] rounded-[4px] border border-rule bg-paper py-2 shadow-[var(--shadow-pop)]"
        >
          <div className="label px-4 pb-2 pt-1">Threads on this document</div>
          <ul className="max-h-[360px] overflow-y-auto">
            {threads.map((t) => {
              const current = t.id === currentId;
              return (
                <li key={t.id} className="group/row relative">
                  <button
                    role="menuitem"
                    onClick={() => {
                      setOpen(false);
                      if (!current) onOpen(t.id);
                    }}
                    className={`flex w-full items-start gap-2.5 px-4 py-2 pr-16 text-left transition-colors duration-150 hover:bg-margin ${current ? "bg-margin" : ""}`}
                  >
                    <span aria-hidden className={`mt-[7px] size-[5px] shrink-0 rounded-full ${current ? "bg-ink" : "bg-transparent"}`} />
                    <span className="min-w-0">
                      <span className="block truncate font-[family-name:var(--doc-font)] text-[14px] leading-snug text-ink">{t.title || "Untitled"}</span>
                      <span className="num mt-0.5 block text-[10.5px] text-faint">
                        {when(t.updatedAt, now)} · {t.asks} {t.asks === 1 ? "ask" : "asks"}
                      </span>
                    </span>
                  </button>
                  <button
                    onClick={() => {
                      if (confirm === t.id) {
                        setConfirm(null);
                        onDelete(t.id);
                      } else setConfirm(t.id);
                    }}
                    onBlur={() => setConfirm((c) => (c === t.id ? null : c))}
                    aria-label={confirm === t.id ? `Confirm delete ${t.title}` : `Delete ${t.title}`}
                    className={`tbtn absolute right-3 top-2.5 text-[11.5px] transition-opacity duration-150 ${confirm === t.id ? "text-pencil-deep opacity-100 hover:text-pencil-deep" : "opacity-0 focus-visible:opacity-100 group-hover/row:opacity-100"}`}
                  >
                    {confirm === t.id ? "Delete?" : "Delete"}
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
