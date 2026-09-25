//! AC.4 — linter catches every Tier-3 rule, with **config-derived** cases:
//! the test iterates every rule declared in `rules/linter.toml` (single
//! enumeration source — adding a rule without a checker, or a checker
//! without a rule, fails here) and asserts `sample_violation` is flagged by
//! its own rule while `sample_clean` passes. The AC's named fixed fixtures
//! are then asserted explicitly.

use wikiloop::checks::linter::LinterConfig;
use wikiloop::checks::linter::gate;

fn cfg() -> LinterConfig {
    LinterConfig::load(std::path::Path::new("rules/linter.toml")).expect("config parses")
}

/// One generated case per declared rule — no enumeration drift.
#[test]
fn ac4_every_declared_rule_flags_its_sample_violation_and_passes_its_clean() {
    let config = cfg();
    assert!(
        !config.rules.is_empty(),
        "config must declare rules for this test to mean anything"
    );
    for rule in config.rules.values() {
        // Violation: everything is "added" (empty base), so added-lines and
        // whole-page scopes both apply.
        let bad = gate("", &rule.sample_violation, &config);
        assert!(
            bad.iter().any(|f| f.rule == rule.id),
            "rule {} did not flag its sample_violation ({:?}); findings: {:?}",
            rule.id,
            rule.sample_violation,
            bad
        );
        // Clean: not flagged by its own rule.
        let good = gate("", &rule.sample_clean, &config);
        assert!(
            !good.iter().any(|f| f.rule == rule.id),
            "rule {} flagged its sample_clean ({:?}); findings: {:?}",
            rule.id,
            rule.sample_clean,
            good
        );
    }
}

/// The checker set and the config stay in lockstep.
#[test]
fn ac4_every_rule_id_has_a_known_checker() {
    let config = cfg();
    let known = [
        "refname-autonumber",
        "sfn-usage",
        "page-pages-consistency",
        "named-ref-with-pinpoint",
        "heading-spacing",
        "semicolon-prose",
        "tense-drift",
        "national-variety-mix",
        "italic-mismatch",
        "see-also-duplication",
        "lead-body-duplication",
    ];
    for rule in config.rules.values() {
        assert!(
            known.contains(&rule.id.as_str()),
            "rule {} declared in linter.toml has no test-known checker",
            rule.id
        );
    }
}

// ---------------------------------------------------------------- fixed fixtures

#[test]
fn ac4_fixed_autonumber_ref_name() {
    let config = cfg();
    let findings = gate(
        "",
        r#"The tower is tall.<ref name=":0">{{cite book|title=Towers}}</ref>"#,
        &config,
    );
    assert!(findings.iter().any(|f| f.rule == "refname-autonumber"));
}

#[test]
fn ac4_fixed_sfn_usage() {
    let config = cfg();
    let findings = gate(
        "",
        "Fielding wrote widely.{{sfn|Fielding|1942|p=12}}",
        &config,
    );
    assert!(findings.iter().any(|f| f.rule == "sfn-usage"));
}

#[test]
fn ac4_fixed_page_pages_inconsistency() {
    let config = cfg();
    let text = "A.<ref name=a>{{cite book|title=X|page=1}}</ref> B.<ref name=b>{{cite book|title=Y|pages=2-3}}</ref>";
    let findings = gate("", text, &config);
    assert!(findings.iter().any(|f| f.rule == "page-pages-consistency"));
}

#[test]
fn ac4_fixed_mos_us_mix() {
    let config = cfg();
    // An American-variety article line carrying a British -ise spelling.
    let findings = gate("", "The harbor was organised in 1901.", &config);
    assert!(
        findings.iter().any(|f| f.rule == "national-variety-mix"),
        "{findings:?}"
    );
    // Consistent American stays clean.
    let clean = gate("", "The harbor was organized in 1901.", &config);
    assert!(
        !clean.iter().any(|f| f.rule == "national-variety-mix"),
        "{clean:?}"
    );
}

#[test]
fn ac4_fixed_mos_tense() {
    let config = cfg();
    let findings = gate(
        "",
        "The company is currently the largest employer in the county.",
        &config,
    );
    assert!(
        findings.iter().any(|f| f.rule == "tense-drift"),
        "{findings:?}"
    );
}

#[test]
fn ac4_fixed_rp_usage() {
    // House style: pinpoints belong in {{rp}}; a named full ref carrying
    // |page= is the violation.
    let config = cfg();
    let findings = gate(
        "",
        r#"Text.<ref name="f1942">{{cite book|title=Travel|page=12}}</ref>"#,
        &config,
    );
    assert!(findings.iter().any(|f| f.rule == "named-ref-with-pinpoint"));
}

#[test]
fn ac4_fixed_heading_spacing() {
    let config = cfg();
    let findings = gate("", "==Life==\nHe was born in 1910.", &config);
    assert!(findings.iter().any(|f| f.rule == "heading-spacing"));
}

#[test]
fn ac4_fixed_see_also_misuse() {
    let config = cfg();
    let text = "Intro mentions the [[suspension bridge]] design.\n\n== See also ==\n* [[Suspension bridge]]\n";
    let findings = gate("", text, &config);
    assert!(
        findings.iter().any(|f| f.rule == "see-also-duplication"),
        "{findings:?}"
    );
}

#[test]
fn ac4_fixed_semicolon_in_drafted_prose() {
    let config = cfg();
    let findings = gate(
        "",
        "The bridge opened in 1937; it cost $35 million.",
        &config,
    );
    assert!(findings.iter().any(|f| f.rule == "semicolon-prose"));
}

#[test]
fn ac4_fixed_mismatched_italics() {
    let config = cfg();
    let text = "''The Sun Also Rises'' sold well, but critics panned The Sun Also Rises at first.";
    let findings = gate("", text, &config);
    assert!(findings.iter().any(|f| f.rule == "italic-mismatch"));
}

#[test]
fn ac4_fixed_lead_body_duplication() {
    let config = cfg();
    let text = "The commitment device is a psychological strategy studied in behavioral economics.\n\n== Concept ==\nScholars describe how the commitment device is a psychological strategy studied in behavioral economics, beginning in 1985.";
    let findings = gate("", text, &config);
    assert!(
        findings.iter().any(|f| f.rule == "lead-body-duplication"),
        "{findings:?}"
    );
}
