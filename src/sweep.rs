//! The source sweep (plan-003 B.3): fetch-or-dispose every cited source
//! BEFORE textual analysis. The 1874→1870→1874 Kidder episode happened
//! because source-accessibility decisions were made during analysis; the
//! sweep makes them once, up front, against the full picture
//! (`docs/design-plans/2026-09-25-mvp2-addendum.md`, Phase-A retrospective).
//!
//! Framework-free core (lands before the server):
//!
//! - [`parse_citations`] turns a base wikitext's citation apparatus (ref
//!   bodies, cite-template URLs, bare URLs, ISBNs, dead-link archive
//!   params) into inventory candidates.
//! - [`classify_fetch`] maps a fetch result to a sweep status using HTTP
//!   status classes plus marker strings configured in `rules/sweep.toml`
//!   (bounded by design — no open-ended heuristics; operator override is
//!   always `wa sweep dispose`).
//! - [`sweep_fetch_one`] runs one pending source through fetch → classify
//!   → (CDX availability for dead links) → ledger status update.

use crate::ledger::Ledger;
use crate::ledger::net::CdxClient;
use crate::ledger::net::NetError;
use crate::ledger::net::SourceFetcher;

/// Sweep statuses (the `sweep_status` field's vocabulary).
pub mod status {
    /// Registered, not yet fetched.
    pub const PENDING: &str = "pending";
    /// Text extracted and stored in the ledger.
    pub const FETCHED: &str = "fetched";
    /// Tooling cannot get the text (paywall/403/lending): operator
    /// resolves by capture (`wa ledger attach`) or disposition.
    pub const NEEDS_OPERATOR: &str = "needs_operator";
    /// Original dead; a Wayback snapshot exists (auto-registered).
    pub const SNAPSHOT_AVAILABLE: &str = "snapshot_available";
    /// Reachable but no extractable text (PDF/image/JS shell).
    pub const NO_TEXT: &str = "no_text";
}

/// One inventory candidate parsed from the citation apparatus.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CitationCandidate {
    /// Fetch target: the cite's `url=` (or `chapter-url=`, or the
    /// `doi.org`/`wikidata.org` resolution for doi/cite-q citations), or
    /// its `archive-url=` when the original is dead (what the live
    /// session did for True West).
    pub url: Option<String>,
    /// The original (dead) URL when `url-status=dead` + `archive-url=`.
    pub dead_original: Option<String>,
    /// `isbn=` for URL-less books (registered as `isbn:<n>` pseudo-URLs).
    pub isbn: Option<String>,
    /// `title=` (inventory metadata).
    pub title: Option<String>,
    /// `work=`/`newspaper=`/`journal=`/`magazine=`/`website=`.
    pub work: Option<String>,
    /// A citation this tool cannot key (`cite:…` pseudo-URL): registered
    /// as a VISIBLE `needs_operator` row — never silently dropped
    /// (review finding: cite templates without a derivable key used to
    /// vanish).
    pub needs_operator: bool,
}

impl CitationCandidate {
    /// The ledger key: the fetch URL, the `isbn:` pseudo-URL, or the
    /// `cite:` visibility pseudo-URL.
    #[must_use]
    pub fn ledger_url(&self) -> Option<String> {
        if let Some(url) = &self.url {
            return Some(url.clone());
        }
        if let Some(isbn) = &self.isbn {
            return Some(format!("isbn:{isbn}"));
        }
        if self.needs_operator {
            let title = self.title.as_deref().unwrap_or("untitled");
            let key: String = title.chars().take(60).collect();
            return Some(format!("cite:{key}"));
        }
        None
    }
}

