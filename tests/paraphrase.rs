//! AC.3 — paraphrase gate discriminates (integration-level per the test
//! matrix): near-verbatim flags too-close; clean paraphrase with supporting
//! quote passes; unrelated text flags no-support.

use wikiloop::checks::paraphrase::ParaphraseVerdict;
use wikiloop::checks::paraphrase::assess_paraphrase;
const SOURCE: &str = "The Golden Gate Bridge was completed in 1937 after a decade of \
                      construction delays and cost overruns that strained municipal \
                      finances during the Depression.";

#[test]
fn ac3_near_verbatim_flags_too_close() {
    let draft = "The Golden Gate Bridge was finished in 1937 following ten years of \
                 building setbacks and budget excesses that stretched city finances \
                 during the Depression.";
    let a = assess_paraphrase(draft, SOURCE);
    assert_eq!(a.verdict, ParaphraseVerdict::TooClose, "{a:?}");
}

#[test]
fn ac3_clean_paraphrase_with_support_passes() {
    let draft = "A ten-year project beset by delays, the Golden Gate Bridge opened in \
                 1937 having cost far more than planned and left the city strained \
                 financially through the Depression years.";
    let a = assess_paraphrase(draft, SOURCE);
    assert_eq!(a.verdict, ParaphraseVerdict::Ok, "{a:?}");
}

#[test]
fn ac3_unrelated_text_flags_no_support() {
    let draft = "Temple Fielding wrote humorous travel guides that fictionalized his \
                 own itineraries, and critics debated whether the joke was on tourism \
                 itself.";
    let a = assess_paraphrase(draft, SOURCE);
    assert_eq!(a.verdict, ParaphraseVerdict::NoSupport, "{a:?}");
}

/// MVP-2 A.2.3 — the thresholds extracted to `rules/paraphrase.toml` are
/// pinned against the compiled defaults (the original constants): drifting
/// the config without recording the decision fails here, and the default
/// path ([`assess_paraphrase`]) agrees with the config-loaded path.
#[test]
fn rules_paraphrase_toml_matches_default_thresholds() {
    let from_file = wikiloop::checks::paraphrase::ParaphraseConfig::load(std::path::Path::new(
        "rules/paraphrase.toml",
    ))
    .expect("paraphrase.toml parses");
    assert_eq!(
        from_file,
        wikiloop::checks::paraphrase::ParaphraseConfig::default(),
        "rules/paraphrase.toml diverged from the compiled defaults — record the \
         threshold decision in docs/design-plans/2026-09-25-mvp2-addendum.md"
    );
    // The default-config wrapper and the config-loaded path agree on a
    // canonical input (verdict pinning across the extraction).
    let draft = "The bridge was finished in 1937 following ten years of construction \
                 delays and cost overruns that strained city finances in the \
                 Depression years.";
    let via_default = assess_paraphrase(draft, SOURCE);
    let via_file = wikiloop::checks::paraphrase::assess_paraphrase_with(&from_file, draft, SOURCE);
    assert_eq!(via_default.verdict, via_file.verdict);
    assert_eq!(via_default, via_file);
}

/// Quote characters inside markup are attribute syntax, not a quotation:
/// verbatim copy carrying a named ref is still too close.
#[test]
fn ref_attribute_quotes_earn_no_exemption() {
    let draft = format!("{SOURCE}<ref name=\"bridge\" />");
    let a = assess_paraphrase(&draft, SOURCE);
    assert_eq!(a.verdict, ParaphraseVerdict::TooClose, "{a:?}");
}

/// The quotation exemption is for spans that locate verbatim in the
/// claimed source; a quoted word the source never uses exempts nothing.
#[test]
fn unlocated_quotation_earns_no_exemption() {
    let draft = "The \"mayor\" embezzled four million dollars and fled to Brazil with \
                 his accomplices before the audit began.";
    let a = assess_paraphrase(draft, SOURCE);
    assert_eq!(a.verdict, ParaphraseVerdict::NoSupport, "{a:?}");
    let quoting = "One account says it came \"after a decade of construction delays\".";
    let a = assess_paraphrase(quoting, SOURCE);
    assert_eq!(a.verdict, ParaphraseVerdict::Ok, "{a:?}");
}

/// A draft shorter than one shingle is not "too close" by default.
#[test]
fn short_draft_is_not_too_close() {
    let a = assess_paraphrase("Born in 1950.", SOURCE);
    assert_ne!(a.verdict, ParaphraseVerdict::TooClose, "{a:?}");
}

#[test]
fn degenerate_thresholds_are_rejected() {
    use wikiloop::checks::paraphrase::ParaphraseConfig;
    let base = include_str!("../rules/paraphrase.toml");
    assert!(ParaphraseConfig::from_toml_str(base).is_ok());
    for bad in ["shingle_n = 0", "too_close_den = 0"] {
        let key = bad.split(' ').next().unwrap();
        let text: String = base
            .lines()
            .map(|l| if l.starts_with(key) { bad } else { l })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(ParaphraseConfig::from_toml_str(&text).is_err(), "{bad}");
    }
}
