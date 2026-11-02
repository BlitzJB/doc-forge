//! Non-secret AI settings, persisted as JSON in the app's config folder.

use crate::Kind;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub base_url: String,
    /// Display hint for a stored key (e.g. "…nJni"). Never the key itself.
    #[serde(default)]
    pub key_hint: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoleChoice {
    pub provider: String,
    pub model: String,
}

/// Which model does which job. `quick` falls back to `editor` when unset.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Roles {
    #[serde(default)]
    pub editor: Option<RoleChoice>,
    #[serde(default)]
    pub quick: Option<RoleChoice>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub roles: Roles,
    /// Apps registered with OAuth servers for browser sign-in (public clients; not secrets).
    #[serde(default)]
    pub oauth_clients: Vec<crate::oauth::Client>,
}

impl Default for AiConfig {
    fn default() -> Self {
        let mut c = AiConfig {
            providers: vec![],
            roles: Roles::default(),
            oauth_clients: vec![],
        };
        c.ensure_built_ins();
        c
    }
}

impl AiConfig {
    pub fn load(path: &Path) -> Self {
        let mut c: AiConfig = fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        c.ensure_built_ins();
        c
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(tmp, path).map_err(|e| e.to_string())
    }

    /// Built-in providers always exist (in catalog order), ahead of custom ones.
    fn ensure_built_ins(&mut self) {
        for (i, kind) in Kind::BUILT_IN.iter().enumerate() {
            let id = builtin_id(*kind);
            if !self.providers.iter().any(|p| p.id == id) {
                let info = kind.info();
                self.providers.insert(
                    i.min(self.providers.len()),
                    ProviderConfig {
                        id: id.into(),
                        kind: *kind,
                        name: info.label.into(),
                        base_url: info.default_base_url.into(),
                        key_hint: None,
                    },
                );
            }
        }
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn provider_mut(&mut self, id: &str) -> Option<&mut ProviderConfig> {
        self.providers.iter_mut().find(|p| p.id == id)
    }

    pub fn add_custom(&mut self, name: &str, base_url: &str) -> String {
        let mut n = 1;
        while self.provider(&format!("custom-{n}")).is_some() {
            n += 1;
        }
        let id = format!("custom-{n}");
        self.providers.push(ProviderConfig {
            id: id.clone(),
            kind: Kind::Custom,
            name: if name.trim().is_empty() {
                "Custom".into()
            } else {
                name.trim().into()
            },
            base_url: base_url.trim().trim_end_matches('/').into(),
            key_hint: None,
        });
        id
    }

    /// Removes a custom provider and any role pointing at it.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.providers.len();
        self.providers
            .retain(|p| !(p.id == id && p.kind == Kind::Custom));
        let removed = self.providers.len() != before;
        if removed {
            if self.roles.editor.as_ref().is_some_and(|r| r.provider == id) {
                self.roles.editor = None;
            }
            if self.roles.quick.as_ref().is_some_and(|r| r.provider == id) {
                self.roles.quick = None;
            }
        }
        removed
    }
}

pub fn builtin_id(kind: Kind) -> &'static str {
    match kind {
        Kind::Fireworks => "fireworks",
        Kind::Anthropic => "anthropic",
        Kind::Openai => "openai",
        Kind::Openrouter => "openrouter",
        Kind::Ollama => "ollama",
        Kind::Custom => "custom",
    }
}
