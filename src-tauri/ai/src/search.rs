//! Web search and page reading for the agent.
//!
//! Search goes through an agent-oriented API (Tavily, Exa or Firecrawl) with the user's
//! own key. `fetch_page` reads a public page as text and refuses local or private
//! addresses; with Firecrawl, `scrape_page` reads it through Firecrawl's renderer instead,
//! which also handles pages built with JavaScript.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::IpAddr;
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    Tavily,
    Exa,
    Firecrawl,
}

impl SearchKind {
    pub const ALL: [SearchKind; 3] = [SearchKind::Tavily, SearchKind::Exa, SearchKind::Firecrawl];

    pub fn id(self) -> &'static str {
        match self {
            SearchKind::Tavily => "search-tavily",
            SearchKind::Exa => "search-exa",
            SearchKind::Firecrawl => "search-firecrawl",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SearchKind::Tavily => "Tavily",
            SearchKind::Exa => "Exa",
            SearchKind::Firecrawl => "Firecrawl",
        }
    }
    pub fn env_var(self) -> &'static str {
        match self {
            SearchKind::Tavily => "TAVILY_API_KEY",
            SearchKind::Exa => "EXA_API_KEY",
            SearchKind::Firecrawl => "FIRECRAWL_API_KEY",
        }
    }
    pub fn key_url(self) -> &'static str {
        match self {
            SearchKind::Tavily => "https://app.tavily.com/home",
            SearchKind::Exa => "https://dashboard.exa.ai/api-keys",
            SearchKind::Firecrawl => "https://www.firecrawl.dev/app/api-keys",
        }
    }
    /// An endpoint that answers unauthenticated requests with an OAuth challenge, for
    /// services that offer browser sign-in instead of (or as well as) a pasted key.
    pub fn sign_in_probe(self) -> Option<&'static str> {
        match self {
            SearchKind::Firecrawl => Some("https://api.firecrawl.dev/v2/search"),
            _ => None,
        }
    }
    pub fn key_placeholder(self) -> &'static str {
        match self {
            SearchKind::Tavily => "tvly-…",
            SearchKind::Exa => "Exa API key",
            SearchKind::Firecrawl => "fc-…",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub async fn search(
    http: &reqwest::Client,
    kind: SearchKind,
    key: &str,
    query: &str,
    max: usize,
) -> Result<Vec<SearchHit>, String> {
    let req = match kind {
        SearchKind::Tavily => http
            .post("https://api.tavily.com/search")
            .bearer_auth(key)
            .json(&json!({ "query": query, "max_results": max, "search_depth": "basic" })),
        SearchKind::Exa => http
            .post("https://api.exa.ai/search")
            .header("x-api-key", key)
            .json(&json!({ "query": query, "numResults": max, "type": "auto", "contents": { "highlights": { "numSentences": 3 } } })),
        SearchKind::Firecrawl => http
            .post(format!("{FIRECRAWL}/v2/search"))
            .bearer_auth(key)
            .json(&json!({ "query": query, "limit": max, "sources": ["web"] })),
    };
    let res = req
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("{} timed out.", kind.label())
            } else {
                format!("{} could not be reached.", kind.label())
            }
        })?;
    let code = res.status();
    let body = res.text().await.unwrap_or_default();
    if code.as_u16() == 401 || code.as_u16() == 403 {
        return Err(format!("{} rejected the search key.", kind.label()));
    }
    if code.as_u16() == 402 {
        return Err(format!("{} is out of credits.", kind.label()));
    }
    if code.as_u16() == 429 {
        return Err(format!(
            "{} is rate limiting searches; try again shortly.",
            kind.label()
        ));
    }
    if !code.is_success() {
        return Err(format!("{} returned {}.", kind.label(), code.as_u16()));
    }
    let v: Value = serde_json::from_str(&body)
        .map_err(|_| format!("{} sent results Doc Forge couldn't read.", kind.label()))?;
    Ok(parse_hits(kind, &v, max))
}

pub fn parse_hits(kind: SearchKind, v: &Value, max: usize) -> Vec<SearchHit> {
    let items = match kind {
        // v2 groups results by source ({ data: { web: [...] } }); v1 sent { data: [...] }.
        SearchKind::Firecrawl => v
            .get("data")
            .and_then(|d| d.get("web").or(Some(d)))
            .and_then(|r| r.as_array()),
        _ => v.get("results").and_then(|r| r.as_array()),
    }
    .cloned()
    .unwrap_or_default();
    items
        .iter()
        .filter_map(|r| {
            let url = r.get("url")?.as_str()?.to_string();
            let title = r
                .get("title")
                .and_then(|t| t.as_str())
                .filter(|t| !t.is_empty())
                .unwrap_or(&url)
                .to_string();
            let snippet = match kind {
                SearchKind::Tavily => r
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string(),
                SearchKind::Exa => r
                    .get("highlights")
                    .and_then(|h| h.as_array())
                    .map(|h| {
                        h.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(" … ")
                    })
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        r.get("text")
                            .and_then(|t| t.as_str())
                            .map(|t| t.chars().take(600).collect())
                    })
                    .unwrap_or_default(),
                SearchKind::Firecrawl => r
                    .get("description")
                    .or_else(|| r.get("snippet"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string(),
            };
            Some(SearchHit {
                title,
                url,
                snippet: clip(&snippet, 700),
            })
        })
        .take(max)
        .collect()
}