/// Split a template's body into top-level `|`-separated parameters
/// (brace/bracket nesting aware — `{{cite web|title={{lang|fr|…}}}}` is
/// one parameter, not three).
fn split_params(body: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    for ch in body.chars() {
        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            '|' if depth <= 0 => {
                params.push(current.clone());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    params.push(current);
    params
}

fn param_value(params: &[String], names: &[&str]) -> Option<String> {
    for p in params {
        for name in names {
            if let Some(rest) = p.trim().strip_prefix(name) {
                let rest = rest.trim_start();
                if let Some(v) = rest.strip_prefix('=') {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// Parse the citation apparatus of a wikitext page into inventory
/// candidates: `<ref>` bodies with cite templates, cite templates outside
/// refs (further-references bullets), and bare URLs. Deduplicates by
/// ledger key; named-ref self-closing tags are covered by their
/// definitions.
// One pass per apparatus element; splitting further would obscure the
// dedupe invariant (all candidates flow through one `push`).
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn parse_citations(wikitext: &str) -> Vec<CitationCandidate> {
    let mut candidates: Vec<CitationCandidate> = Vec::new();
    {
        let candidates = &mut candidates;
        // All keys flow through one dedupe.
        let mut push = |c: CitationCandidate| {
            let Some(key) = c.ledger_url() else { return };
            if !candidates
                .iter()
                .any(|e| e.ledger_url().is_some_and(|k| k == key))
            {
                candidates.push(c);
            }
        };

        let ref_bodies = collect_ref_bodies(wikitext);

        // Harvester 1 — cite/citation templates (inside refs and loose):
        // url=, chapter-url=, doi=, cite-q positionals, archive pairs,
        // ISBNs; anything else becomes a visible `cite:` row.
        for text in ref_bodies.iter().map(String::as_str).chain([wikitext]) {
            for body in templates_in(text).into_iter().flatten() {
                harvest_cite_template(&body, &mut push);
            }
        }

        // Harvester 2 — every http(s):// token anywhere in the raw
        // wikitext (refs included): plain external-link refs, URLs inside
        // non-cite templates ({{Official website|…}}), bullets, and
        // mid-line links. Substring suppression keeps a dead original
        // embedded in its own Wayback URL from double-registering.
        for url in url_tokens(wikitext) {
            push(CitationCandidate {
                url: Some(url),
                ..Default::default()
            });
        }
    }
    candidates
}

/// Cite-template harvest (one template body). Keys derive in order:
/// `url=` (or the archive pair when dead), `chapter-url=`, `doi=`
/// (resolved to doi.org), cite-q positionals (wikidata.org), `isbn=`,
/// and — only when none of those exist — a visible `cite:` pseudo-key.
fn harvest_cite_template(body: &str, push: &mut impl FnMut(CitationCandidate)) {
    let lowered = body.trim_start().to_ascii_lowercase();
    // First token: "cite" family always; bare "citation" is the citation
    // template, but "citation needed" (et al.) is a maintenance tag, not
    // a citation.
    let first_word = lowered
        .split(|c: char| c.is_whitespace() || c == '|')
        .next()
        .unwrap_or_default();
    let is_cite = first_word == "cite"
        || (first_word == "citation"
            && lowered
                .strip_prefix("citation")
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('|')));
    if !is_cite {
        return;
    }
    let params = split_params(body);
    let url = param_value(&params, &["url", "URL"]);
    let archive_url = param_value(&params, &["archive-url", "archive_url"]);
    let dead = matches!(
        param_value(&params, &["url-status", "dead-url", "urlstatus"]).as_deref(),
        Some("dead" | "yes" | "true")
    );
    let title = param_value(&params, &["title"]);
    let work = param_value(
        &params,
        &[
            "work",
            "newspaper",
            "journal",
            "magazine",
            "website",
            "publisher",
        ],
    );

    // cite q: positional Wikidata ids → a fetchable wikidata.org URL.
    if lowered.starts_with("cite q") {
        let positional = params
            .iter()
            .skip(1)
            .map(|p| p.trim().to_string())
            .find(|p| {
                p.len() > 1
                    && p[..1].eq_ignore_ascii_case("q")
                    && p[1..].chars().all(|c| c.is_ascii_digit())
            });
        if let Some(qid) = positional {
            push(CitationCandidate {
                url: Some(format!("https://www.wikidata.org/wiki/{qid}")),
                title,
                work,
                ..Default::default()
            });
            return;
        }
    }

    let mut candidate = if dead && archive_url.is_some() {
        CitationCandidate {
            url: archive_url,
            dead_original: url.clone(),
            isbn: None,
            title,
            work,
            needs_operator: false,
        }
    } else {
        let isbn = param_value(&params, &["isbn", "ISBN"]);
        CitationCandidate {
            url,
            dead_original: None,
            isbn,
            title,
            work,
            needs_operator: false,
        }
    };
    if candidate.url.is_none() {
        // No url=: try chapter-url, then doi, then a visible pseudo-key.
        if let Some(chapter) = param_value(&params, &["chapter-url", "chapterurl"]) {
            candidate.url = Some(chapter);
        } else if let Some(doi) = param_value(&params, &["doi", "DOI"]) {
            candidate.url = Some(format!("https://doi.org/{doi}"));
        }
    }
    if candidate.url.is_none() && candidate.isbn.is_none() {
        // A citation with no derivable key: visible, never dropped.
        candidate.needs_operator = true;
    }
    push(candidate);
}

/// Extract every `http(s)://…` token from raw wikitext, dropping tokens
/// that are substrings of a longer token from the same text (a dead
/// original embedded inside its own Wayback snapshot URL).
fn url_tokens(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        // Scheme start: the 'h' of http(s)://, on a char boundary.
        if c == 'h' && (text[pos..].starts_with("https://") || text[pos..].starts_with("http://")) {
            let mut j = i + 1;
            while j < chars.len() {
                let ch = chars[j].1;
                if ch.is_whitespace() || "|]}<>\"'),".contains(ch) {
                    break;
                }
                j += 1;
            }
            let end = if j < chars.len() {
                chars[j].0
            } else {
                text.len()
            };
            let url: String = text[pos..end].trim_end_matches(['.', ';', ':']).to_string();
            if url.contains('.') && !tokens.contains(&url) {
                tokens.push(url);
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    // Substring suppression: a token contained in a longer token (e.g.
    // the dead original inside its archive.org wrapper) is that longer
    // URL's payload, not a separate source.
    tokens
        .iter()
        .filter(|t| {
            !tokens
                .iter()
                .any(|o| o.len() > t.len() && o.contains(t.as_str()))
        })
        .cloned()
        .collect()
}

fn collect_ref_bodies(wikitext: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut rest = wikitext.to_string();
    while let Some(start) = rest.find("<ref") {
        let Some(gt_rel) = rest[start..].find('>') else {
            break;
        };
        let body_start = start + gt_rel + 1;
        if rest.as_bytes().get(start + gt_rel - 1) == Some(&b'/') {
            // Self-closing <ref name=x />.
            rest = rest[body_start..].to_string();
            continue;
        }
        let Some(len) = rest[body_start..].find("</ref>") else {
            break;
        };
        bodies.push(rest[body_start..body_start + len].to_string());
        rest = rest[body_start + len + 6..].to_string();
    }
    bodies
}

/// Yield the bodies of `{{ … }}` templates (nesting-aware, innermost
/// first not guaranteed — outer bodies include inner ones; parameter
/// splitting tolerates that).
fn templates_in(text: &str) -> Vec<Option<String>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '{' && chars[i + 1] == '{' {
            stack.push(i);
            i += 2;
            continue;
        }
        if chars[i] == '}' && chars[i + 1] == '}' {
            if let Some(start) = stack.pop() {
                let body: String = chars[start + 2..i].iter().collect();
                out.push(Some(body));
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

/// Strip `<ref>` tags (whole bodies) and `{{ … }}` templates (outermost
/// span, inner templates included), leaving prose and bare URLs.
#[must_use]
pub fn strip_refs_and_templates(wikitext: &str) -> String {
    strip_templates(&strip_ref_bodies(wikitext))
}

fn strip_ref_bodies(text: &str) -> String {
    let mut s = text.to_string();
    while let Some(open) = s.find("<ref") {
        let Some(gt) = s[open..].find('>') else { break };
        if s.as_bytes().get(open + gt - 1) == Some(&b'/') {
            // Self-closing <ref name=x />.
            s.replace_range(open..=open + gt, "");
            continue;
        }
        let body_start = open + gt + 1;
        let Some(close) = s[body_start..].find("</ref>") else {
            break;
        };
        s.replace_range(open..body_start + close + 6, "");
    }
    s
}

fn strip_templates(text: &str) -> String {
    let mut s = text.to_string();
    while let Some(open) = s.find("{{") {
        let chars: Vec<char> = s.chars().collect();
        let open_chars = s[..open].chars().count();
        let mut depth = 0_i32;
        let mut i = 0_usize;
        let mut replaced = false;
        while i + 1 < chars.len() {
            if chars[i] == '{' && chars[i + 1] == '{' {
                depth += 1;
                i += 2;
                continue;
            }
            if chars[i] == '}' && chars[i + 1] == '}' {
                depth -= 1;
                if depth == 0 {
                    let mut out = String::with_capacity(s.len());
                    out.extend(chars[..open_chars].iter());
                    out.extend(chars[i + 2..].iter());
                    s = out;
                    replaced = true;
                    break;
                }
                i += 2;
                continue;
            }
            i += 1;
        }
        if !replaced {
            break;
        }
    }
    s
}

/// Sweep classification config (`rules/sweep.toml`): bounded marker
/// strings — HTTP status classes plus these, nothing open-ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SweepConfig {
    /// Marker strings that mean "paywall" in fetched page text
    /// (case-insensitive substring) → `needs_operator`.
    pub paywall_markers: Vec<String>,
    /// Marker strings that mean "lending/registration gate" →
    /// `needs_operator`.
    pub lending_markers: Vec<String>,
}

impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            paywall_markers: vec![
                "subscribe to continue reading".into(),
                "subscribe now to read".into(),
                "create a free account to continue".into(),
                "already a subscriber? sign in".into(),
                "subscribe today".into(),
            ],
            lending_markers: vec![
                "borrow this book".into(),
                "lending library".into(),
                "log in to borrow".into(),
                "register to read".into(),
            ],
        }
    }
}

impl SweepConfig {
    /// Parse from TOML text (`rules/sweep.toml`).
    ///
    /// # Errors
    /// Malformed TOML.
    pub fn from_toml_str(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("sweep config: {e}"))
    }

    /// Load from a path on disk.
    ///
    /// # Errors
    /// Unreadable file or malformed TOML.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_toml_str(&text)
    }
}

/// A fetch outcome before sweep-status mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    /// Text extracted and stored.
    Fetched,
    /// Tooling cannot reach the text; the operator resolves
    /// (capture/attach or disposition). Carries the reason.
    NeedsOperator(String),
    /// The URL is dead — check the Wayback CDX for a snapshot.
    Dead,
    /// Reachable, nothing extractable (PDF/image/JS shell).
    NoText,
    /// Transient (rate-limit, 5xx): stays `pending` so the next
    /// `wa sweep fetch` retries it — never misfiled as terminal.
    RetryLater(String),
}

/// Classify one fetch result (bounded: status classes + configured
/// markers, nothing else — `rules/sweep.toml` is the tuning surface).
#[must_use]
pub fn classify_fetch(status: u16, text: &str, cfg: &SweepConfig) -> FetchOutcome {
    if status == 401 || status == 403 {
        return FetchOutcome::NeedsOperator(format!("HTTP {status} (bot-block/forbidden)"));
    }
    if status == 404 || status == 410 {
        return FetchOutcome::Dead;
    }
    if status == 429 || (500..=599).contains(&status) {
        return FetchOutcome::RetryLater(format!("HTTP {status} (transient)"));
    }
    if (400..=499).contains(&status) {
        // Other 4xx: real but not one of the known classes — visible,
        // not silently terminal.
        return FetchOutcome::NeedsOperator(format!("HTTP {status} — classify manually"));
    }
    let lowered = text.to_lowercase();
    for marker in &cfg.paywall_markers {
        if lowered.contains(&marker.to_lowercase()) {
            return FetchOutcome::NeedsOperator(format!("paywall marker: {marker}"));
        }
    }
    for marker in &cfg.lending_markers {
        if lowered.contains(&marker.to_lowercase()) {
            return FetchOutcome::NeedsOperator(format!("lending marker: {marker}"));
        }
    }
    let substantial = text.trim().chars().count() > 200;
    if substantial {
        FetchOutcome::Fetched
    } else {
        FetchOutcome::NoText
    }
}

/// Result of one sweep-fetch pass over a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepOutcome {
    /// The new canonical `sweep_status`.
    pub status: String,
    /// Human-facing note for the manifest (e.g. the paywall marker hit).
    pub note: Option<String>,
}

/// Fetch, classify, and update one pending sweep source — the same
/// fetch/extract path as `wa ledger fetch` (HTML → text). Dead links get
/// a CDX availability check (`snapshot_available` with the snapshot URL
/// auto-registered; dead without a snapshot → `needs_operator`).
///
/// # Errors
/// [`SweepError`] for unknown ids, ledger failures, or transport errors
/// that are not classifications.
pub async fn sweep_fetch_one(
    fetcher: &SourceFetcher,
    cdx: &CdxClient,
    ledger: &mut Ledger,
    source_id: &str,
    cfg: &SweepConfig,
) -> Result<SweepOutcome, SweepError> {
    use crate::ledger::net::html_to_text;

    let entry = ledger
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .cloned()
        .ok_or_else(|| SweepError::UnknownSource(source_id.to_string()))?;
    if !entry.url.starts_with("http://") && !entry.url.starts_with("https://") {
        // isbn:/print sources: already auto-dispositioned at inventory.
        ledger.set_sweep_status(source_id, status::NO_TEXT)?;
        return Ok(SweepOutcome {
            status: status::NO_TEXT.into(),
            note: None,
        });
    }
    // Prefer the sweep-found Wayback snapshot (dead originals).
    let fetch_target = entry
        .metadata
        .as_ref()
        .and_then(|m| m.snapshot_url.clone())
        .unwrap_or_else(|| entry.url.clone());

    let fetch_result = fetcher.fetch_text(&fetch_target).await;
    let (outcome, text) = match fetch_result {
        Ok(body) => {
            let text = if body.trim_start().starts_with('<') {
                html_to_text(&body)
            } else {
                body
            };
            (classify_fetch(200, &text, cfg), Some(text))
        }
        Err(NetError::Status { status, .. }) => (classify_fetch(status, "", cfg), None),
        // Transport failures (DNS, timeouts, local outages) are NOT dead
        // links: visible needs_operator with the error text, so a live
        // source is never mislabeled dead over a transient network blip.
        Err(NetError::Transport(e)) => {
            (FetchOutcome::NeedsOperator(format!("transport: {e}")), None)
        }
        Err(e) => return Err(e.into()),
    };

    match outcome {
        FetchOutcome::Fetched => {
            let text = text.unwrap_or_default();
            ledger.attach_fetched_text(source_id, text)?;
            ledger.set_sweep_status(source_id, status::FETCHED)?;
            Ok(SweepOutcome {
                status: status::FETCHED.into(),
                note: None,
            })
        }
        FetchOutcome::NoText => {
            ledger.set_sweep_status(source_id, status::NO_TEXT)?;
            Ok(SweepOutcome {
                status: status::NO_TEXT.into(),
                note: Some("reachable, no extractable text".into()),
            })
        }
        FetchOutcome::NeedsOperator(reason) => {
            ledger.set_sweep_status(source_id, status::NEEDS_OPERATOR)?;
            Ok(SweepOutcome {
                status: status::NEEDS_OPERATOR.into(),
                note: Some(reason),
            })
        }
        FetchOutcome::RetryLater(reason) => {
            // Stays pending: the next `wa sweep fetch` retries it.
            Ok(SweepOutcome {
                status: status::PENDING.into(),
                note: Some(reason),
            })
        }
        FetchOutcome::Dead => {
            let snapshot = cdx.closest_snapshot(&entry.url).await?;
            if let Some(url) = snapshot {
                ledger.set_snapshot_url(source_id, &url)?;
                ledger.set_sweep_status(source_id, status::SNAPSHOT_AVAILABLE)?;
                Ok(SweepOutcome {
                    status: status::SNAPSHOT_AVAILABLE.into(),
                    note: Some(url),
                })
            } else {
                ledger.set_sweep_status(source_id, status::NEEDS_OPERATOR)?;
                Ok(SweepOutcome {
                    status: status::NEEDS_OPERATOR.into(),
                    note: Some("dead link, no Wayback snapshot".into()),
                })
            }
        }
    }
}

/// Sweep pipeline errors.
#[derive(Debug, thiserror::Error)]
pub enum SweepError {
    /// Unknown ledger source id.
    #[error("unknown source {0}")]
    UnknownSource(String),
    /// Network failure that is not a classification.
    #[error("network: {0}")]
    Net(#[from] NetError),
    /// Ledger update failure.
    #[error("ledger: {0}")]
    Ledger(#[from] crate::ledger::LedgerError),
}

#[cfg(test)]
mod tests {
    use super::{SweepConfig, classify_fetch, parse_citations, split_params};

    #[test]
    fn param_split_respects_nesting() {
        let params = split_params("cite web|url=http://x|title={{lang|fr|Oui}}|isbn=1");
        assert_eq!(params.len(), 4, "{params:?}");
    }

    #[test]
    fn parses_dead_link_archive_pair() {
        let w = r#"<ref name="tw">{{cite magazine|url=http://live.example/|title=Railroad's First Lady|archive-url=https://web.archive.org/web/2013/http://live.example/|archive-date=2013-02-02|url-status=dead}}</ref>"#;
        let cands = parse_citations(w);
        assert_eq!(cands.len(), 1);
        assert_eq!(
            cands[0].url.as_deref(),
            Some("https://web.archive.org/web/2013/http://live.example/")
        );
        assert_eq!(
            cands[0].dead_original.as_deref(),
            Some("http://live.example/")
        );
    }

    #[test]
    fn url_less_book_is_isbn_candidate() {
        let w = "* {{Cite book|title=Never Come|year=1986|isbn=0961526106}}";
        let cands = parse_citations(w);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].isbn.as_deref(), Some("0961526106"));
        assert_eq!(cands[0].ledger_url().as_deref(), Some("isbn:0961526106"));
    }

    #[test]
    fn dedupes_by_url_across_refs() {
        let w = "<ref name=\"a\">{{cite web|url=http://x/|title=One}}</ref><ref name=\"b\">{{cite web|url=http://x/|title=One}}</ref>{{cite web|url=http://x/|title=One}}";
        assert_eq!(parse_citations(w).len(), 1);
    }

    #[test]
    fn bare_url_bullets_inventory() {
        let w = "==External links==\n* [http://example.org/a Example]\n* http://example.org/b\n";
        let cands = parse_citations(w);
        let urls: Vec<_> = cands.iter().filter_map(|c| c.url.clone()).collect();
        assert!(
            urls.contains(&"http://example.org/a".to_string()),
            "{urls:?}"
        );
        assert!(
            urls.contains(&"http://example.org/b".to_string()),
            "{urls:?}"
        );
    }

    /// Review finding 1: plain external-link refs (`<ref>[url label]</ref>`)
    /// are inventory — never silently missed.
    #[test]
    fn plain_external_link_refs_are_inventoried() {
        let w = r"Text.<ref>[https://example.com/obit Obituary in the Daily Example]</ref>";
        let cands = parse_citations(w);
        assert_eq!(cands.len(), 1, "{cands:?}");
        assert_eq!(cands[0].url.as_deref(), Some("https://example.com/obit"));
    }

    /// Review findings 2–3: cite templates without `url=`/`isbn=` derive a
    /// key (cite q → wikidata, doi → doi.org, chapter-url), URLs inside
    /// NON-cite templates are harvested, and truly unkeyable citations
    /// become a VISIBLE `cite:` row — nothing silently drops.
    #[test]
    fn unkeyed_citations_derive_keys_or_degrade_visibly() {
        let cands = parse_citations("{{cite q|Q1|Q42|title=Thing}}");
        assert_eq!(
            cands[0].url.as_deref(),
            Some("https://www.wikidata.org/wiki/Q1"),
            "{cands:?}"
        );
        let cands = parse_citations("{{cite journal|title=T|doi=10.1000/x}}");
        assert_eq!(cands[0].url.as_deref(), Some("https://doi.org/10.1000/x"));
        let cands = parse_citations("{{cite book|title=T|chapter-url=https://ex.example/c}}");
        assert_eq!(cands[0].url.as_deref(), Some("https://ex.example/c"));
        let cands = parse_citations("{{cite book|title=Only In Print Vol 2}}");
        assert_eq!(
            cands[0].ledger_url().as_deref(),
            Some("cite:Only In Print Vol 2")
        );
        let cands = parse_citations("{{Official website|https://example.org/official}}");
        assert!(
            cands
                .iter()
                .any(|c| c.url.as_deref() == Some("https://example.org/official"))
        );
        // {{citation needed}} is a maintenance tag, NOT a citation.
        assert!(parse_citations("Uncited{{citation needed|date=2019}}.").is_empty());
    }

    /// Review finding 4: mid-line and multiple bracketed links inventory.
    #[test]
    fn midline_links_are_inventoried() {
        let w = "See [https://example.com/b the article] and visit https://example.com/c today.\n\
                 * [https://example.com/d One] and [https://example.com/e Two]\n";
        let urls: Vec<_> = parse_citations(w)
            .into_iter()
            .filter_map(|c| c.url)
            .collect();
        for expected in [
            "https://example.com/b",
            "https://example.com/c",
            "https://example.com/d",
            "https://example.com/e",
        ] {
            assert!(
                urls.contains(&expected.to_string()),
                "missing {expected}: {urls:?}"
            );
        }
    }

    /// The dead original embedded inside its own Wayback URL does not
    /// double-register (substring suppression).
    #[test]
    fn embedded_dead_original_is_not_a_second_source() {
        let w = "https://web.archive.org/web/2013/http://dead.example/x";
        let urls: Vec<_> = parse_citations(w)
            .into_iter()
            .filter_map(|c| c.url)
            .collect();
        assert_eq!(urls.len(), 1, "{urls:?}");
        assert!(urls[0].starts_with("https://web.archive.org/"));
    }

    /// Review finding 5: transient statuses (429/5xx) stay retryable;
    /// other 4xx are visible, not silently terminal.
    #[test]
    fn transient_statuses_are_retry_later() {
        let cfg = SweepConfig::default();
        assert!(matches!(
            classify_fetch(429, "", &cfg),
            super::FetchOutcome::RetryLater(_)
        ));
        assert!(matches!(
            classify_fetch(503, "", &cfg),
            super::FetchOutcome::RetryLater(_)
        ));
        assert!(matches!(
            classify_fetch(451, "", &cfg),
            super::FetchOutcome::NeedsOperator(_)
        ));
    }

    #[test]
    fn classification_status_classes() {
        let cfg = SweepConfig::default();
        assert!(matches!(
            classify_fetch(403, "", &cfg),
            super::FetchOutcome::NeedsOperator(_)
        ));
        assert!(matches!(
            classify_fetch(404, "", &cfg),
            super::FetchOutcome::Dead
        ));
        assert!(matches!(
            classify_fetch(200, "word ".repeat(100).as_str(), &cfg),
            super::FetchOutcome::Fetched
        ));
        assert!(matches!(
            classify_fetch(200, "", &cfg),
            super::FetchOutcome::NoText
        ));
    }

    #[test]
    fn classification_marker_config_only() {
        let cfg = SweepConfig {
            paywall_markers: vec!["SUBSCRIBE TODAY".into()],
            lending_markers: vec![],
        };
        assert!(matches!(
            classify_fetch(
                200,
                "Welcome! Subscribe Today to keep reading our work and more",
                &cfg
            ),
            super::FetchOutcome::NeedsOperator(_)
        ));
        // Marker behavior only: text without the configured marker passes.
        assert!(matches!(
            classify_fetch(200, "plain article body ".repeat(30).as_str(), &cfg),
            super::FetchOutcome::Fetched
        ));
    }
}
