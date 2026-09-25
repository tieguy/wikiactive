//! Network layer for the session ledger: SSRF-guarded source fetch,
//! archive.org save-page-now, and the Earwig copyvio client.
//!
//! Etiquette (product-internalized, `docs/api-etiquette.md`): identifying UA
//! on every request, redirect/size caps, Retry-After-aware backoff on
//! 429/503, and simple error codes instead of panics. Contract behavior is
//! pinned by `tests/integrations.rs` with httpmock (AC.12).

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use url::Url;

/// Maximum bytes fetched from any source (2 MiB).
pub const MAX_FETCH_BYTES: usize = 2 * 1024 * 1024;
/// Maximum redirects followed.
pub const MAX_REDIRECTS: usize = 5;
/// Backoff ceiling for Retry-After honoring.
const MAX_BACKOFF_SECS: u64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("url rejected by SSRF guard: {0}")]
    Ssrf(String),
    #[error("http status {status} fetching {url}")]
    Status { status: u16, url: String },
    #[error("fetch exceeded {MAX_FETCH_BYTES} bytes: {0}")]
    TooLarge(String),
    #[error("network: {0}")]
    Transport(String),
    #[error("json: {0}")]
    Json(String),
    #[error("earwig status not ok: {0}")]
    EarwigStatus(String),
    #[error("archive.org save failed: {0}")]
    SaveFailed(String),
}

/// Reject loopback/private/link-local targets and non-HTTP(S) schemes.
/// Host matching uses the parsed [`url::Host`] enum (not strings), so IPv6
/// literals and alternate IPv4 encodings cannot slip through. Redirects
/// are guarded per hop via [`guarded_redirect_policy`] — a public URL that
/// 302s to a private target is refused, not followed.
///
/// # Errors
/// [`NetError::Ssrf`] when the URL is not fetchable by policy.
pub fn ssrf_guard(url: &Url) -> Result<(), NetError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(NetError::Ssrf(format!(
            "scheme {} not allowed",
            url.scheme()
        )));
    }
    let port_ok = matches!(url.port_or_known_default(), Some(80 | 443) | None);
    if !port_ok {
        return Err(NetError::Ssrf(format!("port {:?} not allowed", url.port())));
    }
    match url.host() {
        Some(url::Host::Domain(domain)) => {
            let lower = domain.to_lowercase();
            // Host-suffix checks, not file-extension checks.
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            let blocked = lower == "localhost"
                || lower == "metadata.google.internal"
                || lower.ends_with(".local")
                || lower.ends_with(".internal")
                || lower.ends_with(".localhost");
            if blocked {
                return Err(NetError::Ssrf(format!("host {domain} not allowed")));
            }
        }
        Some(url::Host::Ipv4(ip)) => {
            if is_blocked_ipv4(ip) {
                return Err(NetError::Ssrf(format!("host {ip} not allowed")));
            }
        }
        Some(url::Host::Ipv6(ip)) => {
            if is_blocked_ipv6(ip) {
                return Err(NetError::Ssrf(format!("host {ip} not allowed")));
            }
        }
        None => {
            return Err(NetError::Ssrf("no host".into()));
        }
    }
    Ok(())
}

/// Private/loopback/link-local IPv4 space (covers every dotted-quad and
/// alternate integer/hex encoding — `Url` normalizes to one `Ipv4Addr`).
fn is_blocked_ipv4(ip: std::net::Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || {
            // 100.64.0.0/10 shared address space (CGNAT).
            let o = ip.octets();
            o[0] == 100 && (64..=127).contains(&o[1])
        }
}

/// Loopback/link-local/unique-local IPv6 space.
fn is_blocked_ipv6(ip: std::net::Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || {
        // Unique local fc00::/7 and link-local fe80::/10.
        let seg = ip.segments();
        (seg[0] & 0xfe00) == 0xfc00 || (seg[0] & 0xffc0) == 0xfe80
    }
}

