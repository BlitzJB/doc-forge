"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Placeholder from "@tiptap/extension-placeholder";
import Highlight from "@tiptap/extension-highlight";
import { Markdown } from "@tiptap/markdown";

import { EARLIER_DRAFT, FIRST_OUTLINE, MIDDLE_DRAFT, SAMPLE_DOC, DOC_TITLE, DOC_FOLDER } from "@/lib/sample-doc";
import {
  SuggestionsExtension,
  findTextRange,
  insertedBlock,
  insertionPoint,
  minimalChange,
  suggestionsKey,
  type SuggestionHandlers,
} from "@/lib/suggestions-extension";
import { runAgent, type ProposedEdit } from "@/lib/mock-agent";
import { uid, type EditMode, type Message, type Part, type Suggestion, type Version } from "@/lib/types";

import { EditorPane } from "./EditorPane";
import { ChatPanel } from "./ChatPanel";
import { SettingsDialog } from "./SettingsDialog";
import { useSettings } from "@/lib/use-settings";
import { desktop, isDesktop, toVersions, type OpenedDoc } from "@/lib/desktop";
import { snapshotBlocks, startAgent, type AgentEvent, type AgentRun } from "@/lib/agent";
import { Welcome } from "./Welcome";
import { CreatePrompt, type MissingFile } from "./CreatePrompt";
import { fallbackTitle, settled, threads, type ThreadMeta } from "@/lib/threads";

const AUTOSAVE_MS = 2500;

const hostOf = (url: string) => {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
};

interface OpenDoc {
  id: string;
  path: string;
  name: string;
  folder: string;
}

