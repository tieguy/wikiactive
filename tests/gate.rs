//! AC.11 — the anchor gate is the "never edit from model memory" invariant:
//! a finding whose evidence quotes fail quote-anchor validation against the
//! session ledger blocks render (no artifact written) and blocks publish;
//! the gate is render's mandatory pre-flight and re-runs before publish's
//! tty confirmation.

use wikiloop::checks::gate::GateInput;
use wikiloop::checks::gate::GateReason;
use wikiloop::checks::gate::run_gate;
use wikiloop::checks::linter::LinterConfig;
use wikiloop::ledger::Ledger;
use wikiloop::session::Assessment;

fn linter() -> LinterConfig {
    LinterConfig::load(std::path::Path::new("rules/linter.toml")).unwrap()
}

fn finding(evidence: Vec<String>) -> Assessment {
    Assessment {
        id: "AS1".into(),
        wikitext_anchor: "L1:C0-L1:C10".into(),
        rendered_span_id: None,
        rules: vec!["WP:V".into()],
        evidence,
        factual_note: "note".into(),
        proposed_fix: "fix".into(),
        loop_id: 2,
    }
}

fn ledger_with_source() -> Ledger {
    let mut ledger = Ledger::default();
    let sid = ledger.register_source("https://example.com/s", "2026-09-24", None);
    ledger
        .attach_fetched_text(&sid, "The real fetched source text sits here.")
        .unwrap();
    ledger
}

#[test]
fn ac11_tampered_quote_blocks() {
    let mut ledger = ledger_with_source();
    let qid = ledger.add_quote("S1", "real fetched source text").unwrap();
    ledger.quotes[0].text = "invented words absent from the source".into();

    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &[finding(vec![qid])],
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    assert!(verdict.blocked);
    assert!(matches!(
        verdict.reasons.first(),
        Some(GateReason::QuoteDoesNotLocate { .. })
    ));
}

#[test]
fn ac11_unknown_quote_id_blocks() {
    let ledger = ledger_with_source();
    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &[finding(vec!["Q42".into()])],
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    assert!(verdict.blocked);
    assert!(matches!(
        verdict.reasons.first(),
        Some(GateReason::UnknownQuoteId { .. })
    ));
}

#[test]
fn ac11_unanchored_finding_never_reaches_review() {
    let ledger = ledger_with_source();
    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &[finding(vec![])],
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    assert!(verdict.blocked);
    assert!(matches!(
        verdict.reasons.first(),
        Some(GateReason::AssessmentWithoutEvidence { .. })
    ));
}

#[test]
fn ac11_gate_reruns_identically_before_publish_confirmation() {
    // The publish path re-runs the same gate; a verdict that blocked at
    // render still blocks at publish (fresh run, same inputs).
    let mut ledger = ledger_with_source();
    let qid = ledger.add_quote("S1", "real fetched source text").unwrap();
    ledger.quotes[0].text = "tampered".into();
    let input_findings = [finding(vec![qid])];

    let at_render = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &input_findings,
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    let before_publish = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &input_findings,
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    assert!(at_render.blocked);
    assert_eq!(at_render, before_publish, "publish re-check must agree");
}

#[test]
fn ac11_clean_gate_passes() {
    let mut ledger = ledger_with_source();
    let qid = ledger.add_quote("S1", "real fetched source text").unwrap();
    ledger
        .add_claim(
            "A paraphrase of the fetched source material, properly reworded.",
            vec![qid.clone()],
        )
        .unwrap();
    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &[finding(vec![qid])],
        base_wikitext: "base text here",
        proposed_wikitext: "A paraphrase of the fetched source material, properly reworded.",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    assert!(!verdict.blocked, "{verdict:?}");
}

