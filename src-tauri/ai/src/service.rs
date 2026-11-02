//! The provider service the app talks to: config + secrets + live checks.

use crate::agent::{ModelTarget, SearchTarget};
use crate::oauth::{self, Grant, RefreshError};
use crate::search::{self, SearchKind};
use crate::{
    complete,
    config::{AiConfig, RoleChoice, Roles},
    key_hint, list_models, test_model, Check, Kind, KindInfo, Secrets, Status, TestResult,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    /// Saved in Doc Forge's credentials file.
    Stored,
    Environment,
    None,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub base_url: String,
    pub info: KindInfo,
    pub key_source: KeySource,
    pub key_hint: Option<String>,
    pub removable: bool,
    /// Last live check this session, if any.
    pub check: Option<Check>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchView {
    pub kind: SearchKind,
    pub label: &'static str,
    pub key_source: KeySource,
    pub key_hint: Option<String>,
    pub env_var: &'static str,
    pub key_url: &'static str,
    pub key_placeholder: &'static str,
    /// Offers browser sign-in.
    pub sign_in: bool,
    /// Signed in through the browser (a token in the credentials file).
    pub signed_in: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AiView {
    pub providers: Vec<ProviderView>,
    pub roles: Roles,
    pub search: Vec<SearchView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Editor,
    Quick,
}

/// A provider resolved for a request: where to send it and with which key.
pub struct Endpoint {
    pub kind: Kind,
    pub base_url: String,
    pub key: Option<String>,
}

pub struct AiService {
    path: PathBuf,
    config: Mutex<AiConfig>,
    secrets: Box<dyn Secrets>,
    http: reqwest::Client,
    checks: Mutex<HashMap<String, Check>>,
    /// Refresh tokens are single-use and rotate, so refreshes never overlap.
    refreshing: tokio::sync::Mutex<()>,
    sign_in_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

fn grant_id(kind: SearchKind) -> String {
    format!("{}:oauth", kind.id())
}

impl AiService {
    pub fn new(path: impl Into<PathBuf>, secrets: Box<dyn Secrets>) -> Self {
        let path = path.into();
        let config = AiConfig::load(&path);
        AiService {
            path,
            config: Mutex::new(config),
            secrets,
            http: crate::http_client(),
            checks: Mutex::new(HashMap::new()),
            refreshing: tokio::sync::Mutex::new(()),
            sign_in_cancel: Mutex::new(None),
        }
    }

    fn persist(&self, config: &AiConfig) -> Result<(), String> {
        config.save(&self.path)
    }

    fn key_for(&self, id: &str, kind: Kind) -> (Option<String>, KeySource) {
        if let Ok(Some(k)) = self.secrets.get(id) {
            return (Some(k), KeySource::Stored);
        }
        if let Some(var) = kind.info().env_var {
            if let Ok(k) = std::env::var(var) {
                if !k.trim().is_empty() {
                    return (Some(k.trim().to_string()), KeySource::Environment);
                }
            }
        }
        (None, KeySource::None)
    }

    pub fn view(&self) -> AiView {
        let config = self.config.lock().unwrap().clone();
        let checks = self.checks.lock().unwrap().clone();
        let providers = config
            .providers
            .iter()
            .map(|p| {
                let (key, source) = self.key_for(&p.id, p.kind);
                let hint = match source {
                    KeySource::Stored => {
                        p.key_hint.clone().or_else(|| key.as_deref().map(key_hint))
                    }
                    KeySource::Environment => key.as_deref().map(key_hint),
                    KeySource::None => None,
                };
                ProviderView {
                    id: p.id.clone(),
                    kind: p.kind,
                    name: p.name.clone(),
                    base_url: p.base_url.clone(),
                    info: p.kind.info(),
                    key_source: source,
                    key_hint: hint,
                    removable: p.kind == Kind::Custom,
                    check: checks.get(&p.id).cloned(),
                }
            })
            .collect();
        let search = SearchKind::ALL
            .iter()
            .map(|k| {
                let (key, source) = self.search_key(*k);
                SearchView {
                    kind: *k,
                    label: k.label(),
                    key_source: source,
                    key_hint: key.as_deref().map(key_hint),
                    env_var: k.env_var(),
                    key_url: k.key_url(),
                    key_placeholder: k.key_placeholder(),
                    sign_in: k.sign_in_probe().is_some(),
                    signed_in: self.grant(*k).is_some(),
                }
            })
            .collect();
        AiView {
            providers,
            roles: config.roles,
            search,
        }
    }

    fn search_key(&self, kind: SearchKind) -> (Option<String>, KeySource) {
        if let Ok(Some(k)) = self.secrets.get(kind.id()) {
            return (Some(k), KeySource::Stored);
        }
        match std::env::var(kind.env_var()) {
            Ok(k) if !k.trim().is_empty() => (Some(k.trim().to_string()), KeySource::Environment),
            _ => (None, KeySource::None),
        }
    }

    fn grant(&self, kind: SearchKind) -> Option<Grant> {
        let raw = self.secrets.get(&grant_id(kind)).ok()??;
        serde_json::from_str(&raw).ok()
    }

    fn store_grant(&self, kind: SearchKind, g: &Grant) -> Result<(), String> {
        let raw = serde_json::to_string(g).map_err(|e| e.to_string())?;
        self.secrets.set(&grant_id(kind), &raw)
    }

    /// A current access token for a signed-in service, refreshed when it's about to expire.
    /// A connection the server has ended is forgotten.
    async fn access_token(&self, kind: SearchKind) -> Option<String> {
        let g = self.grant(kind)?;
        if !g.stale(oauth::now_ms()) {
            return Some(g.access_token);
        }
        let _one_at_a_time = self.refreshing.lock().await;
        // Someone else may have refreshed while this waited.
        let g = self.grant(kind)?;
        if !g.stale(oauth::now_ms()) {
            return Some(g.access_token);
        }
        match oauth::refresh(&self.http, &g).await {
            Ok(fresh) => {
                self.store_grant(kind, &fresh).ok()?;
                Some(fresh.access_token)
            }
            Err(RefreshError::Revoked(_)) => {
                let _ = self.secrets.delete(&grant_id(kind));
                None
            }
            Err(RefreshError::Transient(_)) => None,
        }
    }

    /// The search backend the agent uses: the first ready one in catalog order
    /// (Tavily, Exa, Firecrawl). A pasted key wins over a browser sign-in.
    pub async fn search_target(&self) -> Option<SearchTarget> {
        for k in SearchKind::ALL {
            if let Some(key) = self.search_key(k).0 {
                return Some(SearchTarget { kind: k, key });
            }
            if let Some(key) = self.access_token(k).await {
                return Some(SearchTarget { kind: k, key });
            }
        }
        None
    }

    /// Browser sign-in (OAuth 2.1 with PKCE, per the MCP authorization spec). Calls
    /// `open` with the page to show, waits up to five minutes for the browser to come
    /// back, then verifies the connection with a real search. Resolves to its result count.
    pub async fn sign_in(
        &self,
        kind: SearchKind,
        open: impl FnOnce(&str),
    ) -> Result<usize, String> {
        let probe = kind
            .sign_in_probe()
            .ok_or_else(|| format!("{} doesn't offer browser sign-in.", kind.label()))?;
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(prev) = self.sign_in_cancel.lock().unwrap().replace(cancel.clone()) {
            prev.store(true, Ordering::Relaxed);
        }
        let d = oauth::discover(&self.http, probe).await?;
        let known = self
            .config
            .lock()
            .unwrap()
            .oauth_clients
            .iter()
            .find(|c| c.issuer == d.server.issuer)
            .cloned();
        let pending = oauth::begin(&self.http, &d, "Doc Forge", known.as_ref()).await?;
        if known.as_ref() != Some(&pending.client) {
            let mut config = self.config.lock().unwrap();
            config
                .oauth_clients
                .retain(|c| c.issuer != pending.client.issuer);
            config.oauth_clients.push(pending.client.clone());
            self.persist(&config)?;
        }
        open(&pending.authorize_url);
        let grant = oauth::finish(&self.http, pending, &cancel, Duration::from_secs(300)).await;
        self.sign_in_cancel.lock().unwrap().take();
        let grant = grant?;
        match search::search(
            &self.http,
            kind,
            &grant.access_token,
            "Doc Forge writing app",
            3,
        )
        .await
        {
            Ok(hits) => {
                self.store_grant(kind, &grant)?;
                Ok(hits.len())
            }
            Err(e) => {
                oauth::revoke(&self.http, &grant).await;
                Err(format!("Signed in, but a test search failed: {e}"))
            }
        }
    }

    pub fn cancel_sign_in(&self) {
        if let Some(c) = self.sign_in_cancel.lock().unwrap().take() {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// Forget the browser sign-in and end it on the server.
    pub async fn sign_out(&self, kind: SearchKind) -> Result<(), String> {
        if let Some(g) = self.grant(kind) {
            self.secrets.delete(&grant_id(kind))?;
            oauth::revoke(&self.http, &g).await;
        }
        Ok(())
    }

    /// Verify a search key with a real query, then store it.
    pub async fn save_search_key(&self, kind: SearchKind, key: &str) -> Result<usize, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("Paste a key first.".into());
        }
        let hits = search::search(&self.http, kind, key, "Doc Forge writing app", 3).await?;
        self.secrets.set(kind.id(), key)?;
        Ok(hits.len())
    }

    pub fn forget_search_key(&self, kind: SearchKind) -> Result<(), String> {
        self.secrets.delete(kind.id())
    }

    /// Resolve a role to a concrete model endpoint for the agent.
    pub fn model_target(&self, role: Role) -> Result<ModelTarget, String> {
        let choice = self
            .role(role)
            .ok_or_else(|| "Choose an Editor model in Settings, under Models.".to_string())?;
        let ep = self.endpoint(&choice.provider)?;
        if ep.kind.info().needs_key && ep.key.is_none() {
            return Err(format!(
                "{} has no key. Add one in Settings, under Models.",
                ep.kind.info().label
            ));
        }
        Ok(ModelTarget {
            kind: ep.kind,
            base_url: ep.base_url,
            key: ep.key,
            model: choice.model,
        })
    }

    pub fn endpoint(&self, id: &str) -> Result<Endpoint, String> {
        let p = self
            .config
            .lock()
            .unwrap()
            .provider(id)
            .cloned()
            .ok_or_else(|| format!("Unknown provider {id}"))?;
        let (key, _) = self.key_for(&p.id, p.kind);
        Ok(Endpoint {
            kind: p.kind,
            base_url: p.base_url,
            key,
        })
    }

    /// The model configured for a role (quick falls back to editor).
    pub fn role(&self, role: Role) -> Option<RoleChoice> {
        let roles = self.config.lock().unwrap().roles.clone();
        match role {
            Role::Editor => roles.editor,
            Role::Quick => roles.quick.or(roles.editor),
        }
    }

    /// Live `/models` check; cached for the view.
    pub async fn check(&self, id: &str) -> Result<Check, String> {
        let ep = self.endpoint(id)?;
        let check = if ep.kind.info().needs_key && ep.key.is_none() {
            Check {
                status: Status::Unauthorized,
                message: "No key yet.".into(),
                latency_ms: 0,
                models: vec![],
            }
        } else if ep.base_url.trim().is_empty() {
            Check {
                status: Status::Error,
                message: "Add a base URL first.".into(),
                latency_ms: 0,
                models: vec![],
            }
        } else {
            list_models(&self.http, ep.kind, &ep.base_url, ep.key.as_deref()).await
        };
        self.checks
            .lock()
            .unwrap()
            .insert(id.to_string(), check.clone());
        Ok(check)
    }

    /// Verify a key against the provider and store it only if it is accepted.
    pub async fn save_key(&self, id: &str, key: &str) -> Result<Check, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("Paste a key first.".into());
        }
        let p = self
            .config
            .lock()
            .unwrap()
            .provider(id)
            .cloned()
            .ok_or_else(|| format!("Unknown provider {id}"))?;
        let check = list_models(&self.http, p.kind, &p.base_url, Some(key)).await;
        if check.status == Status::Connected {
            self.secrets.set(id, key)?;
            let mut config = self.config.lock().unwrap();
            if let Some(pc) = config.provider_mut(id) {
                pc.key_hint = Some(key_hint(key));
            }
            self.persist(&config)?;
            self.checks
                .lock()
                .unwrap()
                .insert(id.to_string(), check.clone());
        }
        Ok(check)
    }

    pub fn forget_key(&self, id: &str) -> Result<(), String> {
        self.secrets.delete(id)?;
        let mut config = self.config.lock().unwrap();
        if let Some(pc) = config.provider_mut(id) {
            pc.key_hint = None;
        }
        self.checks.lock().unwrap().remove(id);
        self.persist(&config)
    }

    /// Empty string resets to the provider's default address.
    pub fn set_base_url(&self, id: &str, base_url: &str) -> Result<(), String> {
        let mut config = self.config.lock().unwrap();
        let pc = config
            .provider_mut(id)
            .ok_or_else(|| format!("Unknown provider {id}"))?;
        let url = base_url.trim().trim_end_matches('/');
        if !(url.is_empty() || url.starts_with("http://") || url.starts_with("https://")) {
            return Err("The address must start with http:// or https://".into());
        }
        pc.base_url = if url.is_empty() {
            pc.kind.info().default_base_url.to_string()
        } else {
            url.to_string()
        };
        self.checks.lock().unwrap().remove(id);
        self.persist(&config)
    }

    pub fn add_custom(&self, name: &str, base_url: &str) -> Result<String, String> {
        let url = base_url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("The address must start with http:// or https://".into());
        }
        let mut config = self.config.lock().unwrap();
        let id = config.add_custom(name, url);
        self.persist(&config)?;
        Ok(id)
    }

    pub fn remove_provider(&self, id: &str) -> Result<(), String> {
        let mut config = self.config.lock().unwrap();
        if !config.remove(id) {
            return Err("Only custom providers can be removed.".into());
        }
        let _ = self.secrets.delete(id);
        self.checks.lock().unwrap().remove(id);
        self.persist(&config)
    }

    pub fn set_role(&self, role: Role, choice: Option<RoleChoice>) -> Result<(), String> {
        let mut config = self.config.lock().unwrap();
        if let Some(c) = &choice {
            if config.provider(&c.provider).is_none() {
                return Err(format!("Unknown provider {}", c.provider));
            }
        }
        match role {
            Role::Editor => config.roles.editor = choice,
            Role::Quick => config.roles.quick = choice,
        }
        self.persist(&config)
    }

    pub async fn test(&self, provider: &str, model: &str) -> Result<TestResult, String> {
        let ep = self.endpoint(provider)?;
        Ok(test_model(&self.http, ep.kind, &ep.base_url, ep.key.as_deref(), model).await)
    }

    /// A short title for an editor thread, from its opening exchange, by the Quick model.
    pub async fn title(&self, prompt: &str, reply: &str) -> Result<String, String> {
        let t = self.model_target(Role::Quick)?;
        let clip = |s: &str, n: usize| s.chars().take(n).collect::<String>();
        let ask = format!(
            "Author: {}\n\nEditor: {}",
            clip(prompt, 1200),
            clip(reply, 1200)
        );
        let r = complete(
            &self.http,
            t.kind,
            &t.base_url,
            t.key.as_deref(),
            &t.model,
            Some(TITLE_PROMPT),
            &ask,
            1024,
        )
        .await;
        if r.status != Status::Connected {
            return Err(r.message);
        }
        clean_title(&r.reply).ok_or_else(|| "The model returned no title.".into())
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }
}

const TITLE_PROMPT: &str = "You name conversations between an author and their editor. Reply with a title of two to six words for the conversation below: specific to what was asked, in sentence case, with no quotes, no final period and no emoji. Reply with the title only.";

/// First line, without quotes, a "Title:" prefix or a final period; at most 60 characters.
pub fn clean_title(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line
        .strip_prefix("Title:")
        .or_else(|| line.strip_prefix("title:"))
        .unwrap_or(line)
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '*' | '`' | '“' | '”'))
        .trim_end_matches('.')
        .trim();
    if line.is_empty() {
        return None;
    }
    let mut t: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        if let Some(i) = t.rfind(' ') {
            t.truncate(i);
        }
        t.push('…');
    }
    Some(t)
}

#[cfg(test)]
mod title_tests {
    use super::clean_title;

    #[test]
    fn cleans_model_titles() {
        assert_eq!(
            clean_title("\"Tightening the launch plan.\"\n").as_deref(),
            Some("Tightening the launch plan")
        );
        assert_eq!(
            clean_title("Title: **df versus du**").as_deref(),
            Some("df versus du")
        );
        assert_eq!(clean_title("  \n"), None);
    }
}
