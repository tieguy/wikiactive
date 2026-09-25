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
