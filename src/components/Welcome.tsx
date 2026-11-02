"use client";

import { useEffect, useState } from "react";
import { Settings } from "lucide-react";
import { desktop, type RecentDoc } from "@/lib/desktop";
import { timeAgo, useNow } from "@/lib/time";
import { ai, type AiView } from "@/lib/ai";
import { CliOffer } from "./CliInstall";

/** Desktop start screen: open a file or pick up a recent one. */
export function Welcome({ onOpen, onOpenSettings, error }: { onOpen: (path?: string) => void; onOpenSettings: () => void; error: string | null }) {
  const [recent, setRecent] = useState<RecentDoc[] | null>(null);
  const now = useNow(60_000);

  const [models, setModels] = useState<AiView | null>(null);

  useEffect(() => {
    desktop.recent().then(setRecent, () => setRecent([]));
    ai.state().then(setModels, () => setModels(null));
  }, []);

  const editor = models?.roles.editor;
  const provider = editor && models?.providers.find((p) => p.id === editor.provider);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div data-tauri-drag-region className="h-12 shrink-0 border-b border-rule" />
      <div className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-8">
        <div className="fade-in w-full max-w-[460px] py-16">
          <p className="font-serif text-[30px] leading-tight text-ink">Open a manuscript.</p>
          <p className="mt-3 max-w-[42ch] text-[13.5px] leading-relaxed text-ink-2">
            Doc Forge keeps its own history of every file you open, separate from any git repository the file already
            lives in. Nothing is written next to your files except your edits.
          </p>
          <div className="mt-7 flex items-center gap-4">
            <button
              onClick={() => onOpen()}
              className="press h-8 rounded-[3px] bg-ink px-4 text-[13px] font-medium text-paper hover:opacity-85"
            >
              Open file…
            </button>
            <span className="num text-[11.5px] text-graphite">⌘O</span>
          </div>
          {error && <p className="swap-fade mt-4 text-[12.5px] text-pencil-deep">{error}</p>}

          {recent && recent.length > 0 && (
            <div className="mt-12">
              <div className="label mb-2">Recent</div>
              <ul className="stagger border-t border-rule">
                {recent.slice(0, 8).map((r) => (
                  <li key={r.id} className="border-b border-rule">
                    <button
                      disabled={!r.exists}
                      onClick={() => onOpen(r.path)}
                      title={r.exists ? r.path : `Missing: ${r.path}`}
                      className="press group flex w-full origin-left items-baseline gap-3 py-2.5 text-left disabled:cursor-default disabled:opacity-45"
                    >
                      <span className="min-w-0 truncate text-[13.5px] text-ink-2 group-hover:text-ink group-disabled:text-ink-2">
                        {r.name}
                      </span>
                      <span className="num min-w-0 flex-1 truncate text-[11px] text-graphite">{r.folder}</span>
                      <span className="num shrink-0 text-[11px] text-faint">{r.exists ? timeAgo(r.openedAt, now) : "missing"}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </div>
      <footer className="flex h-8 shrink-0 items-center gap-4 border-t border-rule px-3 text-[11px] text-graphite">
        <button
          onClick={onOpenSettings}
          aria-label="Settings"
          title="Settings (⌘,)"
          className="press group grid size-6 place-items-center rounded-[3px] text-graphite hover:bg-well hover:text-ink"
        >
          <Settings size={13.5} strokeWidth={1.75} className="transition-transform duration-300 ease-[var(--ease-out)] group-hover:rotate-45" />
        </button>
        {models &&
          (editor ? (
            <span className="swap-fade num truncate text-faint">
              Forge · {editor.model.split("/").pop()}
              {provider ? ` on ${provider.name}` : ""}
            </span>
          ) : (
            <span className="swap-fade">Set up a model provider in Settings to work with Forge.</span>
          ))}
        <span className="ml-auto min-w-0">
          <CliOffer />
        </span>
      </footer>
    </div>
  );
}
