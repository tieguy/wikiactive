//! The enforcement gate — render's mandatory pre-flight (AC.11).
//!
//! Nothing whose evidence fails verification ever becomes an artifact:
//! - every finding's evidence quote ids must exist in the ledger AND the
//!   quote text must re-locate verbatim in the fetched source bytes;
//! - every finding must carry evidence (findings without anchors never reach
//!   the review diff);
//! - every claim's prose must pass the paraphrase gate against its claimed
//!   source (too-close blocks; no-support requires anchor resolution);
//! - the wikitext linter gate must report no error-severity findings.
//!
//! The same gate re-runs immediately before publish's confirmation.
//! A blocked verdict writes NO artifact; the reasons enumerate everything to
//! fix (all at once, so one revision pass can clear the gate).

use crate::checks::linter;
use crate::checks::linter::LinterConfig;
use crate::checks::paraphrase;
use crate::checks::quote_anchor::locate_quote;
use crate::ledger::Ledger;
use crate::session::Assessment;

/// What clearing a blocked reason requires (MVP-2 A.2.1): ledger/evidence
/// wiring vs revising the drafted text itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Evidence not yet verifiable — register/fetch sources, add quotes.
    NeedsAnchor,
    /// The drafted prose, the quote, or the lint must be fixed before
    /// render.
    HardBlock,
}

/// Why the gate blocked (every variant is blocking; warnings are not gate
/// output). `span` locates the offending wikitext when known: a finding's
/// anchor, the located claim prose in the proposed wikitext, or the linter
/// line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateReason {
    AssessmentWithoutEvidence {
        assessment_id: String,
        span: Option<String>,
    },
    UnknownQuoteId {
        assessment_id: String,
        quote_id: String,
        span: Option<String>,
    },
    QuoteDoesNotLocate {
        assessment_id: String,
        quote_id: String,
        source_id: String,
        span: Option<String>,
    },
    SourceNotFetched {
        assessment_id: String,
        source_id: String,
        span: Option<String>,
    },
    /// A claim cites no quotes at all: nothing anchors its prose.
    ClaimWithoutQuotes {
        claim_id: String,
        span: Option<String>,
    },
    /// A claim's quote, or that quote's fetched source text, is missing
    /// from the ledger.
    ClaimQuoteUnresolved {
        claim_id: String,
        quote_id: String,
        span: Option<String>,
    },
    /// Claim sequencing (loop-mechanization Phase 4): the claim's prose
    /// is staged nowhere — neither in the base wikitext (pre-existing
    /// article text) nor in the proposal (the edit stages it). A claim
    /// must ride the edit or describe text that already exists.
    ClaimNotStaged {
        claim_id: String,
        prose: String,
        span: Option<String>,
    },
    ClaimParaphraseTooClose {
        claim_id: String,
        detail: String,
        span: Option<String>,
    },
    ClaimParaphraseNoSupport {
        claim_id: String,
        detail: String,
        span: Option<String>,
    },
    LinterError {
        rule: String,
        detail: String,
        span: Option<String>,
    },
    /// Source-sweep completeness (plan-003 B.3): a swept source has
    /// neither fetched text nor a disposition. Opt-in per session —
    /// ledgers without sweep state never produce this reason.
    SweepSourceUnresolved {
        source_id: String,
        status: String,
        url: String,
    },
}

impl GateReason {
    /// Disposition per the A.2.1 contract: missing-evidence reasons are
    /// anchor work; locate/paraphrase/linter failures need revision.
    #[must_use]
    pub fn disposition(&self) -> Disposition {
        match self {
            Self::AssessmentWithoutEvidence { .. }
            | Self::UnknownQuoteId { .. }
            | Self::SourceNotFetched { .. }
            | Self::ClaimWithoutQuotes { .. }
            | Self::ClaimQuoteUnresolved { .. }
            | Self::ClaimNotStaged { .. }
            | Self::SweepSourceUnresolved { .. } => Disposition::NeedsAnchor,
            Self::QuoteDoesNotLocate { .. }
            | Self::ClaimParaphraseTooClose { .. }
            | Self::ClaimParaphraseNoSupport { .. }
            | Self::LinterError { .. } => Disposition::HardBlock,
        }
    }

