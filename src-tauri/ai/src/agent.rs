//! Forge, the editing agent.
//!
//! A streaming tool-use loop that runs entirely on the Rust side (keys never reach the
//! web layer). The model sees the manuscript as numbered plain-text blocks, the same text
//! the editor's proof-mark matcher searches, and changes it only through tools:
//!
//! - `propose_edits`: exact find/replace inside one block. `find` must match exactly once;
//!   errors tell the model how to fix the call (the str_replace discipline).
//! - `insert_paragraphs`: new paragraphs after a block.
//! - `web_search` / `fetch_url`: when the user allows web access.
//!
//! Two wire adapters: OpenAI-style chat completions (Fireworks, OpenAI, OpenRouter,
//! Ollama, custom) and Anthropic Messages.

use crate::history::{self, Side, Snapshot};
use crate::search::{self, SearchHit, SearchKind};
use crate::{classify, Kind, Protocol, Usage};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const MAX_STEPS: u32 = 10;
const MAX_TOKENS: u32 = 8192;
const SEARCH_RESULTS: usize = 5;
const PAGE_CHARS: usize = 12_000;

// ---- Request / events ---------------------------------------------------------------

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DocBlock {
    /// h1 | h2 | h3 | p | li | oli | quote | code
    pub kind: String,
    pub text: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DocSnapshot {
    pub name: String,
    /// The folder the file lives in, for context (not a path the agent can use).
    #[serde(default)]
    pub folder: Option<String>,
    pub blocks: Vec<DocBlock>,
    /// The editor's current text as Markdown, so it compares like for like with saved versions.
    #[serde(default)]
    pub markdown: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// "user" | "assistant"
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentRequest {
    pub prompt: String,
    #[serde(default)]
    pub quote: Option<String>,
    pub document: DocSnapshot,
    #[serde(default)]
    pub history: Vec<Turn>,
    #[serde(default)]
    pub web: bool,
    #[serde(default)]
    pub apply_directly: bool,
    /// The author's local date, e.g. "Monday, 2 November 2026".
    #[serde(default)]
    pub today: Option<String>,
    /// The author's UTC offset in minutes (east positive), for version times.
    #[serde(default)]
    pub utc_offset: Option<i32>,
    /// The open document, whose saved versions the agent may read.
    #[serde(default)]
    pub doc_id: Option<String>,
}

/// Saved versions plus what "current" means, for the history tools.
pub struct HistoryCtx<'a> {
    pub versions: &'a [Snapshot],
    pub current: Option<&'a str>,
    pub offset: i32,
}

pub struct ModelTarget {
    pub kind: Kind,
    pub base_url: String,
    pub key: Option<String>,
    pub model: String,
}

pub struct SearchTarget {
    pub kind: SearchKind,
    pub key: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EditKind {
    Replace,
    Insert,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProposedEdit {
    pub kind: EditKind,
    /// Replace: the exact text to replace. Insert: the full text of the block to insert after.
    pub find: String,
    /// Replace: the new text. Insert: the new paragraphs, separated by blank lines.
    pub replace: String,
    pub reason: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub n: usize,
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentEvent {
    Reasoning {
        delta: String,
    },
    Text {
        delta: String,
    },
    ToolStart {
        id: String,
        tool: String,
        label: String,
    },
    ToolEnd {
        id: String,
        ok: bool,
        summary: String,
        sources: Vec<Source>,
    },
    Edits {
        edits: Vec<ProposedEdit>,
    },
    Done {
        usage: Usage,
        steps: u32,
        stopped: bool,
    },
    Error {
        message: String,
    },
}

// ---- Prompt -------------------------------------------------------------------------

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "h1" => "title",
        "h2" => "heading",
        "h3" => "subheading",
        "li" | "oli" => "list item",
        "quote" => "quote",
        "code" => "code",
        _ => "paragraph",
    }
}

pub fn render_document(doc: &DocSnapshot, blocks: &[DocBlock]) -> String {
    let mut s = format!("<document name=\"{}\">\n", doc.name.replace('"', "'"));
    for (i, b) in blocks.iter().enumerate() {
        s.push_str(&format!(
            "[{}] {}: {}\n",
            i + 1,
            kind_label(&b.kind),
            b.text
        ));
    }
    s.push_str("</document>");
    s
}

pub fn system_prompt(req: &AgentRequest, search_available: bool, versions: usize) -> String {
    let (web, apply_directly) = (req.web, req.apply_directly);
    let history = if versions > 0 {
        format!("- This document has {versions} saved versions (v1 is the first, v{versions} the latest). When the author mentions earlier drafts, asks what changed, or wants something back, use list_versions and read_version instead of guessing. To bring old text back, propose it as an edit. Refer to versions the way the author sees them, like \"v4, yesterday at 16:40\".\n")
    } else {
        String::new()
    };
    let today = req
        .today
        .as_deref()
        .map(|t| format!("- Today is {t}. When facts may have changed, prefer recent sources and say how current they are.\n"))
        .unwrap_or_default();
    let file = match req.document.folder.as_deref().filter(|f| !f.is_empty()) {
        Some(f) => format!("{} (in {})", req.document.name, f),
        None => req.document.name.clone(),
    };
    let mode = if apply_directly {
        "Your edits are applied to the manuscript immediately and recorded in its history."
    } else {
        "Your edits appear in the manuscript as proof marks the author accepts or declines."
    };
    let evidence = match (web, search_available) {
        (true, true) => "When a claim needs evidence, search the web and read the best sources. Cite what you use with its source number in square brackets, like [2], in your reply and in any sentence you add to the manuscript. Only cite sources you actually read in this conversation.",
        (true, false) => "You can read web pages the author mentions with fetch_url, but you can't search. If a request needs outside facts you can't verify, say so instead of guessing.",
        (false, _) => "You have no web access in this conversation. If a request needs outside facts, say so instead of guessing.",
    };
    format!(
        "You are Forge, an editor working in the margin of the author's manuscript in Doc Forge.

Context
{today}- The manuscript is the file {file}. The author writes on the left; you sit in the margin on the right.
- Doc Forge is a desktop writing app. It keeps its own version history of every file it opens (separate from any git repository the file is in), shows your edits as numbered proof marks, and opens files from the terminal with the `df` command (`df notes.md`; anything else, like `df -h`, still runs the system's disk-free command).
{history}- The author's messages may be about the manuscript, about Doc Forge, or general questions. Read short or ambiguous messages in light of the conversation so far and the manuscript. If what they mean is still unclear, ask one short question rather than guessing or searching.

How you work
- The manuscript is given as numbered blocks inside <document>. It is the current text; block numbers are only for your tools, so never mention them to the author.
- Change the manuscript only through tools. Never paste a rewritten passage into your reply.
- {mode}
- Make the smallest edits that achieve the request. Keep the author's voice, terminology, facts and structure. Leave passages you weren't asked about alone.
- propose_edits: `find` is copied exactly from a single block, as plain text without Markdown, and is long enough to occur exactly once. `replace` is the complete new text for that span. Prefer one edit per sentence or clause over rewriting a whole paragraph.
- insert_paragraphs adds new paragraphs after a block. Use it for genuinely new material.
- If a tool reports a problem, correct the call and try again.
- Never invent facts, figures, quotes or sources. {evidence}
- If the author asks a question rather than for changes, answer it without editing.

Your reply
- Write to the author like a thoughtful editor: brief and specific. Usually one to four sentences, or a few \"- \" bullets when listing points.
- Plain text only. Use **bold** sparingly. No headings, tables or code blocks.
- Don't use em dashes; use commas, colons or parentheses. In the manuscript, only use dashes if the author's own text does.
- After proposing edits, say in a sentence or two what you changed and why. The edits are shown to the author separately, so don't repeat them word for word."
    )
}

fn tool_specs(
    search_available: bool,
    web: bool,
    has_history: bool,
) -> Vec<(&'static str, &'static str, Value)> {
    let mut tools = vec![
        (
            "propose_edits",
            "Propose targeted replacements in the manuscript. Each `find` must be copied exactly from one block (plain text, no Markdown) and must occur exactly once in the whole document; include surrounding words if needed. `replace` is the full new text for that span and must not contain line breaks.",
            json!({
                "type": "object",
                "properties": {
                    "edits": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "find": { "type": "string", "description": "Exact text to replace, from a single block." },
                                "replace": { "type": "string", "description": "Replacement text." },
                                "reason": { "type": "string", "description": "A few words on why, shown to the author (e.g. \"Cuts hedging\")." }
                            },
                            "required": ["find", "replace", "reason"]
                        }
                    }
                },
                "required": ["edits"]
            }),
        ),
        (
            "insert_paragraphs",
            "Propose new paragraphs after an existing block. Use for new material such as an added example, caveat or section text.",
            json!({
                "type": "object",
                "properties": {
                    "after_block": { "type": "integer", "description": "Number of the block to insert after." },
                    "paragraphs": { "type": "array", "minItems": 1, "items": { "type": "string" }, "description": "One string per new paragraph, plain text. A section heading is its own item starting with \"## \" (or \"### \" for a subsection)." },
                    "reason": { "type": "string", "description": "A few words on why, shown to the author." }
                },
                "required": ["after_block", "paragraphs", "reason"]
            }),
        ),
    ];
    if has_history {
        tools.push((
            "list_versions",
            "List this document's saved versions, newest first: number, local time, who saved it (author or Forge), its label, length, and how many words changed from the version before.",
            json!({ "type": "object", "properties": {} }),
        ));
        tools.push((
            "read_version",
            "Read one saved version as Markdown, or compare it. With `compare_to`, returns a paragraph-level diff from `version` to `compare_to` (\"current\" for the text as it is now, or another version number).",
            json!({
                "type": "object",
                "properties": {
                    "version": { "type": "integer", "description": "Version number (1 is the first)." },
                    "compare_to": { "description": "\"current\" or a version number. Omit to read the version in full.", "anyOf": [{ "type": "string", "enum": ["current"] }, { "type": "integer" }] }
                },
                "required": ["version"]
            }),
        ));
    }
    if web && search_available {
        tools.push((
            "web_search",
            "Search the web. Returns numbered sources with snippets; cite them by number.",
            json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
        ));
    }
    if web {
        tools.push((
            "fetch_url",
            "Read a public web page as text, e.g. a search result worth checking in full.",
            json!({ "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] }),
        ));
    }
    tools
}

// ---- Edit validation --------------------------------------------------------------------

/// Checks proposed edits against the document the model was shown.
pub struct EditBook {
    blocks: Vec<DocBlock>,
    claimed: Vec<(usize, usize, usize)>,
}

fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

impl EditBook {
    pub fn new(blocks: Vec<DocBlock>) -> Self {
        EditBook {
            blocks,
            claimed: vec![],
        }
    }

    pub fn replace(&mut self, find: &str, replace: &str) -> Result<(), String> {
        if find.is_empty() {
            return Err("`find` is empty.".into());
        }
        if find.contains('\n') {
            return Err("`find` spans more than one block. Edit one block at a time.".into());
        }
        if replace.contains('\n') {
            return Err("`replace` contains a line break. Keep replacements inside the paragraph, and use insert_paragraphs for new paragraphs.".into());
        }
        if find == replace {
            return Err("`replace` is identical to `find`, so nothing would change.".into());
        }
        let hits: Vec<(usize, usize)> = self
            .blocks
            .iter()
            .enumerate()
            .flat_map(|(bi, b)| {
                b.text
                    .match_indices(find)
                    .map(move |(at, _)| (bi, at))
                    .collect::<Vec<_>>()
            })
            .collect();
        match hits.len() {
            0 => {
                let needle = normalize(find);
                let near = self
                    .blocks
                    .iter()
                    .position(|b| normalize(&b.text).contains(&needle));
                Err(match near {
                    Some(bi) => format!(
                        "Not found exactly, but block {} has a close match that differs in spacing, capitalisation or punctuation. Copy the text exactly as it appears there.",
                        bi + 1
                    ),
                    None => "Not found. Copy `find` exactly from a single block, as plain text without Markdown formatting.".into(),
                })
            }
            1 => {
                let (bi, start) = hits[0];
                let end = start + find.len();
                if self
                    .claimed
                    .iter()
                    .any(|&(b, s, e)| b == bi && start < e && s < end)
                {
                    return Err("Overlaps an edit you already proposed in this reply. Combine them into one edit.".into());
                }
                self.claimed.push((bi, start, end));
                Ok(())
            }
            n => {
                let mut where_: Vec<String> =
                    hits.iter().map(|(b, _)| (b + 1).to_string()).collect();
                where_.dedup();
                Err(format!(
                    "Found {n} times (block{} {}). Include more surrounding words so it matches exactly once.",
                    if where_.len() == 1 { "" } else { "s" },
                    where_.join(", ")
                ))
            }
        }
    }

    /// Returns the anchor text (the block's full text) for the insertion.
    pub fn insert(
        &mut self,
        after_block: i64,
        paragraphs: &[String],
    ) -> Result<(String, String), String> {
        let n = self.blocks.len() as i64;
        if after_block < 1 || after_block > n {
            return Err(format!(
                "There is no block {after_block}. Blocks are numbered 1 to {n}."
            ));
        }
        let paras: Vec<String> = paragraphs
            .iter()
            .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|p| !p.is_empty())
            .collect();
        if paras.is_empty() {
            return Err("`paragraphs` is empty.".into());
        }
        let anchor = self.blocks[(after_block - 1) as usize].text.clone();
        let repeats = self
            .blocks
            .iter()
            .filter(|b| b.text.contains(&anchor))
            .count();
        if repeats > 1 {
            return Err(format!("Block {after_block} repeats text found elsewhere, so it can't anchor an insertion. Insert after a neighbouring block instead."));
        }
        Ok((anchor, paras.join("\n\n")))
    }
}

// ---- Conversation (provider-neutral) -----------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Call {
    id: String,
    name: String,
    args: String,
}

