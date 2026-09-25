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
    /// ISO-8601 access date.
    pub access_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SourceMetadata>,
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
            access_date: access_date.into(),
            metadata,
        });
        id
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
