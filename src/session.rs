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

/// Operator overrides for the assess entry checks (loop-mechanization
/// Phase 3): the two refusals with an explicit way past. Unknown quote
/// ids have no bypass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntryChecks {
    /// Proceed past a stale `context.md` (`--allow-stale-analyze`).
    pub allow_stale_analyze: bool,
    /// Proceed past unresolved fetch sources
    /// (`--allow-unresolved-fetch`).
    pub allow_unresolved_fetch: bool,
}

/// Why an assess batch was refused at entry (loop-mechanization Phase 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryRefusal {
    /// Evidence cites quote ids the ledger does not have. No bypass:
    /// register the quotes or fix the batch.
    UnknownQuote { ids: Vec<String> },
    /// `context.md` (the analyze bundle) predates this iteration's state
    /// — older than `proposed.wikitext`'s last modification or the
    /// newest round-advancing event. Bypass: `--allow-stale-analyze`.
    StaleAnalyze { older_than: String },
    /// A fetched source is still unresolved (no text, no disposition).
    /// Bypass: `--allow-unresolved-fetch`.
    UnresolvedFetch { sources: Vec<(String, String)> },
}

impl std::fmt::Display for EntryRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownQuote { ids } => write!(
                f,
                "evidence cites quote id(s) not in the ledger: {} — register the quotes \
                 (wa ledger quote) or fix the batch",
                ids.join(", ")
            ),
            Self::StaleAnalyze { older_than } => write!(
                f,
                "context.md (the analyze bundle) is stale — {older_than}; run \
                 `wa analyze <slug>` and re-submit (or pass --allow-stale-analyze \
                 to proceed anyway)"
            ),
            Self::UnresolvedFetch { sources } => write!(
                f,
                "unresolved fetch source(s): {} — fetch (wa fetch <slug>), attach an \
                 operator capture (wa ledger attach), or record a disposition \
                 (wa fetch dispose) (or pass --allow-unresolved-fetch to proceed anyway)",
                sources
                    .iter()
                    .map(|(id, status)| format!("{id} ({status})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// The assess entry checks both admission paths run (CLI `wa assess add`
/// and the serve Assess save): evidence must exist in the ledger, the
/// analyze bundle must be fresh for this iteration, and every fetched
/// source must be resolved. Any refusal ⇒ the batch saves nothing.
///
/// Freshness mirrors the `artifact_state` staleness pattern:
/// `context.md` must be newer than `proposed.wikitext`'s mtime and newer
/// than the newest `rendered`/`published`/`comments-resolved` round
/// entry (no advancing entries yet ⇒ only the proposed comparison).
///
/// # Errors
/// Every refusal at once (so the operator can fix in one pass).
pub fn assess_entry_checks(
    dir: &std::path::Path,
    ledger: &crate::ledger::Ledger,
    evidence: &[String],
    opts: EntryChecks,
) -> Result<(), Vec<EntryRefusal>> {
    let mut refusals = Vec::new();

    let unknown: Vec<String> = evidence
        .iter()
        .filter(|qid| ledger.quote(qid).is_none())
        .cloned()
        .collect();
    if !unknown.is_empty() {
        refusals.push(EntryRefusal::UnknownQuote { ids: unknown });
    }

    if !opts.allow_stale_analyze
        && let Some(older_than) = analyze_staleness(dir)
    {
        refusals.push(EntryRefusal::StaleAnalyze { older_than });
    }

    if !opts.allow_unresolved_fetch && ledger.has_sweep_state() {
        let unresolved: Vec<(String, String)> = ledger
            .sweep_unresolved()
            .into_iter()
            .map(|(s, status)| (s.id.clone(), status.to_string()))
            .collect();
        if !unresolved.is_empty() {
            refusals.push(EntryRefusal::UnresolvedFetch {
                sources: unresolved,
            });
        }
    }

    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

/// The stale relation, if any: what `context.md` is older than.
fn analyze_staleness(dir: &std::path::Path) -> Option<String> {
    let ctx_mtime = std::fs::metadata(dir.join("context.md"))
        .ok()
        .and_then(|m| m.modified().ok())
        .map_or_else(
            || Some("missing".to_string()),
            |t| {
                let ctx = t;
                if let Ok(prop) =
                    std::fs::metadata(dir.join("proposed.wikitext")).and_then(|m| m.modified())
                    && ctx <= prop
                {
                    return Some("older than proposed.wikitext's last modification".into());
                }
                // The newest round-advancing event (rendered, published,
                // comments-resolved): the analysis must postdate it.
                let newest = std::fs::read_to_string(dir.join("rounds.jsonl"))
                    .ok()
                    .map(|text| {
                        text.lines()
                            .filter_map(|l| serde_json::from_str::<RoundEntry>(l).ok())
                            .filter(|e| {
                                matches!(
                                    e.phase.as_str(),
                                    "rendered" | "published" | "comments-resolved"
                                )
                            })
                            .filter_map(|e| {
                                chrono::DateTime::parse_from_rfc3339(&e.timestamp)
                                    .ok()
                                    .map(|ts| (ts, e))
                            })
                            .max_by_key(|(ts, _)| *ts)
                    })
                    .flatten();
                if let Some((ts, e)) = newest
                    && chrono::DateTime::<chrono::Utc>::from(ctx) <= ts
                {
                    return Some(format!(
                        "older than the newest round-advancing event ({} round {})",
                        e.phase, e.round
                    ));
                }
                None
            },
        );
    ctx_mtime
}

#[cfg(test)]
mod tests {
    use super::{Assessment, AssessmentsFile, EntryChecks, EntryRefusal, assess_entry_checks};

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("wa-entry-checks-{tag}-{id}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ledger_with_quote() -> crate::ledger::Ledger {
        let mut ledger = crate::ledger::Ledger::default();
        let sid = ledger.register_source("https://example.com/s", "2026-09-30", None);
        ledger
            .attach_fetched_text(&sid, "the quoted words live here")
            .unwrap();
        ledger.add_quote(&sid, "quoted words").unwrap();
        ledger
    }

    /// Write files with strictly increasing mtimes (ns resolution).
    fn write_seq(dir: &std::path::Path, files: &[&str]) {
        for f in files {
            std::fs::write(dir.join(f), "content\n").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }

    fn round_entry(phase: &str, timestamp: &str) -> String {
        format!(
            r#"{{"round":1,"timestamp":"{timestamp}","summary":"s","phase":"{phase}","detail":[]}}"#
        )
    }

    /// loopmech.AC1.1 — evidence ids absent from the ledger are named.
    #[test]
    fn unknown_quote_ids_are_named() {
        let dir = tmp_dir("q");
        write_seq(&dir, &["proposed.wikitext", "context.md"]);
        let ledger = ledger_with_quote();
        let err = assess_entry_checks(
            &dir,
            &ledger,
            &["Q1".into(), "Q9".into()],
            EntryChecks::default(),
        )
        .unwrap_err();
        assert!(
            matches!(&err[0], EntryRefusal::UnknownQuote { ids } if ids == &vec!["Q9".to_string()]),
            "{err:?}"
        );
        assert!(err[0].to_string().contains("Q9"));
    }

    /// loopmech.AC1.3 — all-known evidence with a fresh bundle passes.
    #[test]
    fn known_evidence_fresh_bundle_passes() {
        let dir = tmp_dir("ok");
        write_seq(&dir, &["proposed.wikitext", "context.md"]);
        let ledger = ledger_with_quote();
        assert!(assess_entry_checks(&dir, &ledger, &["Q1".into()], EntryChecks::default()).is_ok());
    }

    /// loopmech.AC2.4 — first iteration: freshness compares only against
    /// proposed.wikitext (no round-advancing entries yet).
    #[test]
    fn first_iteration_compares_only_against_proposed() {
        let dir = tmp_dir("first");
        // No rounds.jsonl at all: newer context passes…
        write_seq(&dir, &["proposed.wikitext", "context.md"]);
        let ledger = ledger_with_quote();
        assert!(assess_entry_checks(&dir, &ledger, &["Q1".into()], EntryChecks::default()).is_ok());
        // …an older context is stale by the proposed relation.
        let dir2 = tmp_dir("first2");
        write_seq(&dir2, &["context.md", "proposed.wikitext"]);
        let err = assess_entry_checks(&dir2, &ledger, &["Q1".into()], EntryChecks::default())
            .unwrap_err();
        assert!(
            matches!(&err[0], EntryRefusal::StaleAnalyze { older_than } if older_than
                .contains("proposed.wikitext")),
            "{err:?}"
        );
    }

    /// loopmech.AC2.1 — older than the newest round-advancing event is
    /// stale, and the message names the relation + prescribes analyze.
    #[test]
    fn older_than_advancing_event_is_stale() {
        let dir = tmp_dir("adv");
        write_seq(&dir, &["proposed.wikitext", "context.md"]);
        // A rendered entry timestamped NOW: the context written just
        // before is older (entry timestamps truncate to the second, so
        // step clearly past the write).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(
            dir.join("rounds.jsonl"),
            round_entry("rendered", &crate::comments::now_iso()),
        )
        .unwrap();
        let ledger = ledger_with_quote();
        let err =
            assess_entry_checks(&dir, &ledger, &["Q1".into()], EntryChecks::default()).unwrap_err();
        let msg = err[0].to_string();
        assert!(
            msg.contains("round-advancing event (rendered round 1)"),
            "{msg}"
        );
        assert!(msg.contains("wa analyze"), "{msg}");
    }

    /// loopmech.AC2.2 — re-running analyze (a newer context.md) makes the
    /// identical evidence fresh again.
    #[test]
    fn re_analyze_makes_it_fresh() {
        let dir = tmp_dir("re");
        write_seq(&dir, &["proposed.wikitext", "context.md"]);
        // The advancing entry postdates the context (timestamps truncate
        // to seconds, so step clearly past the write).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(
            dir.join("rounds.jsonl"),
            round_entry("comments-resolved", &crate::comments::now_iso()),
        )
        .unwrap();
        let ledger = ledger_with_quote();
        assert!(
            assess_entry_checks(&dir, &ledger, &["Q1".into()], EntryChecks::default()).is_err()
        );
        // wa analyze rewrites context.md — newer than everything.
        std::thread::sleep(std::time::Duration::from_millis(5));
        write_seq(&dir, &["context.md"]);
        assert!(assess_entry_checks(&dir, &ledger, &["Q1".into()], EntryChecks::default()).is_ok());
    }

    /// loopmech.AC2.5 / AC3.2 — the bypass flags proceed past their
    /// guards.
    #[test]
    fn bypass_flags_proceed() {
        let dir = tmp_dir("bypass");
        write_seq(&dir, &["context.md", "proposed.wikitext"]);
        let ledger = ledger_with_quote();
        let opts = EntryChecks {
            allow_stale_analyze: true,
            allow_unresolved_fetch: true,
        };
        assert!(assess_entry_checks(&dir, &ledger, &["Q1".into()], opts).is_ok());
    }

    /// loopmech.AC3.1 / AC3.5 — unresolved fetch sources are listed with
    /// their statuses; sessions without sweep state are unaffected.
    #[test]
    fn unresolved_fetch_listed_and_no_sweep_state_unaffected() {
        let dir = tmp_dir("fetch");
        write_seq(&dir, &["proposed.wikitext", "context.md"]);

        // No sweep state: sources exist, none carry sweep_status — clean.
        let mut ledger = crate::ledger::Ledger::default();
        let sid = ledger.register_source("https://example.com/s", "2026-09-30", None);
        ledger.attach_fetched_text(&sid, "text").unwrap();
        ledger.add_quote(&sid, "text").unwrap();
        assert!(assess_entry_checks(&dir, &ledger, &["Q1".into()], EntryChecks::default()).is_ok());

        // Sweep state with an unresolved source: named with its status
        // (no evidence offered, so only the fetch guard fires).
        let mut swept = crate::ledger::Ledger::default();
        swept.register_sweep_source("https://example.com/paywalled", None);
        let err = assess_entry_checks(&dir, &swept, &[], EntryChecks::default()).unwrap_err();
        let msg = err[0].to_string();
        assert!(msg.contains("S1 (pending)"), "{msg}");
        assert!(msg.contains("--allow-unresolved-fetch"), "{msg}");
    }

    /// A missing context.md is stale with the missing relation named.
    #[test]
    fn missing_context_is_stale() {
        let dir = tmp_dir("missing");
        write_seq(&dir, &["proposed.wikitext"]);
        let ledger = ledger_with_quote();
        let err =
            assess_entry_checks(&dir, &ledger, &["Q1".into()], EntryChecks::default()).unwrap_err();
        assert!(err[0].to_string().contains("missing"), "{err:?}");
    }

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