#[derive(Clone, Debug)]
enum Msg {
    User(String),
    Assistant { text: String, calls: Vec<Call> },
    Results(Vec<(Call, String)>),
}

struct Step {
    text: String,
    calls: Vec<Call>,
    usage: Usage,
}

fn add_usage(total: &mut Usage, u: &Usage) {
    let add = |a: &mut Option<u64>, b: Option<u64>| {
        if let Some(b) = b {
            *a = Some(a.unwrap_or(0) + b);
        }
    };
    add(&mut total.input_tokens, u.input_tokens);
    add(&mut total.output_tokens, u.output_tokens);
    add(&mut total.reasoning_tokens, u.reasoning_tokens);
}

// ---- The loop -------------------------------------------------------------------------

pub async fn run(
    http: &reqwest::Client,
    target: &ModelTarget,
    search_target: Option<&SearchTarget>,
    versions: &[Snapshot],
    req: AgentRequest,
    cancel: Arc<AtomicBool>,
    emit: &(dyn Fn(AgentEvent) + Send + Sync),
) {
    let blocks: Vec<DocBlock> = req
        .document
        .blocks
        .iter()
        .filter(|b| !b.text.trim().is_empty())
        .cloned()
        .collect();
    let words: usize = blocks
        .iter()
        .map(|b| b.text.split_whitespace().count())
        .sum();
    emit(AgentEvent::ToolStart {
        id: "doc".into(),
        tool: "read_document".into(),
        label: req.document.name.clone(),
    });
    emit(AgentEvent::ToolEnd {
        id: "doc".into(),
        ok: true,
        summary: format!("{words} words · {} blocks", blocks.len()),
        sources: vec![],
    });

    let search_available = search_target.is_some();
    let system = system_prompt(&req, search_available, versions.len());
    let tools = tool_specs(search_available, req.web, !versions.is_empty());
    let hist = HistoryCtx {
        versions,
        current: req.document.markdown.as_deref(),
        offset: req.utc_offset.unwrap_or(0),
    };

    let mut convo: Vec<Msg> = req
        .history
        .iter()
        .filter(|t| !t.content.trim().is_empty())
        .map(|t| {
            if t.role == "assistant" {
                Msg::Assistant {
                    text: t.content.clone(),
                    calls: vec![],
                }
            } else {
                Msg::User(t.content.clone())
            }
        })
        .collect();
    let mut first = render_document(&req.document, &blocks);
    if let Some(q) = req.quote.as_deref().filter(|q| !q.trim().is_empty()) {
        first.push_str(&format!(
            "\n\n<selection>\n{q}\n</selection>\nThe author selected this passage; focus on it."
        ));
    }
    first.push_str(&format!("\n\n{}", req.prompt));
    convo.push(Msg::User(first));

    let mut book = EditBook::new(blocks.clone());
    let mut sources: Vec<SearchHit> = vec![];
    let mut usage = Usage::default();
    let mut steps = 0;
    let mut wrote_text = false;

    while steps < MAX_STEPS {
        steps += 1;
        if cancel.load(Ordering::Relaxed) {
            emit(AgentEvent::Done {
                usage,
                steps,
                stopped: true,
            });
            return;
        }
        // Text written after a tool round starts a new paragraph instead of running on.
        let lead = if wrote_text { "\n\n" } else { "" };
        let step =
            match stream_step(http, target, &system, &tools, &convo, &cancel, lead, emit).await {
                Ok(s) => s,
                Err(StepError::Cancelled) => {
                    emit(AgentEvent::Done {
                        usage,
                        steps,
                        stopped: true,
                    });
                    return;
                }
                Err(StepError::Failed(message)) => {
                    emit(AgentEvent::Error { message });
                    return;
                }
            };
        add_usage(&mut usage, &step.usage);
        wrote_text |= !step.text.trim().is_empty();
        let calls = step.calls.clone();
        convo.push(Msg::Assistant {
            text: step.text,
            calls: calls.clone(),
        });
        if calls.is_empty() {
            emit(AgentEvent::Done {
                usage,
                steps,
                stopped: false,
            });
            return;
        }
        let mut results = vec![];
        for call in calls {
            if cancel.load(Ordering::Relaxed) {
                emit(AgentEvent::Done {
                    usage,
                    steps,
                    stopped: true,
                });
                return;
            }
            let out = execute(
                http,
                &call,
                &mut book,
                search_target,
                &hist,
                &mut sources,
                emit,
            )
            .await;
            results.push((call, out));
        }
        convo.push(Msg::Results(results));
    }
    emit(AgentEvent::Text {
        delta:
            "\n\nI stopped here to keep this reply focused. Ask me to continue if you want more."
                .into(),
    });
    emit(AgentEvent::Done {
        usage,
        steps,
        stopped: false,
    });
}

