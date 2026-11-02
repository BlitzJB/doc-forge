/**
 * Bridge to the provider configuration in src-tauri/ai. Keys are sent once to be
 * verified and saved in Doc Forge's private credentials file; they never come back to the frontend.
 */

export type Status = "connected" | "unauthorized" | "unreachable" | "error";
export type RoleName = "editor" | "quick";

export interface ModelInfo {
  id: string;
  name: string;
  contextLength: number | null;
  tools: boolean | null;
  vision: boolean | null;
  router: boolean;
}

export interface Check {
  status: Status;
  message: string;
  latencyMs: number;
  models: ModelInfo[];
}

export interface KindInfo {
  label: string;
  defaultBaseUrl: string;
  envVar: string | null;
  needsKey: boolean;
  keyUrl: string | null;
  keyPlaceholder: string;
  local: boolean;
}

export interface ProviderView {
  id: string;
  kind: string;
  name: string;
  baseUrl: string;
  info: KindInfo;
  keySource: "stored" | "environment" | "none";
  keyHint: string | null;
  removable: boolean;
  check: Check | null;
}

export interface RoleChoice {
  provider: string;
  model: string;
}

export interface SearchView {
  kind: "tavily" | "exa" | "firecrawl";
  label: string;
  keySource: "stored" | "environment" | "none";
  keyHint: string | null;
  envVar: string;
  keyUrl: string;
  keyPlaceholder: string;
  /** Offers browser sign-in (OAuth). */
  signIn: boolean;
  signedIn: boolean;
}

export interface AiView {
  providers: ProviderView[];
  roles: { editor: RoleChoice | null; quick: RoleChoice | null };
  search: SearchView[];
}

export interface TestResult {
  status: Status;
  reply: string;
  reasoned: boolean;
  latencyMs: number;
  usage: { inputTokens: number | null; outputTokens: number | null; reasoningTokens: number | null };
  message: string;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const ai = {
  state: () => call<AiView>("ai_state"),
  check: (id: string) => call<Check>("ai_check", { id }),
  saveKey: (id: string, key: string) => call<Check>("ai_save_key", { id, key }),
  forgetKey: (id: string) => call<void>("ai_forget_key", { id }),
  setBaseUrl: (id: string, baseUrl: string) => call<void>("ai_set_base_url", { id, baseUrl }),
  addCustom: (name: string, baseUrl: string) => call<string>("ai_add_custom", { name, baseUrl }),
  removeProvider: (id: string) => call<void>("ai_remove_provider", { id }),
  setRole: (role: RoleName, choice: RoleChoice | null) =>
    call<void>("ai_set_role", { role, provider: choice?.provider ?? null, model: choice?.model ?? null }),
  test: (provider: string, model: string) => call<TestResult>("ai_test", { provider, model }),
  /** Verifies with a real query; resolves to the number of results, rejects with a readable error. */
  saveSearchKey: (kind: SearchView["kind"], key: string) => call<number>("ai_save_search_key", { kind, key }),
  forgetSearchKey: (kind: SearchView["kind"]) => call<void>("ai_forget_search_key", { kind }),
  /** Opens the browser and resolves once it comes back; to the test search's result count. */
  searchSignIn: (kind: SearchView["kind"]) => call<number>("ai_search_sign_in", { kind }),
  cancelSignIn: () => call<void>("ai_cancel_sign_in"),
  searchSignOut: (kind: SearchView["kind"]) => call<void>("ai_search_sign_out", { kind }),
  async openUrl(url: string) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  },
};

/** 1048576 → "1M", 262144 → "256K". */
export function formatContext(n: number | null): string | null {
  if (!n) return null;
  if (n >= 1_000_000) return `${Math.round(n / 1_048_576) || 1}M`;
  return `${Math.round(n / 1024)}K`;
}
