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
