"use client";

import { EditorContent, useEditorState, type Editor } from "@tiptap/react";
import { BubbleMenu } from "@tiptap/react/menus";
import {
  Bold,
  Highlighter,
  Italic,
  List,
  ListOrdered,
  Quote,
  Redo2,
  Settings,
  Strikethrough,
  Underline,
  Undo2,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Suggestion, Version } from "@/lib/types";
import { useNow, timeAgo } from "@/lib/time";
import { diffBlocks, diffStats, htmlToBlocks } from "@/lib/diff";
import { HistoryRail } from "./HistoryRail";
import { DiffDocument } from "./DiffView";
import { Segmented } from "./Segmented";

interface Props {
  editor: Editor | null;
  /** Running in the Tauri shell: leave room for the window controls, make the header draggable. */
  desktop?: boolean;
  folder: string;
  title: string;
  titleEditable?: boolean;
  /** Full path of the open file (desktop). */
  path?: string;
  saveError?: string | null;
  onOpenFile?: () => void;
  setTitle: (t: string) => void;
  versionCount: number;
  savedAt: number | null;
  pending: Suggestion[];
  versions: Version[];
  currentHtml: string;
  previewId: string | null;
  onPreview: (id: string | null) => void;
  onRestore: (v: Version) => void;
  onAcceptAll: () => void;
  onRejectAll: () => void;
  onAsk: (selectedText: string) => void;
  onOpenSettings: () => void;
}

