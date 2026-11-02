/**
 * Editor threads: the margin conversations about one document. In the desktop app they
 * live in Doc Forge's data folder (src-tauri/src/threads.rs); the browser demo keeps
 * them in localStorage.
 */
import { isDesktop } from "./desktop";
import type { Message, Suggestion } from "./types";

export interface ThreadMeta {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  /** Number of author messages. */
  asks: number;
}

export interface ThreadRecord {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: Message[];
  suggestions: Suggestion[];
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

const LOCAL = (doc: string) => `forge:threads:${doc}`;

function localAll(doc: string): Record<string, ThreadRecord> {
  try {
    return JSON.parse(localStorage.getItem(LOCAL(doc)) ?? "{}");
  } catch {
    return {};
  }
}

function localPut(doc: string, all: Record<string, ThreadRecord>) {
  try {
    localStorage.setItem(LOCAL(doc), JSON.stringify(all));
  } catch {
    // Storage full or unavailable: the demo just forgets.
  }
}

const metaOf = (t: ThreadRecord): ThreadMeta => ({
  id: t.id,
  title: t.title,
  createdAt: t.createdAt,
  updatedAt: t.updatedAt,
  asks: t.messages.filter((m) => m.role === "user").length,
});

export const threads = {
  /** Newest first. */
  async list(doc: string): Promise<ThreadMeta[]> {
    if (isDesktop()) return call<ThreadMeta[]>("thread_list", { doc });
    return Object.values(localAll(doc))
      .map(metaOf)
      .sort((a, b) => b.updatedAt - a.updatedAt);
  },
  async load(doc: string, id: string): Promise<ThreadRecord | null> {
    if (isDesktop()) return call<ThreadRecord>("thread_load", { doc, id }).catch(() => null);
    return localAll(doc)[id] ?? null;
  },
  async save(doc: string, thread: ThreadRecord): Promise<void> {
    if (isDesktop()) return call("thread_save", { doc, thread });
    localPut(doc, { ...localAll(doc), [thread.id]: thread });
  },
  async remove(doc: string, id: string): Promise<void> {
    if (isDesktop()) return call("thread_delete", { doc, id });
    const all = localAll(doc);
    delete all[id];
    localPut(doc, all);
  },
  /** A short title from the opening exchange (the Quick model); rejects when none is set up. */
  async title(prompt: string, reply: string): Promise<string> {
    if (!isDesktop()) throw new Error("No model in the browser demo");
    return call<string>("thread_title", { prompt, reply });
  },
};

/** Until the model names it: the first prompt, cut at a word near 48 characters. */
export function fallbackTitle(prompt: string): string {
  const flat = prompt.replace(/\s+/g, " ").trim();
  const sentence = flat.split(/(?<=[.?!])\s/)[0];
  if (sentence.length <= 48) return sentence.replace(/[.]$/, "");
  const cut = sentence.slice(0, 48);
  const i = cut.lastIndexOf(" ");
  return `${(i > 24 ? cut.slice(0, i) : cut).replace(/[,;:]$/, "")}…`;
}

/** Messages restored from disk never resume streaming. */
export function settled(messages: Message[]): Message[] {
  return messages.map((m) => (m.streaming ? { ...m, streaming: false } : m));
}
