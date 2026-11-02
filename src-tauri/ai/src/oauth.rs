//! Browser sign-in for services that follow the MCP authorization spec (2025-11-25):
//! OAuth 2.1 for a native public client.
//!
//! 1. Discovery: an unauthenticated request gets a 401/403 whose `WWW-Authenticate`
//!    names the protected-resource metadata (RFC 9728), which names the authorization
//!    server, whose metadata (RFC 8414) lists the endpoints.
//! 2. Registration: Dynamic Client Registration (RFC 7591) as a public client with an
//!    exact loopback redirect (`http://127.0.0.1:<port>/callback`, RFC 8252). The client
//!    is remembered and reused while its port is free.
//! 3. Authorization code with PKCE (S256 only), `state`, and `resource` (RFC 8707) so the
//!    token is bound to the service it's for.
//! 4. Refresh tokens rotate and are single-use: callers must serialize refreshes and
//!    never retry one that failed with `invalid_grant`.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// What discovery found: the resource to ask a token for, and where to ask.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Discovered {
    pub resource: String,
    pub resource_name: Option<String>,
    pub scopes: Vec<String>,
    pub server: ServerMeta,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ServerMeta {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

/// A registered public client (not a secret; kept in the app's settings).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub issuer: String,
    pub client_id: String,
    pub redirect_uri: String,
}

/// A signed-in connection (secret; kept in the credentials file).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub client_id: String,
    pub resource: String,
    pub token_endpoint: String,
    pub revocation_endpoint: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix milliseconds; 0 if the server didn't say.
    pub expires_at: u64,
    pub scope: Option<String>,
}

impl Grant {
    /// True when the access token is missing its expiry margin.
    pub fn stale(&self, now_ms: u64) -> bool {
        self.expires_at != 0 && now_ms + 60_000 >= self.expires_at
    }
}

#[derive(Debug, PartialEq)]
pub enum RefreshError {
    /// The connection is over (revoked, expired, or the refresh token was reused).
    Revoked(String),
    /// Try again later; the stored grant is still good.
    Transient(String),
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("system randomness");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// RFC 7636 S256: base64url(sha256(verifier)), no padding.
pub fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// `resource_metadata="…"` from a `WWW-Authenticate: Bearer …` header.
pub fn resource_metadata_url(www_authenticate: &str) -> Option<String> {
    let i = www_authenticate.find("resource_metadata=")?;
    let rest = &www_authenticate[i + "resource_metadata=".len()..];
    let v = if let Some(q) = rest.strip_prefix('"') {
        &q[..q.find('"')?]
    } else {
        rest.split([',', ' ']).next()?
    };
    Some(v.to_string())
}

/// RFC 8414 well-known URLs for an issuer, path-inserted first.
fn server_metadata_urls(issuer: &str) -> Vec<String> {
    let Ok(u) = reqwest::Url::parse(issuer) else {
        return vec![];
    };
    let origin = u.origin().ascii_serialization();
    let path = u.path().trim_end_matches('/');
    if path.is_empty() {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server"),
            format!("{origin}/.well-known/openid-configuration"),
        ]
    } else {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server{path}"),
            format!("{origin}/.well-known/openid-configuration{path}"),
            format!("{origin}{path}/.well-known/openid-configuration"),
        ]
    }
}

async fn get_json(http: &reqwest::Client, url: &str) -> Result<Value, String> {
    let res = http
        .get(url)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| format!("Couldn't reach {url}"))?;
    if !res.status().is_success() {
        return Err(format!("{url} returned {}", res.status().as_u16()));
    }
    res.json().await.map_err(|_| format!("{url} isn't JSON"))
}

/// Discover how to sign in to `probe_url` (any endpoint of the protected service).
pub async fn discover(http: &reqwest::Client, probe_url: &str) -> Result<Discovered, String> {
    let res = http
        .post(probe_url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body("{}")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| "The service could not be reached.".to_string())?;
    let header = res
        .headers()
        .get("www-authenticate")
        .and_then(|h| h.to_str().ok())
        .and_then(resource_metadata_url);
    let probe = reqwest::Url::parse(probe_url).map_err(|e| e.to_string())?;
    let origin = probe.origin().ascii_serialization();
    let candidates = match header {
        Some(u) => vec![u],
        None => vec![
            format!(
                "{origin}/.well-known/oauth-protected-resource{}",
                probe.path().trim_end_matches('/')
            ),
            format!("{origin}/.well-known/oauth-protected-resource"),
        ],
    };
    let mut prm = None;
    for c in &candidates {
        if let Ok(v) = get_json(http, c).await {
            prm = Some(v);
            break;
        }
    }
    let prm = prm.ok_or("This service doesn't offer browser sign-in.")?;
    discovered_from(http, &prm).await
}

