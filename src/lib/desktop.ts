/**
 * Bridge to the Tauri side (src-tauri). Only used inside the desktop app; the browser
 * build keeps working with mock history. Tauri modules load lazily so they never run
 * outside the desktop shell.
 */
import { marked } from "marked";
import type { Version } from "./types";

export interface DesktopVersion {
  sha: string;
  parent: string | null;
  author: "you" | "agent";
  label: string;
  body: string;
  createdAt: number;
  content: string;
}

export interface OpenedDoc {
  id: string;
  path: string;
  name: string;
  folder: string;
  content: string;
  history: DesktopVersion[];
}

export interface Synced {
  changed: boolean;
  content: string;
  history: DesktopVersion[];
}

export interface RecentDoc {
  id: string;
  path: string;
  name: string;
  folder: string;
  exists: boolean;
  openedAt: number;
}

export const isDesktop = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const desktop = {
  /** Native open dialog, then start tracking the chosen file. Null if cancelled. */
  async pickAndOpen(): Promise<OpenedDoc | null> {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Documents", extensions: ["md", "markdown", "mdx", "txt"] }],
    });
    if (!path) return null;
    return call<OpenedDoc>("doc_open", { path });
  },
  open: (path: string) => call<OpenedDoc>("doc_open", { path }),
  save: (id: string, content: string, message: string, author: "you" | "agent") =>
    call<DesktopVersion[]>("doc_save", { id, content, message, author }),
  sync: (id: string) => call<Synced>("doc_sync", { id }),
  restore: (id: string, sha: string, message: string) => call<Synced>("doc_restore", { id, sha, message }),
  recent: () => call<RecentDoc[]>("doc_recent"),
  /** What's at a path sent from outside: a file, or why it can't be opened as one. */
  probe: (path: string) => call<"file" | "missing" | "no-folder" | "not-a-file">("doc_probe", { path }),
  /** Create an empty document (never overwrites) and start tracking it. */
  create: (path: string) => call<OpenedDoc>("doc_create", { path }),
  /** The `df` terminal command: where it's linked, and whether a foreign df is in the way. */
  cliStatus: () => call<{ installedAt: string | null; current: boolean; foreign: string | null }>("cli_status"),
  /** Links the bundled `df`; may show the macOS administrator prompt. Resolves to the link path. */
  cliInstall: () => call<string>("cli_install"),
  cliUninstall: () => call<void>("cli_uninstall"),
  /** Files sent from outside the window (the `df` command, Finder), each returned once. */
  takeOpens: () => call<string[]>("take_opens"),
};

/** Shadow-git snapshots → the timeline's Version shape (HTML for diffing). */
export function toVersions(history: DesktopVersion[]): Version[] {
  return history.map((v, i) => ({
    id: v.sha,
    n: i + 1,
    createdAt: v.createdAt,
    author: v.author,
    label: v.label,
    html: marked.parse(v.content, { async: false, gfm: true }),
  }));
}
