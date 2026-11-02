//! The whole browser sign-in against a local fake authorization server that enforces
//! what real ones do: PKCE S256, exact redirect URIs, `resource` binding, single-use
//! rotating refresh tokens (reuse ends the connection).

use forge_ai::oauth::{self, RefreshError};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Default)]
struct Auth {
    /// client_id → redirect_uris
    clients: HashMap<String, Vec<String>>,
    /// code → (client_id, redirect_uri, challenge, resource)
    codes: HashMap<String, (String, String, String, String)>,
    /// live refresh tokens
    refresh: Vec<String>,
    revoked: bool,
    registrations: usize,
    seq: usize,
}

struct Req {
    method: String,
    path: String,
    query: HashMap<String, String>,
    body: String,
}

async fn read_req(sock: &mut tokio::net::TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let len = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            while buf.len() < end + 4 + len {
                let n = sock.read(&mut tmp).await.ok()?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            let mut first = head.lines().next()?.split_whitespace();
            let method = first.next()?.to_string();
            let target = first.next()?.to_string();
            let url = reqwest::Url::parse(&format!("http://x{target}")).ok()?;
            return Some(Req {
                method,
                path: url.path().to_string(),
                query: url.query_pairs().into_owned().collect(),
                body: String::from_utf8_lossy(&buf[end + 4..]).to_string(),
            });
        }
    }
}

fn form(body: &str) -> HashMap<String, String> {
    reqwest::Url::parse(&format!("http://x/?{body}"))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

fn reply(status: u16, headers: &[(&str, String)], body: &str) -> String {
    let mut s = format!(
        "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        s += &format!("{k}: {v}\r\n");
    }
    s + "\r\n" + body
}

fn json_reply(status: u16, v: Value) -> String {
    reply(
        status,
        &[("content-type", "application/json".into())],
        &v.to_string(),
    )
}

async fn serve(listener: TcpListener, auth: Arc<Mutex<Auth>>) {
    let base = format!("http://{}", listener.local_addr().unwrap());
    let resource = format!("{base}/api/");
    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            return;
        };
        let (auth, base, resource) = (auth.clone(), base.clone(), resource.clone());
        tokio::spawn(async move {
            let Some(r) = read_req(&mut sock).await else {
                return;
            };
            let out = handle(&r, &auth, &base, &resource);
            let _ = sock.write_all(out.as_bytes()).await;
        });
    }
}

fn handle(r: &Req, auth: &Mutex<Auth>, base: &str, resource: &str) -> String {
    let mut a = auth.lock().unwrap();
    match (r.method.as_str(), r.path.as_str()) {
        ("POST", "/api/search") => reply(
            403,
            &[(
                "www-authenticate",
                format!(
                    r#"Bearer resource_metadata="{base}/.well-known/oauth-protected-resource""#
                ),
            )],
            "",
        ),
        ("GET", "/.well-known/oauth-protected-resource") => json_reply(
            200,
            json!({ "resource": resource, "authorization_servers": [base], "scopes_supported": ["svc:global"] }),
        ),
        ("GET", "/.well-known/oauth-authorization-server") => json_reply(
            200,
            json!({
                "issuer": base,
                "authorization_endpoint": format!("{base}/authorize"),
                "token_endpoint": format!("{base}/token"),
                "registration_endpoint": format!("{base}/register"),
                "revocation_endpoint": format!("{base}/revoke"),
                "code_challenge_methods_supported": ["S256"],
                "scopes_supported": ["svc:global", "offline_access"],
                "token_endpoint_auth_methods_supported": ["none"],
            }),
        ),
        ("POST", "/register") => {
            let v: Value = serde_json::from_str(&r.body).unwrap();
            assert_eq!(v["token_endpoint_auth_method"], "none");
            let uris: Vec<String> = v["redirect_uris"]
                .as_array()
                .unwrap()
                .iter()
                .map(|u| u.as_str().unwrap().to_string())
                .collect();
            assert!(uris
                .iter()
                .all(|u| u.starts_with("http://127.0.0.1:") && u.ends_with("/callback")));
            a.registrations += 1;
            a.seq += 1;
            let id = format!("client-{}", a.seq);
            a.clients.insert(id.clone(), uris);
            json_reply(201, json!({ "client_id": id }))
        }
        ("GET", "/authorize") => {
            let q = &r.query;
            let redirect = &q["redirect_uri"];
            assert!(
                a.clients[&q["client_id"]].contains(redirect),
                "redirect must be registered exactly"
            );
            assert_eq!(q["code_challenge_method"], "S256");
            assert_eq!(q["resource"], resource);
            assert_eq!(q["scope"], "svc:global offline_access");
            a.seq += 1;
            let code = format!("code-{}", a.seq);
            a.codes.insert(
                code.clone(),
                (
                    q["client_id"].clone(),
                    redirect.clone(),
                    q["code_challenge"].clone(),
                    q["resource"].clone(),
                ),
            );
            let to = format!("{redirect}?code={code}&state={}", q["state"]);
            reply(302, &[("location", to)], "")
        }
        ("POST", "/token") => {
            let f = form(&r.body);
            a.seq += 1;
            let n = a.seq;
            match f["grant_type"].as_str() {
                "authorization_code" => {
                    let Some((client, redirect, challenge, res)) = a.codes.remove(&f["code"])
                    else {
                        return json_reply(400, json!({ "error": "invalid_grant" }));
                    };
                    assert_eq!(f["client_id"], client);
                    assert_eq!(f["redirect_uri"], redirect);
                    assert_eq!(f["resource"], res);
                    if oauth::pkce_challenge(&f["code_verifier"]) != challenge {
                        return json_reply(
                            400,
                            json!({ "error": "invalid_grant", "error_description": "PKCE failed" }),
                        );
                    }
                    let rt = format!("rt-{n}");
                    a.refresh.push(rt.clone());
                    json_reply(
                        200,
                        json!({ "access_token": format!("at-{n}"), "token_type": "Bearer", "expires_in": 3600, "refresh_token": rt, "scope": "svc:global offline_access" }),
                    )
                }
                "refresh_token" => {
                    let rt = &f["refresh_token"];
                    if a.revoked || !a.refresh.contains(rt) {
                        // Reuse of a spent token ends the whole connection.
                        a.revoked = true;
                        a.refresh.clear();
                        return json_reply(400, json!({ "error": "invalid_grant" }));
                    }
                    assert_eq!(f["resource"], resource);
                    a.refresh.retain(|x| x != rt);
                    let next = format!("rt-{n}");
                    a.refresh.push(next.clone());
                    json_reply(
                        200,
                        json!({ "access_token": format!("at-{n}"), "expires_in": 3600, "refresh_token": next }),
                    )
                }
                _ => json_reply(400, json!({ "error": "unsupported_grant_type" })),
            }
        }
        ("POST", "/revoke") => {
            a.revoked = true;
            a.refresh.clear();
            reply(200, &[], "")
        }
        _ => reply(404, &[], ""),
    }
}