export default function DocForge() {
  // Inside the Tauri shell, documents are real files with shadow-git history.
  const [desktopMode] = useState(isDesktop);
  // The ProseMirror widgets call back into React through this ref (kept fresh in an effect).
  const handlersRef = useRef<SuggestionHandlers | null>(null);

  const editor = useEditor({
    immediatelyRender: false,
    extensions: [
      StarterKit,
      Highlight,
      Markdown,
      Placeholder.configure({ placeholder: "Begin here." }),
      // getHandlers is only invoked from DOM event handlers on suggestion widgets, never during render.
      // eslint-disable-next-line react-hooks/refs
      SuggestionsExtension.configure({ getHandlers: () => handlersRef.current }),
    ],
    content: isDesktop() ? "" : SAMPLE_DOC,
    editorProps: { attributes: { class: "prose-doc", spellcheck: "false" } },
  });

  const [title, setTitle] = useState(DOC_TITLE);
  const [messages, setMessages] = useState<Message[]>([]);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [versions, setVersions] = useState<Version[]>([]);
  const [mode, setMode] = useState<EditMode>("suggest");
  const [webSearch, setWebSearch] = useState(true);
  const [running, setRunning] = useState(false);
  const [quote, setQuote] = useState<string | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [previewId, setPreviewId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { settings, update: updateSettings } = useSettings();
  const [docTick, setDocTick] = useState(0);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [doc, setDoc] = useState<OpenDoc | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const docRef = useRef<OpenDoc | null>(null);

  // The margin conversation is a thread; each document keeps its threads.
  const [threadId, setThreadId] = useState(uid);
  const [threadTitle, setThreadTitle] = useState("");
  const [threadCreatedAt, setThreadCreatedAt] = useState(() => Date.now());
  const [threadList, setThreadList] = useState<ThreadMeta[]>([]);
  const threadIdRef = useRef(threadId);
  /** What was last loaded or saved, so opening a thread doesn't count as changing it. */
  const threadSynced = useRef<{ messages: Message[]; suggestions: Suggestion[]; title: string } | null>(null);
  const threadDoc = desktopMode ? (doc?.id ?? null) : "sample";
  const saveChain = useRef<Promise<unknown>>(Promise.resolve());
  const pendingSave = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // Refs so async callbacks always see fresh state.
  const versionsRef = useRef(versions);
  const messagesRef = useRef(messages);
  const agentRun = useRef<AgentRun | null>(null);
  const suggestionsRef = useRef(suggestions);
  const modeRef = useRef(mode);
  const runToken = useRef<{ aborted: boolean } | null>(null);
  useEffect(() => {
    versionsRef.current = versions;
    messagesRef.current = messages;
    docRef.current = doc;
    suggestionsRef.current = suggestions;
    modeRef.current = mode;
    threadIdRef.current = threadId;
  });

  // ---- Versions ------------------------------------------------------------

  /** Desktop: write the file and snapshot it in the shadow repo. Saves are queued, never concurrent. */
  const saveToDisk = useCallback(
    (author: Version["author"], label: string) => {
      const d = docRef.current;
      if (!editor || !d) return Promise.resolve();
      // Files end with exactly one newline, like most editors write them.
      const markdown = editor.getMarkdown().replace(/\s*$/, "\n");
      const run = saveChain.current.then(async () => {
        try {
          const history = await desktop.save(d.id, markdown, label, author);
          if (docRef.current?.id !== d.id) return;
          const next = toVersions(history);
          versionsRef.current = next;
          setVersions(next);
          setSavedAt(Date.now());
          setSaveError(null);
        } catch (e) {
          setSaveError(String(e));
        }
      });
      saveChain.current = run;
      return run;
    },
    [editor],
  );

  const createVersion = useCallback(
    (author: Version["author"], label: string) => {
      if (!editor) return;
      if (desktopMode) {
        void saveToDisk(author, label);
        return;
      }
      const html = editor.getHTML();
      const prev = versionsRef.current;
      if (prev.length && prev[prev.length - 1].html === html) return;
      const v: Version = { id: uid(), n: prev.length + 1, createdAt: Date.now(), author, label, html };
      versionsRef.current = [...prev, v];
      setVersions(versionsRef.current);
      setSavedAt(Date.now());
    },
    [editor, desktopMode, saveToDisk],
  );

  // Seed mock history once the editor exists (browser demo only).
  useEffect(() => {
    if (!editor || desktopMode || versionsRef.current.length) return;
    const now = Date.now();
    const H = 3_600_000;
    const at = (daysAgo: number, hour: number, min: number) => {
      const d = new Date(now - daysAgo * 24 * H);
      d.setHours(hour, min, 0, 0);
      return d.getTime();
    };
    const seed = (n: number, createdAt: number, author: Version["author"], label: string, html: string): Version => ({
      id: uid(), n, createdAt, author, label, html,
    });
    const seeded: Version[] = [
      seed(1, at(9, 10, 2), "you", "Created document", FIRST_OUTLINE),
      seed(2, at(9, 10, 21), "you", "Drafted problem and audience", EARLIER_DRAFT),
      seed(3, at(1, 16, 40), "you", "Expanded audience and strategy", MIDDLE_DRAFT),
      seed(4, now - 42 * 60_000, "you", "Sharpened the problem and metrics", editor.getHTML()),
    ];
    versionsRef.current = seeded;
    setVersions(seeded);
    setSavedAt(seeded[seeded.length - 1].createdAt);
  }, [editor, desktopMode]);

  // Autosave a version after the user stops typing.
  useEffect(() => {
    if (!editor) return;
    const onUpdate = ({ transaction }: { transaction: { getMeta: (k: string) => unknown } }) => {
      setDocTick((n) => n + 1);
      if (transaction.getMeta("agentEdit") || transaction.getMeta("loadDocument")) return;
      clearTimeout(pendingSave.current);
      pendingSave.current = setTimeout(() => {
        pendingSave.current = undefined;
        createVersion("you", "Manual edits");
      }, AUTOSAVE_MS);
    };
    editor.on("update", onUpdate);
    return () => {
      clearTimeout(pendingSave.current);
      editor.off("update", onUpdate);
    };
  }, [editor, createVersion]);

  // ---- Suggestions ---------------------------------------------------------

  const applyToDoc = useCallback(
    (s: Pick<Suggestion, "find" | "replace" | "kind">) => {
      if (!editor) return false;
      if (s.kind === "insert") {
        const at = insertionPoint(editor.state.doc, s.find);
        if (at === null) return false;
        const { schema } = editor.state;
        const nodes = s.replace
          .split(/\n{2,}/)
          .map((t) => t.trim())
          .filter(Boolean)
          .map((t) => {
            const b = insertedBlock(t);
            return b.level
              ? schema.nodes.heading.create({ level: b.level }, schema.text(b.text))
              : schema.nodes.paragraph.create(null, schema.text(b.text));
          });
        if (!nodes.length) return false;
        const size = nodes.reduce((n, node) => n + node.nodeSize, 0);
        const flash = { id: uid(), from: at + 1, to: at + size - 1 };
        editor.view.dispatch(editor.state.tr.insert(at, nodes).setMeta("agentEdit", true).setMeta(suggestionsKey, { flash }));
        setTimeout(() => {
          if (!editor.isDestroyed) editor.view.dispatch(editor.state.tr.setMeta(suggestionsKey, { unflash: flash.id }).setMeta("addToHistory", false));
        }, 1200);
        return true;
      }
      const r = findTextRange(editor.state.doc, s.find);
      if (!r) return false;
      // Only the changed span gets the settle tint, not the unchanged context around it.
      const { prefix, inserted } = minimalChange(s.find, s.replace);
      const flash = { id: uid(), from: r.from + prefix, to: r.from + prefix + inserted.length };
      editor.view.dispatch(
        editor.state.tr.insertText(s.replace, r.from, r.to).setMeta("agentEdit", true).setMeta(suggestionsKey, { flash }),
      );
      setTimeout(() => {
        if (!editor.isDestroyed) editor.view.dispatch(editor.state.tr.setMeta(suggestionsKey, { unflash: flash.id }).setMeta("addToHistory", false));
      }, 1200);
      return true;
    },
    [editor],
  );

  const setStatus = (ids: string[], status: Suggestion["status"]) =>
    setSuggestions((all) => {
      const next = all.map((s) => (ids.includes(s.id) ? { ...s, status } : s));
      suggestionsRef.current = next;
      return next;
    });

  const accept = useCallback(
    (ids: string[]) => {
      const done: string[] = [];
      for (const id of ids) {
        const s = suggestionsRef.current.find((x) => x.id === id);
        if (s?.status === "pending" && applyToDoc(s)) done.push(id);
      }
      if (!done.length) return;
      setStatus(done, "accepted");
      createVersion(
        "agent",
        done.length === 1
          ? `Accepted: ${suggestionsRef.current.find((s) => s.id === done[0])?.reason ?? "agent edit"}`
          : `Accepted ${done.length} agent edits`,
      );
    },
    [applyToDoc, createVersion],
  );

  const bornAt = useRef(new Map<string, number>());

  // Declined marks fade out in the manuscript before they are removed.
  const [leaving, setLeaving] = useState<string[]>([]);
  const reject = useCallback((ids: string[]) => {
    setLeaving((l) => [...l, ...ids]);
    setTimeout(() => {
      setStatus(ids, "rejected");
      setLeaving((l) => l.filter((x) => !ids.includes(x)));
    }, 160);
  }, []);

  const propose = useCallback(
    (edits: (ProposedEdit & { kind?: Suggestion["kind"] })[], messageId: string) => {
      const created: Suggestion[] = edits.map((e) => ({ id: uid(), messageId, ...e, status: "pending" }));
      created.forEach((c) => bornAt.current.set(c.id, Date.now()));
      if (modeRef.current === "auto") {
        let applied = 0;
        for (const s of created) {
          if (applyToDoc(s)) {
            s.status = "applied";
            applied++;
          }
        }
        if (applied) createVersion("agent", applied === 1 ? `Forge: ${created[0].reason}` : `Forge applied ${applied} edits`);
      }
      setSuggestions((all) => {
        const next = [...all, ...created];
        suggestionsRef.current = next;
        return next;
      });
      return created.map((s) => s.id);
    },
    [applyToDoc, createVersion],
  );

  useEffect(() => {
    handlersRef.current = {
      accept: (id) => accept([id]),
      reject: (id) => reject([id]),
      focus: (id) => setActiveId(id),
    };
  }, [accept, reject]);

  // Push pending suggestions + active id into the ProseMirror plugin.
  useEffect(() => {
    if (!editor) return;
    const items = suggestions
      .map((s, i) => ({ ...s, n: i + 1, bornAt: bornAt.current.get(s.id) }))
      .filter((s) => s.status === "pending");
    editor.view.dispatch(
      editor.state.tr.setMeta(suggestionsKey, { items, activeId, leaving }).setMeta("addToHistory", false),
    );
  }, [editor, suggestions, activeId, leaving]);

  /** Pending suggestions whose target text still exists in the doc. */
  const livePending = useMemo(() => {
    void docTick;
    if (!editor) return [];
    return suggestions.filter((s) => s.status === "pending" && findTextRange(editor.state.doc, s.find));
  }, [editor, suggestions, docTick]);

  const isStale = useCallback(
    (s: Suggestion) => s.status === "pending" && !livePending.some((x) => x.id === s.id),
    [livePending],
  );

  const locate = useCallback((id: string) => {
    setActiveId(id);
    setPreviewId(null);
    requestAnimationFrame(() => {
      const sel = `.ProseMirror [data-sid="${id}"]`;
      document.querySelector(sel)?.scrollIntoView({ behavior: "smooth", block: "center" });
      // Pulse once the scroll has mostly settled; look elements up now, not before re-renders.
      setTimeout(() => {
        document.querySelectorAll<HTMLElement>(sel).forEach((el) => {
          el.classList.remove("sg-pulse");
          void el.offsetWidth;
          el.classList.add("sg-pulse");
          el.addEventListener("animationend", () => el.classList.remove("sg-pulse"), { once: true });
        });
      }, 280);
    });
  }, []);

  // ---- Chat ----------------------------------------------------------------

  /** The conversation so far, as the agent should remember it: words plus what became of its edits. */
  const buildHistory = useCallback(() => {
    const out: { role: "user" | "assistant"; content: string }[] = [];
    for (const m of messagesRef.current.slice(-12)) {
      if (m.role === "user") {
        const text = m.parts.map((p) => (p.type === "text" ? p.text : "")).join("");
        out.push({ role: "user", content: (m.quote ? `About this passage: "${m.quote}"\n\n` : "") + text });
        continue;
      }
      const words = m.parts.map((p) => (p.type === "text" ? p.text : "")).join("").trim();
      const marks = m.parts
        .flatMap((p) => (p.type === "suggestions" ? p.ids : []))
        .map((id) => suggestionsRef.current.find((s) => s.id === id))
        .filter((s): s is Suggestion => !!s)
        .map((s) => {
          const outcome = s.status === "pending" ? "awaiting review" : s.status === "rejected" ? "declined" : "accepted";
          return s.kind === "insert" ? `- New paragraph: "${s.replace.slice(0, 160)}" (${outcome})` : `- "${s.find.slice(0, 120)}" → "${s.replace.slice(0, 120)}" (${outcome})`;
        });
      const content = [words, marks.length ? `Edits I proposed:\n${marks.join("\n")}` : ""].filter(Boolean).join("\n\n");
      if (content) out.push({ role: "assistant", content });
    }
    return out;
  }, []);

  /** Run the real agent (desktop) and fold its event stream into the message's parts. */
  const runForge = useCallback(async (text: string, q: string | null, aid: string, setParts: (fn: (parts: Part[]) => Part[]) => void) => {
    if (!editor) return;
    let reasoningStarted = 0;
    const closeReasoning = (parts: Part[]) =>
      parts.map((p) =>
        p.type === "reasoning" && !p.done ? { ...p, done: true, seconds: Math.max(1, Math.round((Date.now() - reasoningStarted) / 1000)) } : p,
      );
    // However a run ends, no tool line is left looking busy.
    const settleTools = (parts: Part[]) =>
      parts.map((p) => (p.type === "tool" && p.status === "running" ? { ...p, status: "done" as const, ok: p.ok ?? false } : p));
    const onEvent = (e: AgentEvent) => {
      switch (e.type) {
        case "reasoning":
          setParts((parts) => {
            const last = parts.at(-1);
            if (last?.type === "reasoning" && !last.done) return [...parts.slice(0, -1), { ...last, text: last.text + e.delta }];
            reasoningStarted = Date.now();
            return [...parts, { type: "reasoning", text: e.delta, done: false }];
          });
          break;
        case "text":
          setParts((parts) => {
            const closed = closeReasoning(parts);
            const last = closed.at(-1);
            if (last?.type === "text") return [...closed.slice(0, -1), { ...last, text: last.text + e.delta }];
            return [...closed, { type: "text", text: e.delta.replace(/^\n+/, "") }];
          });
          break;
        case "toolStart":
          setParts((parts) => [...closeReasoning(parts), { type: "tool", id: e.id, tool: e.tool, status: "running", query: e.label }]);
          break;
        case "toolEnd":
          setParts((parts) =>
            parts.map((p) =>
              p.type === "tool" && p.id === e.id
                ? {
                    ...p,
                    status: "done",
                    ok: e.ok,
                    summary: e.summary,
                    results: e.sources.map((s) => ({ ...s, domain: hostOf(s.url) })),
                  }
                : p,
            ),
          );
          break;
        case "edits": {
          const ids = propose(e.edits, aid);
          setParts((parts) => {
            const closed = closeReasoning(parts);
            const last = closed.at(-1);
            if (last?.type === "suggestions") return [...closed.slice(0, -1), { ...last, ids: [...last.ids, ...ids] }];
            return [...closed, { type: "suggestions", ids }];
          });
          break;
        }
        case "done":
          setParts((parts) => settleTools(closeReasoning(parts)));
          break;
        case "error":
          setParts((parts) => [...settleTools(closeReasoning(parts)), { type: "error", message: e.message, retry: { prompt: text, quote: q } }]);
          break;
      }
    };
    try {
      const run = await startAgent(
        {
          prompt: text,
          quote: q,
          document: {
            name: docRef.current?.name ?? title,
            folder: docRef.current?.folder,
            blocks: snapshotBlocks(editor.state.doc),
            markdown: editor.getMarkdown(),
          },
          docId: docRef.current?.id,
          utcOffset: -new Date().getTimezoneOffset(),
          today: new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long", year: "numeric" }),
          history: buildHistory(),
          web: webSearch,
          applyDirectly: modeRef.current === "auto",
        },
        onEvent,
      );
      agentRun.current = run;
      await run.finished;
    } catch (err) {
      const message = String(err).replace(/^Error:\s*/, "");
      setParts((parts) => [
        ...parts,
        /Settings, under Models/.test(message) ? { type: "setup", message } : { type: "error", message, retry: { prompt: text, quote: q } },
      ]);
    } finally {
      agentRun.current = null;
    }
  }, [editor, propose, title, webSearch, buildHistory]);

  const send = useCallback(
    async (text: string, retryQuote?: string | null) => {
      if (!editor || running || !text.trim()) return;
      const q = retryQuote !== undefined ? retryQuote : quote;
      const userMsg: Message = { id: uid(), role: "user", parts: [{ type: "text", text }], quote: q, createdAt: Date.now() };
      const aid = uid();
      const assistant: Message = { id: aid, role: "assistant", parts: [], createdAt: Date.now(), streaming: true };
      setMessages((m) => [...m, userMsg, assistant]);
      setQuote(null);
      setRunning(true);
      const token = { aborted: false };
      runToken.current = token;

      const setParts = (fn: (parts: Part[]) => Part[]) =>
        setMessages((ms) => ms.map((m) => (m.id === aid ? { ...m, parts: fn(m.parts) } : m)));

      if (desktopMode) {
        await runForge(text, q, aid, setParts);
      } else {
        await runAgent({
          prompt: text,
          quote: q,
          webSearch,
          io: {
            setParts,
            propose: (edits) => propose(edits, aid),
            isAborted: () => token.aborted,
            getDocText: () => editor.getText(),
          },
        });
      }

      setMessages((ms) => ms.map((m) => (m.id === aid ? { ...m, streaming: false } : m)));
      setRunning(false);
    },
    [editor, running, quote, webSearch, propose, desktopMode, runForge],
  );

  const stop = () => {
    if (runToken.current) runToken.current.aborted = true;
    void agentRun.current?.cancel();
  };

  // ---- Threads -------------------------------------------------------------

  const refreshThreads = useCallback(async (docId: string) => {
    try {
      const list = await threads.list(docId);
      if (docRef.current?.id === docId || docId === "sample") setThreadList(list);
    } catch {
      setThreadList([]);
    }
  }, []);

  /** A fresh, empty thread. Undecided marks stay with the thread they came from. */
  const startThread = useCallback(() => {
    const id = uid();
    const empty = { messages: [] as Message[], suggestions: [] as Suggestion[], title: "" };
    threadSynced.current = empty;
    threadIdRef.current = id;
    setThreadId(id);
    setThreadTitle("");
    setThreadCreatedAt(Date.now());
    setMessages(empty.messages);
    setSuggestions(empty.suggestions);
    suggestionsRef.current = empty.suggestions;
    setActiveId(null);
    setQuote(null);
  }, []);

  const openThread = useCallback(async (docId: string, id: string) => {
    const t = await threads.load(docId, id);
    if (!t) return false;
    const messages = settled(t.messages);
    threadSynced.current = { messages, suggestions: t.suggestions, title: t.title };
    threadIdRef.current = t.id;
    setThreadId(t.id);
    setThreadTitle(t.title);
    setThreadCreatedAt(t.createdAt);
    setMessages(messages);
    setSuggestions(t.suggestions);
    suggestionsRef.current = t.suggestions;
    setActiveId(null);
    setQuote(null);
    return true;
  }, []);

  /** Resume a document's latest thread, or start one. */
  const enterThreads = useCallback(
    async (docId: string) => {
      startThread();
      let list: ThreadMeta[] = [];
      try {
        list = await threads.list(docId);
      } catch {
        // No threads yet.
      }
      setThreadList(list);
      if (list[0]) await openThread(docId, list[0].id);
    },
    [startThread, openThread],
  );

  const deleteThread = useCallback(
    async (id: string) => {
      if (!threadDoc) return;
      await threads.remove(threadDoc, id);
      if (id === threadIdRef.current) startThread();
      void refreshThreads(threadDoc);
    },
    [threadDoc, startThread, refreshThreads],
  );

  // The browser demo has one document.
  useEffect(() => {
    if (desktopMode || !editor) return;
    // Starts on a fresh thread already; resume the latest saved one, if any.
    threads
      .list("sample")
      .then((list) => {
        setThreadList(list);
        if (list[0]) return openThread("sample", list[0].id);
      })
      .catch(() => {});
  }, [desktopMode, editor, openThread]);

  const firstAsk = messages.find((m) => m.role === "user");
  const shownTitle = threadTitle || (firstAsk ? fallbackTitle(textOf(firstAsk)) : "");

  // Save the thread once a reply settles (never mid-stream), and whenever marks change.
  useEffect(() => {
    if (!threadDoc || !messages.length || messages.some((m) => m.streaming)) return;
    const s = threadSynced.current;
    if (s && s.messages === messages && s.suggestions === suggestions && s.title === shownTitle) return;
    const record = {
      id: threadId,
      title: shownTitle,
      createdAt: threadCreatedAt,
      updatedAt: Math.max(...messages.map((m) => m.createdAt)),
      messages,
      suggestions,
    };
    const t = setTimeout(() => {
      threads
        .save(threadDoc, record)
        .then(() => {
          threadSynced.current = { messages, suggestions, title: shownTitle };
          return refreshThreads(threadDoc);
        })
        .catch(() => {});
    }, 300);
    return () => clearTimeout(t);
  }, [threadDoc, threadId, threadCreatedAt, shownTitle, messages, suggestions, refreshThreads]);

  // Name the thread from its opening exchange (the Quick model), once per thread.
  const titled = useRef(new Set<string>());
  useEffect(() => {
    if (running || threadTitle || titled.current.has(threadId)) return;
    const reply = messages.find((m) => m.role === "assistant" && !m.streaming);
    if (!firstAsk || !reply) return;
    titled.current.add(threadId);
    const id = threadId;
    threads
      .title(textOf(firstAsk), textOf(reply))
      .then((t) => {
        if (threadIdRef.current === id) setThreadTitle(t);
      })
      .catch(() => {});
  }, [running, threadTitle, threadId, messages, firstAsk]);

  // ---- History -------------------------------------------------------------

  /** Replace the editor's content without triggering autosave or polluting undo history. */
  const loadIntoEditor = useCallback(
    (markdown: string) => {
      editor?.chain().setMeta("loadDocument", true).setMeta("addToHistory", false).setContent(markdown, { contentType: "markdown" }).run();
    },
    [editor],
  );

  const showDocument = useCallback(
    (opened: OpenedDoc) => {
      clearTimeout(pendingSave.current);
      pendingSave.current = undefined;
      const d = { id: opened.id, path: opened.path, name: opened.name, folder: opened.folder };
      docRef.current = d;
      setDoc(d);
      loadIntoEditor(opened.content);
      const next = toVersions(opened.history);
      versionsRef.current = next;
      setVersions(next);
      setSavedAt(opened.history.at(-1)?.createdAt ?? null);
      setSaveError(null);
      setPreviewId(null);
      // Marks from another document can't apply here; this one's threads take over.
      void enterThreads(opened.id);
    },
    [loadIntoEditor, enterThreads],
  );

  const [openError, setOpenError] = useState<string | null>(null);
  const [missingFile, setMissingFile] = useState<MissingFile | null>(null);
  const openDocument = useCallback(
    async (path?: string) => {
      try {
        // Don't lose the last few seconds of typing in the current document.
        if (pendingSave.current) {
          clearTimeout(pendingSave.current);
          pendingSave.current = undefined;
          await saveToDisk("you", "Manual edits");
        }
        const opened = path ? await desktop.open(path) : await desktop.pickAndOpen();
        if (opened) {
          showDocument(opened);
          setOpenError(null);
        }
      } catch (e) {
        setOpenError(String(e));
      }
    },
    [saveToDisk, showDocument],
  );

  // ⌘, opens Settings (the platform convention), from anywhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        setSettingsOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // ⌘O / Ctrl+O opens a file (desktop).
  useEffect(() => {
    if (!desktopMode) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void openDocument();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [desktopMode, openDocument]);

  // Files sent from outside: `df notes.md`, Finder's Open With, a second launch.
  // Drain once on load (paths from this launch) and again on every ping.
  useEffect(() => {
    if (!desktopMode || !editor) return;
    let off: (() => void) | undefined;
    let gone = false;
    const drain = async () => {
      const paths = await desktop.takeOpens();
      // One document at a time: the last one named wins.
      const last = paths.at(-1);
      if (!last || gone) return;
      // A path that isn't there yet is the author's call: ask before creating anything.
      const status = await desktop.probe(last).catch(() => "file" as const);
      if (status === "file") await openDocument(last);
      else setMissingFile({ path: last, status });
    };
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const un = await listen("open-paths", () => void drain());
      if (gone) un();
      else off = un;
      void drain();
    })();
    return () => {
      gone = true;
      off?.();
    };
  }, [desktopMode, editor, openDocument]);

  // Flush typing when the window loses focus; pick up outside edits when it regains it.
  useEffect(() => {
    if (!desktopMode) return;
    const onBlur = () => {
      if (!pendingSave.current) return;
      clearTimeout(pendingSave.current);
      pendingSave.current = undefined;
      void saveToDisk("you", "Manual edits");
    };
    const onFocus = async () => {
      const d = docRef.current;
      if (!d || pendingSave.current) return;
      await saveChain.current;
      try {
        const synced = await desktop.sync(d.id);
        if (docRef.current?.id !== d.id) return;
        if (synced.changed) loadIntoEditor(synced.content);
        const next = toVersions(synced.history);
        versionsRef.current = next;
        setVersions(next);
      } catch (e) {
        setSaveError(String(e));
      }
    };
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
    };
  }, [desktopMode, saveToDisk, loadIntoEditor]);

  const restore = (v: Version) => {
    if (!editor) return;
    if (desktopMode) {
      const d = docRef.current;
      if (!d) return;
      clearTimeout(pendingSave.current);
      pendingSave.current = undefined;
      saveChain.current = saveChain.current.then(async () => {
        try {
          const r = await desktop.restore(d.id, v.id, `Restored v${v.n}`);
          loadIntoEditor(r.content);
          const next = toVersions(r.history);
          versionsRef.current = next;
          setVersions(next);
          setSavedAt(Date.now());
        } catch (e) {
          setSaveError(String(e));
        }
      });
      setPreviewId(null);
      return;
    }
    editor.chain().setMeta("agentEdit", true).setContent(v.html).run();
    createVersion("you", `Restored v${v.n}`);
    setPreviewId(null);
  };

  return (
    <div className="flex h-dvh w-full overflow-hidden bg-paper text-ink">
      <main className="relative flex min-w-0 flex-1 flex-col">
        {desktopMode && !doc ? (
          <Welcome onOpen={openDocument} onOpenSettings={() => setSettingsOpen(true)} error={openError} />
        ) : (
        <EditorPane
          editor={editor}
          desktop={desktopMode}
          folder={doc ? doc.folder : DOC_FOLDER}
          title={doc ? doc.name : title}
          titleEditable={!doc}
          path={doc?.path}
          saveError={saveError}
          onOpenFile={desktopMode ? () => void openDocument() : undefined}
          setTitle={setTitle}
          versionCount={versions.length}
          savedAt={savedAt}
          pending={livePending}
          versions={versions}
          currentHtml={editor?.getHTML() ?? ""}
          previewId={previewId}
          onPreview={setPreviewId}
          onRestore={restore}
          onAcceptAll={() => accept(livePending.map((s) => s.id))}
          onRejectAll={() => reject(livePending.map((s) => s.id))}
          onOpenSettings={() => setSettingsOpen(true)}
          onAsk={(text) => {
            setQuote(text);
            requestAnimationFrame(() => document.getElementById("composer")?.focus());
          }}
        />
        )}
      </main>

      <CreatePrompt
        file={missingFile}
        onClose={() => setMissingFile(null)}
        onCreate={async (path) => {
          if (pendingSave.current) {
            clearTimeout(pendingSave.current);
            pendingSave.current = undefined;
            await saveToDisk("you", "Manual edits");
          }
          const opened = await desktop.create(path);
          showDocument(opened);
          setOpenError(null);
        }}
      />

      <SettingsDialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        settings={settings}
        update={updateSettings}
        mode={mode}
        setMode={setMode}
        webSearch={webSearch}
        setWebSearch={setWebSearch}
      />

      <ChatPanel
        desktop={desktopMode}
        onRetry={(prompt, q) => void send(prompt, q)}
        onOpenSettings={() => setSettingsOpen(true)}
        messages={messages}
        suggestions={suggestions}
        running={running}
        mode={mode}
        setMode={setMode}
        webSearch={webSearch}
        setWebSearch={setWebSearch}
        quote={quote}
        clearQuote={() => setQuote(null)}
        activeId={activeId}
        setActiveId={setActiveId}
        isStale={isStale}
        onSend={send}
        onStop={stop}
        threads={threadList}
        threadId={threadId}
        threadTitle={shownTitle}
        onNewThread={() => {
          stop();
          startThread();
        }}
        onOpenThread={(id) => threadDoc && void openThread(threadDoc, id)}
        onDeleteThread={(id) => void deleteThread(id)}
        onAccept={(id) => accept([id])}
        onReject={(id) => reject([id])}
        onAcceptMany={accept}
        onLocate={locate}
      />
    </div>
  );
}

function textOf(m: Message) {
  return m.parts.map((p) => (p.type === "text" ? p.text : "")).join("").trim();
}