async fn execute(
    http: &reqwest::Client,
    call: &Call,
    book: &mut EditBook,
    search_target: Option<&SearchTarget>,
    hist: &HistoryCtx<'_>,
    sources: &mut Vec<SearchHit>,
    emit: &(dyn Fn(AgentEvent) + Send + Sync),
) -> String {
    let args: Value = serde_json::from_str(if call.args.trim().is_empty() {
        "{}"
    } else {
        &call.args
    })
    .unwrap_or(Value::Null);
    if args.is_null() {
        return "Your tool arguments weren't valid JSON. Call the tool again with valid JSON arguments.".into();
    }
    match call.name.as_str() {
        "propose_edits" => {
            let items = args
                .get("edits")
                .and_then(|e| e.as_array())
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                return "`edits` is empty.".into();
            }
            let mut accepted = vec![];
            let mut report = vec![];
            for (i, e) in items.iter().enumerate() {
                let find = e.get("find").and_then(|v| v.as_str()).unwrap_or("");
                let replace = e.get("replace").and_then(|v| v.as_str()).unwrap_or("");
                let reason = e
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                match book.replace(find, replace) {
                    Ok(()) => {
                        accepted.push(ProposedEdit {
                            kind: EditKind::Replace,
                            find: find.into(),
                            replace: replace.into(),
                            reason: if reason.is_empty() {
                                "Suggested edit".into()
                            } else {
                                reason.into()
                            },
                        });
                        report.push(format!("{}. proposed", i + 1));
                    }
                    Err(why) => report.push(format!("{}. not proposed: {why}", i + 1)),
                }
            }
            let n = accepted.len();
            if n > 0 {
                emit(AgentEvent::Edits { edits: accepted });
            }
            let mut out = report.join("\n");
            if n < items.len() {
                out.push_str("\nFix the edits that were not proposed and call propose_edits again with only those.");
            }
            out
        }
        "insert_paragraphs" => {
            let after = args
                .get("after_block")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let paragraphs: Vec<String> = args
                .get("paragraphs")
                .and_then(|p| p.as_array())
                .map(|p| {
                    p.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let reason = args
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            match book.insert(after, &paragraphs) {
                Ok((anchor, text)) => {
                    emit(AgentEvent::Edits {
                        edits: vec![ProposedEdit {
                            kind: EditKind::Insert,
                            find: anchor,
                            replace: text,
                            reason: if reason.is_empty() {
                                "New paragraph".into()
                            } else {
                                reason
                            },
                        }],
                    });
                    "Proposed.".into()
                }
                Err(why) => format!("Not proposed: {why}"),
            }
        }
        "web_search" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let Some(st) = search_target else {
                return "Web search isn't available.".into();
            };
            if query.is_empty() {
                return "`query` is empty.".into();
            }
            emit(AgentEvent::ToolStart {
                id: call.id.clone(),
                tool: "web_search".into(),
                label: query.clone(),
            });
            match search::search(http, st.kind, &st.key, &query, SEARCH_RESULTS).await {
                Ok(hits) => {
                    let mut listed = vec![];
                    let mut out = String::new();
                    for h in hits {
                        // Number sources once per reply so citations stay stable across searches.
                        let n = match sources.iter().position(|s| s.url == h.url) {
                            Some(i) => i + 1,
                            None => {
                                sources.push(h.clone());
                                sources.len()
                            }
                        };
                        out.push_str(&format!("[{n}] {}\n{}\n{}\n\n", h.title, h.url, h.snippet));
                        listed.push(Source {
                            n,
                            title: h.title,
                            url: h.url,
                            snippet: h.snippet,
                        });
                    }
                    let summary = format!(
                        "{} source{}",
                        listed.len(),
                        if listed.len() == 1 { "" } else { "s" }
                    );
                    emit(AgentEvent::ToolEnd {
                        id: call.id.clone(),
                        ok: true,
                        summary,
                        sources: listed,
                    });
                    if out.is_empty() {
                        "No results.".into()
                    } else {
                        out
                    }
                }
                Err(e) => {
                    emit(AgentEvent::ToolEnd {
                        id: call.id.clone(),
                        ok: false,
                        summary: e.clone(),
                        sources: vec![],
                    });
                    format!("Search failed: {e}")
                }
            }
        }
        "fetch_url" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            emit(AgentEvent::ToolStart {
                id: call.id.clone(),
                tool: "fetch_url".into(),
                label: url.clone(),
            });
            // Firecrawl renders pages (JavaScript included); anything else, or a Firecrawl
            // failure, reads the page directly.
            let page = match search_target {
                Some(st) if st.kind == SearchKind::Firecrawl => {
                    match search::scrape_page(http, &st.key, &url, PAGE_CHARS).await {
                        Ok(p) => Ok(p),
                        Err(_) => search::fetch_page(http, &url, PAGE_CHARS).await,
                    }
                }
                _ => search::fetch_page(http, &url, PAGE_CHARS).await,
            };
            match page {
                Ok((title, text)) => {
                    let n = match sources.iter().position(|s| s.url == url) {
                        Some(i) => i + 1,
                        None => {
                            sources.push(SearchHit {
                                title: if title.is_empty() {
                                    url.clone()
                                } else {
                                    title.clone()
                                },
                                url: url.clone(),
                                snippet: search::clip(&text, 300),
                            });
                            sources.len()
                        }
                    };
                    let s = &sources[n - 1];
                    emit(AgentEvent::ToolEnd {
                        id: call.id.clone(),
                        ok: true,
                        summary: format!("{} words", text.split_whitespace().count()),
                        sources: vec![Source {
                            n,
                            title: s.title.clone(),
                            url: s.url.clone(),
                            snippet: s.snippet.clone(),
                        }],
                    });
                    format!("[{n}] {}\n{url}\n\n{text}", s.title)
                }
                Err(e) => {
                    emit(AgentEvent::ToolEnd {
                        id: call.id.clone(),
                        ok: false,
                        summary: e.clone(),
                        sources: vec![],
                    });
                    format!("Could not read the page: {e}")
                }
            }
        }
        "list_versions" => {
            emit(AgentEvent::ToolStart {
                id: call.id.clone(),
                tool: "history".into(),
                label: "Version history".into(),
            });
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            emit(AgentEvent::ToolEnd {
                id: call.id.clone(),
                ok: true,
                summary: format!("{} versions", hist.versions.len()),
                sources: vec![],
            });
            history::list(hist.versions, now, hist.offset)
        }
        "read_version" => {
            let find = |n: i64| hist.versions.iter().find(|v| v.n as i64 == n);
            let n = args.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
            let compare = args.get("compare_to").filter(|v| !v.is_null());
            let label = match compare {
                Some(c) if c.as_str() == Some("current") => format!("v{n} against now"),
                Some(c) => format!("v{n} against v{}", c.as_i64().unwrap_or(0)),
                None => format!("v{n}"),
            };
            emit(AgentEvent::ToolStart {
                id: call.id.clone(),
                tool: "history".into(),
                label,
            });
            let result: Result<String, String> = (|| {
                let v = find(n).ok_or_else(|| {
                    format!(
                        "There is no v{n}; versions run from v1 to v{}.",
                        hist.versions.len()
                    )
                })?;
                let Some(c) = compare else {
                    return Ok(history::text(v));
                };
                let other = if c.as_str() == Some("current") {
                    Side::Current(hist.current.ok_or(
                        "The current text isn't available to compare; compare two versions instead.",
                    )?)
                } else {
                    let m = c
                        .as_i64()
                        .ok_or("`compare_to` must be \"current\" or a version number.")?;
                    Side::Version(find(m).ok_or_else(|| format!("There is no v{m}."))?)
                };
                // Always diff from older to newer, so "-" means cut and "+" means added.
                let older_first = match &other {
                    Side::Version(o) => o.n > v.n,
                    Side::Current(_) => true,
                };
                Ok(if older_first {
                    history::diff(&Side::Version(v), &other)
                } else {
                    history::diff(&other, &Side::Version(v))
                })
            })();
            emit(AgentEvent::ToolEnd {
                id: call.id.clone(),
                ok: result.is_ok(),
                summary: match &result {
                    Ok(r) if r.starts_with("No differences") => "no differences".into(),
                    Ok(_) => "read".into(),
                    Err(e) => e.clone(),
                },
                sources: vec![],
            });
            result.unwrap_or_else(|e| e)
        }
        other => format!("There is no tool called {other}."),
    }
}