export function EditorPane(props: Props) {
  const { editor, versions, previewId } = props;
  const now = useNow();
  const n = props.pending.length;
  const [compareTo, setCompareTo] = useState<"previous" | "current">("previous");
  const [historyOpen, setHistoryOpen] = useState(() => {
    try {
      return localStorage.getItem("forge:history") === "1";
    } catch {
      return false;
    }
  });
  // Opening by keyboard skips the entrance: shortcuts should feel instant.
  const [railAnimated, setRailAnimated] = useState(false);
  const toggleHistory = (fromKeyboard = false) => {
    const next = !historyOpen;
    setRailAnimated(!fromKeyboard);
    setHistoryOpen(next);
    if (!next) props.onPreview(null);
    try {
      localStorage.setItem("forge:history", next ? "1" : "0");
    } catch {}
  };
  // ⌥H toggles the rail.
  const toggleRef = useRef(toggleHistory);
  useEffect(() => {
    toggleRef.current = toggleHistory;
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey && e.code === "KeyH") {
        e.preventDefault();
        toggleRef.current(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const [onlyChanges, setOnlyChanges] = useState(false);

  const preview = previewId ? versions.find((v) => v.id === previewId) ?? null : null;
  const prevVersion = preview ? versions[versions.findIndex((v) => v.id === preview.id) - 1] ?? null : null;
  const isCurrent = preview?.id === versions[versions.length - 1]?.id;
  const rows = useMemo(() => {
    if (!preview) return [];
    if (compareTo === "current") return diffBlocks(htmlToBlocks(preview.html), htmlToBlocks(props.currentHtml));
    return diffBlocks(prevVersion ? htmlToBlocks(prevVersion.html) : [], htmlToBlocks(preview.html));
  }, [preview, prevVersion, compareTo, props.currentHtml]);

  return (
    <div className="@container flex min-h-0 flex-1 flex-col">
      <header
        data-tauri-drag-region
        className={`flex h-12 shrink-0 items-center gap-4 whitespace-nowrap border-b border-rule px-5 @3xl:gap-6 @3xl:px-6 ${props.desktop ? "pl-[84px] @3xl:pl-[84px]" : ""}`}
      >
        <div data-tauri-drag-region className="flex min-w-0 flex-1 items-baseline gap-2 text-[13px]" title={props.path}>
          <span className="hidden text-graphite @2xl:inline">{props.folder}</span>
          <span className="hidden text-faint @2xl:inline">/</span>
          {props.titleEditable === false ? (
            <span className="min-w-0 truncate font-medium text-ink">{props.title}</span>
          ) : (
            <input
              aria-label="Document title"
              value={props.title}
              onChange={(e) => props.setTitle(e.target.value)}
              className="min-w-0 max-w-full truncate bg-transparent font-medium text-ink outline-none [field-sizing:content]"
            />
          )}
          {props.onOpenFile && (
            <button onClick={props.onOpenFile} title="Open another file (⌘O)" className="tbtn ml-2 hidden text-[12.5px] @3xl:inline">
              Open…
            </button>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-4 text-[13px] @3xl:gap-6">
          {n > 0 && (
            <div className="fade-in flex items-center gap-4">
              <span className="text-ink-2">
                <span key={n} className="num swap-in text-pencil">{n}</span> {n === 1 ? "mark" : "marks"}
                <span className="hidden @4xl:inline"> to review</span>
              </span>
              <button onClick={props.onRejectAll} className="tbtn hidden @3xl:inline">Decline all</button>
              <button onClick={props.onAcceptAll} className="tbtn-pencil">Accept all</button>
              <span className="hidden h-4 w-px bg-rule @3xl:block" />
            </div>
          )}
          <span className="num text-[11.5px] text-graphite" title={props.saveError ?? undefined}>
            {props.saveError ? (
              <span className="swap-in text-pencil-deep">not saved</span>
            ) : props.savedAt ? (
              <>
                <span key={props.versionCount} className="swap-in">v{props.versionCount}</span>
                <span className={n > 0 ? "hidden @5xl:inline" : "hidden @2xl:inline"}> · saved {timeAgo(props.savedAt, now)}</span>
              </>
            ) : (
              "unsaved"
            )}
          </span>
          <button
            id="history-toggle"
            onClick={() => toggleHistory()}
            aria-pressed={historyOpen}
            title="Version history (⌥H)"
            className={`press relative ${historyOpen ? "text-ink" : "text-graphite hover:text-ink"}`}
          >
            History
            <span aria-hidden className="toggle-line" data-on={historyOpen} />
          </button>
          <button className="press h-7 rounded-[3px] bg-ink px-3 text-[12.5px] font-medium text-paper hover:opacity-85">
            Share
          </button>
        </div>
      </header>

      <div className="flex shrink-0">
        {historyOpen && <div aria-hidden className="w-11 shrink-0 @min-[808px]:hidden" />}
        <div className="min-w-0 flex-1">
          {preview ? (
            <CompareBar
              key="compare"
              n={preview.n}
              label={preview.label}
              baseN={compareTo === "current" ? null : prevVersion?.n ?? 0}
              stats={diffStats(rows)}
              compareTo={compareTo}
              setCompareTo={setCompareTo}
              onlyChanges={onlyChanges}
              setOnlyChanges={setOnlyChanges}
              canRestore={!isCurrent}
              onRestore={() => props.onRestore(preview)}
              onClose={() => props.onPreview(null)}
            />
          ) : (
            editor && <Toolbar key="tools" editor={editor} />
          )}
        </div>
      </div>
      <div className="h-px shrink-0 bg-rule" />

      <div className="relative flex min-h-0 flex-1">
      {historyOpen && (
        <HistoryRail versions={versions} previewId={previewId} onPreview={props.onPreview} onRestore={props.onRestore} animateIn={railAnimated} />
      )}
      <div className="relative min-h-0 flex-1 overflow-y-auto">
        <article className="mx-auto w-full max-w-[var(--doc-measure)] px-10 pb-40 pt-10 @3xl:pt-14">
          <div className={preview ? "hidden" : undefined}>
            <EditorContent editor={editor} />
          </div>
          {preview && (
            <div key={`${preview.id}-${compareTo}`} className="prose-doc swap-fade">
              <DiffDocument rows={rows} onlyChanges={onlyChanges} />
            </div>
          )}
        </article>

        {editor && (
          <BubbleMenu
            editor={editor}
            options={{ placement: "top", offset: 10 }}
            shouldShow={({ state }) => !state.selection.empty && editor.isEditable}
          >
            <div className="bubble flex items-center rounded-[4px] bg-ink p-0.5 text-paper shadow-[var(--shadow-bar)]">
              <button
                className="press rounded-[3px] px-2.5 py-1.5 text-[12.5px] font-medium hover:bg-paper/10"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  const { from, to } = editor.state.selection;
                  props.onAsk(editor.state.doc.textBetween(from, to, " "));
                }}
              >
                Ask the editor
              </button>
              <span className="mx-0.5 h-4 w-px bg-paper/15" />
              <InvBtn label="Bold" onClick={() => editor.chain().focus().toggleBold().run()}><Bold size={14} strokeWidth={2} /></InvBtn>
              <InvBtn label="Italic" onClick={() => editor.chain().focus().toggleItalic().run()}><Italic size={14} strokeWidth={2} /></InvBtn>
              <InvBtn label="Highlight" onClick={() => editor.chain().focus().toggleHighlight().run()}><Highlighter size={14} strokeWidth={2} /></InvBtn>
            </div>
          </BubbleMenu>
        )}
      </div>
      </div>

      {editor && <Colophon editor={editor} onOpenSettings={props.onOpenSettings} />}
    </div>
  );
}

function CompareBar(p: {
  n: number;
  label: string;
  baseN: number | null;
  stats: { added: number; removed: number };
  compareTo: "previous" | "current";
  setCompareTo: (c: "previous" | "current") => void;
  onlyChanges: boolean;
  setOnlyChanges: (b: boolean) => void;
  canRestore: boolean;
  onRestore: () => void;
  onClose: () => void;
}) {
  const scope = p.baseN === null ? `v${p.n} → now` : p.baseN === 0 ? `v${p.n}, first draft` : `v${p.baseN} → v${p.n}`;
  return (
    <div className="swap-fade">
      <div className="mx-auto flex h-11 w-full max-w-[var(--doc-measure)] items-center gap-4 whitespace-nowrap px-10 text-[12.5px]">
        <span key={scope} className="num swap-in text-ink">{scope}</span>
        <span key={`${p.stats.added}/${p.stats.removed}`} className="num swap-in hidden text-[11.5px] text-graphite @3xl:inline">
          +{p.stats.added} −{p.stats.removed}
        </span>
        <span className="h-3.5 w-px bg-rule-2" />
        <Segmented
          label="Compare with"
          value={p.compareTo}
          onChange={p.setCompareTo}
          options={[
            { value: "previous", label: "Its changes" },
            { value: "current", label: "Since then" },
          ]}
        />
        <button
          role="switch"
          aria-checked={p.onlyChanges}
          onClick={() => p.setOnlyChanges(!p.onlyChanges)}
          className={`press hidden @4xl:inline ${p.onlyChanges ? "text-ink" : "text-graphite hover:text-ink"}`}
        >
          Changes only
        </button>
        <span className="ml-auto flex items-center gap-4">
          {p.canRestore && (
            <button onClick={p.onRestore} className="tbtn-pencil">Restore v{p.n}</button>
          )}
          <button onClick={p.onClose} className="press h-7 rounded-[3px] bg-ink px-3 font-medium text-paper hover:opacity-85">
            Done
          </button>
        </span>
      </div>
    </div>
  );
}

function Toolbar({ editor }: { editor: Editor }) {
  const s = useEditorState({
    editor,
    selector: ({ editor: e }) => ({
      p: e.isActive("paragraph") && !e.isActive("bulletList") && !e.isActive("orderedList") && !e.isActive("blockquote"),
      h1: e.isActive("heading", { level: 1 }),
      h2: e.isActive("heading", { level: 2 }),
      bold: e.isActive("bold"),
      italic: e.isActive("italic"),
      underline: e.isActive("underline"),
      strike: e.isActive("strike"),
      highlight: e.isActive("highlight"),
      bullet: e.isActive("bulletList"),
      ordered: e.isActive("orderedList"),
      quote: e.isActive("blockquote"),
      canUndo: e.can().undo(),
      canRedo: e.can().redo(),
    }),
  });
  const c = () => editor.chain().focus();

  return (
    <div className="swap-fade">
      <div className="mx-auto flex h-11 w-full max-w-[var(--doc-measure)] items-center gap-1 whitespace-nowrap px-5 text-[12.5px] @3xl:px-10">
        <TextTool active={s.p} onClick={() => c().setParagraph().run()}>Body</TextTool>
        <TextTool active={s.h1} onClick={() => c().toggleHeading({ level: 1 }).run()}>Title</TextTool>
        <TextTool active={s.h2} onClick={() => c().toggleHeading({ level: 2 }).run()}>Section</TextTool>
        <Sep />
        <IconTool label="Bold" active={s.bold} onClick={() => c().toggleBold().run()}><Bold size={15} /></IconTool>
        <IconTool label="Italic" active={s.italic} onClick={() => c().toggleItalic().run()}><Italic size={15} /></IconTool>
        <IconTool label="Underline" active={s.underline} onClick={() => c().toggleUnderline().run()}><Underline size={15} /></IconTool>
        <IconTool label="Strikethrough" active={s.strike} onClick={() => c().toggleStrike().run()}><Strikethrough size={15} /></IconTool>
        <IconTool label="Highlight" active={s.highlight} onClick={() => c().toggleHighlight().run()}><Highlighter size={15} /></IconTool>
        <Sep />
        <IconTool label="Bulleted list" active={s.bullet} onClick={() => c().toggleBulletList().run()}><List size={15} /></IconTool>
        <IconTool label="Numbered list" active={s.ordered} onClick={() => c().toggleOrderedList().run()}><ListOrdered size={15} /></IconTool>
        <IconTool label="Quote" active={s.quote} onClick={() => c().toggleBlockquote().run()}><Quote size={15} /></IconTool>
        <span className="ml-auto flex items-center gap-0.5">
          <IconTool label="Undo" disabled={!s.canUndo} onClick={() => c().undo().run()}><Undo2 size={15} /></IconTool>
          <IconTool label="Redo" disabled={!s.canRedo} onClick={() => c().redo().run()}><Redo2 size={15} /></IconTool>
        </span>
      </div>
    </div>
  );
}

function Colophon({ editor, onOpenSettings }: { editor: Editor; onOpenSettings: () => void }) {
  const words = useEditorState({
    editor,
    selector: ({ editor: e }) => e.getText().split(/\s+/).filter(Boolean).length,
  });
  return (
    <footer className="flex h-8 shrink-0 items-center gap-4 whitespace-nowrap border-t border-rule px-3 text-[11px] text-graphite @3xl:px-4">
      <button
        onClick={onOpenSettings}
        aria-label="Settings"
        title="Settings"
        className="press group -mr-1 grid size-6 place-items-center rounded-[3px] text-graphite hover:bg-well hover:text-ink"
      >
        <Settings size={13.5} strokeWidth={1.75} className="transition-transform duration-300 ease-[var(--ease-out)] group-hover:rotate-45" />
      </button>
      <span className="num">{words} words</span>
      <span className="num">{Math.max(1, Math.round(words / 230))} min read</span>
      <span className="ml-auto hidden @3xl:inline">Select text to ask the editor about a passage</span>
    </footer>
  );
}

function TextTool({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      className={`press h-7 rounded-[3px] px-2 ${active ? "text-ink" : "text-graphite hover:text-ink"}`}
    >
      <span className={active ? "underline decoration-pencil decoration-[1.5px] underline-offset-[6px]" : ""}>{children}</span>
    </button>
  );
}

function IconTool({ label, active, disabled, onClick, children }: { label: string; active?: boolean; disabled?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      title={label}
      aria-label={label}
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
      className={`press grid size-7 place-items-center rounded-[3px] disabled:opacity-25 ${
        active ? "bg-well text-ink" : "text-graphite hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}

function InvBtn({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button title={label} aria-label={label} onClick={onClick} className="press grid size-7 place-items-center rounded-[3px] text-paper/80 hover:bg-paper/10 hover:text-paper">
      {children}
    </button>
  );
}

const Sep = () => <span className="mx-2 h-3.5 w-px bg-rule-2" />;