/// Redirect policy that re-runs [`ssrf_guard`] on every hop and caps the
/// hop count at [`MAX_REDIRECTS`].
fn guarded_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        if ssrf_guard(attempt.url()).is_err() {
            return attempt.error("redirect target rejected by SSRF guard");
        }
        attempt.follow()
    })
}

/// Build the etiquette-conformant reqwest client (UA, per-hop guarded
/// redirect policy).
///
/// # Errors
/// Client construction failure.
pub fn http_client() -> Result<reqwest::Client, NetError> {
    reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .redirect(guarded_redirect_policy())
        .timeout(Duration::from_mins(1))
        .build()
        .map_err(|e| NetError::Transport(e.to_string()))
}

/// The shared fetcher for source pages.
pub struct SourceFetcher {
    http: reqwest::Client,
    /// SSRF guard bypass for loopback — used ONLY by tests against local
    /// mock servers (and local dev wikis). Production paths construct via
    /// [`SourceFetcher::new`], which never allows loopback.
    allow_local: bool,
}

impl Default for SourceFetcher {
    fn default() -> Self {
        Self::new().expect("reqwest client builds")
    }
}

impl SourceFetcher {
    /// Construct with the standard client (SSRF guard fully armed).
    ///
    /// # Errors
    /// Client construction failure.
    pub fn new() -> Result<Self, NetError> {
        Ok(Self {
            http: http_client()?,
            allow_local: false,
        })
    }

    /// Construct allowing loopback targets (tests, local dev wikis).
    ///
    /// # Errors
    /// Client construction failure.
    pub fn with_allow_local() -> Result<Self, NetError> {
        Ok(Self {
            http: http_client()?,
            allow_local: true,
        })
    }

    /// Fetch a source URL (SSRF-guarded, size-capped, Retry-After backoff on
    /// 429/503) and return the raw body text.
    ///
    /// # Errors
    /// SSRF rejection, status, size, transport errors.
    pub async fn fetch_text(&self, url: &str) -> Result<String, NetError> {
        let parsed = Url::parse(url).map_err(|e| NetError::Ssrf(e.to_string()))?;
        if !self.allow_local {
            ssrf_guard(&parsed)?;
        } else if !matches!(parsed.scheme(), "http" | "https") {
            return Err(NetError::Ssrf(format!(
                "scheme {} not allowed",
                parsed.scheme()
            )));
        }
        let mut attempt = 0;
        loop {
            attempt += 1;
            let resp = self
                .http
                .get(parsed.clone())
                .send()
                .await
                .map_err(|e| NetError::Transport(e.to_string()))?;
            let status = resp.status();
            if status.as_u16() == 429 || status.as_u16() == 503 {
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(2u64.saturating_mul(u64::try_from(attempt).unwrap_or(2)))
                    .min(MAX_BACKOFF_SECS);
                if attempt >= 3 {
                    return Err(NetError::Status {
                        status: status.as_u16(),
                        url: url.to_string(),
                    });
                }
                tracing::warn!(url, status = status.as_u16(), retry_after, "backing off");
                tokio::time::sleep(Duration::from_secs(retry_after)).await;
                continue;
            }
            if !status.is_success() {
                return Err(NetError::Status {
                    status: status.as_u16(),
                    url: url.to_string(),
                });
            }
            // Enforce the size cap while reading.
            let mut body: Vec<u8> = Vec::new();
            let mut stream = resp;
            while let Some(chunk) = stream
                .chunk()
                .await
                .map_err(|e| NetError::Transport(e.to_string()))?
            {
                body.extend_from_slice(&chunk);
                if body.len() > MAX_FETCH_BYTES {
                    return Err(NetError::TooLarge(url.to_string()));
                }
            }
            return Ok(String::from_utf8_lossy(&body)
                .into_owned()
                .trim()
                .to_string());
        }
    }
}

