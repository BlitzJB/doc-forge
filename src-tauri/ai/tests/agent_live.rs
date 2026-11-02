//! Live agent runs against Fireworks. Skipped without FIREWORKS_API_KEY.
//!   set -a; . ../.env.local; set +a; cargo test -p forge-ai --test agent_live -- --nocapture --test-threads=1
//! FORGE_TEST_MODEL overrides the model.

use forge_ai::agent::{
    run, AgentEvent, AgentRequest, DocBlock, DocSnapshot, EditKind, ModelTarget,
};
use forge_ai::history::Snapshot;
use forge_ai::Kind;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

fn target() -> Option<ModelTarget> {
    let key = std::env::var("FIREWORKS_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())?;
    Some(ModelTarget {
        kind: Kind::Fireworks,
        base_url: Kind::Fireworks.info().default_base_url.into(),
        key: Some(key),
        model: std::env::var("FORGE_TEST_MODEL")
            .unwrap_or_else(|_| "accounts/fireworks/models/deepseek-v4p1-flash".into()),
    })
}

fn doc() -> DocSnapshot {
    let b = |kind: &str, text: &str| DocBlock {
        kind: kind.into(),
        text: text.into(),
    };
    DocSnapshot {
        name: "plan.md".into(),
        folder: Some("launch".into()),
        markdown: None,
        blocks: vec![
            b("h1", "Northwind: Launch Plan for Q1"),
            b("p", "Northwind is a lightweight analytics tool for small ecommerce teams. This document outlines how we plan to launch the product in the first quarter, who we are targeting, and how we will measure whether the launch was a success."),
            b("h2", "Problem"),
            b("p", "Small ecommerce teams have a lot of data but they don't really have the time or the expertise to make sense of it. Most existing analytics tools are very complicated and expensive, and they are built for large companies with dedicated data teams."),
            b("h2", "Launch strategy"),
            b("p", "We will start with a private beta for around 50 merchants in January. After that we will open a public waitlist and launch on Product Hunt in late February. We think that word of mouth will be our biggest channel."),
            b("h2", "Success metrics"),
            b("p", "We will consider the launch successful if we reach 500 paying customers by the end of March and keep monthly churn below 5%."),
        ],
    }
}

struct Transcript {
    events: Vec<AgentEvent>,
}

impl Transcript {
    fn text(&self) -> String {
        self.events
            .iter()
            .filter_map(|e| {
                if let AgentEvent::Text { delta } = e {
                    Some(delta.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
    fn reasoning_chars(&self) -> usize {
        self.events
            .iter()
            .map(|e| {
                if let AgentEvent::Reasoning { delta } = e {
                    delta.len()
                } else {
                    0
                }
            })
            .sum()
    }
    fn edits(&self) -> Vec<forge_ai::agent::ProposedEdit> {
        self.events
            .iter()
            .flat_map(|e| {
                if let AgentEvent::Edits { edits } = e {
                    edits.clone()
                } else {
                    vec![]
                }
            })
            .collect()
    }
    fn tools(&self) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|e| {
                if let AgentEvent::ToolStart { tool, label, .. } = e {
                    Some(format!("{tool}({label})"))
                } else {
                    None
                }
            })
            .collect()
    }
    fn done(&self) -> Option<&AgentEvent> {
        self.events
            .iter()
            .find(|e| matches!(e, AgentEvent::Done { .. } | AgentEvent::Error { .. }))
    }
    fn print(&self, title: &str) {
        eprintln!("\n=== {title}");
        eprintln!("tools: {:?}", self.tools());
        eprintln!("reasoning: {} chars", self.reasoning_chars());
        for e in self.edits() {
            eprintln!(
                "edit [{:?}] {:?}\n   -> {:?}\n   why: {}",
                e.kind, e.find, e.replace, e.reason
            );
        }
        eprintln!("reply: {}", self.text().trim());
        eprintln!("end: {:?}", self.done());
    }
}

async fn ask(prompt: &str, web: bool, cancel_after: Option<usize>) -> Option<Transcript> {
    ask_with(prompt, web, cancel_after, vec![], None).await
}

async fn ask_with(
    prompt: &str,
    web: bool,
    cancel_after: Option<usize>,
    versions: Vec<Snapshot>,
    markdown: Option<String>,
) -> Option<Transcript> {
    let t = target()?;
    let mut document = doc();
    document.markdown = markdown;
    let req = AgentRequest {
        prompt: prompt.into(),
        quote: None,
        document,
        history: vec![],
        web,
        apply_directly: false,
        today: Some("Monday, 2 November 2026".into()),
        utc_offset: Some(330),
        doc_id: None,
    };
    let events = Arc::new(Mutex::new(vec![]));
    let timeline = Arc::new(Mutex::new(vec![]));
    let cancel = Arc::new(AtomicBool::new(false));
    let (ev, c, tl) = (events.clone(), cancel.clone(), timeline.clone());
    let started = std::time::Instant::now();
    let emit = move |e: AgentEvent| {
        tl.lock()
            .unwrap()
            .push(serde_json::json!({ "at": started.elapsed().as_millis() as u64, "event": e }));
        let mut v = ev.lock().unwrap();
        v.push(e);
        if cancel_after.is_some_and(|n| v.len() >= n) {
            c.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let http = forge_ai::http_client();
    run(&http, &t, None, &versions, req, cancel, &emit).await;
    let events = events.lock().unwrap().clone();
    assert!(!events.is_empty(), "the agent emitted nothing");
    // Optional: keep a timed recording for replaying real runs in UI tests.
    if let Ok(dir) = std::env::var("FORGE_DUMP_DIR") {
        let name: String = prompt
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
            .take(24)
            .collect::<String>()
            .trim()
            .replace(' ', "-")
            .to_lowercase();
        let _ = std::fs::write(
            format!("{dir}/{name}.json"),
            serde_json::to_vec_pretty(&*timeline.lock().unwrap()).unwrap(),
        );
    }
    Some(Transcript { events })
}

fn assert_valid_against_doc(edits: &[forge_ai::agent::ProposedEdit]) {
    let d = doc();
    for e in edits {
        let hits: usize = d
            .blocks
            .iter()
            .map(|b| b.text.matches(e.find.as_str()).count())
            .sum();
        assert_eq!(hits, 1, "edit find must match exactly once: {:?}", e.find);
    }
}

#[tokio::test]
async fn tightens_with_valid_edits() {
    let Some(t) = ask(
        "Tighten the writing throughout. Cut filler and hedging.",
        false,
        None,
    )
    .await
    else {
        return eprintln!("no key; skipped");
    };
    t.print("tighten");
    assert!(
        matches!(t.done(), Some(AgentEvent::Done { stopped: false, .. })),
        "run should finish cleanly"
    );
    let edits = t.edits();
    assert!(edits.len() >= 2, "expected several edits");
    assert!(edits.iter().all(|e| e.kind == EditKind::Replace));
    assert_valid_against_doc(&edits);
    assert!(
        !t.text().trim().is_empty(),
        "expected a short note to the author"
    );
}

#[tokio::test]
async fn answers_questions_without_editing() {
    let Some(t) = ask(
        "What's the single biggest risk in this plan? Don't change anything.",
        false,
        None,
    )
    .await
    else {
        return;
    };
    t.print("question");
    assert!(t.edits().is_empty(), "a question shouldn't produce edits");
    assert!(t.text().len() > 40);
}

#[tokio::test]
async fn inserts_new_paragraphs() {
    let Some(t) = ask(
        "Add a short paragraph on launch risks right after the launch strategy paragraph.",
        false,
        None,
    )
    .await
    else {
        return;
    };
    t.print("insert");
    let edits = t.edits();
    let ins: Vec<_> = edits
        .iter()
        .filter(|e| e.kind == EditKind::Insert)
        .collect();
    assert_eq!(ins.len(), 1, "expected one insertion");
    assert!(
        ins[0].find.starts_with("We will start with a private beta"),
        "anchored after the launch strategy paragraph, got {:?}",
        ins[0].find
    );
}

#[tokio::test]
async fn stops_when_cancelled() {
    let Some(t) = ask("Tighten the writing throughout.", false, Some(6)).await else {
        return;
    };
    eprintln!(
        "\n=== cancel: {} events, end {:?}",
        t.events.len(),
        t.done()
    );
    assert!(matches!(
        t.done(),
        Some(AgentEvent::Done { stopped: true, .. })
    ));
}

#[tokio::test]
async fn reads_a_page_when_web_is_on() {
    let Some(t) = ask("Read https://example.com and tell me in one sentence what that page is for. Don't edit the plan.", true, None).await else { return };
    t.print("fetch");
    assert!(
        t.tools().iter().any(|x| x.starts_with("fetch_url(")),
        "expected fetch_url"
    );
    assert!(t.edits().is_empty());
}

/// The author's real exchange: a test file opened with `df`, then a terse aside about `df`.
#[tokio::test]
async fn reads_a_terse_aside_in_context() {
    let Some(t) = target() else { return };
    let b = |kind: &str, text: &str| DocBlock {
        kind: kind.into(),
        text: text.into(),
    };
    let req = AgentRequest {
        prompt: "oh wait df is usually used for disk usage command uh?".into(),
        quote: None,
        document: DocSnapshot {
            name: "cold.md".into(),
            folder: Some("scratchpad".into()),
            markdown: None,
            blocks: vec![b("h1", "Cold start"), b("p", "Opened from the terminal with df.")],
        },
        history: vec![
            forge_ai::agent::Turn { role: "user".into(), content: "Hey can you try running websearch".into() },
            forge_ai::agent::Turn { role: "assistant".into(), content: "Web search works. I searched \"df command terminal disk usage\": df reports free and used space on mounted filesystems, and df -h prints human-readable sizes. Want me to fold any of this into the manuscript, or was this just a test run?".into() },
        ],
        web: true,
        apply_directly: false,
        today: Some("Monday, 2 November 2026".into()),
        utc_offset: Some(330),
        doc_id: None,
    };
    let events = Arc::new(Mutex::new(vec![]));
    let ev = events.clone();
    let emit = move |e: AgentEvent| ev.lock().unwrap().push(e);
    run(
        &forge_ai::http_client(),
        &t,
        None,
        &[],
        req,
        Arc::new(AtomicBool::new(false)),
        &emit,
    )
    .await;
    let t = Transcript {
        events: events.lock().unwrap().clone(),
    };
    t.print("terse aside");
    assert!(
        t.edits().is_empty(),
        "an aside shouldn't edit the manuscript"
    );
    assert!(
        t.text().contains("Doc Forge"),
        "should connect `df` to Doc Forge's command"
    );
}

/// Forge looks through saved versions when the author asks about an earlier draft,
/// and brings cut text back as a proposed edit.
#[tokio::test]
async fn recovers_cut_text_from_history() {
    let d = doc();
    let current: String = d
        .blocks
        .iter()
        .map(|b| match b.kind.as_str() {
            "h1" => format!("# {}", b.text),
            "h2" => format!("## {}", b.text),
            _ => b.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let pricing = "Pricing starts at $29 per month for up to three seats, with a 14-day free trial and no card required.";
    let v1 = current.replace(
        "## Success metrics",
        &format!("## Pricing\n\n{pricing}\n\n## Success metrics"),
    );
    let day = 86_400_000i64;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let snap = |n: usize, ago: i64, label: &str, content: &str| Snapshot {
        n,
        author: "you".into(),
        label: label.into(),
        created_at: now - ago,
        content: content.into(),
    };
    let versions = vec![
        snap(1, 3 * day, "First draft", &v1),
        snap(2, day + 3_600_000, "Cut the pricing section", &current),
        snap(3, 3_600_000, "Small fixes", &current),
    ];
    let Some(t) = ask_with(
        "What did I cut from the first draft? I want that pricing bit back where it was.",
        false,
        None,
        versions,
        Some(current),
    )
    .await
    else {
        return;
    };
    t.print("history");
    assert!(
        t.tools().iter().any(|x| x.starts_with("history(")),
        "expected the history tools"
    );
    let edits = t.edits();
    assert!(
        edits.iter().any(|e| e.replace.contains("$29")),
        "expected the pricing text proposed back"
    );
    assert!(
        edits.iter().any(|e| e.replace.contains("## Pricing")),
        "the section heading should come back as a heading"
    );
}