async fn discovered_from(http: &reqwest::Client, prm: &Value) -> Result<Discovered, String> {
    let resource = prm
        .get("resource")
        .and_then(|v| v.as_str())
        .ok_or("The sign-in metadata names no resource.")?
        .to_string();
    let issuer = prm
        .get("authorization_servers")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .ok_or("The sign-in metadata names no authorization server.")?;
    let mut server = None;
    for u in server_metadata_urls(issuer) {
        if let Ok(v) = get_json(http, &u).await {
            server = serde_json::from_value::<ServerMeta>(v).ok();
            if server.is_some() {
                break;
            }
        }
    }
    let server = server.ok_or("The authorization server's metadata couldn't be read.")?;
    // The spec requires PKCE; refuse servers that don't say they support S256.
    if !server
        .code_challenge_methods_supported
        .iter()
        .any(|m| m == "S256")
    {
        return Err("The authorization server doesn't support PKCE (S256).".into());
    }
    let strs = |v: Option<&Value>| -> Vec<String> {
        v.and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(Discovered {
        resource,
        resource_name: prm
            .get("resource_name")
            .and_then(|v| v.as_str())
            .map(String::from),
        scopes: strs(prm.get("scopes_supported")),
        server,
    })
}

/// Scopes to request: the resource's, plus `offline_access` for a refresh token when offered.
pub fn scopes_for(d: &Discovered) -> String {
    let mut s = if d.scopes.is_empty() {
        d.server
            .scopes_supported
            .iter()
            .filter(|s| *s != "offline_access")
            .cloned()
            .collect()
    } else {
        d.scopes.clone()
    };
    if d.server
        .scopes_supported
        .iter()
        .any(|x| x == "offline_access")
        && !s.iter().any(|x| x == "offline_access")
    {
        s.push("offline_access".into());
    }
    s.join(" ")
}

async fn register(
    http: &reqwest::Client,
    d: &Discovered,
    client_name: &str,
    redirect_uri: &str,
) -> Result<Client, String> {
    let endpoint =
        d.server.registration_endpoint.as_deref().ok_or(
            "The authorization server doesn't accept new apps (no registration endpoint).",
        )?;
    let res = http
        .post(endpoint)
        .timeout(Duration::from_secs(15))
        .json(&serde_json::json!({
            "client_name": client_name,
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
            "scope": scopes_for(d),
        }))
        .send()
        .await
        .map_err(|_| "The authorization server could not be reached.".to_string())?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if !ok {
        return Err(oauth_error(&v, "Registering Doc Forge was refused."));
    }
    let client_id = v
        .get("client_id")
        .and_then(|c| c.as_str())
        .ok_or("Registration returned no client id.")?;
    Ok(Client {
        issuer: d.server.issuer.clone(),
        client_id: client_id.into(),
        redirect_uri: redirect_uri.into(),
    })
}

fn oauth_error(v: &Value, fallback: &str) -> String {
    v.get("error_description")
        .or_else(|| v.get("error"))
        .and_then(|e| e.as_str())
        .map(|e| {
            let mut e = e.to_string();
            if !e.ends_with('.') {
                e.push('.');
            }
            e
        })
        .unwrap_or_else(|| fallback.into())
}

/// A sign-in waiting for the browser to come back.
pub struct Pending {
    listener: TcpListener,
    pub client: Client,
    pub authorize_url: String,
    verifier: String,
    state: String,
    discovered: Discovered,
}

/// Prepare a sign-in: listen on loopback, register (or reuse) the client, build the URL
/// to open in the browser.
pub async fn begin(
    http: &reqwest::Client,
    d: &Discovered,
    client_name: &str,
    known: Option<&Client>,
) -> Result<Pending, String> {
    let known = known.filter(|c| c.issuer == d.server.issuer);
    // Reuse the registered port when free, so the registered redirect still matches.
    let reuse_port = known
        .and_then(|c| reqwest::Url::parse(&c.redirect_uri).ok())
        .and_then(|u| u.port());
    let listener = match reuse_port {
        Some(p) => match TcpListener::bind(("127.0.0.1", p)).await {
            Ok(l) => l,
            Err(_) => TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|e| e.to_string())?,
        },
        None => TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| e.to_string())?,
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let client = match known {
        Some(c) if c.redirect_uri == redirect_uri => c.clone(),
        _ => register(http, d, client_name, &redirect_uri).await?,
    };
    let verifier = random_token(48);
    let state = random_token(24);
    let mut url =
        reqwest::Url::parse(&d.server.authorization_endpoint).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", &client.redirect_uri)
        .append_pair("code_challenge", &pkce_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("scope", &scopes_for(d))
        .append_pair("resource", &d.resource);
    Ok(Pending {
        listener,
        client,
        authorize_url: url.into(),
        verifier,
        state,
        discovered: d.clone(),
    })
}

const PAGE_DONE: &str = "You're signed in. You can close this tab and return to Doc Forge.";
const PAGE_FAILED: &str = "Sign-in didn't finish. Return to Doc Forge to try again.";

fn page(message: &str) -> String {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Doc Forge</title>\
         <body style=\"font:16px/1.5 Georgia,serif;margin:18vh auto;max-width:28rem;padding:0 1.5rem;color:#2a2622;background:#f6f2ea\">\
         <p style=\"font:600 11px/1 system-ui;letter-spacing:.12em;text-transform:uppercase;color:#8a8178\">Doc Forge</p><p>{message}</p>"
    );
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Query pairs of a request line like `GET /callback?code=…&state=… HTTP/1.1`.
fn callback_query(request: &str) -> Option<Vec<(String, String)>> {
    let target = request.lines().next()?.split_whitespace().nth(1)?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    Some(url.query_pairs().into_owned().collect())
}

/// Wait for the browser to return (up to `timeout`, or until `cancel`), then trade the
/// code for tokens.
pub async fn finish(
    http: &reqwest::Client,
    pending: Pending,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Grant, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    let code = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Sign-in was cancelled.".into());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("Sign-in timed out. Try again.".into());
        }
        let accepted =
            tokio::time::timeout(Duration::from_millis(250), pending.listener.accept()).await;
        let Ok(Ok((mut sock, _))) = accepted else {
            continue;
        };
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf))
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let Some(q) = callback_query(&req) else {
            // Favicon and other stray requests.
            let _ = sock
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await;
            continue;
        };
        let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone());
        if get("state").as_deref() != Some(pending.state.as_str()) {
            let _ = sock.write_all(page(PAGE_FAILED).as_bytes()).await;
            return Err("Sign-in came back for a different request. Try again.".into());
        }
        if let Some(err) = get("error") {
            let _ = sock.write_all(page(PAGE_FAILED).as_bytes()).await;
            return Err(match err.as_str() {
                "access_denied" => "Sign-in was declined.".into(),
                _ => get("error_description").unwrap_or(err),
            });
        }
        let Some(code) = get("code") else {
            let _ = sock.write_all(page(PAGE_FAILED).as_bytes()).await;
            return Err("Sign-in came back without a code.".into());
        };
        let _ = sock.write_all(page(PAGE_DONE).as_bytes()).await;
        break code;
    };
    let d = &pending.discovered;
    let res = http
        .post(&d.server.token_endpoint)
        .timeout(Duration::from_secs(20))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", pending.client.redirect_uri.as_str()),
            ("client_id", pending.client.client_id.as_str()),
            ("code_verifier", pending.verifier.as_str()),
            ("resource", d.resource.as_str()),
        ])
        .send()
        .await
        .map_err(|_| "The authorization server could not be reached.".to_string())?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if !ok {
        return Err(oauth_error(&v, "The sign-in code was refused."));
    }
    grant_from(&v, &pending.client.client_id, d, None)
        .ok_or_else(|| "The authorization server sent no token.".into())
}