/// Convert an HTML page to plain text for quote anchoring: strip script/style
/// and tags, collapse whitespace. (Portable subset of SP42 `html_to_text`;
/// re-implemented, not copied.)
#[must_use]
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_skip = false;
    let mut skip_depth = 0usize;
    let mut tag = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match (in_tag, ch) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                // NB: the tag buffer holds the name WITHOUT the leading '<'
                // (e.g. "/style", "p", "br").
                let lower = tag.to_lowercase();
                let closing = lower.starts_with('/');
                let name = lower.trim_start_matches('/');
                if in_skip {
                    if closing && (name == "script" || name == "style") {
                        skip_depth = skip_depth.saturating_sub(1);
                        if skip_depth == 0 {
                            in_skip = false;
                        }
                    } else if !closing && (name == "script" || name == "style") {
                        skip_depth += 1;
                    }
                } else if name == "script" || name == "style" {
                    in_skip = true;
                    skip_depth = 1;
                }
                // Block-ish boundaries become word separators.
                if !in_skip
                    && ((closing
                        && matches!(
                            name,
                            "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                        ))
                        || name == "br"
                        || (closing && matches!(name, "tr" | "td" | "ul" | "ol" | "table")))
                {
                    out.push('\n');
                }
            }
            (true, other) => tag.push(other),
            (false, other) => {
                if !in_skip {
                    out.push(other);
                }
            }
        }
    }
    // Collapse whitespace runs but keep single newlines as separators.
    let mut collapsed = String::with_capacity(out.len());
    let mut last_space = false;
    for ch in out.chars() {
        if ch == '\n' {
            if !last_space {
                collapsed.push('\n');
                last_space = true;
            }
        } else if ch.is_whitespace() {
            if !last_space {
                collapsed.push(' ');
                last_space = true;
            }
        } else {
            collapsed.push(ch);
            last_space = false;
        }
    }
    collapsed.trim().to_string()
}

/// archive.org save-page-now client.
pub struct SavePageNow {
    http: reqwest::Client,
    /// Base endpoint (tests point this at a mock server).
    pub base: String,
}

impl Default for SavePageNow {
    fn default() -> Self {
        Self {
            http: http_client().expect("reqwest client builds"),
            base: "https://web.archive.org/save".to_string(),
        }
    }
}

impl SavePageNow {
    /// Construct with a custom endpoint (tests).
    ///
    /// # Panics
    /// If the shared reqwest client cannot be constructed.
    #[must_use]
    pub fn with_base(base: &str) -> Self {
        Self {
            http: http_client().expect("reqwest client builds"),
            base: base.to_string(),
        }
    }

    /// Request a snapshot, returning the archive URL (the `/web/<ts>/<url>`
    /// form) when available. Errors on rate-limit/denial responses (record
    /// nothing rather than a fake archive URL).
    ///
    /// # Errors
    /// Transport or SPN denial.
    pub async fn save(&self, url: &str) -> Result<String, NetError> {
        let resp = self
            .http
            .post(&self.base)
            .form(&[("url", url)])
            .send()
            .await
            .map_err(|e| NetError::Transport(e.to_string()))?;
        let status = resp.status();
        if status.as_u16() == 429 {
            return Err(NetError::SaveFailed(
                "rate limited (429); retry later".into(),
            ));
        }
        if !status.is_success() {
            return Err(NetError::SaveFailed(format!(
                "save-page-now returned {status}"
            )));
        }
        // Preferred: the SPA-style header; fallback: Location; last resort:
        // the /web/2/ always-latest form (explicitly marked non-pinned).
        if let Some(loc) = resp
            .headers()
            .get("content-location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
        {
            return Ok(normalize_archive_url(&loc, url));
        }
        if let Some(loc) = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
        {
            return Ok(normalize_archive_url(&loc, url));
        }
        Ok(format!("https://web.archive.org/web/2/{url}"))
    }
}

/// Turn a possibly-relative archive location into an absolute /web/ URL.
fn normalize_archive_url(loc: &str, original: &str) -> String {
    if loc.starts_with("http") {
        loc.to_string()
    } else if loc.starts_with('/') {
        format!("https://web.archive.org{loc}")
    } else {
        format!("https://web.archive.org/web/2/{original}")
    }
}

/// Earwig copyvio client (`copyvios.toolforge.org/api.json`), used
/// **post-publish** (it only sees on-wiki revisions).
pub struct EarwigClient {
    http: reqwest::Client,
    /// Base endpoint (tests point this at a mock server).
    pub base: String,
}

impl Default for EarwigClient {
    fn default() -> Self {
        Self {
            http: http_client().expect("reqwest client builds"),
            base: "https://copyvios.toolforge.org/api.json".to_string(),
        }
    }
}

impl EarwigClient {
    /// Construct with a custom endpoint (tests).
    ///
    /// # Panics
    /// If the shared reqwest client cannot be constructed.
    #[must_use]
    pub fn with_base(base: &str) -> Self {
        Self {
            http: http_client().expect("reqwest client builds"),
            base: base.to_string(),
        }
    }