/// Plays the browser: follows the authorization redirect back to the app's loopback.
fn browser(url: &str) {
    let url = url.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
        assert!(
            page.contains("signed in"),
            "the browser should see a success page: {page}"
        );
    });
}

#[tokio::test]
async fn signs_in_refreshes_and_detects_reuse() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let auth = Arc::new(Mutex::new(Auth::default()));
    tokio::spawn(serve(listener, auth.clone()));
    let http = forge_ai::http_client();

    // Discovery from the service's own challenge.
    let d = oauth::discover(&http, &format!("{base}/api/search"))
        .await
        .unwrap();
    assert_eq!(d.resource, format!("{base}/api/"));
    assert_eq!(d.server.token_endpoint, format!("{base}/token"));

    // First sign-in registers a client.
    let p = oauth::begin(&http, &d, "Doc Forge", None).await.unwrap();
    let client = p.client.clone();
    browser(&p.authorize_url);
    let g = oauth::finish(&http, p, &AtomicBool::new(false), Duration::from_secs(10))
        .await
        .unwrap();
    assert!(g.access_token.starts_with("at-"));
    assert!(g.refresh_token.is_some());
    assert!(!g.stale(oauth::now_ms()));

    // Signing in again reuses the registered client (same port).
    let p = oauth::begin(&http, &d, "Doc Forge", Some(&client))
        .await
        .unwrap();
    assert_eq!(p.client, client);
    assert_eq!(auth.lock().unwrap().registrations, 1);
    browser(&p.authorize_url);
    let g = oauth::finish(&http, p, &AtomicBool::new(false), Duration::from_secs(10))
        .await
        .unwrap();

    // Refresh rotates; the spent token must not be reused.
    let g2 = oauth::refresh(&http, &g).await.unwrap();
    assert_ne!(g2.refresh_token, g.refresh_token);
    assert_ne!(g2.access_token, g.access_token);
    match oauth::refresh(&http, &g).await {
        Err(RefreshError::Revoked(_)) => {}
        other => panic!("reusing a refresh token should end the connection, got {other:?}"),
    }
    // ...which really did end it, including the newest token.
    assert!(matches!(
        oauth::refresh(&http, &g2).await,
        Err(RefreshError::Revoked(_))
    ));
}

#[tokio::test]
async fn a_wrong_state_or_cancel_fails_cleanly() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(serve(listener, Arc::new(Mutex::new(Auth::default()))));
    let http = forge_ai::http_client();
    let d = oauth::discover(&http, &format!("{base}/api/search"))
        .await
        .unwrap();

    // A forged callback (wrong state) is refused.
    let p = oauth::begin(&http, &d, "Doc Forge", None).await.unwrap();
    let forged = format!("{}?code=evil&state=nope", p.client.redirect_uri);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = reqwest::get(&forged).await;
    });
    let err = oauth::finish(&http, p, &AtomicBool::new(false), Duration::from_secs(10))
        .await
        .unwrap_err();
    assert!(err.contains("different request"), "{err}");

    // Cancelling stops waiting.
    let p = oauth::begin(&http, &d, "Doc Forge", None).await.unwrap();
    let err = oauth::finish(&http, p, &AtomicBool::new(true), Duration::from_secs(10))
        .await
        .unwrap_err();
    assert!(err.contains("cancelled"), "{err}");
}

#[tokio::test]
async fn discovers_firecrawl_live() {
    // Read-only: discovery documents only. No registration, no sign-in.
    let http = forge_ai::http_client();
    let d = match oauth::discover(&http, "https://api.firecrawl.dev/v2/search").await {
        Ok(d) => d,
        Err(e) => return eprintln!("offline? skipped: {e}"),
    };
    eprintln!("firecrawl: {d:#?}");
    assert_eq!(d.resource, "https://api.firecrawl.dev/");
    assert!(d.server.registration_endpoint.is_some());
    assert_eq!(oauth::scopes_for(&d), "firecrawl:global offline_access");
}
