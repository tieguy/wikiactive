//! Session state types: the finding model and the session directory layout.
//!
//! A session directory (`sessions/<article-slug>/`) holds:
//! - `ledger.json`      — the source ledger ([`crate::ledger::Ledger`])
//! - `findings.json`    — the findings file ([`FindingsFile`])
//! - `proposed.wikitext` — the current proposed article text
//! - `review.html`      — the regenerated-in-place review artifact
//! - `rounds.jsonl`     — append-only round log (one JSON object per round)
//!
//! Finding field ownership (plan): the model authors `id`, `rules[]`,
//! `evidence` (ledger quote ids), `factual_note`, `proposed_fix`, and a draft
//! `wikitext_anchor`; the pipeline back-fills `rendered_span_id` at render
//! time and verifies/resolves anchors at poll time. Findings enter via
//! `wa findings add --json`, schema-validated.

use serde::{Deserialize, Serialize};

/// One finding: a rule-grounded observation with evidence and a fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable finding id, e.g. `F1` (model-authored, unique in session).
    pub id: String,
    /// Draft wikitext anchor, e.g. `L12:C0-L14:C120` (model-authored; the
    /// pipeline verifies it against the proposed wikitext at render time).
    pub wikitext_anchor: String,
    /// Back-filled at render time: the artifact element id (`wa-N` / `ev-N`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_span_id: Option<String>,
    /// Rule identifiers (WP shortcuts, card ids, linter rule ids).
    pub rules: Vec<String>,
    /// Evidence: ledger quote ids (`Q<n>`).
    pub evidence: Vec<String>,
    /// What the evidence shows, in one or two sentences.
    pub factual_note: String,
    /// The specific proposed fix (one logical edit).
    pub proposed_fix: String,
    /// Which loop of the ladder owns this finding (1–5).
    #[serde(rename = "loop")]
    pub loop_id: u8,
}

impl Finding {
    /// Validate a finding for admission (`wa findings add --json`).
    ///
    /// # Errors
    /// Returns a human-readable list of everything wrong (all problems at
    /// once, so the model can fix in one pass).
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.id.is_empty() {
            problems.push("id must be non-empty".into());
        }
        if !self.id.starts_with('F') || self.id[1..].parse::<u32>().is_err() {
            problems.push("id must match F<number>".into());
        }
        if self.wikitext_anchor.is_empty() {
            problems.push("wikitext_anchor must be non-empty (draft L..C..-L..C.. is fine)".into());
        }
        if self.rules.is_empty() {
            problems.push("rules[] must name at least one rule".into());
        }
        if self.evidence.is_empty() {
            problems.push("evidence[] must cite at least one ledger quote id".into());
        }
        for qid in &self.evidence {
            if !(qid.starts_with('Q') && qid[1..].parse::<u32>().is_ok()) {
                problems.push(format!(
                    "evidence entry {qid:?} is not a ledger quote id (Q<number>)"
                ));
            }
        }
        if self.factual_note.trim().is_empty() {
            problems.push("factual_note must be non-empty".into());
        }
        if self.proposed_fix.trim().is_empty() {
            problems.push("proposed_fix must be non-empty".into());
        }
        if !(1..=5).contains(&self.loop_id) {
            problems.push("loop must be 1-5".into());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// The findings file (`findings.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FindingsFile {
    pub findings: Vec<Finding>,
}

impl FindingsFile {
    /// Parse and validate every finding.
    ///
    /// # Errors
    /// Malformed JSON, or any finding failing [`Finding::validate`] (with all
    /// problems reported).
    pub fn parse_validated(text: &str) -> Result<Self, String> {
        let file: Self =
            serde_json::from_str(text).map_err(|e| format!("findings.json malformed: {e}"))?;
        let mut seen = std::collections::HashSet::new();
        for finding in &file.findings {
            if !seen.insert(finding.id.clone()) {
                return Err(format!("duplicate finding id {}", finding.id));
            }
            finding.validate().map_err(|problems| {
                format!("finding {} invalid: {}", finding.id, problems.join("; "))
            })?;
        }
        Ok(file)
    }