    /// `action=compare`: compare a page/revision against one URL.
    /// Returns `(verdict, ratio)`.
    ///
    /// # Errors
    /// Transport, non-ok status, or unexpected shape.
    pub async fn compare(
        &self,
        title: &str,
        oldid: u64,
        url: &str,
    ) -> Result<(String, f64), NetError> {
        let resp: Value = self
            .http
            .get(&self.base)
            .query(&[
                ("action", "compare"),
                ("project", "wikipedia"),
                ("lang", "en"),
                ("title", title),
                ("oldid", &oldid.to_string()),
                ("url", url),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|e| NetError::Transport(e.to_string()))?
            .error_for_status()
            .map_err(|e| NetError::Transport(e.to_string()))?
            .json()
            .await
            .map_err(|e| NetError::Json(e.to_string()))?;
        if resp.get("status").and_then(Value::as_str) != Some("ok") {
            return Err(NetError::EarwigStatus(
                resp.get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string(),
            ));
        }
        let verdict = resp
            .pointer("/result/verdict")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let ratio = resp
            .pointer("/result/ratio")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        Ok((verdict, ratio))
    }

    /// `action=search`: search for unknown copies of a page. Returns the
    /// best source `(url, confidence)` when found.
    ///
    /// # Errors
    /// Transport, non-ok status, or unexpected shape.
    pub async fn search(&self, title: &str, oldid: u64) -> Result<Option<(String, f64)>, NetError> {
        let resp: Value = self
            .http
            .get(&self.base)
            .query(&[
                ("action", "search"),
                ("project", "wikipedia"),
                ("lang", "en"),
                ("title", title),
                ("oldid", &oldid.to_string()),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|e| NetError::Transport(e.to_string()))?
            .error_for_status()
            .map_err(|e| NetError::Transport(e.to_string()))?
            .json()
            .await
            .map_err(|e| NetError::Json(e.to_string()))?;
        if resp.get("status").and_then(Value::as_str) != Some("ok") {
            return Err(NetError::EarwigStatus(
                resp.get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string(),
            ));
        }
        let best = resp.get("best");
        if best.is_none() || best == Some(&Value::Null) {
            return Ok(None);
        }
        let url = best
            .and_then(|b| b.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let confidence = best
            .and_then(|b| b.get("confidence"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if url.is_empty() {
            return Ok(None);
        }
        Ok(Some((url, confidence)))
    }
}

/// Parsed `rsp-seed.tsv` row.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RspSeedRow {
    pub source_id: String,
    pub tier: String,
    pub note: String,
}

/// Load the RSP seed table from TSV (tab-separated, `#` comments allowed).
///
/// # Errors
/// Io or malformed-row errors.
pub fn load_rsp_seed(path: &std::path::Path) -> Result<Vec<RspSeedRow>, NetError> {
    let text = std::fs::read_to_string(path).map_err(|e| NetError::Json(e.to_string()))?;
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            return Err(NetError::Json(format!(
                "rsp seed row not 3 columns: {line}"
            )));
        }
        rows.push(RspSeedRow {
            source_id: parts[0].to_string(),
            tier: parts[1].to_string(),
            note: parts[2].to_string(),
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::{EarwigClient, SavePageNow, SourceFetcher, html_to_text, ssrf_guard};
    use url::Url;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn ssrf_guard_blocks_private_and_allows_public() {
        assert!(ssrf_guard(&u("http://127.0.0.1/x")).is_err());
        assert!(ssrf_guard(&u("http://10.0.0.1/x")).is_err());
        assert!(ssrf_guard(&u("http://192.168.1.1/x")).is_err());
        assert!(ssrf_guard(&u("http://169.254.169.254/latest/meta-data")).is_err());
        assert!(ssrf_guard(&u("http://172.16.0.1/x")).is_err());
        assert!(ssrf_guard(&u("http://localhost:8080/x")).is_err());
        assert!(ssrf_guard(&u("file:///etc/passwd")).is_err());
        assert!(ssrf_guard(&u("ftp://example.com/x")).is_err());
        assert!(ssrf_guard(&u("http://82.94.9.168/")).is_ok());
        assert!(ssrf_guard(&u("https://example.com/page")).is_ok());
    }

    #[test]
    fn ssrf_guard_blocks_ipv6_and_alternate_encodings() {
        // IPv6 loopback / link-local / unique-local.
        assert!(ssrf_guard(&u("http://[::1]/x")).is_err());
        assert!(ssrf_guard(&u("http://[fe80::1]/x")).is_err());
        assert!(ssrf_guard(&u("http://[fc00::1]/x")).is_err());
        // Alternate IPv4 encodings normalize to 127.0.0.1.
        assert!(ssrf_guard(&u("http://0x7f.0.0.1/x")).is_err());
        assert!(ssrf_guard(&u("http://2130706433/x")).is_err());
        assert!(ssrf_guard(&u("http://127.1/x")).is_err());
        // CGNAT shared space.
        assert!(ssrf_guard(&u("http://100.64.0.1/x")).is_err());
        // Public IPv6 is fine.
        assert!(ssrf_guard(&u("http://[2606:2800:220:1:248:1893:25c8:1946]/")).is_ok());
    }

    #[test]
    fn html_to_text_strips_and_collapses() {
        let html = r"<html><head><style>.x{color:red}</style></head><body>
            <p>First paragraph with <b>bold</b> words.</p><script>var x = 1;</script>
            <p>Second   paragraph.</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("First paragraph with bold words."), "{text}");
        assert!(text.contains("Second paragraph."), "{text}");
        assert!(!text.contains("color:red"), "{text}");
        assert!(!text.contains("var x"), "{text}");
    }

    #[tokio::test]
    async fn fetch_text_rejects_srf_before_network() {
        let fetcher = SourceFetcher::default();
        assert!(fetcher.fetch_text("http://127.0.0.1:9/nope").await.is_err());
    }

    #[test]
    fn endpoints_default_to_production() {
        assert!(SavePageNow::default().base.contains("web.archive.org/save"));
        assert!(
            EarwigClient::default()
                .base
                .contains("copyvios.toolforge.org")
        );
    }

    #[test]
    fn rsp_seed_parses() {
        let rows =
            super::load_rsp_seed(std::path::Path::new("rules/sources/rsp-seed.tsv")).unwrap();
        assert!(rows.len() >= 20, "{} rows", rows.len());
        assert!(
            rows.iter()
                .any(|r| r.source_id == "find-a-grave" && r.tier == "complement-only")
        );
        assert!(rows.iter().any(|r| r.tier == "deny"));
    }
}
