//! Session source ledger — the anti-hallucination store.
//!
//! Every content change is grounded in a quoted source from this ledger
//! ("never edit from model memory"). Sources are registered with URL,
//! archive.org URL, fetched full text, and access date; quotes are verbatim
//! spans that re-locate in the fetched bytes (verified at registration and
//! re-verified by the render/publish gate); claims tie article prose to
//! quote ids.
//!
//! Persistence: `sessions/<slug>/ledger.json` (schema-versioned JSON).
//! Network fetching lives in the fetch client layer ([`crate::wikipedia`]
//! module family); this module is pure data + verification so it is fully
//! unit-testable offline.

use crate::checks::quote_anchor::locate_quote;
use serde::{Deserialize, Serialize};

pub mod net;

pub use net::{EarwigClient, NetError, SavePageNow, SourceFetcher};

/// Ledger schema version (bump on breaking change; loader refuses newer).
pub const LEDGER_SCHEMA_VERSION: u32 = 1;

/// Bibliographic metadata for a source (citoid-shaped subset).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// RSP-style tier from `rules/sources/rsp-seed.tsv`: ok | caution |
    /// complement-only | deny.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Wayback snapshot URL found by the sweep's CDX availability check
    /// (the original URL was dead; the snapshot is the fetch target).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_url: Option<String>,
}

/// One registered source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEntry {
    /// Ledger id, `S1`, `S2`, ...
    pub id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_text: Option<String>,
    /// How the text arrived: absent (auto-fetch) or "operator" (attached
    /// from operator-provided bytes for paywalled/bot-protected sources).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_via: Option<String>,
    /// ISO-8601 access date.
    pub access_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SourceMetadata>,
    /// Sweep lifecycle (plan-003 B.3): `pending` | `fetched` |
    /// `needs_operator` | `snapshot_available` | `no_text`. Absent on
    /// pre-sweep ledgers — serde defaults keep version-1 files loadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_status: Option<String>,
    /// The once-and-for-all disposition: auto (`print: no web text`) or
    /// operator-signed (`attested-unreachable`, `dropped: paywall`, …).
    /// A disposition (or attached text) resolves the sweep gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<String>,
}

/// One verbatim quote from a ledger source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    /// Ledger id, `Q1`, `Q2`, ...
    pub id: String,
    pub source_id: String,
    /// The verbatim quote text (may contain ellipsis elisions).
    pub text: String,
    /// Byte offset verified at registration time; re-verified by the gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub located_at: Option<usize>,
}

/// One article-claim ↔ quotes binding. The L5 (Wikidata) writeback hooks:
/// `quote_ids` + the quotes' `source_id`s carry the full provenance chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// Ledger id, `C1`, `C2`, ...
    pub id: String,
    /// The article prose this claim covers (added or changed sentences).
    pub prose: String,
    /// Quote ids supporting the prose.
    pub quote_ids: Vec<String>,
}