// ---- Streaming one step ------------------------------------------------------------------

enum StepError {
    Cancelled,
    Failed(String),
}

#[allow(clippy::too_many_arguments)]
async fn stream_step(
    http: &reqwest::Client,
    t: &ModelTarget,
    system: &str,
    tools: &[(&'static str, &'static str, Value)],
    convo: &[Msg],
    cancel: &AtomicBool,
    lead: &str,
    emit: &(dyn Fn(AgentEvent) + Send + Sync),
) -> Result<Step, StepError> {
    let info = t.kind.info();
    let base = t.base_url.trim().trim_end_matches('/');
    let (url, body) = match info.protocol {
        Protocol::OpenAi => (
            format!("{base}/chat/completions"),
            openai_body(t, system, tools, convo),
        ),
        Protocol::Anthropic => (
            format!("{base}/messages"),
            anthropic_body(t, system, tools, convo),
        ),
    };
    let mut req = http
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(600));
    req = match (info.protocol, t.key.as_deref()) {
        (Protocol::Anthropic, Some(k)) => req
            .header("x-api-key", k)
            .header("anthropic-version", "2023-06-01"),
        (Protocol::Anthropic, None) => req.header("anthropic-version", "2023-06-01"),
        (Protocol::OpenAi, Some(k)) => req.bearer_auth(k),
        (Protocol::OpenAi, None) => req,
    };
    let res = req
        .send()
        .await
        .map_err(|e| StepError::Failed(classify(None, "", Some(&e), info.label).1))?;
    if !res.status().is_success() {
        let code = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(StepError::Failed(
            classify(Some(code), &text, None, info.label).1,
        ));
    }

    let mut parser = match info.protocol {
        Protocol::OpenAi => Parser::OpenAi(OpenAiState {
            lead: lead.to_string(),
            ..Default::default()
        }),
        Protocol::Anthropic => Parser::Anthropic(AnthropicState {
            lead: lead.to_string(),
            ..Default::default()
        }),
    };
    let mut stream = res.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            return Err(StepError::Cancelled);
        }
        let chunk = chunk.map_err(|_| {
            StepError::Failed(format!(
                "The connection to {} dropped mid-reply.",
                info.label
            ))
        })?;
        buf.push_str(&String::from_utf8_lossy(&chunk).replace("\r\n", "\n"));
        while let Some(pos) = buf.find("\n\n") {
            let event: String = buf.drain(..pos + 2).collect();
            for line in event.lines() {
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                parser.feed(&v, emit).map_err(StepError::Failed)?;
            }
        }
    }
    Ok(parser.finish())
}