/// A claim is anchored by its quotes: one that cites none, or cites a
/// quote the ledger cannot produce, blocks in its own name (the findings
/// pass never looks at claims).
#[test]
fn claims_without_resolvable_quotes_block() {
    let run = |ledger: &Ledger| {
        run_gate(&GateInput {
            ledger,
            assessments: &[],
            base_wikitext: "Old text.\n",
            proposed_wikitext: "Old text. The keep was rebuilt in stone.\n",
            linter_config: &linter(),
            paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
        })
    };

    let mut ledger = ledger_with_source();
    ledger
        .add_claim("The keep was rebuilt in stone.", vec![])
        .unwrap();
    let verdict = run(&ledger);
    assert!(verdict.blocked);
    assert!(matches!(
        verdict.reasons.first(),
        Some(GateReason::ClaimWithoutQuotes { .. })
    ));

    let mut ledger = ledger_with_source();
    let qid = ledger.add_quote("S1", "real fetched source text").unwrap();
    ledger
        .add_claim("The keep was rebuilt in stone.", vec![qid])
        .unwrap();
    ledger.claims[0].quote_ids = vec!["Q99".into()];
    let verdict = run(&ledger);
    assert!(verdict.blocked);
    assert!(matches!(
        verdict.reasons.first(),
        Some(GateReason::ClaimQuoteUnresolved { .. })
    ));
}

// -------------------------------------- loop-mechanization AC4 claim staging

/// loopmech.AC4.1 — a claim whose prose appears in NEITHER base nor
/// proposed wikitext is a distinct NEEDS ANCHOR reason naming the claim
/// id and its prose.
#[test]
fn claim_staged_nowhere_fires_claim_not_staged() {
    let mut ledger = ledger_with_source();
    let qid = ledger.add_quote("S1", "real fetched source text").unwrap();
    ledger
        .add_claim("The keep was rebuilt in stone.", vec![qid])
        .unwrap();
    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &[],
        base_wikitext: "The tower is old.\n",
        proposed_wikitext: "The tower is ancient.\n",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    assert!(verdict.blocked, "unstaged claim blocks: {verdict:?}");
    match verdict.reasons.first() {
        Some(GateReason::ClaimNotStaged {
            claim_id, prose, ..
        }) => {
            assert_eq!(claim_id, "C1");
            assert!(prose.contains("The keep was rebuilt in stone."), "{prose}");
        }
        other => panic!("the new reason fires first: {other:?}"),
    }
    // It is anchor work by disposition (groups under NEEDS ANCHOR).
    assert_eq!(
        verdict.reasons[0].disposition(),
        wikiloop::checks::gate::Disposition::NeedsAnchor
    );
}

/// loopmech.AC4.2 — inherited prose (base only) does NOT fire the new
/// reason: the existing inherited skip keeps the gate green.
#[test]
fn inherited_claim_does_not_fire_claim_not_staged() {
    let mut ledger = ledger_with_source();
    let qid = ledger.add_quote("S1", "real fetched source text").unwrap();
    ledger.add_claim("The tower is old.", vec![qid]).unwrap();
    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &[],
        base_wikitext: "The tower is old.\n",
        proposed_wikitext: "The tower is ancient.\n",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    assert!(!verdict.blocked, "inherited claim stays green: {verdict:?}");
}

/// loopmech.AC4.3 — staged prose (proposed only) does NOT fire the new
/// reason (the existing quote/paraphrase checks apply instead).
#[test]
fn staged_claim_does_not_fire_claim_not_staged() {
    let mut ledger = ledger_with_source();
    let qid = ledger.add_quote("S1", "real fetched source text").unwrap();
    ledger
        .add_claim("The tower is ancient.", vec![qid])
        .unwrap();
    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &[],
        base_wikitext: "The tower is old.\n",
        proposed_wikitext: "The tower is ancient.\n",
        linter_config: &linter(),
        paraphrase_config: &wikiloop::checks::paraphrase::ParaphraseConfig::default(),
    });
    let fired = verdict
        .reasons
        .iter()
        .any(|r| matches!(r, GateReason::ClaimNotStaged { .. }));
    assert!(
        !fired,
        "staged claim does not fire the new reason: {verdict:?}"
    );
}
