"use client";

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { diffBlocks, diffStats, htmlToBlocks, type DiffRow } from "@/lib/diff";
import { dayLabel, timeline } from "@/lib/history";
import type { Version } from "@/lib/types";
import { clockTime, useNow } from "@/lib/time";
import { FirstChange } from "./DiffView";

interface Props {
  versions: Version[];
  previewId: string | null;
  onPreview: (id: string | null) => void;
  onRestore: (v: Version) => void;
  /** Skip the entrance when opened from the keyboard. */
  animateIn: boolean;
}

const OPEN_DELAY = 180;
const CLOSE_DELAY = 140;
const POP_W = 320;

export function HistoryRail({ versions, previewId, onPreview, onRestore, animateIn }: Props) {
  const now = useNow(60_000);
  const nodes = useMemo(() => timeline(versions), [versions]);
  const currentId = versions[versions.length - 1]?.id;

  // Per-version diff against its predecessor.
  const diffs = useMemo(() => {
    const m = new Map<string, { rows: DiffRow[]; added: number; removed: number }>();
    const blocks = versions.map((v) => htmlToBlocks(v.html));
    versions.forEach((v, i) => {
      const rows = diffBlocks(i > 0 ? blocks[i - 1] : [], blocks[i]);
      m.set(v.id, { rows, ...diffStats(rows) });
    });
    return m;
  }, [versions]);

  // ---- Popover state: delayed first open, instant (gliding) switches, quick exit.
  const navRef = useRef<HTMLElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const [shownId, setShownId] = useState<string | null>(null); // content (kept during exit)
  const [open, setOpen] = useState(false);
  const [instant, setInstant] = useState(true);
  const [anchorY, setAnchorY] = useState(0);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const openTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const openRef = useRef(false);
  useEffect(() => {
    openRef.current = open;
  }, [open]);

  useEffect(
    () => () => {
      clearTimeout(openTimer.current);
      clearTimeout(closeTimer.current);
    },
    [],
  );

  const present = (id: string, el: HTMLElement) => {
    clearTimeout(closeTimer.current);
    clearTimeout(openTimer.current);
    const r = el.getBoundingClientRect();
    const reveal = () => {
      setAnchorY(r.top + r.height / 2);
      setShownId(id);
      setOpen(true);
    };
    if (openRef.current) {
      setInstant(false); // already open: glide to the new tick, no delay
      reveal();
    } else {
      setInstant(true); // first open: appear in place, after a short intent delay
      openTimer.current = setTimeout(reveal, OPEN_DELAY);
    }
  };

  const dismiss = (delay = CLOSE_DELAY) => {
    clearTimeout(openTimer.current);
    clearTimeout(closeTimer.current);
    closeTimer.current = setTimeout(() => setOpen(false), delay);
  };
  const hold = () => clearTimeout(closeTimer.current);

  // Place beside the rail, vertically centred on the tick, clamped to the viewport.
  useLayoutEffect(() => {
    if (!shownId) return;
    const nav = navRef.current?.getBoundingClientRect();
    const h = popRef.current?.offsetHeight ?? 240;
    const x = Math.min((nav?.right ?? 0) + 8, window.innerWidth - POP_W - 12);
    const y = Math.max(12, Math.min(anchorY - 30, window.innerHeight - h - 12));
    setPos({ x, y });
  }, [shownId, anchorY]);

  const shown = shownId ? versions.find((v) => v.id === shownId) : null;
  const sd = shown ? diffs.get(shown.id) : null;

  return (
    <nav
      ref={navRef}
      aria-label="Version history"
      onMouseLeave={() => dismiss()}
      className={`${animateIn ? "rail-enter" : ""} w-11 shrink-0 overflow-y-auto overflow-x-hidden pb-16 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden pl-6 pt-[62px] @3xl:pt-[76px] @min-[808px]:absolute @min-[808px]:inset-y-0 @min-[808px]:left-0 @min-[808px]:z-[5]`}
    >
      <ol className="relative">
        {/* The spine */}
        <span aria-hidden className="absolute bottom-[7px] left-[3.5px] top-[7px] w-px bg-rule-2" />
        {nodes.map(({ version: v, space }, i) => {
          // Invisible hit area: full rail width, reaching halfway to each neighbour (max 14px).
          const reachUp = i === 0 ? 14 : Math.min(14, space / 2);
          const reachDown = Math.min(14, (nodes[i + 1]?.space ?? 28) / 2);
          const selected = previewId === v.id;
          const lit = open && shownId === v.id;
          const agent = v.author === "agent";
          const current = v.id === currentId;
          return (
            <li key={v.id} className="relative" style={{ marginTop: space }}>
              <button
                aria-label={`v${v.n}: ${v.label}, ${clockTime(v.createdAt)}`}
                aria-current={selected}
                onMouseEnter={(e) => present(v.id, e.currentTarget)}
                onFocus={(e) => present(v.id, e.currentTarget)}
                onBlur={() => dismiss()}
                onClick={() => {
                  onPreview(selected ? null : v.id);
                  dismiss(0);
                }}
                className="group relative flex h-[14px] w-full items-center focus-visible:outline-none [&:focus-visible_.node]:outline [&:focus-visible_.node]:outline-[1.5px] [&:focus-visible_.node]:outline-offset-[3px] [&:focus-visible_.node]:outline-ink"
              >
                <span aria-hidden className="absolute -left-6 right-0" style={{ top: -reachUp, bottom: -reachDown }} />
                <span className="tick-grow grid size-2 place-items-center" style={{ ["--i" as string]: i }}>
                  <span
                    className="node block size-2 rounded-full"
                    data-strong={selected || lit}
                    data-current={current}
                    style={{
                      background: agent ? "var(--pencil)" : "var(--ink-2)",
                      opacity: previewId && !selected ? 0.4 : 1,
                    }}
                  />
                </span>
              </button>
            </li>
          );
        })}
      </ol>

      {/* Portalled: a transformed ancestor would otherwise trap position:fixed and clip it. */}
      {shown &&
        sd &&
        createPortal(
          <div
            ref={popRef}
            role="tooltip"
            data-open={open}
            data-instant={instant}
            onMouseEnter={hold}
            onMouseLeave={() => dismiss()}
            className="popover rounded-[4px] border border-rule bg-paper p-4 shadow-[var(--shadow-pop)]"
            style={{ width: POP_W, left: 0, translate: `${pos.x}px ${pos.y}px` }}
          >
            <div key={shown.id} className={instant ? undefined : "swap-fade"}>
              <div className="flex items-baseline gap-2">
                <span className="num text-[12px] font-medium text-ink">v{shown.n}</span>
                <span className={`label text-[9.5px] ${shown.author === "agent" ? "text-pencil" : ""}`}>
                  {shown.author === "agent" ? "Forge" : "You"}
                </span>
                <span className="num ml-auto text-[10.5px] text-graphite">
                  {dayLabel(shown.createdAt, now)}, {clockTime(shown.createdAt)}
                </span>
              </div>
              <p className="mt-1.5 text-[13.5px] leading-snug text-ink">{shown.label}</p>
              <p className="num mt-1 text-[11px] text-graphite">
                +{sd.added} −{sd.removed} words
              </p>
              <p className="mt-3 border-t border-rule pt-3 font-serif text-[14px] leading-[1.6] text-ink-2">
                <FirstChange rows={sd.rows} />
              </p>
              <div className="mt-3.5 flex items-center gap-5 text-[12.5px]">
                <button
                  onClick={() => {
                    onPreview(previewId === shown.id ? null : shown.id);
                    dismiss(0);
                  }}
                  className="tbtn-ink"
                >
                  {previewId === shown.id ? "Close comparison" : "Show changes"}
                </button>
                {shown.id !== currentId && (
                  <button
                    onClick={() => {
                      onRestore(shown);
                      dismiss(0);
                    }}
                    className="tbtn-pencil"
                  >
                    Restore
                  </button>
                )}
              </div>
            </div>
          </div>,
          document.body,
        )}
    </nav>
  );
}