enum Parser {
    OpenAi(OpenAiState),
    Anthropic(AnthropicState),
}

impl Parser {
    fn feed(&mut self, v: &Value, emit: &(dyn Fn(AgentEvent) + Send + Sync)) -> Result<(), String> {
        match self {
            Parser::OpenAi(s) => s.feed(v, emit),
            Parser::Anthropic(s) => s.feed(v, emit),
        }
    }
    fn finish(self) -> Step {
        match self {
            Parser::OpenAi(s) => s.finish(),
            Parser::Anthropic(s) => s.finish(),
        }
    }
}

#[derive(Default)]
struct OpenAiState {
    /// Prefix for the first text delta of this step.
    lead: String,
    text: String,
    calls: BTreeMap<u64, Call>,
    usage: Usage,
}

impl OpenAiState {
    fn feed(&mut self, v: &Value, emit: &(dyn Fn(AgentEvent) + Send + Sync)) -> Result<(), String> {
        if let Some(err) = v.get("error") {
            return Err(err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("The provider reported an error.")
                .to_string());
        }
        if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
            self.usage = Usage {
                input_tokens: u.get("prompt_tokens").and_then(|x| x.as_u64()),
                output_tokens: u.get("completion_tokens").and_then(|x| x.as_u64()),
                reasoning_tokens: u
                    .pointer("/completion_tokens_details/reasoning_tokens")
                    .and_then(|x| x.as_u64()),
            };
        }
        let Some(delta) = v.pointer("/choices/0/delta") else {
            return Ok(());
        };
        if let Some(r) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(|r| r.as_str())
        {
            if !r.is_empty() {
                emit(AgentEvent::Reasoning {
                    delta: r.to_string(),
                });
            }
        }
        if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
            if !c.is_empty() {
                let delta = format!("{}{c}", std::mem::take(&mut self.lead));
                self.text.push_str(c);
                emit(AgentEvent::Text { delta });
            }
        }
        if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
            for (fallback, tc) in tcs.iter().enumerate() {
                let index = tc
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .unwrap_or(fallback as u64);
                let entry = self.calls.entry(index).or_default();
                if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                    if !id.is_empty() {
                        entry.id = id.to_string();
                    }
                }
                if let Some(name) = tc.pointer("/function/name").and_then(|n| n.as_str()) {
                    if entry.name.is_empty() {
                        entry.name = name.to_string();
                    }
                }
                if let Some(args) = tc.pointer("/function/arguments").and_then(|a| a.as_str()) {
                    entry.args.push_str(args);
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Step {
        let calls = self
            .calls
            .into_values()
            .enumerate()
            .filter(|(_, c)| !c.name.is_empty())
            .map(|(i, mut c)| {
                if c.id.is_empty() {
                    c.id = format!("call_{i}");
                }
                c
            })
            .collect();
        Step {
            text: self.text,
            calls,
            usage: self.usage,
        }
    }
}