    /// Wikitext span of the offending text, when known.
    #[must_use]
    pub fn span(&self) -> Option<&str> {
        match self {
            Self::AssessmentWithoutEvidence { span, .. }
            | Self::UnknownQuoteId { span, .. }
            | Self::QuoteDoesNotLocate { span, .. }
            | Self::SourceNotFetched { span, .. }
            | Self::ClaimWithoutQuotes { span, .. }
            | Self::ClaimQuoteUnresolved { span, .. }
            | Self::ClaimNotStaged { span, .. }
            | Self::ClaimParaphraseTooClose { span, .. }
            | Self::ClaimParaphraseNoSupport { span, .. }
            | Self::LinterError { span, .. } => span.as_deref(),
            Self::SweepSourceUnresolved { .. } => None,
        }
    }
}

/// Rule-enforcement item 4: the warn-severity lint findings for a
/// proposal. Established by reading `run_gate`: only `Severity::Error`
/// findings become gate reasons, and `wa check` printed only gate
/// verdicts — so warn findings (tense-drift, see-also-duplication) were
/// invisible everywhere except standalone `wa lint`. These are advice
/// the reviewer must still SEE; they never block.
#[must_use]
pub fn lint_warnings(
    base: &str,
    proposed: &str,
    cfg: &crate::checks::linter::LinterConfig,
) -> Vec<crate::checks::linter::LintFinding> {
    crate::checks::linter::gate(base, proposed, cfg)
        .into_iter()
        .filter(|f| f.severity == crate::checks::linter::Severity::Warn)
        .collect()
}

/// Structured gate report shared by `wa check` (standalone fail-fast) and
/// render/publish's blocked output: reasons grouped by disposition, each
/// with its wikitext span when known.
#[must_use]
pub fn format_reasons(reasons: &[GateReason]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("gate blocked — {} reason(s)\n", reasons.len());
    let needs: Vec<&GateReason> = reasons
        .iter()
        .filter(|r| r.disposition() == Disposition::NeedsAnchor)
        .collect();
    let hard: Vec<&GateReason> = reasons
        .iter()
        .filter(|r| r.disposition() == Disposition::HardBlock)
        .collect();
    if !needs.is_empty() {
        let _ = writeln!(
            out,
            "\nNEEDS ANCHOR ({}): ledger wiring — register/fetch sources, add quotes",
            needs.len()
        );
        for r in needs {
            let _ = writeln!(out, "  - {r}{}", span_note(r));
        }
    }
    if !hard.is_empty() {
        let _ = writeln!(
            out,
            "\nHARD BLOCK ({}): revise the draft, quote, or lint before render",
            hard.len()
        );
        for r in hard {
            let _ = writeln!(out, "  - {r}{}", span_note(r));
        }
    }
    out
}

fn span_note(r: &GateReason) -> String {
    match r.span() {
        Some(s) => format!(" [at {s}]"),
        None => String::new(),
    }
}

