//! Tauri commands for AI provider configuration. Keys go in, never come back out.

use forge_ai::agent::{self, AgentEvent, AgentRequest};
use forge_ai::history::Snapshot;
use forge_ai::search::SearchKind;
use forge_ai::{AiService, AiView, Check, Role, RoleChoice, TestResult};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::State;

/// Cancellation flags for runs in flight, by run id.
#[derive(Default)]
pub struct Runs(Mutex<HashMap<String, Arc<AtomicBool>>>);

type Ai<'a> = State<'a, Arc<AiService>>;
type Res<T> = Result<T, String>;

#[tauri::command]
pub fn ai_state(ai: Ai<'_>) -> AiView {
    ai.view()
}

#[tauri::command]
pub async fn ai_check(ai: Ai<'_>, id: String) -> Res<Check> {
    ai.check(&id).await
}

#[tauri::command]
pub async fn ai_save_key(ai: Ai<'_>, id: String, key: String) -> Res<Check> {
    ai.save_key(&id, &key).await
}

#[tauri::command]
pub fn ai_forget_key(ai: Ai<'_>, id: String) -> Res<()> {
    ai.forget_key(&id)
}

#[tauri::command]
pub fn ai_set_base_url(ai: Ai<'_>, id: String, base_url: String) -> Res<()> {
    ai.set_base_url(&id, &base_url)
}

#[tauri::command]
pub fn ai_add_custom(ai: Ai<'_>, name: String, base_url: String) -> Res<String> {
    ai.add_custom(&name, &base_url)
}

#[tauri::command]
pub fn ai_remove_provider(ai: Ai<'_>, id: String) -> Res<()> {
    ai.remove_provider(&id)
}

#[tauri::command]
pub fn ai_set_role(
    ai: Ai<'_>,
    role: Role,
    provider: Option<String>,
    model: Option<String>,
) -> Res<()> {
    let choice = match (provider, model) {
        (Some(provider), Some(model)) => Some(RoleChoice { provider, model }),
        _ => None,
    };
    ai.set_role(role, choice)
}

#[tauri::command]
pub async fn ai_test(ai: Ai<'_>, provider: String, model: String) -> Res<TestResult> {
    ai.test(&provider, &model).await
}

#[tauri::command]
pub async fn ai_save_search_key(ai: Ai<'_>, kind: SearchKind, key: String) -> Res<usize> {
    ai.save_search_key(kind, &key).await
}

/// Browser sign-in: opens the service's page in the default browser and resolves when
/// the browser comes back (or fails, times out, or is cancelled).
#[tauri::command]
pub async fn ai_search_sign_in(app: tauri::AppHandle, ai: Ai<'_>, kind: SearchKind) -> Res<usize> {
    use tauri_plugin_opener::OpenerExt;
    let opener = app.opener();
    ai.sign_in(kind, |url| {
        let _ = opener.open_url(url, None::<&str>);
    })
    .await
}

#[tauri::command]
pub fn ai_cancel_sign_in(ai: Ai<'_>) {
    ai.cancel_sign_in()
}

#[tauri::command]
pub async fn ai_search_sign_out(ai: Ai<'_>, kind: SearchKind) -> Res<()> {
    ai.sign_out(kind).await
}

#[tauri::command]
pub fn ai_forget_search_key(ai: Ai<'_>, kind: SearchKind) -> Res<()> {
    ai.forget_search_key(kind)
}

/// Start an agent run. Returns its id immediately; events stream over `channel`.
#[tauri::command]
pub fn agent_run(
    ai: Ai<'_>,
    runs: State<'_, Arc<Runs>>,
    store: State<'_, Arc<forge_shadow::Store>>,
    request: AgentRequest,
    channel: Channel<AgentEvent>,
) -> Res<String> {
    let target = ai.model_target(Role::Editor)?;
    let id = format!(
        "run-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let cancel = Arc::new(AtomicBool::new(false));
    runs.0.lock().unwrap().insert(id.clone(), cancel.clone());
    let (service, runs, run_id) = (ai.inner().clone(), runs.inner().clone(), id.clone());
    let store = store.inner().clone();
    tauri::async_runtime::spawn(async move {
        let emit = move |e: AgentEvent| {
            let _ = channel.send(e);
        };
        // Resolved here: a signed-in search service may need to refresh its token first.
        let search = if request.web {
            service.search_target().await
        } else {
            None
        };
        // The open document's saved versions, numbered as the history rail shows them.
        let versions = match request.doc_id.clone() {
            Some(doc) => tauri::async_runtime::spawn_blocking(move || store.history(&doc))
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or_default(),
            None => vec![],
        };
        let versions: Vec<Snapshot> = versions
            .into_iter()
            .enumerate()
            .map(|(i, v)| Snapshot {
                n: i + 1,
                author: match v.author {
                    forge_shadow::Author::Agent => "agent".into(),
                    forge_shadow::Author::You => "you".into(),
                },
                label: v.label,
                created_at: v.created_at,
                content: v.content,
            })
            .collect();
        agent::run(
            service.http(),
            &target,
            search.as_ref(),
            &versions,
            request,
            cancel,
            &emit,
        )
        .await;
        runs.0.lock().unwrap().remove(&run_id);
    });
    Ok(id)
}

#[tauri::command]
pub fn agent_cancel(runs: State<'_, Arc<Runs>>, id: String) {
    if let Some(flag) = runs.0.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Debug builds only: read `<repo>/.env.local` so the dev app can use keys kept there.
/// Existing environment variables win. Release builds never read it.
pub fn load_dev_env() {
    #[cfg(debug_assertions)]
    {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.env.local");
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim().trim_matches('"'));
                if k.ends_with("_API_KEY") && std::env::var_os(k).is_none() {
                    std::env::set_var(k, v);
                }
            }
        }
    }
}
