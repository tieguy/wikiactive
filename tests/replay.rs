//! AC.8 — replay surfaces known findings: all mechanically-checkable
//! fixtures (TF auto ref names, TF unspaced headings, CD defects) reported
//! by the deterministic checks over frozen wikitext. Judgment findings are
//! captured in the replay soft report (full offline PLAYBOOK run, publishing
//! disabled) and graded by the operator — never asserted by code.
//!
//! Frozen fixtures (pre-session 2026-09-24):
//! - Temple Fielding @ 1372827284
//! - Commitment device @ 1343452323
//!
//! Plan-vs-reality note (recorded for the operator): the plan expected CD
//! lead/"Concept and mechanisms" duplication to trip the 8-gram shingle
//! check. At the frozen revid the maximum shared lead/body run is a 6-gram
//! ("with their backs to a river", the Cortés example) — below the 8-gram
//! duplication threshold, which is calibrated so legitimate summary-style
//! overlap (5–7 grams) does not flag. The test asserts the true mechanical
//! state; the near-duplication is recorded here for the operator's grading.

use wikiloop::checks::linter::LinterConfig;
use wikiloop::checks::linter::scan_whole_page;

fn scan_fixture(path: &str) -> Vec<wikiloop::checks::linter::LintFinding> {
    let wikitext = std::fs::read_to_string(path).unwrap();
    let config = LinterConfig::load(std::path::Path::new("rules/linter.toml")).unwrap();
    scan_whole_page(&wikitext, &config)
}

#[test]
fn ac8_temple_fielding_auto_ref_names() {
    let findings = scan_fixture("fixtures/replay/temple-fielding@1372827284.wikitext");
    let autonumber = findings
        .iter()
        .find(|f| f.rule == "refname-autonumber")
        .expect("refname findings present");
    // All 12 occurrences (3×:0, 5×:1, 4×:2) are reported in the finding.
    assert!(
        autonumber.detail.contains("+11 more"),
        "TF fixture should report all 12 ':N' auto ref names; got: {}",
        autonumber.detail
    );
}

#[test]
fn ac8_temple_fielding_unspaced_headings() {
    let findings = scan_fixture("fixtures/replay/temple-fielding@1372827284.wikitext");
    let headings = findings
        .iter()
        .filter(|f| f.rule == "heading-spacing")
        .map(|f| f.detail.clone())
        .collect::<Vec<_>>();
    for expected in ["==Publications==", "==Bibliography==", "==External links=="] {
        assert!(
            headings.iter().any(|d| d.contains(expected)),
            "TF unspaced heading {expected} missing; got {headings:?}"
        );
    }
}

#[test]
fn ac8_commitment_device_mechanical_defects() {
    let findings = scan_fixture("fixtures/replay/commitment-device@1343452323.wikitext");
    let headings = findings
        .iter()
        .filter(|f| f.rule == "heading-spacing")
        .count();
    assert!(
        headings >= 8,
        "CD fixture should report its unspaced headings, got {headings}"
    );
}

/// The lead/body near-duplication observation documented above: exactly one
/// shared 6-gram exists (sub-threshold for the 8-gram error), and no 8-gram
/// duplication is reported. Pins both facts so drift in either direction is
/// visible.
#[test]
fn ac8_commitment_device_lead_body_overlap_is_subthreshold() {
    let findings = scan_fixture("fixtures/replay/commitment-device@1343452323.wikitext");
    let dup = findings
        .iter()
        .filter(|f| f.rule == "lead-body-duplication")
        .count();
    assert_eq!(
        dup, 0,
        "no 8-gram lead/body duplication at the frozen CD revid"
    );
    // The documented 6-gram near-dup is asserted at the fixture level:
    let wikitext =
        std::fs::read_to_string("fixtures/replay/commitment-device@1343452323.wikitext").unwrap();
    let lead_body_both = wikitext.matches("backs to a river").count();
    assert!(
        lead_body_both >= 2,
        "the Cortés 6-gram appears in both lead and body (near-dup observation for grading)"
    );
}
