export type SuggestionStatus = "pending" | "accepted" | "rejected" | "applied";

export interface Suggestion {
  id: string;
  messageId: string;
  find: string;
  replace: string;
  reason: string;
  status: SuggestionStatus;
  /** replace: swap `find` for `replace`. insert: add `replace` (paragraphs) after the block containing `find`. */
  kind?: "replace" | "insert";
}

export interface SearchResult {
  /** Citation number, stable within one reply. */
  n?: number;
  title: string;
  url: string;
  domain: string;
  snippet: string;
}

export type ToolName = "web_search" | "read_document" | "fetch_url" | "history";

export type Part =
  | { type: "text"; text: string }
  | { type: "reasoning"; text: string; done: boolean; seconds?: number }
  | {
      type: "tool";
      /** Matches tool start/end events from the agent. */
      id?: string;
      tool: ToolName;
      status: "running" | "done";
      ok?: boolean;
      query?: string;
      results?: SearchResult[];
      summary?: string;
    }
  | { type: "suggestions"; ids: string[] }
  /** The run failed; `retry` re-sends the prompt. */
  | { type: "error"; message: string; retry?: { prompt: string; quote: string | null } }
  /** Forge needs a model before it can work. */
  | { type: "setup"; message: string };

export interface Message {
  id: string;
  role: "user" | "assistant";
  parts: Part[];
  quote?: string | null;
  createdAt: number;
  streaming?: boolean;
}

export interface Version {
  id: string;
  n: number;
  createdAt: number;
  author: "you" | "agent";
  label: string;
  html: string;
}

export type EditMode = "suggest" | "auto";

export const uid = () => Math.random().toString(36).slice(2, 10);