fn grant_from(
    v: &Value,
    client_id: &str,
    d: &Discovered,
    old_refresh: Option<&str>,
) -> Option<Grant> {
    let access = v.get("access_token")?.as_str()?;
    let expires = v.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(0);
    Some(Grant {
        client_id: client_id.into(),
        resource: d.resource.clone(),
        token_endpoint: d.server.token_endpoint.clone(),
        revocation_endpoint: d.server.revocation_endpoint.clone(),
        access_token: access.into(),
        refresh_token: v
            .get("refresh_token")
            .and_then(|r| r.as_str())
            .map(String::from)
            .or_else(|| old_refresh.map(String::from)),
        expires_at: if expires > 0 {
            now_ms() + expires * 1000
        } else {
            0
        },
        scope: v.get("scope").and_then(|s| s.as_str()).map(String::from),
    })
}

/// Trade the refresh token for a new pair. The old refresh token is spent either way.
pub async fn refresh(http: &reqwest::Client, g: &Grant) -> Result<Grant, RefreshError> {
    let Some(rt) = g.refresh_token.as_deref() else {
        return Err(RefreshError::Revoked("The sign-in expired.".into()));
    };
    let res = http
        .post(&g.token_endpoint)
        .timeout(Duration::from_secs(20))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", rt),
            ("client_id", g.client_id.as_str()),
            ("resource", g.resource.as_str()),
        ])
        .send()
        .await
        .map_err(|_| {
            RefreshError::Transient("The authorization server could not be reached.".into())
        })?;
    let code = res.status().as_u16();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if (200..300).contains(&code) {
        let d = Discovered {
            resource: g.resource.clone(),
            resource_name: None,
            scopes: vec![],
            server: ServerMeta {
                issuer: String::new(),
                authorization_endpoint: String::new(),
                token_endpoint: g.token_endpoint.clone(),
                registration_endpoint: None,
                revocation_endpoint: g.revocation_endpoint.clone(),
                code_challenge_methods_supported: vec![],
                scopes_supported: vec![],
            },
        };
        return grant_from(&v, &g.client_id, &d, Some(rt))
            .ok_or_else(|| RefreshError::Transient("The refresh sent no token.".into()));
    }
    let err = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
    if code == 400 || code == 401 || err == "invalid_grant" || err == "invalid_client" {
        Err(RefreshError::Revoked(
            "The sign-in has ended. Sign in again.".into(),
        ))
    } else {
        Err(RefreshError::Transient(format!(
            "The authorization server returned {code}."
        )))
    }
}