const FIRECRAWL: &str = "https://api.firecrawl.dev";

// ---- Page reading --------------------------------------------------------------------

/// Refuse anything that isn't a public http(s) address.
async fn public_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "That isn't a valid URL.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only http and https pages can be read.".into());
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if is_private_host(&host) {
        return Err("Local and private network addresses can't be read.".into());
    }
    // Resolve and refuse private targets (covers names that point at local services).
    let port = parsed.port_or_known_default().unwrap_or(443);
    if let Ok(addrs) = tokio_lookup(&host, port).await {
        if addrs.iter().any(|ip| is_private_ip(*ip)) {
            return Err("Local and private network addresses can't be read.".into());
        }
    }
    Ok(parsed)
}

/// Read a public page through Firecrawl as Markdown (main content only), capped in size.
/// Private addresses are refused here too: they mean nothing to Firecrawl's servers and
/// shouldn't leave the machine.
pub async fn scrape_page(
    http: &reqwest::Client,
    key: &str,
    url: &str,
    max_chars: usize,
) -> Result<(String, String), String> {
    let parsed = public_url(url).await?;
    let res = http
        .post(format!("{FIRECRAWL}/v2/scrape"))
        .bearer_auth(key)
        .timeout(Duration::from_secs(45))
        .json(&json!({ "url": parsed.as_str(), "formats": ["markdown"], "onlyMainContent": true }))
        .send()
        .await
        .map_err(|_| "Firecrawl could not be reached.".to_string())?;
    let code = res.status().as_u16();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    match code {
        200..=299 => {}
        401 | 403 => return Err("Firecrawl rejected the key.".into()),
        402 => return Err("Firecrawl is out of credits.".into()),
        _ => {
            let why = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
            return Err(format!("Firecrawl returned {code}. {why}")
                .trim()
                .to_string());
        }
    }
    parse_scrape(&v, max_chars)
        .ok_or_else(|| "Firecrawl sent a page Doc Forge couldn't read.".into())
}

pub fn parse_scrape(v: &Value, max_chars: usize) -> Option<(String, String)> {
    let data = v.get("data")?;
    let text = data.get("markdown")?.as_str()?;
    let title = data
        .get("metadata")
        .and_then(|m| m.get("title").or_else(|| m.get("ogTitle")))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    Some((title, clip(text, max_chars)))
}

/// Fetch a public web page as plain text, capped in size.
pub async fn fetch_page(
    http: &reqwest::Client,
    url: &str,
    max_chars: usize,
) -> Result<(String, String), String> {
    let parsed = public_url(url).await?;
    let res = http
        .get(parsed.clone())
        .timeout(Duration::from_secs(20))
        .header("accept", "text/html,text/plain;q=0.9,*/*;q=0.1")
        .send()
        .await
        .map_err(|_| "The page could not be reached.".to_string())?;
    if !res.status().is_success() {
        return Err(format!("The page returned {}.", res.status().as_u16()));
    }
    let ctype = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !(ctype.is_empty()
        || ctype.contains("text/")
        || ctype.contains("xhtml")
        || ctype.contains("json"))
    {
        return Err(format!(
            "Only text pages can be read (this one is {ctype})."
        ));
    }
    let bytes = res
        .bytes()
        .await
        .map_err(|_| "The page could not be read.".to_string())?;
    let raw = String::from_utf8_lossy(&bytes[..bytes.len().min(3_000_000)]).to_string();
    let (title, text) = if ctype.contains("html") || raw.trim_start().starts_with('<') {
        html_to_text(&raw)
    } else {
        (String::new(), raw)
    };
    Ok((title, clip(&text, max_chars)))
}

async fn tokio_lookup(host: &str, port: u16) -> Result<Vec<IpAddr>, ()> {
    let lookup = tokio::net::lookup_host((host, port));
    match tokio::time::timeout(Duration::from_secs(3), lookup).await {
        Ok(Ok(addrs)) => Ok(addrs.map(|a| a.ip()).collect()),
        _ => Err(()),
    }
}

fn is_private_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.is_empty()
    {
        return true;
    }
    h.parse::<IpAddr>().map(is_private_ip).unwrap_or(false)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64 // CGNAT
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            v.is_loopback()
                || v.is_unspecified()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || v.to_ipv4_mapped()
                    .is_some_and(|m| is_private_ip(IpAddr::V4(m)))
        }
    }
}

