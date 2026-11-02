# Doc Forge

A writing app for Mac with an editor in the margin. You write the manuscript on the left. Forge, an AI editor running on your own model provider, works on the right: it proposes changes as numbered proof marks you accept or decline, reads earlier versions when you ask about them, and searches the web with citations when you let it.

**[docforge.bharathi.fyi](https://docforge.bharathi.fyi)** · Download for Mac (Apple silicon, preview build)

- **Proof marks, not rewrites.** Edits are exact, validated replacements shown in place; nothing changes until you accept it (or switch to Apply directly).
- **Every version kept.** Each file gets its own history in a private git store, separate from any repository it lives in.
- **Threads per document.** Margin conversations are saved per file and titled automatically.
- **`df` in the terminal.** `df notes.md` opens a file the way `code` does for VS Code; the app installs the command.
- **Your models, your keys.** Fireworks, Anthropic, OpenAI, OpenRouter, Ollama or any OpenAI-compatible endpoint. Keys stay in a private file on your Mac. Web search through Tavily, Exa or Firecrawl.

Built with Next.js (static export), Tiptap and Tauri 2; the agent, history store and provider layer are Rust.

```bash
npm install
npm run desktop         # the desktop app, against next dev on :3000
npm run dev             # browser-only demo with a scripted agent, http://localhost:3000
npm run desktop:build   # release bundle
```

In a plain browser a scripted stand-in (`src/lib/mock-agent.ts`) keeps the interface working without keys; the real agent runs in the desktop app.

## Try it
- **Scripted requests:** the mock agent picks a scenario from keywords in your note. Try "tighten the writing", "back this up with sources" (web search), "help me decide the free tier question", or "summarize".
- **Suggest vs Apply directly** (composer toggle). With Suggest, each edit appears in the text as a proof mark: hover it to Accept or Decline, or use the numbered note in the margin. Accept all and Decline all sit in the header. Apply directly writes edits straight into the doc.
- **Select text → Ask the editor** sends that passage to the margin as a quote.
- **History** (header toggle, or ⌥H). A thin timeline beside the manuscript: one dot per version (vermilion for Forge, ink for you). Spacing follows the time between edits, so a day's edits cluster and separate days sit further apart. Hover a dot for who, when, word counts and the first change. Click it to see the diff in place in the manuscript ("Its changes" or "Since then"), then Restore. Manual edits autosave after 2.5s idle.

- **Settings** (gear, bottom left). Appearance: theme (light, dark, system), manuscript typeface, text size, line spacing, line length and the editor's ink colour, all live with a preview. Saved locally and applied before first paint. The Editor section holds the edit mode and web search.
- **Activity marks.** While Forge thinks or writes, a nib draws a short stroke. While it reads or searches, an underline sweeps through three lines.

## Layout
| File | What |
|---|---|
| `src/components/DocForge.tsx` | App state: editor, messages, suggestions, versions |
| `src/components/EditorPane.tsx` | Header, toolbar / compare bar, manuscript, bubble menu, colophon |
| `src/components/HistoryRail.tsx` | Timeline dots, hover popover (portalled to `<body>`) |
| `src/components/DiffView.tsx` | Block and word diff rendering, first-change excerpt |
| `src/components/ChatPanel.tsx` | The margin: exchange log, sources, numbered marks, composer |
| `src/lib/history.ts` | Timeline spacing and date labels |
| `src/lib/settings.ts` | Appearance options, CSS-variable application, pre-paint boot script |
| `src/components/SettingsDialog.tsx` | Two-pane settings dialog |
| `src/components/Loaders.tsx` | Scribe and Proofread activity marks |
| `src/lib/desktop.ts` | Frontend bridge to the Tauri commands |
| `src/components/Welcome.tsx` | Desktop start screen (open, recent files) |
| `src-tauri/shadow/` | Shadow-git store (Rust, plumbing only) |
| `src-tauri/src/lib.rs` | Tauri commands: `doc_open`, `doc_save`, `doc_sync`, `doc_restore`, `doc_recent` |
| `src-tauri/src/ai_commands.rs` | Tauri commands for providers, roles, search keys, `agent_run` / `agent_cancel` |
| `src-tauri/ai/` | `forge-ai`: provider catalog, credentials file, model discovery, search, the agent loop |
| `src/lib/ai.ts`, `src/lib/agent.ts` | Frontend bridges for provider settings and agent runs |
| `src/components/ModelsSettings.tsx` | Settings → Models |
| `src/components/Segmented.tsx` | Text toggle with a sliding underline |
| `src/lib/suggestions-extension.ts` | ProseMirror plugin that renders suggestions as inline decorations |
| `src/lib/diff.ts` | HTML → blocks → block diff and word diff |
| `src/lib/mock-agent.ts` | Scripted agent |
| `src/components/ThreadMenu.tsx`, `src/lib/threads.ts` | Editor threads: list, switch, save, titles |
| `src/components/CreatePrompt.tsx` | "This file doesn't exist yet" prompt for `df` |
| `src/components/CliInstall.tsx` | Install / remove the `df` command |
| `src-tauri/src/launch.rs` | Files sent from outside: command line, macOS open events, second launch |
| `src-tauri/src/cli.rs` | `df` installer |
| `src-tauri/src/threads.rs` | Thread storage |
| `src-tauri/ai/src/agent.rs` | The agent loop, tools, streaming adapters |
| `src-tauri/ai/src/history.rs` | Version list and paragraph diffs for the agent |
| `src-tauri/ai/src/oauth.rs` | Browser sign-in (OAuth 2.1, MCP authorization spec) |
| `bin/df` | The `df` script |
| `site/` | Landing page and its deploy kit |

## Desktop app (Tauri)

```bash
npm run desktop         # dev: launches the native window against next dev on :3000
npm run desktop:build   # release bundle (.app / .dmg) from the static export in ./out
```

`src-tauri/rust-toolchain.toml` pins Rust 1.90 for this project only (Tauri 2.12 needs it); rustup fetches it on first use.

The app icon (a two-stem pilcrow in Source Serif 4 Black on proofreader vermilion) is drawn in [`src-tauri/icons/source/icon.html`](src-tauri/icons/source/icon.html); render it to 1024px and run `npx tauri icon src-tauri/icons/source/icon-1024.png` to regenerate every size.

In the desktop app, documents are real files on disk (Markdown). Open one with ⌘O or from the start screen.

### Landing page

[`site/`](site/) is the product page: static HTML, CSS and one script, with no build step or dependencies, on the app's own design tokens. One idea per screen, each a short title, one sentence and the product itself: the headline proofreads itself ("assistant" struck, "editor" inked in), a miniature of the app replays a Forge exchange, then marks to accept, a version timeline to scrub, and a terminal running `df`. Scenes run only while on screen and settle to their final state under reduced motion.

```bash
npm run site:dmg   # build the app and package site/downloads/Doc-Forge.dmg (hdiutil; app + Applications shortcut)
npm run site       # serve site/ on http://127.0.0.1:4100
```

Live at **https://docforge.bharathi.fyi**. `DOCFORGE_REMOTE=root@<server> sh site/deploy/deploy.sh` publishes a release (`--setup` the first time: Let's Encrypt certificate over Cloudflare DNS, then a proxied DNS record). It uses a single SSH connection (ControlMaster), since the VM rate-limits SSH. Each release lands in `/var/www/docforge/releases/<timestamp>` and `current` is swapped atomically; the last 5 are kept. Hardening, in [`site/deploy/nginx/`](site/deploy/nginx/):

- Only Cloudflare can reach the origin (shared gate with learn.bharathi.fyi); real client IPs are restored for logs and limits. Direct requests to the IP get 403.
- GET and HEAD only; nothing is served outside an explicit allowlist of paths (no listings, no dotfiles); per-IP request and connection limits, tighter on the DMG.
- Strict CSP generated per release (`default-src 'none'`, inline script allowed only by hash, fonts self-hosted so no third-party requests), HSTS, `frame-ancestors 'none'`, nosniff, COOP/CORP, a locked-down Permissions-Policy.
- The DMG is served as an attachment with a published SHA-256 next to it. An nginx config that fails `nginx -t` is rolled back, so other sites on the box never inherit a broken reload.

The DMG is ad-hoc signed. Public downloads need a Developer ID signature and notarization, or macOS will ask people to approve the first launch in System Settings (the page says so under the button).

### The `df` command

The app installs it: the start screen offers **Install** until it's set up, and Settings → Editor → Terminal command installs, re-points or removes it ([`src-tauri/src/cli.rs`](src-tauri/src/cli.rs)). It links the script shipped in the bundle into `/opt/homebrew/bin`, or `/usr/local/bin` through the standard macOS administrator prompt, and never replaces a `df` it didn't put there. Like `code` for VS Code: `df notes.md` opens the file in Doc Forge, launching it if needed, or switches the open window to it. For a file that doesn't exist yet, Doc Forge asks whether to create it (the script itself never creates files; it starts the app binary directly, since `open` refuses missing paths).

```bash
ln -s "$PWD/bin/df" /opt/homebrew/bin/df   # any PATH folder that comes before /bin
```

`df` is also the system's disk-usage command, so [`bin/df`](bin/df) hands anything that isn't a list of documents to `/bin/df` untouched: no arguments, flags (`df -h`), folders (`df .`), devices. It targets the installed app (`/Applications` or `~/Applications`) through `open -a`; without one, it hands files to a running `npm run desktop`. Inside the app, paths arrive from the command line, from macOS open events, or from a second launch (tauri-plugin-single-instance), and wait in a queue until the page is ready ([`src-tauri/src/launch.rs`](src-tauri/src/launch.rs)).

### Shadow history
Every opened file gets its own **bare git repository** in Doc Forge's app-data folder (`~/Library/Application Support/app.docforge.desktop/history/repos/<id>.git` on macOS), keyed by a document id rather than the path. Snapshots are written with git plumbing only (`hash-object`, `mktree`, `commit-tree`, `update-ref`), fed from the file's bytes:

- Git is never pointed at the user's folder, so their repository, hooks, `.gitignore`, `.gitattributes`, filters and config are never read. Global/system git config and `GIT_*` environment variables are cut out of every call.
- Nothing is written next to the user's file except the file itself.
- Each version in the timeline is a commit: author `You` or `Forge`, the version label as the subject. Unchanged content never creates a commit. Ref updates are compare-and-swap with a reflog.
- Edits made in other apps are picked up when the window regains focus ("Edited outside Doc Forge"). Pending typing is saved when it loses focus.
- A file moved or renamed while Doc Forge was closed is matched back to its history by content.

The store is a separate crate, `src-tauri/shadow`, with no Tauri dependency: `cargo test -p forge-shadow` (from `src-tauri/`) runs the integration tests, including one that opens a file inside a hostile repository (hooks, filters, ignore rules, a stray `GIT_DIR`) and checks it stays untouched.

## Models (Settings → Models)

- **Roles.** *Editor* does drafting, edits and research, so it needs a model that can use tools. *Quick* handles titles and summaries, and uses the Editor model unless you pick one.
- **Providers.** Fireworks, Anthropic, OpenAI, OpenRouter, Ollama (local), and any OpenAI-compatible address. Every address can be overridden for gateways.
- **Keys** live in `credentials.json` in the app's config folder (`~/Library/Application Support/app.docforge.desktop/`), created with mode 600 and written atomically; no system keychain, so no permission prompts. Keys can also come from `FIREWORKS_API_KEY` and similar environment variables. In debug builds the app also reads `.env.local`. A key is only saved after the provider accepts it. Keys never reach the web layer; the frontend sees a hint like `…nJni`.
- **Models are discovered live** from each provider's `/models` endpoint, keeping the capability flags providers report: tool use, vision, context window, routers. Embedders and rerankers are filtered out.
- **Send a test message** round-trips through the Editor model and shows the reply, response time, and tokens split into answer and reasoning.
- **Web search** keys (Tavily, Exa or Firecrawl) live in the same file, and are verified with a real query before they're saved. With Firecrawl, Forge also reads linked pages through Firecrawl's renderer (Markdown, main content, JavaScript pages included), falling back to a direct fetch. Firecrawl also offers **Sign in with Firecrawl** instead of a key: browser OAuth 2.1 as the MCP authorization spec describes it ([`ai/src/oauth.rs`](src-tauri/ai/src/oauth.rs)): discovery from the API's own 401/403 challenge (RFC 9728 → RFC 8414), dynamic registration as a public client with an exact loopback redirect, PKCE S256, `state`, `resource` binding (RFC 8707), and single-use rotating refresh tokens refreshed one at a time. The token lives in the credentials file; Sign out revokes it on Firecrawl's side. `tests/oauth_flow.rs` runs the whole flow against a strict fake authorization server.
- **Connecting from the terminal**: `cargo run -p forge-ai --example connect -- fireworks <editor-model> [quick-model]` (run in `src-tauri` with the provider's key variable set) does what Settings does: verify the key, save it in the credentials file, and choose the roles.

## Forge, the agent

The loop runs in Rust (`src-tauri/ai/src/agent.rs`) and streams to the margin over a Tauri Channel: reasoning, text, tool lines, proof marks. A run can be stopped at any point.

- The model sees the manuscript as numbered plain-text blocks, the same text the proof marks match against, so a valid edit always lands.
- **`propose_edits`**: exact find/replace inside one block. `find` must match exactly once. If it doesn't, the model gets an error it can act on: "Found 2 times… include more surrounding words", "close match in block 4 differs in spacing", "overlaps an edit you already proposed".
- **`insert_paragraphs`** adds new paragraphs after a block. These appear as a proposed block in the insertion ink.
- **`web_search`** (with a search key) and **`fetch_url`** (refuses local and private addresses) when Web search is on. Sources are numbered once per reply, so citations stay stable.
- **`list_versions`** and **`read_version`** (whenever the document has saved versions): Forge can list the shadow history (v1, v2… as the rail numbers them, with local times, author, label and words changed), read any version, and diff it against another version or the current text, paragraph by paragraph ([`ai/src/history.rs`](src-tauri/ai/src/history.rs)). Old text comes back through the usual tools, as proof marks; an inserted paragraph starting with `## ` becomes a heading.
- **Suggest** shows edits as proof marks. **Apply directly** writes them at once, and each is still a history snapshot.
- Each turn also sends the conversation so far, including which of Forge's edits were accepted or declined.
- The system prompt carries context: the author's date (for dating searches and claims), the file and its folder, what Doc Forge is (history, proof marks, the `df` command), and to read terse asides in light of the conversation, asking one short question when still unsure.

### Editor threads

Each document keeps its margin conversations as threads, stored per document in Doc Forge's data folder (`threads/<doc id>/<thread>.json`, [`src-tauri/src/threads.rs`](src-tauri/src/threads.rs)); the browser demo keeps them in localStorage. Opening a document resumes its latest thread. The thread's title in the margin's header lists the others (newest first, with time and number of asks; delete takes two clicks). A thread saves once a reply settles and whenever its marks change, and keeps its own undecided marks: they leave the manuscript when you switch away and return with the thread if their text still exists. The Quick model titles a thread after its first exchange; until then, the first prompt stands in.
- Adapters: OpenAI-style chat completions (Fireworks, OpenAI, OpenRouter, Ollama, custom) and Anthropic Messages, both streaming with tool use.

### Tests
From `src-tauri/`:
```bash
cargo test --workspace                                   # unit + store tests (live tests skip without keys)
set -a; . ../.env.local; set +a
cargo test -p forge-ai --test live -- --nocapture        # provider config against Fireworks
cargo test -p forge-ai --test agent_live -- --nocapture --test-threads=1   # Forge end to end
```
`FORGE_TEST_MODEL` picks the model for the agent tests. `FORGE_DUMP_DIR` saves timed recordings of real runs, for replaying in UI tests.

## Motion
All motion lives in `src/app/globals.css` under *Proof marks* and *Microinteractions*. It uses transform and opacity with strong ease-out curves, stays under 300ms, and skips the entrance when history is opened with the keyboard. `prefers-reduced-motion` keeps only fades.

## Known mock limitations
- A suggestion targets text by exact match within one paragraph (`find`/`replace`). If that text has changed, the card shows "Text changed" and can't be applied. A real implementation would anchor suggestions to positions mapped through transactions.
- Accepting an edit replaces the range as plain text, so inline formatting inside that range is dropped.
- Versions are kept in memory and don't persist.