    /// Load from disk (validated).
    ///
    /// # Errors
    /// Io error or validation failure.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        Self::parse_validated(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
    }

    /// Save pretty JSON.
    ///
    /// # Errors
    /// Io error.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}

/// Session paths for one article slug.
#[derive(Debug, Clone)]
pub struct SessionPaths {
    pub dir: std::path::PathBuf,
}

impl SessionPaths {
    /// Create the path set for a slug (directories created lazily on save).
    #[must_use]
    pub fn new(slug: &str) -> Self {
        Self {
            dir: std::path::Path::new("sessions").join(slug),
        }
    }

    #[must_use]
    pub fn ledger(&self) -> std::path::PathBuf {
        self.dir.join("ledger.json")
    }
    #[must_use]
    pub fn findings(&self) -> std::path::PathBuf {
        self.dir.join("findings.json")
    }
    #[must_use]
    pub fn proposed(&self) -> std::path::PathBuf {
        self.dir.join("proposed.wikitext")
    }
    #[must_use]
    pub fn base(&self) -> std::path::PathBuf {
        self.dir.join("base.wikitext")
    }
    #[must_use]
    pub fn review_html(&self) -> std::path::PathBuf {
        self.dir.join("review.html")
    }
    #[must_use]
    pub fn rounds(&self) -> std::path::PathBuf {
        self.dir.join("rounds.jsonl")
    }
    #[must_use]
    pub fn meta(&self) -> std::path::PathBuf {
        self.dir.join("session.json")
    }
}

/// Session metadata (`session.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub article: String,
    pub base_revid: u64,
    /// ISO-8601 timestamp of session init.
    pub started: String,
    /// Which loop the triage selected (1–5).
    pub entry_loop: u8,
    /// Latest published round (publish re-pins `base_revid`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_published_diff_url: Option<String>,
    /// Drift-review pin (MVP-2 A.2.2): the operator's last-edit revid when
    /// the session was initialized with `--review-since-user`; the wikitext
    /// at that revid is stored as `review-since.wikitext` and the analyze
    /// bundle embeds the drift diff against it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_since_revid: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_since_user: Option<String>,
}

/// One round-log entry (`rounds.jsonl`, append-only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundEntry {
    pub round: u32,
    pub timestamp: String,
    pub summary: String,
    /// `proposed` | `rendered` | `comments-resolved` | `published` | `aborted`.
    pub phase: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{Finding, FindingsFile};

    fn valid_finding() -> Finding {
        Finding {
            id: "F1".into(),
            wikitext_anchor: "L3:C0-L3:C120".into(),
            rendered_span_id: None,
            rules: vec!["WP:V".into()],
            evidence: vec!["Q1".into()],
            factual_note: "The count is scope-widened vs the quote.".into(),
            proposed_fix: "Restore 'in Japan by 1986' qualifier.".into(),
            loop_id: 2,
        }
    }

    #[test]
    fn valid_finding_passes() {
        assert!(valid_finding().validate().is_ok());
    }

    #[test]
    fn missing_evidence_rejected() {
        let mut f = valid_finding();
        f.evidence = vec![];
        let problems = f.validate().unwrap_err();
        assert!(problems.iter().any(|p| p.contains("evidence")));
    }

    #[test]
    fn bad_evidence_id_rejected() {
        let mut f = valid_finding();
        f.evidence = vec!["http://example.com".into()];
        let problems = f.validate().unwrap_err();
        assert!(problems.iter().any(|p| p.contains("Q<number>")));
    }

    #[test]
    fn loop_out_of_range_rejected() {
        let mut f = valid_finding();
        f.loop_id = 6;
        assert!(f.validate().is_err());
    }

    #[test]
    fn findings_file_rejects_malformed_and_duplicates() {
        assert!(FindingsFile::parse_validated("{not json").is_err());
        let one = serde_json::to_string(&valid_finding()).unwrap();
        let two = one.clone();
        let file = format!("{{\"findings\":[{one},{two}]}}");
        let err = FindingsFile::parse_validated(&file).unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn finding_json_field_is_loop() {
        let json = serde_json::to_string(&valid_finding()).unwrap();
        assert!(json.contains("\"loop\":2"), "{json}");
    }
}