/// The session ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub schema_version: u32,
    pub sources: Vec<SourceEntry>,
    pub quotes: Vec<Quote>,
    pub claims: Vec<Claim>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            schema_version: LEDGER_SCHEMA_VERSION,
            sources: Vec::new(),
            quotes: Vec::new(),
            claims: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("source {0} not found in ledger")]
    UnknownSource(String),
    #[error("quote text does not locate verbatim in source {source_id}: {quote:?}")]
    QuoteDoesNotLocate { source_id: String, quote: String },
    #[error("source {0} has no fetched text yet")]
    SourceNotFetched(String),
    #[error("duplicate id {0}")]
    DuplicateId(String),
    #[error("schema version {0} is newer than supported {LEDGER_SCHEMA_VERSION}")]
    NewerSchema(u32),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Ledger {
    /// Register a source, returning its id (`S<n>`).
    pub fn register_source(
        &mut self,
        url: impl Into<String>,
        access_date: impl Into<String>,
        metadata: Option<SourceMetadata>,
    ) -> String {
        let id = format!("S{}", self.sources.len() + 1);
        self.sources.push(SourceEntry {
            id: id.clone(),
            url: url.into(),
            archive_url: None,
            fetched_text: None,
            fetched_via: None,
            access_date: access_date.into(),
            metadata,
            sweep_status: None,
            disposition: None,
        });
        id
    }

    /// Register a source from the sweep inventory, deduplicating by URL:
    /// a source already in the ledger keeps its id (and any fetched
    /// text), and — if it predates the sweep — is BACK-FILLED to
    /// `pending` so pre-registered sources join the manifest instead of
    /// bypassing the gate. Returns the id and whether it was newly added.
    /// URL-less inventory entries (books/ISBNs) use an `isbn:<n>`
    /// pseudo-URL and carry the auto disposition `print: no web text` —
    /// the gate never dead-ends on non-web sources; unkeyable citations
    /// (`cite:…` pseudo-URLs) register as visible `needs_operator` rows.
    pub fn register_sweep_source(
        &mut self,
        url: &str,
        metadata: Option<SourceMetadata>,
    ) -> (String, bool) {
        let url_is_web = url.starts_with("http://") || url.starts_with("https://");
        if let Some(existing) = self.sources.iter_mut().find(|s| s.url == url) {
            if existing.sweep_status.is_none() {
                existing.sweep_status = Some(if url_is_web {
                    "pending".into()
                } else {
                    "no_text".into()
                });
            }
            return (existing.id.clone(), false);
        }
        let id = format!("S{}", self.sources.len() + 1);
        let (sweep_status, disposition) = if url_is_web {
            ("pending".to_string(), None)
        } else if url.starts_with("isbn:") {
            (
                "no_text".to_string(),
                Some("print: no web text".to_string()),
            )
        } else {
            // cite:/other pseudo-URLs: visible, the operator resolves.
            ("needs_operator".to_string(), None)
        };
        self.sources.push(SourceEntry {
            id: id.clone(),
            url: url.to_string(),
            archive_url: None,
            fetched_text: None,
            fetched_via: None,
            access_date: chrono::Utc::now().date_naive().to_string(),
            metadata,
            sweep_status: Some(sweep_status),
            disposition,
        });
        (id, true)
    }

    /// Set a source's sweep status (idempotent).
    ///
    /// # Errors
    /// Unknown source id.
    pub fn set_sweep_status(&mut self, source_id: &str, status: &str) -> Result<(), LedgerError> {
        let entry = self
            .sources
            .iter_mut()
            .find(|s| s.id == source_id)
            .ok_or_else(|| LedgerError::UnknownSource(source_id.to_string()))?;
        entry.sweep_status = Some(status.to_string());
        Ok(())
    }

    /// Record a disposition on a source (operator-signed or auto).
    ///
    /// # Errors
    /// Unknown source id.
    pub fn set_disposition(
        &mut self,
        source_id: &str,
        disposition: &str,
    ) -> Result<(), LedgerError> {
        let entry = self
            .sources
            .iter_mut()
            .find(|s| s.id == source_id)
            .ok_or_else(|| LedgerError::UnknownSource(source_id.to_string()))?;
        entry.disposition = Some(disposition.to_string());
        Ok(())
    }

    /// Record the Wayback snapshot URL a CDX availability check found.
    ///
    /// # Errors
    /// Unknown source id.
    pub fn set_snapshot_url(
        &mut self,
        source_id: &str,
        snapshot_url: &str,
    ) -> Result<(), LedgerError> {
        let entry = self
            .sources
            .iter_mut()
            .find(|s| s.id == source_id)
            .ok_or_else(|| LedgerError::UnknownSource(source_id.to_string()))?;
        let metadata = entry.metadata.get_or_insert_with(SourceMetadata::default);
        metadata.snapshot_url = Some(snapshot_url.to_string());
        Ok(())
    }

    /// Sweep sources still unresolved: swept (status recorded), no
    /// fetched text, and no disposition. These block the gate (B.3.3).
    #[must_use]
    pub fn sweep_unresolved(&self) -> Vec<(&SourceEntry, &str)> {
        self.sources
            .iter()
            .filter_map(
                |s| match (&s.sweep_status, &s.disposition, &s.fetched_text) {
                    (Some(status), None, None) => Some((s, status.as_str())),
                    _ => None,
                },
            )
            .collect()
    }

    /// Whether any sweep state exists (gates only fire for swept
    /// sessions — sweep is opt-in per session).
    #[must_use]
    pub fn has_sweep_state(&self) -> bool {
        self.sources.iter().any(|s| s.sweep_status.is_some())
    }

    /// Attach fetched full text to a source.
    ///
    /// # Errors
    /// Unknown source id.
    pub fn attach_fetched_text(
        &mut self,
        source_id: &str,
        text: impl Into<String>,
    ) -> Result<(), LedgerError> {
        let entry = self
            .sources
            .iter_mut()
            .find(|s| s.id == source_id)
            .ok_or_else(|| LedgerError::UnknownSource(source_id.to_string()))?;
        entry.fetched_text = Some(text.into());
        Ok(())
    }

    /// Attach operator-provided text (paywalled, bot-protected, or
    /// lending-gated sources the fetcher cannot reach): same verification
    /// standing as an auto-fetch, with the provenance recorded (`via`
    /// describes the capture: "operator", "operator:warc", …).
    ///
    /// # Errors
    /// [`LedgerError::UnknownSource`] when the id is not registered.
    pub fn attach_operator_text(
        &mut self,
        source_id: &str,
        text: impl Into<String>,
        via: &str,
    ) -> Result<(), LedgerError> {
        let entry = self
            .sources
            .iter_mut()
            .find(|s| s.id == source_id)
            .ok_or_else(|| LedgerError::UnknownSource(source_id.to_string()))?;
        entry.fetched_text = Some(text.into());
        entry.fetched_via = Some(via.to_string());
        Ok(())
    }

    /// Extract ledger text from an operator-saved capture. Supported
    /// formats (sniffed by path hint, then content): WARC (the ISO 28500
    /// archival standard — the largest text/html response record is used),
    /// MHTML (Chromium "Web Page, Single File" — first text/html part,
    /// quoted-printable decoded), single-file HTML, and plain text. The
    /// same `html_to_text` conversion the auto-fetcher uses keeps
    /// quote-anchoring uniform. OCR of image-only captures is a separate
    /// (future) concern.
    #[must_use]
    pub fn text_from_capture(path_hint: &str, raw: &str) -> (String, String) {
        let has_ext = |ext: &str| {
            std::path::Path::new(path_hint)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        };
        let trimmed = raw.trim_start();
        // Blink saves MHTML under .html filenames with a "From:" preamble
        // before MIME-Version — sniff by content signature too.
        let head = &raw[..raw.len().min(1024)];
        let looks_mhtml = trimmed.starts_with("MIME-Version:")
            || trimmed.starts_with("From: ")
            || head.contains("Content-Type: multipart");
        let format = if has_ext("warc") || trimmed.starts_with("WARC/1.") {
            "warc"
        } else if has_ext("mht") || has_ext("mhtml") || looks_mhtml {
            "mhtml"
        } else if has_ext("html") || has_ext("htm") || trimmed.starts_with('<') {
            "html"
        } else {
            "text"
        };
        let text = match format {
            "warc" => crate::ledger::net::html_to_text(&Self::largest_html_from_warc(raw)),
            "mhtml" => {
                crate::ledger::net::html_to_text(&Self::decode_qp(&Self::html_part_of_mhtml(raw)))
            }
            "html" => crate::ledger::net::html_to_text(raw),
            _ => raw.to_string(),
        };
        (text, format.to_string())
    }

    /// Largest `text/html` response body in a (plain, uncompressed) WARC
    /// stream. Record boundaries follow Content-Length when present.
    #[must_use]
    fn largest_html_from_warc(raw: &str) -> String {
        let mut best = String::new();
        let mut idx = 0usize;
        while let Some(pos) = raw[idx..].find("WARC/1.") {
            let start = idx + pos;
            let Some(hdr_end) = raw[start..].find("\r\n\r\n") else {
                break;
            };
            let headers = &raw[start..start + hdr_end];
            let body_start = start + hdr_end + 4;
            let is_html = headers.to_ascii_lowercase().contains("text/html");
            let len = headers.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())?
            });
            let Some(len) = len else {
                idx = body_start;
                continue;
            };
            let end = (body_start + len).min(raw.len());
            if is_html {
                let body = &raw[body_start..end];
                if body.len() > best.len() {
                    best = body.to_string();
                }
            }
            idx = end.max(body_start);
        }
        best
    }

    /// First `text/html` part of an MHTML document (headers stripped,
    /// truncated at the declared MIME boundary — a generic `\n--` cut
    /// truncates inside URLs like CDNC's `-------en--` query params, a
    /// live catch; quoted-printable is decoded by the caller).
    #[must_use]
    fn html_part_of_mhtml(raw: &str) -> String {
        let lower = raw.to_ascii_lowercase();
        let Some(ct) = lower.find("content-type: text/html") else {
            return String::new();
        };
        let rest = &raw[ct..];
        let body_start = rest.find("\r\n\r\n").map_or_else(
            || rest.find("\n\n").map_or(rest.len(), |p| p + 2),
            |p| p + 4,
        );
        let body = &rest[body_start..];
        let boundary = Self::mhtml_boundary(raw);
        let end = boundary
            .as_deref()
            .and_then(|b| {
                body.find(&format!("\n--{b}"))
                    .or_else(|| body.find(&format!("\r\n--{b}")))
            })
            .unwrap_or(body.len());
        body[..end].to_string()
    }

    /// The `boundary="..."` parameter of a Content-Type header, if present.
    #[must_use]
    fn mhtml_boundary(raw: &str) -> Option<String> {
        let lower = raw.to_ascii_lowercase();
        let pos = lower.find("boundary=")?;
        let rest = raw[pos + "boundary=".len()..].trim_start();
        let quoted = rest.starts_with('"');
        let s = if quoted { &rest[1..] } else { rest };
        let end = if quoted {
            s.find('"')?
        } else {
            s.find([' ', ';', '\r', '\n'])?
        };
        Some(s[..end].to_string())
    }

    /// Minimal quoted-printable decode (RFC 2045 §6.2 subset: =XX hex,
    /// soft line breaks; other bytes pass through, lossy on non-UTF-8).
    #[must_use]
    fn decode_qp(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'=' if bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                    && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit) =>
                {
                    let hex = |b: u8| {
                        let d = (b as char).to_digit(16).unwrap_or(0);
                        u8::try_from(d).unwrap_or(0)
                    };
                    out.push(hex(bytes[i + 1]) * 16 + hex(bytes[i + 2]));
                    i += 3;
                }
                b'=' if bytes.get(i + 1) == Some(&b'\r') || bytes.get(i + 1) == Some(&b'\n') => {
                    // Soft break.
                    i += if bytes.get(i + 1) == Some(&b'\r') {
                        2
                    } else {
                        1
                    };
                }
                b => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).to_string()
    }

    /// Record the archive.org URL for a source (after save-page-now).
    ///
    /// # Errors
    /// Unknown source id.
    pub fn attach_archive_url(
        &mut self,
        source_id: &str,
        archive_url: impl Into<String>,
    ) -> Result<(), LedgerError> {
        let entry = self
            .sources
            .iter_mut()
            .find(|s| s.id == source_id)
            .ok_or_else(|| LedgerError::UnknownSource(source_id.to_string()))?;
        entry.archive_url = Some(archive_url.into());
        Ok(())
    }

    /// Add a quote, verifying it locates verbatim in the fetched source text
    /// at registration time. Anti-fabrication: quotes that do not locate are
    /// rejected here — the gate re-verifies everything anyway.
    ///
    /// # Errors
    /// Unknown/unfetched source, or the quote does not locate (fabricated or
    /// mis-transcribed).
    pub fn add_quote(
        &mut self,
        source_id: &str,
        text: impl Into<String>,
    ) -> Result<String, LedgerError> {
        let text = text.into();
        let source = self
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .ok_or_else(|| LedgerError::UnknownSource(source_id.to_string()))?;
        let fetched = source
            .fetched_text
            .as_deref()
            .ok_or_else(|| LedgerError::SourceNotFetched(source_id.to_string()))?;
        let located =
            locate_quote(&text, fetched).ok_or_else(|| LedgerError::QuoteDoesNotLocate {
                source_id: source_id.to_string(),
                quote: text.clone(),
            })?;
        let id = format!("Q{}", self.quotes.len() + 1);
        self.quotes.push(Quote {
            id: id.clone(),
            source_id: source_id.to_string(),
            text,
            located_at: Some(located),
        });
        Ok(id)
    }

    /// Add a claim binding prose to quote ids.
    ///
    /// # Errors
    /// Unknown quote id.
    pub fn add_claim(
        &mut self,
        prose: impl Into<String>,
        quote_ids: Vec<String>,
    ) -> Result<String, LedgerError> {
        for qid in &quote_ids {
            if !self.quotes.iter().any(|q| &q.id == qid) {
                return Err(LedgerError::DuplicateId(format!("unknown quote {qid}")));
            }
        }
        let id = format!("C{}", self.claims.len() + 1);
        self.claims.push(Claim {
            id: id.clone(),
            prose: prose.into(),
            quote_ids,
        });
        Ok(id)
    }

    /// Fetched text of a source, if present.
    #[must_use]
    pub fn source_text(&self, source_id: &str) -> Option<&str> {
        self.sources
            .iter()
            .find(|s| s.id == source_id)
            .and_then(|s| s.fetched_text.as_deref())
    }

    /// A quote by id.
    #[must_use]
    pub fn quote(&self, quote_id: &str) -> Option<&Quote> {
        self.quotes.iter().find(|q| q.id == quote_id)
    }

    /// A claim by id.
    #[must_use]
    pub fn claim(&self, claim_id: &str) -> Option<&Claim> {
        self.claims.iter().find(|c| c.id == claim_id)
    }

    /// Serialize to pretty JSON.
    ///
    /// # Errors
    /// Serialization failure.
    pub fn to_json(&self) -> Result<String, LedgerError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parse from JSON, refusing newer schema versions.
    ///
    /// # Errors
    /// Malformed JSON or unsupported schema version.
    pub fn from_json(text: &str) -> Result<Self, LedgerError> {
        let ledger: Self = serde_json::from_str(text)?;
        if ledger.schema_version > LEDGER_SCHEMA_VERSION {
            return Err(LedgerError::NewerSchema(ledger.schema_version));
        }
        Ok(ledger)
    }

    /// Load from `sessions/<slug>/ledger.json`.
    ///
    /// # Errors
    /// Missing file, io error, malformed JSON, unsupported schema.
    pub fn load(path: &std::path::Path) -> Result<Self, LedgerError> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    /// Save to `sessions/<slug>/ledger.json` (atomic via temp+rename).
    ///
    /// # Errors
    /// Io error.
    pub fn save(&self, path: &std::path::Path) -> Result<(), LedgerError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json()?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Ledger, LedgerError};

    const SOURCE_TEXT: &str = "Temple Fielding's travel guides sold millions of copies and \
                               fictionalized his own itineraries for comic effect.";

    fn ledger_with_source() -> (Ledger, String) {
        let mut ledger = Ledger::default();
        let id = ledger.register_source("https://example.com/fielding", "2026-09-24", None);
        ledger.attach_fetched_text(&id, SOURCE_TEXT).unwrap();
        (ledger, id)
    }

    #[test]
    fn register_fetch_quote_claim_roundtrip() {
        let (mut ledger, sid) = ledger_with_source();
        let qid = ledger
            .add_quote(&sid, "sold millions of copies")
            .expect("verbatim quote locates");
        assert!(qid.starts_with('Q'));
        let cid = ledger
            .add_claim("His guides sold millions of copies.", vec![qid.clone()])
            .expect("claim added");
        assert_eq!(ledger.claim(&cid).unwrap().quote_ids, vec![qid]);
    }

    #[test]
    fn operator_attached_text_verifies_quotes_and_records_provenance() {
        let mut ledger = Ledger::default();
        let sid = ledger.register_source("https://paywall.example/article", "2026-09-28", None);
        ledger
            .attach_operator_text(&sid, SOURCE_TEXT, "operator")
            .expect("attach");
        let src = ledger.source_text(&sid).expect("text present");
        assert!(src.contains("millions of copies"));
        let entry = ledger.sources.iter().find(|s| s.id == sid).unwrap();
        assert_eq!(entry.fetched_via.as_deref(), Some("operator"));
        // Quotes verify against operator text exactly like auto-fetches.
        let qid = ledger
            .add_quote(&sid, "sold millions of copies")
            .expect("verbatim quote locates in operator text");
        assert!(qid.starts_with('Q'));
    }

    /// Capture-format extraction: WARC (largest text/html record), MHTML
    /// (first text/html part, QP-decoded), and single-file HTML all land
    /// as `html_to_text` output; plain text passes through.
    #[test]
    fn text_from_capture_extracts_each_format() {
        let warc = "WARC/1.0\r\nWARC-Type: response\r\nContent-Type: text/html\r\nContent-Length: 44\r\n\r\n<p>John Kidder married Sarah Clark in 1870.</p>\r\n\r\n";
        let (text, fmt) = Ledger::text_from_capture("capture.warc", warc);
        assert_eq!(fmt, "warc");
        assert!(text.contains("married Sarah Clark in 1870"), "{text}");

        let mhtml = "MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"BB\"\r\n\r\n--BB\r\nContent-Type: text/html\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n<p>died April 11, 1901=2C aged 71.</p>=\r\n--BB--\r\n";
        let (text, fmt) = Ledger::text_from_capture("page.mhtml", mhtml);
        assert_eq!(fmt, "mhtml");
        assert!(text.contains("died April 11, 1901, aged 71."), "{text}");

        let (_, fmt) = Ledger::text_from_capture("p.html", "<p>html page</p>");
        assert_eq!(fmt, "html");
        let (text, fmt) = Ledger::text_from_capture("note.txt", "plain words");
        assert_eq!((text.as_str(), fmt.as_str()), ("plain words", "text"));
        // Blink saves MHTML under .html filenames with a From: preamble —
        // content sniffing must route it to the MHTML extractor.
        let blink = "From: <Saved by Blink>\nSnapshot-Content-Location: https://example.com/x\nMIME-Version: 1.0\nContent-Type: multipart/related;\n\tboundary=\"--B\"\n\n--B\nContent-Type: text/html\r\n\r\n<p>died 1901=2C Grass Valley.</p>=\r\n--B--\n";
        let (text, fmt) = Ledger::text_from_capture("Saved Page.html", blink);
        assert_eq!(fmt, "mhtml");
        assert!(text.contains("died 1901, Grass Valley."), "{text}");
    }

    #[test]
    fn fabricated_quote_rejected_at_registration() {
        let (mut ledger, sid) = ledger_with_source();
        let err = ledger
            .add_quote(&sid, "won the Nobel Prize for travel writing")
            .unwrap_err();
        assert!(matches!(err, LedgerError::QuoteDoesNotLocate { .. }));
    }

    #[test]
    fn quote_before_fetch_rejected() {
        let mut ledger = Ledger::default();
        let sid = ledger.register_source("https://example.com", "2026-09-24", None);
        let err = ledger.add_quote(&sid, "anything at all").unwrap_err();
        assert!(matches!(err, LedgerError::SourceNotFetched(_)));
    }

    #[test]
    fn json_roundtrip_preserves_state() {
        let (mut ledger, sid) = ledger_with_source();
        ledger.add_quote(&sid, "sold millions of copies").unwrap();
        let json = ledger.to_json().unwrap();
        let back = Ledger::from_json(&json).unwrap();
        assert_eq!(back, ledger);
    }

    #[test]
    fn newer_schema_refused() {
        let json = r#"{"schema_version":99,"sources":[],"quotes":[],"claims":[]}"#;
        assert!(matches!(
            Ledger::from_json(json),
            Err(LedgerError::NewerSchema(99))
        ));
    }

    #[test]
    fn save_and_load_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.json");
        let (mut ledger, sid) = ledger_with_source();
        ledger.add_quote(&sid, "sold millions of copies").unwrap();
        ledger.save(&path).unwrap();
        let back = Ledger::load(&path).unwrap();
        assert_eq!(back.quotes.len(), 1);
    }
}
