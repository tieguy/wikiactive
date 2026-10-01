//! AC.1 — rules corpus + context bundle load and validate: tier1-core
//! exists with the three clusters, ≥10 cards, linter config parses, house
//! rules include the semicolon ban; and the `wa analyze` context bundle
//! embeds the tier1 clauses verbatim plus the triage-selected cards (a
//! missing corpus or card fails loudly — it can never be silently dropped
//! from the bundle).

use wikiloop::rules::ArticleState;
use wikiloop::rules::RulesCorpus;
use wikiloop::rules::build_context_bundle;
use wikiloop::rules::cards_for_loop;

#[test]
fn ac1_rules_corpus_loads_and_validates() {
    let corpus = RulesCorpus::load(std::path::Path::new("rules")).expect("corpus loads");

    // Three clusters.
    for cluster in ["Cluster A", "Cluster B", "Cluster C"] {
        assert!(
            corpus.tier1_core.contains(cluster),
            "tier1-core missing {cluster}"
        );
    }
    // The standing checklist.
    assert!(corpus.tier1_core.contains("Standing checklist"));

    // ≥10 trigger cards, each with operative clause + canonical link +
    // failure examples.
    assert!(
        corpus.card_slugs.len() >= 10,
        "{} cards",
        corpus.card_slugs.len()
    );
    for slug in &corpus.card_slugs {
        let card = &corpus.cards[slug];
        assert!(
            card.contains("Operative clause"),
            "{slug}: no operative clause"
        );
        assert!(card.contains("Canonical:"), "{slug}: no canonical link");
        assert!(
            card.contains("Failure examples"),
            "{slug}: no failure examples"
        );
    }

    // Linter config parses (rules present).
    assert!(!corpus.linter.rules.is_empty());

    // House rules include the semicolon ban.
    assert!(corpus.house_rules.prose.ban_semicolons);
    assert!(
        corpus
            .house_rules
            .disclosure
            .suffix
            .starts_with("LLM-Disclosure:")
    );

    // RSP seed has deny/caution/complement-only tiers.
    for tier in ["deny", "caution", "complement-only", "ok"] {
        assert!(
            corpus.rsp_seed.iter().any(|r| r.tier == tier),
            "rsp seed missing tier {tier}"
        );
    }
}

#[test]
fn ac1_context_bundle_embeds_tier1_verbatim_and_triage_cards() {
    let corpus = RulesCorpus::load(std::path::Path::new("rules")).unwrap();
    let article = ArticleState {
        title: "Commitment device".into(),
        base_revid: 1_343_452_323,
        wikitext: "…".into(),
        entry_loop: 3,
        prior_session_diff: None,
    };
    let bundle = build_context_bundle(&corpus, &article, &[], &wikiloop::ledger::Ledger::default())
        .expect("bundle builds");

    // Tier 1 verbatim: an exact multi-line clause from the corpus file is
    // embedded unchanged.
    assert!(bundle.text.contains("Tier 1 — judgment core (verbatim)"));
    assert!(bundle.text.contains("**A1. Attribution over wikivoice.**"));

    // Triage-selected cards for loop 3 are embedded with their headers.
    for slug in cards_for_loop(3) {
        let upper = match slug.as_str() {
            "undue" => "UNDUE",
            "notlitreview" => "NOTLITREVIEW",
            "proseline" => "PROSELINE",
            "recentism" => "RECENTISM",
            "leadcite" => "LEADCITE",
            "summary-spinoff" => "SUMMARY-SPINOFF",
            "efn-conflicts" => "EFN-CONFLICTS",
            other => other,
        };
        assert!(
            bundle.text.contains(&format!("Card: {upper}")),
            "loop-3 card {slug} missing from bundle"
        );
    }

    // Article state carried.
    assert!(bundle.text.contains("Commitment device"));
    assert!(bundle.text.contains("1343452323"));
}

#[test]
fn ac1_dropped_corpus_fails_the_bundle_not_silently() {
    let mut corpus = RulesCorpus::load(std::path::Path::new("rules")).unwrap();
    corpus.cards.remove("undue");
    let article = ArticleState {
        title: "T".into(),
        base_revid: 1,
        wikitext: String::new(),
        entry_loop: 3,
        prior_session_diff: None,
    };
    let err = build_context_bundle(&corpus, &article, &[], &wikiloop::ledger::Ledger::default())
        .unwrap_err();
    assert!(err.contains("undue"), "{err}");
}

// --------------------------------- loop-mechanization Phase 6 defect scan

/// loopmech.AC9.1 — the analyze bundle lists base-article mechanical
/// defects as candidate assessments under a clearly-labeled section,
/// with severity, rule id, and line; a clean base emits no section.
#[test]
fn defect_scan_lists_base_defects_as_candidates() {
    let corpus = RulesCorpus::load(std::path::Path::new("rules")).unwrap();
    let dirty = ArticleState {
        title: "Dirty".into(),
        base_revid: 1,
        wikitext: "==History==\nThe railroad is now the largest employer in the county.\n".into(),
        entry_loop: 2,
        prior_session_diff: None,
    };
    let bundle = build_context_bundle(&corpus, &dirty, &[], &wikiloop::ledger::Ledger::default())
        .expect("bundle builds");
    assert!(
        bundle.text.contains("Base-article defect candidates"),
        "labeled section present: {}",
        bundle.text
    );
    assert!(bundle.text.contains("tense-drift"), "{}", bundle.text);
    assert!(bundle.text.contains("heading-spacing"), "{}", bundle.text);
    assert!(bundle.text.contains("detection only"), "{}", bundle.text);

    let clean = ArticleState {
        title: "Clean".into(),
        base_revid: 1,
        wikitext: "The tower is old.\n".into(),
        entry_loop: 2,
        prior_session_diff: None,
    };
    let bundle = build_context_bundle(&corpus, &clean, &[], &wikiloop::ledger::Ledger::default())
        .expect("bundle builds");
    assert!(
        !bundle.text.contains("Base-article defect candidates"),
        "clean base, no section"
    );
}

/// loopmech.AC9.2 / AC9.3 — the scan is informational: a base full of
/// defects with an UNCHANGED proposed text passes the gate (scan output
/// alone never blocks), and the drafted-lines scope keeps gating only
/// lines the tool drafts (pre-existing defects in the base are not the
/// gate's business).
#[test]
fn defect_scan_alone_never_blocks_and_scope_is_unchanged() {
    let corpus = RulesCorpus::load(std::path::Path::new("rules")).unwrap();
    let base = "==History==\nThe railroad is now the largest employer in the county.\n";
    // The bundle lists the defects…
    let article = ArticleState {
        title: "Dirty".into(),
        base_revid: 1,
        wikitext: base.into(),
        entry_loop: 2,
        prior_session_diff: None,
    };
    let bundle =
        build_context_bundle(&corpus, &article, &[], &wikiloop::ledger::Ledger::default()).unwrap();
    assert!(bundle.text.contains("tense-drift"));

    // …but the gate over base == proposed is green: detection is not
    // enforcement, and the scan feeds no gate reason.
    let verdict = wikiloop::checks::gate::run_gate(&wikiloop::checks::gate::GateInput {
        ledger: &wikiloop::ledger::Ledger::default(),
        assessments: &[],
        base_wikitext: base,
        proposed_wikitext: base,
        linter_config: &corpus.linter,
        paraphrase_config: &corpus.paraphrase,
    });
    assert!(
        !verdict.blocked,
        "detection alone never blocks: {verdict:?}"
    );
}