impl std::fmt::Display for GateReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssessmentWithoutEvidence { assessment_id, .. } => {
                write!(
                    f,
                    "finding {assessment_id}: no evidence quotes (findings never reach the diff unanchored)"
                )
            }
            Self::ClaimWithoutQuotes { claim_id, .. } => {
                write!(
                    f,
                    "claim {claim_id}: cites no quotes — nothing anchors its prose"
                )
            }
            Self::ClaimQuoteUnresolved {
                claim_id, quote_id, ..
            } => {
                write!(
                    f,
                    "claim {claim_id}: quote {quote_id} is not in the ledger, or its source has no fetched text"
                )
            }
            Self::ClaimNotStaged {
                claim_id, prose, ..
            } => {
                write!(
                    f,
                    "claim {claim_id}: prose staged nowhere — neither in base nor proposed \
                     wikitext: \"{prose}\" (claims ride the edit, or cite text the article \
                     already carries)"
                )
            }
            Self::UnknownQuoteId {
                assessment_id,
                quote_id,
                ..
            } => {
                write!(
                    f,
                    "finding {assessment_id}: evidence quote {quote_id} not in ledger"
                )
            }
            Self::QuoteDoesNotLocate {
                assessment_id,
                quote_id,
                source_id,
                ..
            } => {
                write!(
                    f,
                    "finding {assessment_id}: quote {quote_id} does not re-locate in fetched text of source {source_id}"
                )
            }
            Self::SourceNotFetched {
                assessment_id,
                source_id,
                ..
            } => {
                write!(
                    f,
                    "finding {assessment_id}: source {source_id} has no fetched text"
                )
            }
            Self::ClaimParaphraseTooClose {
                claim_id, detail, ..
            } => {
                write!(
                    f,
                    "claim {claim_id}: too close to source ({detail}) — revise the prose"
                )
            }
            Self::ClaimParaphraseNoSupport {
                claim_id, detail, ..
            } => {
                write!(
                    f,
                    "claim {claim_id}: prose unsupported by claimed source ({detail}) — resolve anchors"
                )
            }
            Self::LinterError { rule, detail, .. } => {
                write!(f, "linter [{rule}]: {detail}")
            }
            Self::SweepSourceUnresolved {
                source_id,
                status,
                url,
            } => {
                write!(
                    f,
                    "sweep source {source_id} unresolved ({status}): {url} — fetch it (wa fetch <slug>), attach an operator capture (wa ledger attach), or record a disposition (wa fetch dispose)"
                )
            }
        }
    }
}

/// The gate verdict.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateVerdict {
    pub blocked: bool,
    pub reasons: Vec<GateReason>,
}

/// Gate inputs: ledger, findings, base and proposed wikitext, linter and
/// paraphrase configs.
pub struct GateInput<'a> {
    pub ledger: &'a Ledger,
    pub assessments: &'a [Assessment],
    pub base_wikitext: &'a str,
    pub proposed_wikitext: &'a str,
    pub linter_config: &'a LinterConfig,
    /// Paraphrase thresholds (MVP-2 A.2.3 — `rules/paraphrase.toml`).
    pub paraphrase_config: &'a crate::checks::paraphrase::ParaphraseConfig,
}

/// Run the gate. [`GateVerdict::blocked`] is true iff `reasons` is non-empty.
#[must_use]
pub fn run_gate(input: &GateInput) -> GateVerdict {
    let mut reasons = Vec::new();

    // 1. Evidence verification: findings must be anchored and quotes must
    //    re-locate in the fetched bytes.
    for finding in input.assessments {
        let span = Some(finding.wikitext_anchor.clone());
        if finding.evidence.is_empty() {
            reasons.push(GateReason::AssessmentWithoutEvidence {
                assessment_id: finding.id.clone(),
                span: span.clone(),
            });
            continue;
        }
        for quote_id in &finding.evidence {
            let Some(quote) = input.ledger.quote(quote_id) else {
                reasons.push(GateReason::UnknownQuoteId {
                    assessment_id: finding.id.clone(),
                    quote_id: quote_id.clone(),
                    span: span.clone(),
                });
                continue;
            };
            match input.ledger.source_text(&quote.source_id) {
                None => reasons.push(GateReason::SourceNotFetched {
                    assessment_id: finding.id.clone(),
                    source_id: quote.source_id.clone(),
                    span: span.clone(),
                }),
                Some(text) => {
                    if locate_quote(&quote.text, text).is_none() {
                        reasons.push(GateReason::QuoteDoesNotLocate {
                            assessment_id: finding.id.clone(),
                            quote_id: quote_id.clone(),
                            source_id: quote.source_id.clone(),
                            span: span.clone(),
                        });
                    }
                }
            }
        }
    }

    // 2. Paraphrase gate: each claim's prose vs its claimed source texts.
    paraphrase_reasons(input, &mut reasons);

    // 3. Linter gate: error-severity findings block.
    for lint in linter::gate(
        input.base_wikitext,
        input.proposed_wikitext,
        input.linter_config,
    ) {
        if lint.severity == linter::Severity::Error {
            reasons.push(GateReason::LinterError {
                rule: lint.rule,
                detail: lint.detail,
                span: (lint.line > 0).then(|| format!("L{}", lint.line)),
            });
        }
    }

    // 4. Sweep completeness (plan-003 B.3): opt-in per session. A swept
    //    source without fetched text and without a disposition blocks —
    //    fetch-or-dispose every cited source BEFORE textual analysis.
    if input.ledger.has_sweep_state() {
        for (entry, status) in input.ledger.sweep_unresolved() {
            reasons.push(GateReason::SweepSourceUnresolved {
                source_id: entry.id.clone(),
                status: status.to_string(),
                url: entry.url.clone(),
            });
        }
    }

    let blocked = !reasons.is_empty();
    GateVerdict { blocked, reasons }
}