/// Best effort: end the connection on the server (revoking the refresh token ends it).
pub async fn revoke(http: &reqwest::Client, g: &Grant) {
    let Some(endpoint) = g.revocation_endpoint.as_deref() else {
        return;
    };
    let token = g.refresh_token.as_deref().unwrap_or(&g.access_token);
    let _ = http
        .post(endpoint)
        .timeout(Duration::from_secs(10))
        .form(&[("token", token), ("client_id", g.client_id.as_str())])
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc_7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn reads_resource_metadata_from_the_challenge() {
        let h = r#"Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource/v2/mcp", error="invalid_token""#;
        assert_eq!(
            resource_metadata_url(h).as_deref(),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource/v2/mcp")
        );
        assert_eq!(
            resource_metadata_url("Bearer resource_metadata=https://a.b/x, scope=y").as_deref(),
            Some("https://a.b/x")
        );
        assert_eq!(resource_metadata_url("Bearer realm=x"), None);
    }

    #[test]
    fn well_known_urls_insert_the_issuer_path() {
        assert_eq!(
            server_metadata_urls("https://auth.example.com/tenant1")[0],
            "https://auth.example.com/.well-known/oauth-authorization-server/tenant1"
        );
        assert_eq!(
            server_metadata_urls("https://www.firecrawl.dev")[0],
            "https://www.firecrawl.dev/.well-known/oauth-authorization-server"
        );
    }

    #[test]
    fn callback_query_only_on_the_callback_path() {
        let q =
            callback_query("GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        assert_eq!(
            q,
            vec![
                ("code".into(), "abc".into()),
                ("state".into(), "xyz".into())
            ]
        );
        assert!(callback_query("GET /favicon.ico HTTP/1.1\r\n").is_none());
    }

    #[test]
    fn asks_for_offline_access_when_offered() {
        let d = Discovered {
            resource: "https://api.example.com/".into(),
            resource_name: None,
            scopes: vec!["svc:global".into()],
            server: ServerMeta {
                issuer: "https://auth".into(),
                authorization_endpoint: "https://auth/a".into(),
                token_endpoint: "https://auth/t".into(),
                registration_endpoint: None,
                revocation_endpoint: None,
                code_challenge_methods_supported: vec!["S256".into()],
                scopes_supported: vec!["svc:global".into(), "offline_access".into()],
            },
        };
        assert_eq!(scopes_for(&d), "svc:global offline_access");
    }
}
