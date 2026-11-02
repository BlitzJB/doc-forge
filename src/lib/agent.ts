/**
 * Bridge to the Rust agent (src-tauri/ai/src/agent.rs). Runs stream over a Tauri
 * Channel; the model call, keys and tools all stay on the Rust side.
 */
import type { SearchResult } from "./types";

export interface DocBlock {
  kind: "h1" | "h2" | "h3" | "p" | "li" | "oli" | "quote" | "code";
  text: string;
}

export interface AgentRequest {
  prompt: string;
  quote: string | null;
  document: { name: string; folder?: string; blocks: DocBlock[]; markdown?: string };
  /** The open document, whose saved versions Forge may read. */
  docId?: string;
  /** Minutes east of UTC, so version times are the author's local times. */
  utcOffset?: number;
  /** The author's local date, so searches and claims are dated correctly. */
  today?: string;
  history: { role: "user" | "assistant"; content: string }[];
  web: boolean;
  applyDirectly: boolean;
}

export interface ProposedEdit {
  kind: "replace" | "insert";
  find: string;
  replace: string;
  reason: string;
}

export type AgentEvent =
  | { type: "reasoning"; delta: string }
  | { type: "text"; delta: string }
  | { type: "toolStart"; id: string; tool: "read_document" | "web_search" | "fetch_url" | "history"; label: string }
  | { type: "toolEnd"; id: string; ok: boolean; summary: string; sources: (SearchResult & { n: number })[] }
  | { type: "edits"; edits: ProposedEdit[] }
  | { type: "done"; usage: { inputTokens: number | null; outputTokens: number | null; reasoningTokens: number | null }; steps: number; stopped: boolean }
  | { type: "error"; message: string };

export interface AgentRun {
  id: string;
  cancel: () => Promise<void>;
  /** Resolves when the run ends (done or error). */
  finished: Promise<void>;
}

export async function startAgent(request: AgentRequest, onEvent: (e: AgentEvent) => void): Promise<AgentRun> {
  const { Channel, invoke } = await import("@tauri-apps/api/core");
  let resolve!: () => void;
  const finished = new Promise<void>((r) => (resolve = r));
  const channel = new Channel<AgentEvent>();
  channel.onmessage = (e) => {
    onEvent(e);
    if (e.type === "done" || e.type === "error") resolve();
  };
  const id = await invoke<string>("agent_run", { request, channel });
  return { id, finished, cancel: () => invoke("agent_cancel", { id }) };
}

/** The editor's blocks as the agent sees them: plain text, the same text proof marks match against. */
export function snapshotBlocks(doc: import("@tiptap/pm/model").Node): DocBlock[] {
  const blocks: DocBlock[] = [];
  doc.descendants((node, _pos, parent) => {
    if (!node.isTextblock) return true;
    let kind: DocBlock["kind"] = "p";
    if (node.type.name === "heading") kind = (`h${Math.min(3, node.attrs.level ?? 1)}` as DocBlock["kind"]);
    else if (node.type.name === "codeBlock") kind = "code";
    else if (parent?.type.name === "listItem") kind = "li";
    else if (parent?.type.name === "blockquote") kind = "quote";
    blocks.push({ kind, text: node.textContent });
    return false;
  });
  return blocks;
}