/// Claim pass of the gate: each claim must resolve to quotes with fetched
/// source text, and its prose is paraphrase-checked against those texts.
fn paraphrase_reasons(input: &GateInput, reasons: &mut Vec<GateReason>) {
    use paraphrase::ParaphraseVerdict;

    let base_norm = input
        .base_wikitext
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let proposed_norm = input
        .proposed_wikitext
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for claim in &input.ledger.claims {
        // Scope: the gate assesses DRAFTED prose. A claim whose prose
        // already exists in the base wikitext is pre-existing article text
        // gaining a citation — not ours to paraphrase-check (operator MOS
        // catch: attributing a well-sourced fact is Words-to-watch
        // hedging; the plain sentence just needs its ref).
        let claim_norm = claim.prose.split_whitespace().collect::<Vec<_>>().join(" ");
        if base_norm.contains(&claim_norm) {
            continue;
        }
        // Claim sequencing (loop-mechanization Phase 4): a claim whose
        // prose is staged NOWHERE — not inherited, not in the proposal —
        // is a registration without an edit. Anchor work: stage the prose
        // or drop the claim.
        if !proposed_norm.contains(&claim_norm) {
            reasons.push(GateReason::ClaimNotStaged {
                claim_id: claim.id.clone(),
                prose: claim.prose.clone(),
                span: None,
            });
            continue;
        }
        let span = crate::render::locate_block_anchor(input.proposed_wikitext, &claim.prose);
        // Nothing here is covered by the findings pass (it walks finding
        // evidence only): a claim with no quotes, or with a quote or
        // source text the ledger cannot produce, blocks in its own name.
        if claim.quote_ids.is_empty() {
            reasons.push(GateReason::ClaimWithoutQuotes {
                claim_id: claim.id.clone(),
                span,
            });
            continue;
        }
        // Each cited source's text once, however many quotes cite it.
        let mut source_ids: Vec<&str> = Vec::new();
        let mut combined_source = String::new();
        let mut missing = false;
        for quote_id in &claim.quote_ids {
            let resolved = input
                .ledger
                .quote(quote_id)
                .and_then(|q| input.ledger.source_text(&q.source_id).map(|text| (q, text)));
            if let Some((quote, text)) = resolved {
                if !source_ids.contains(&quote.source_id.as_str()) {
                    source_ids.push(&quote.source_id);
                    combined_source.push_str(text);
                    combined_source.push('\n');
                }
            } else {
                missing = true;
                reasons.push(GateReason::ClaimQuoteUnresolved {
                    claim_id: claim.id.clone(),
                    quote_id: quote_id.clone(),
                    span: span.clone(),
                });
            }
        }
        if missing {
            continue;
        }
        let assessment = paraphrase::assess_paraphrase_with(
            input.paraphrase_config,
            &claim.prose,
            &combined_source,
        );
        match assessment.verdict {
            ParaphraseVerdict::TooClose => {
                reasons.push(GateReason::ClaimParaphraseTooClose {
                    claim_id: claim.id.clone(),
                    detail: format!(
                        "{}/{} shingles shared, longest run {} words",
                        assessment.shared_shingles,
                        assessment.draft_shingles,
                        assessment.longest_common_run
                    ),
                    span,
                });
            }
            ParaphraseVerdict::NoSupport => {
                reasons.push(GateReason::ClaimParaphraseNoSupport {
                    claim_id: claim.id.clone(),
                    detail: format!(
                        "{}/{} shingles shared",
                        assessment.shared_shingles, assessment.draft_shingles
                    ),
                    span,
                });
            }
            ParaphraseVerdict::Ok => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GateInput, GateReason, run_gate};
    use crate::checks::linter::LinterConfig;
    use crate::ledger::Ledger;
    use crate::session::Assessment;

    const SOURCE_TEXT: &str = "Temple Fielding's travel guides sold millions of copies and \
                               fictionalized his own itineraries for comic effect. The 1942 \
                               edition alone sold three million copies in Japan by 1986.";

    fn setup() -> (Ledger, LinterConfig) {
        let mut ledger = Ledger::default();
        let sid = ledger.register_source("https://example.com/fielding", "2026-09-24", None);
        ledger.attach_fetched_text(&sid, SOURCE_TEXT).unwrap();
        let cfg = LinterConfig::from_toml_str(include_str!("../../rules/linter.toml")).unwrap();
        (ledger, cfg)
    }

    fn finding(evidence: Vec<String>) -> Assessment {
        Assessment {
            id: "AS1".into(),
            wikitext_anchor: "L1:C0-L1:C50".into(),
            rendered_span_id: None,
            rules: vec!["WP:V".into()],
            evidence,
            factual_note: "note".into(),
            proposed_fix: "fix".into(),
            loop_id: 2,
        }
    }

    #[test]
    fn clean_inputs_pass() {
        let (mut ledger, cfg) = setup();
        let qid = ledger
            .add_quote("S1", "sold three million copies in Japan by 1986")
            .unwrap();
        ledger
            .add_claim(
                "By 1986 Japanese readers had bought three million copies of the 1942 edition.",
                vec![qid],
            )
            .unwrap();
        let findings = vec![finding(vec![ledger.quotes[0].id.clone()])];
        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &findings,
            base_wikitext: "Old text.",
            proposed_wikitext: "By 1986 Japanese readers had bought three million copies of the 1942 edition.",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(!verdict.blocked, "{verdict:?}");
    }

    #[test]
    fn ac11_quote_fails_validation_blocks() {
        let (mut ledger, cfg) = setup();
        // A legitimate quote entered via add_quote always re-locates; simulate
        // tampering by editing the stored quote text directly.
        let qid = ledger.add_quote("S1", "sold millions of copies").unwrap();
        ledger.quotes[0].text = "won the Nobel Prize for travel writing".into();
        let findings = vec![finding(vec![qid])];
        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &findings,
            base_wikitext: "",
            proposed_wikitext: "text",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(verdict.blocked);
        assert!(matches!(
            verdict.reasons[0],
            GateReason::QuoteDoesNotLocate { .. }
        ));
    }

    #[test]
    fn ac11_unknown_quote_and_missing_evidence_block() {
        let (ledger, cfg) = setup();
        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &[finding(vec!["Q99".into()])],
            base_wikitext: "",
            proposed_wikitext: "text",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(verdict.blocked);
        assert!(matches!(
            verdict.reasons[0],
            GateReason::UnknownQuoteId { .. }
        ));

        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &[finding(vec![])],
            base_wikitext: "",
            proposed_wikitext: "text",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(matches!(
            verdict.reasons[0],
            GateReason::AssessmentWithoutEvidence { .. }
        ));
    }

    #[test]
    fn ac11_gate_blocks_publish_side_too() {
        // The same gate re-runs before publish; blocked stays blocked.
        let (ledger, cfg) = setup();
        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &[finding(vec![])],
            base_wikitext: "",
            proposed_wikitext: "text",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(verdict.blocked);
        // Re-run (publish side) yields the same verdict.
        let again = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &[finding(vec![])],
            base_wikitext: "",
            proposed_wikitext: "text",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert_eq!(verdict, again);
    }

    #[test]
    fn linter_error_blocks_gate() {
        let (ledger, cfg) = setup();
        // Proposed introduces an unspaced heading (whole-page error, new).
        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &[],
            base_wikitext: "",
            proposed_wikitext: "==Life==\nHe was born.",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(verdict.blocked);
        assert!(matches!(verdict.reasons[0], GateReason::LinterError { .. }));
    }

    /// MVP-2 A.2.1 — every reason carries a span when locatable, and the
    /// shared report groups by disposition.
    #[test]
    fn mvp2_a21_spans_and_disposition_groups() {
        let (ledger, cfg) = setup();
        let mut f = finding(vec![]);
        f.wikitext_anchor = "L7:C0-L7:C40".into();
        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &[f],
            base_wikitext: "",
            proposed_wikitext: "==Life==\ntext",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(verdict.blocked);
        for r in &verdict.reasons {
            assert!(r.span().is_some(), "{r} lacks span");
        }
        let needs = verdict
            .reasons
            .iter()
            .filter(|r| r.disposition() == super::Disposition::NeedsAnchor)
            .count();
        let hard = verdict
            .reasons
            .iter()
            .filter(|r| r.disposition() == super::Disposition::HardBlock)
            .count();
        assert!(needs >= 1, "finding-without-evidence is anchor work");
        assert!(hard >= 1, "linter error is a hard block");
        let report = super::format_reasons(&verdict.reasons);
        assert!(report.contains("NEEDS ANCHOR"), "{report}");
        assert!(report.contains("HARD BLOCK"), "{report}");
        assert!(report.contains("[at L7:C0-L7:C40]"), "{report}");
    }

    /// Operator MOS catch (live L2 edit 2): citing pre-existing article
    /// text is not drafting — the paraphrase gate skips claim prose that
    /// already exists in the base wikitext.
    #[test]
    fn inherited_claim_prose_skips_paraphrase_gate() {
        let (mut ledger, cfg) = setup();
        let qid = ledger.add_quote("S1", "sold millions of copies").unwrap();
        ledger
            .add_claim(
                "Temple Fielding's travel guides sold millions of copies.",
                vec![qid],
            )
            .unwrap();
        let base = "Temple Fielding's travel guides sold millions of copies.\n";
        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &[],
            base_wikitext: base,
            proposed_wikitext: "text",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(
            !verdict.blocked,
            "inherited claim must not be assessed: {verdict:?}"
        );
    }

    #[test]
    fn close_paraphrase_claim_blocks() {
        let (mut ledger, cfg) = setup();
        let qid = ledger.add_quote("S1", "sold millions of copies").unwrap();
        ledger
            .add_claim(
                "Temple Fielding's travel guide sold millions of copies and fictionalized his \
                 own itineraries for comic effect.",
                vec![qid],
            )
            .unwrap();
        let verdict = run_gate(&GateInput {
            ledger: &ledger,
            assessments: &[],
            base_wikitext: "",
            proposed_wikitext: "Temple Fielding's travel guide sold millions of copies and fictionalized his \
                 own itineraries for comic effect.",
            linter_config: &cfg,
            paraphrase_config: &crate::checks::paraphrase::ParaphraseConfig::default(),
        });
        assert!(verdict.blocked, "{verdict:?}");
        assert!(matches!(
            verdict.reasons[0],
            GateReason::ClaimParaphraseTooClose { .. }
        ));
    }
}
