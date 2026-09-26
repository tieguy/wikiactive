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