#[derive(Default)]
struct AnthropicState {
    lead: String,
    text: String,
    blocks: BTreeMap<u64, Call>,
    usage: Usage,
}

impl AnthropicState {
    fn feed(&mut self, v: &Value, emit: &(dyn Fn(AgentEvent) + Send + Sync)) -> Result<(), String> {
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "error" => {
                return Err(v
                    .pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("The provider reported an error.")
                    .to_string())
            }
            "message_start" => {
                self.usage.input_tokens = v
                    .pointer("/message/usage/input_tokens")
                    .and_then(|x| x.as_u64());
            }
            "content_block_start" => {
                let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                let block = v.get("content_block").cloned().unwrap_or(Value::Null);
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    self.blocks.insert(
                        index,
                        Call {
                            id: block
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("")
                                .to_string(),
                            name: block
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string(),
                            args: String::new(),
                        },
                    );
                }
            }
            "content_block_delta" => {
                let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                let d = v.get("delta").cloned().unwrap_or(Value::Null);
                match d.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "text_delta" => {
                        if let Some(t) = d.get("text").and_then(|t| t.as_str()) {
                            let delta = format!("{}{t}", std::mem::take(&mut self.lead));
                            self.text.push_str(t);
                            emit(AgentEvent::Text { delta });
                        }
                    }
                    "thinking_delta" => {
                        if let Some(t) = d.get("thinking").and_then(|t| t.as_str()) {
                            emit(AgentEvent::Reasoning {
                                delta: t.to_string(),
                            });
                        }
                    }
                    "input_json_delta" => {
                        if let (Some(call), Some(p)) = (
                            self.blocks.get_mut(&index),
                            d.get("partial_json").and_then(|p| p.as_str()),
                        ) {
                            call.args.push_str(p);
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(o) = v.pointer("/usage/output_tokens").and_then(|x| x.as_u64()) {
                    self.usage.output_tokens = Some(o);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Step {
        Step {
            text: self.text,
            calls: self.blocks.into_values().collect(),
            usage: self.usage,
        }
    }
}

// ---- Request bodies ------------------------------------------------------------------------

fn openai_body(
    t: &ModelTarget,
    system: &str,
    tools: &[(&'static str, &'static str, Value)],
    convo: &[Msg],
) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": system })];
    for m in convo {
        match m {
            Msg::User(text) => messages.push(json!({ "role": "user", "content": text })),
            Msg::Assistant { text, calls } => {
                let mut msg = Map::new();
                msg.insert("role".into(), json!("assistant"));
                msg.insert(
                    "content".into(),
                    if text.is_empty() && !calls.is_empty() {
                        Value::Null
                    } else {
                        json!(text)
                    },
                );
                if !calls.is_empty() {
                    msg.insert(
                        "tool_calls".into(),
                        json!(calls
                            .iter()
                            .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": if c.args.is_empty() { "{}" } else { c.args.as_str() } } }))
                            .collect::<Vec<_>>()),
                    );
                }
                messages.push(Value::Object(msg));
            }
            Msg::Results(results) => {
                for (call, out) in results {
                    messages
                        .push(json!({ "role": "tool", "tool_call_id": call.id, "content": out }));
                }
            }
        }
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|(name, desc, schema)| json!({ "type": "function", "function": { "name": name, "description": desc, "parameters": schema } }))
        .collect();
    json!({
        "model": t.model,
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto",
        "stream": true,
        "max_tokens": MAX_TOKENS,
    })
}

fn anthropic_body(
    t: &ModelTarget,
    system: &str,
    tools: &[(&'static str, &'static str, Value)],
    convo: &[Msg],
) -> Value {
    let mut messages: Vec<Value> = vec![];
    for m in convo {
        match m {
            Msg::User(text) => messages.push(json!({ "role": "user", "content": text })),
            Msg::Assistant { text, calls } => {
                let mut content = vec![];
                if !text.is_empty() {
                    content.push(json!({ "type": "text", "text": text }));
                }
                for c in calls {
                    let input: Value =
                        serde_json::from_str(if c.args.is_empty() { "{}" } else { &c.args })
                            .unwrap_or_else(|_| json!({}));
                    content.push(
                        json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": input }),
                    );
                }
                if content.is_empty() {
                    content.push(json!({ "type": "text", "text": "…" }));
                }
                messages.push(json!({ "role": "assistant", "content": content }));
            }
            Msg::Results(results) => {
                let content: Vec<Value> = results.iter().map(|(c, out)| json!({ "type": "tool_result", "tool_use_id": c.id, "content": out })).collect();
                messages.push(json!({ "role": "user", "content": content }));
            }
        }
    }
    let tools: Vec<Value> = tools.iter().map(|(name, desc, schema)| json!({ "name": name, "description": desc, "input_schema": schema })).collect();
    json!({ "model": t.model, "system": system, "messages": messages, "tools": tools, "stream": true, "max_tokens": MAX_TOKENS })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn blocks(texts: &[&str]) -> Vec<DocBlock> {
        texts
            .iter()
            .map(|t| DocBlock {
                kind: "p".into(),
                text: t.to_string(),
            })
            .collect()
    }

    #[test]
    fn edit_book_enforces_unique_exact_matches() {
        let mut b = EditBook::new(blocks(&[
            "We think that word of mouth will be our biggest channel.",
            "The plan is the plan.",
        ]));
        assert!(b.replace("biggest channel", "strongest channel").is_ok());
        assert!(b
            .replace("our biggest", "x")
            .unwrap_err()
            .contains("Overlaps"));
        assert!(b
            .replace("plan", "x")
            .unwrap_err()
            .starts_with("Found 2 times (block 2)"));
        assert!(b
            .replace("We Think that", "x")
            .unwrap_err()
            .contains("block 1 has a close match"));
        assert!(b
            .replace("nowhere", "x")
            .unwrap_err()
            .starts_with("Not found"));
        assert!(b
            .replace("We think", "a\nb")
            .unwrap_err()
            .contains("line break"));
        assert!(b
            .replace("We think", "We think")
            .unwrap_err()
            .contains("identical"));
        let (anchor, text) = b
            .insert(2, &["  New   para ".into(), "".into(), "Second".into()])
            .unwrap();
        assert_eq!(anchor, "The plan is the plan.");
        assert_eq!(text, "New para\n\nSecond");
        assert!(b
            .insert(9, &["x".into()])
            .unwrap_err()
            .contains("numbered 1 to 2"));
    }

    #[test]
    fn openai_stream_assembles_tool_calls() {
        let events = Mutex::new(vec![]);
        let emit = |e: AgentEvent| events.lock().unwrap().push(e);
        let mut s = OpenAiState::default();
        for v in [
            json!({ "choices": [{ "delta": { "reasoning_content": "thinking" } }] }),
            json!({ "choices": [{ "delta": { "content": "Hi" } }] }),
            json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": "c1", "function": { "name": "propose_edits", "arguments": "{\"ed" } }] } }] }),
            json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": "its\":[]}" } }] } }] }),
            json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 5 } }),
        ] {
            s.feed(&v, &emit).unwrap();
        }
        let step = s.finish();
        assert_eq!(step.text, "Hi");
        assert_eq!(step.calls.len(), 1);
        assert_eq!(step.calls[0].name, "propose_edits");
        assert_eq!(step.calls[0].args, "{\"edits\":[]}");
        assert_eq!(step.usage.output_tokens, Some(5));
        assert_eq!(events.lock().unwrap().len(), 2);
    }

    #[test]
    fn anthropic_stream_assembles_tool_use() {
        let emit = |_e: AgentEvent| {};
        let mut s = AnthropicState::default();
        for v in [
            json!({ "type": "message_start", "message": { "usage": { "input_tokens": 12 } } }),
            json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text" } }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Sure." } }),
            json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "tool_use", "id": "tu1", "name": "insert_paragraphs" } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "{\"after_block\": 2," } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "\"paragraphs\":[\"x\"],\"reason\":\"y\"}" } }),
            json!({ "type": "message_delta", "usage": { "output_tokens": 40 } }),
        ] {
            s.feed(&v, &emit).unwrap();
        }
        let step = s.finish();
        assert_eq!(step.text, "Sure.");
        assert_eq!(step.calls[0].id, "tu1");
        assert_eq!(
            serde_json::from_str::<Value>(&step.calls[0].args).unwrap()["after_block"],
            2
        );
        assert_eq!(step.usage.input_tokens, Some(12));
        assert_eq!(step.usage.output_tokens, Some(40));
    }

    #[test]
    fn bodies_round_trip_tool_calls() {
        let t = ModelTarget {
            kind: Kind::Fireworks,
            base_url: "x".into(),
            key: None,
            model: "m".into(),
        };
        let call = Call {
            id: "c1".into(),
            name: "propose_edits".into(),
            args: "{\"edits\":[]}".into(),
        };
        let convo = vec![
            Msg::User("hi".into()),
            Msg::Assistant {
                text: String::new(),
                calls: vec![call.clone()],
            },
            Msg::Results(vec![(call, "ok".into())]),
        ];
        let tools = tool_specs(false, false, false);
        let o = openai_body(&t, "sys", &tools, &convo);
        assert_eq!(
            o["messages"][2]["tool_calls"][0]["function"]["name"],
            "propose_edits"
        );
        assert_eq!(o["messages"][3]["role"], "tool");
        assert_eq!(o["tools"].as_array().unwrap().len(), 2);
        let a = anthropic_body(&t, "sys", &tools, &convo);
        assert_eq!(a["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(a["messages"][2]["content"][0]["tool_use_id"], "c1");
    }
}