/// Minimal, dependency-free HTML → text: drop script/style/nav chrome, keep block breaks.
pub fn html_to_text(html: &str) -> (String, String) {
    let lower = html.to_ascii_lowercase();
    let title = lower
        .find("<title")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .and_then(|s| lower[s..].find("</title>").map(|e| (s, s + e)))
        .map(|(s, e)| decode_entities(html[s..e].trim()))
        .unwrap_or_default();

    let mut out = String::with_capacity(html.len() / 3);
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut skip_until: Option<&'static str> = None;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let end = match html[i..].find('>') {
                Some(e) => i + e,
                None => break,
            };
            let tag = lower[i + 1..end]
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("")
                .to_string();
            let closing = lower[i + 1..end].starts_with('/');
            if let Some(stop) = skip_until {
                if closing && tag == stop {
                    skip_until = None;
                }
            } else if !closing
                && [
                    "script", "style", "noscript", "svg", "nav", "footer", "header", "form",
                    "head", "title",
                ]
                .contains(&tag.as_str())
            {
                skip_until = Some(match tag.as_str() {
                    "head" => "head",
                    "title" => "title",
                    "script" => "script",
                    "style" => "style",
                    "noscript" => "noscript",
                    "svg" => "svg",
                    "nav" => "nav",
                    "footer" => "footer",
                    "header" => "header",
                    _ => "form",
                });
            } else if [
                "p",
                "div",
                "br",
                "li",
                "h1",
                "h2",
                "h3",
                "h4",
                "tr",
                "section",
                "article",
                "blockquote",
            ]
            .contains(&tag.as_str())
            {
                out.push('\n');
            }
            i = end + 1;
            continue;
        }
        let next = html[i..].find('<').map(|n| i + n).unwrap_or(bytes.len());
        if skip_until.is_none() {
            out.push_str(&html[i..next]);
        }
        i = next;
    }
    let text = decode_entities(&out);
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    lines.dedup();
    (title, lines.join("\n"))
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&rsquo;", "\u{2019}")
        .replace("&lsquo;", "\u{2018}")
        .replace("&ldquo;", "\u{201c}")
        .replace("&rdquo;", "\u{201d}")
        .replace("&mdash;", "\u{2014}")
        .replace("&ndash;", "\u{2013}")
}

pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tavily_and_exa() {
        let t = json!({ "results": [{ "title": "A", "url": "https://a.example", "content": "alpha" }, { "url": "https://b.example", "content": "beta" }] });
        let h = parse_hits(SearchKind::Tavily, &t, 5);
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].title, "https://b.example");
        let e = json!({ "results": [{ "title": "X", "url": "https://x.example", "highlights": ["one", "two"] }] });
        assert_eq!(parse_hits(SearchKind::Exa, &e, 5)[0].snippet, "one … two");
    }

    #[test]
    fn parses_firecrawl_v2_and_v1() {
        let v2 = json!({ "success": true, "data": { "web": [
            { "url": "https://a.com", "title": "A", "description": "about a", "position": 1 }
        ] } });
        let h = parse_hits(SearchKind::Firecrawl, &v2, 5);
        assert_eq!(
            (h[0].title.as_str(), h[0].snippet.as_str()),
            ("A", "about a")
        );
        let v1 = json!({ "success": true, "data": [ { "url": "https://b.com", "title": "", "description": "b" } ] });
        assert_eq!(
            parse_hits(SearchKind::Firecrawl, &v1, 5)[0].title,
            "https://b.com"
        );
        let page = json!({ "success": true, "data": { "markdown": "# Hi\n\nBody", "metadata": { "title": "Hi there", "statusCode": 200 } } });
        assert_eq!(
            parse_scrape(&page, 100),
            Some(("Hi there".into(), "# Hi\n\nBody".into()))
        );
    }

    #[test]
    fn refuses_private_addresses() {
        for h in [
            "localhost",
            "127.0.0.1",
            "10.0.0.8",
            "192.168.1.1",
            "169.254.169.254",
            "[::1]",
            "printer.local",
            "100.64.0.1",
            "fd00::1",
        ] {
            assert!(is_private_host(h), "{h} should be refused");
        }
        assert!(!is_private_host("example.com"));
        assert!(!is_private_host("93.184.216.34"));
    }

    #[test]
    fn html_to_text_keeps_content_drops_chrome() {
        let (title, text) = html_to_text("<html><head><title>T &amp; Co</title><style>x{}</style></head><body><nav>Menu</nav><h1>Head</h1><p>One&nbsp;two</p><script>evil()</script><p>Three</p></body></html>");
        assert_eq!(title, "T & Co");
        assert_eq!(text, "Head\nOne two\nThree");
    }
}
