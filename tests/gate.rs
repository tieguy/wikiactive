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
use wikiloop::session::Finding;

fn linter() -> LinterConfig {
    LinterConfig::load(std::path::Path::new("rules/linter.toml")).unwrap()
}

fn finding(evidence: Vec<String>) -> Finding {
    Finding {
        id: "F1".into(),
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
        findings: &[finding(vec![qid])],
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
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
        findings: &[finding(vec!["Q42".into()])],
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
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
        findings: &[finding(vec![])],
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
    });
    assert!(verdict.blocked);
    assert!(matches!(
        verdict.reasons.first(),
        Some(GateReason::FindingWithoutEvidence { .. })
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
        findings: &input_findings,
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
    });
    let before_publish = run_gate(&GateInput {
        ledger: &ledger,
        findings: &input_findings,
        base_wikitext: "",
        proposed_wikitext: "text",
        linter_config: &linter(),
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
        findings: &[finding(vec![qid])],
        base_wikitext: "base text here",
        proposed_wikitext: "A paraphrase of the fetched source material, properly reworded.",
        linter_config: &linter(),
    });
    assert!(!verdict.blocked, "{verdict:?}");
}
