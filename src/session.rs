//! Session state types: the assessment model and the session directory layout.
//!
//! A session directory (`sessions/<article-slug>/`) holds:
//! - `ledger.json`      — the source ledger ([`crate::ledger::Ledger`])
//! - `assessments.json` — the assessments file ([`AssessmentsFile`])
//! - `proposed.wikitext` — the current proposed article text
//! - `review.html`      — the regenerated-in-place review artifact
//! - `rounds.jsonl`     — append-only round log (one JSON object per round)
//! - `comments.jsonl`   — the review comment queue (plan-004;
//!   [`crate::comments::CommentQueue`])
//!
//! Assessment field ownership (plan): the model authors `id`, `rules[]`,
//! `evidence` (ledger quote ids), `factual_note`, `proposed_fix`, and a draft
//! `wikitext_anchor`; the pipeline back-fills `rendered_span_id` at render
//! time and verifies/resolves anchors at poll time. Assessments enter via
//! `wa assess add`, schema-validated.

use serde::{Deserialize, Serialize};

/// One assessment: a rule-grounded observation with evidence and a fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assessment {
    /// Stable assessment id, e.g. `AS1` (model-authored, unique in session).
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
    /// Which loop of the session owns this assessment (1–5).
    #[serde(rename = "loop")]
    pub loop_id: u8,
}

impl Assessment {
    /// Validate an assessment for admission (`wa assess add`).
    ///
    /// # Errors
    /// Returns a human-readable list of everything wrong (all problems at
    /// once, so the model can fix in one pass).
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.id.is_empty() {
            problems.push("id must be non-empty".into());
        }
        if !is_numbered_id(&self.id, "AS") {
            problems.push("id must match AS<number>".into());
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
            if !is_numbered_id(qid, "Q") {
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

/// The assessments file (`assessments.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssessmentsFile {
    pub assessments: Vec<Assessment>,
}

impl AssessmentsFile {
    /// Parse and validate every assessment.
    ///
    /// # Errors
    /// Malformed JSON, or any assessment failing [`Assessment::validate`]
    /// (with all problems reported).
    pub fn parse_validated(text: &str) -> Result<Self, String> {
        let file: Self =
            serde_json::from_str(text).map_err(|e| format!("assessments.json malformed: {e}"))?;
        let mut seen = std::collections::HashSet::new();
        for assessment in &file.assessments {
            if !seen.insert(assessment.id.clone()) {
                return Err(format!("duplicate assessment id {}", assessment.id));
            }
            assessment.validate().map_err(|problems| {
                format!(
                    "assessment {} invalid: {}",
                    assessment.id,
                    problems.join("; ")
                )
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
    pub fn assessments(&self) -> std::path::PathBuf {
        self.dir.join("assessments.json")
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
    /// The review comment queue (plan-004): append-only JSONL, the single
    /// reviewer↔loop interface.
    #[must_use]
    pub fn comments(&self) -> std::path::PathBuf {
        self.dir.join("comments.jsonl")
    }
    /// The rule-review result (rule-enforcement item 5): the model's
    /// clause-by-clause advice for the current round, keyed by round.
    #[must_use]
    pub fn rule_review(&self) -> std::path::PathBuf {
        self.dir.join("rule-review.json")
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

/// `<prefix><digits>` exactly (`AS3`, `Q12`): digits only after the prefix,
/// so `Q+1` — which integer parsing would accept — is not an id.
fn is_numbered_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::{Assessment, AssessmentsFile};

    fn valid_finding() -> Assessment {
        Assessment {
            id: "AS1".into(),
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
    fn assessment_id_must_be_as_prefixed() {
        // loopmech.AC6.1: ids are AS<n> (the F<n> shape is the retired
        // findings vocabulary).
        let mut a = valid_finding();
        a.id = "AS1".into();
        assert!(a.validate().is_ok(), "AS1 is the id shape");
        a.id = "F1".into();
        let problems = a.validate().unwrap_err();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("id must match AS<number>")),
            "{problems:?}"
        );
        a.id = "AS+1".into();
        assert!(a.validate().is_err(), "digits only after the prefix");
        a.id = "AS".into();
        assert!(a.validate().is_err(), "bare prefix is not an id");
    }

    #[test]
    fn findings_file_rejects_malformed_and_duplicates() {
        assert!(AssessmentsFile::parse_validated("{not json").is_err());
        let one = serde_json::to_string(&valid_finding()).unwrap();
        let two = one.clone();
        let file = format!("{{\"assessments\":[{one},{two}]}}");
        let err = AssessmentsFile::parse_validated(&file).unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn assessment_json_field_is_loop() {
        let json = serde_json::to_string(&valid_finding()).unwrap();
        assert!(json.contains("\"loop\":2"), "{json}");
    }

    #[test]
    fn file_shape_is_assessments_array() {
        // loopmech.AC6.1: the on-disk shape is {"assessments":[…]}.
        let one = serde_json::to_string(&valid_finding()).unwrap();
        let file = AssessmentsFile::parse_validated(&format!("{{\"assessments\":[{one}]}}"))
            .expect("new shape parses");
        assert_eq!(file.assessments.len(), 1);
        assert!(
            AssessmentsFile::parse_validated(&format!("{{\"findings\":[{one}]}}")).is_err(),
            "old findings.json shape is a documented break, not a fallback"
        );
    }
}
