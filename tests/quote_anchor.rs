//! AC.2 — quote-anchor validator behavior (integration-level, per the test
//! matrix): verbatim passes; absent fails; punctuation/whitespace-varied
//! passes; wrong-source fails.

use wikiloop::checks::quote_anchor::locate_quote;

const SOURCE: &str = "Fielding's 1942 guide sold three million copies in Japan by 1986, \
                      a figure the publisher repeated for decades.";

#[test]
fn ac2_verbatim_quote_passes() {
    assert!(locate_quote("sold three million copies in Japan by 1986", SOURCE).is_some());
}

#[test]
fn ac2_absent_quote_fails() {
    assert_eq!(
        locate_quote("sold three million copies worldwide", SOURCE),
        None
    );
}

#[test]
fn ac2_punctuation_and_whitespace_varied_quote_passes() {
    let varied_source = "She said “the  guide  sold\nthree million copies” in its 1986 report.";
    let quote = "she said \"the guide sold three million copies\" in its 1986 report";
    assert!(locate_quote(quote, varied_source).is_some());
}

#[test]
fn ac2_wrong_source_quote_fails() {
    let other_source = "The bridge was completed in 1937 after a decade of construction delays.";
    let quote = "sold three million copies in Japan by 1986";
    assert!(locate_quote(quote, SOURCE).is_some());
    assert_eq!(locate_quote(quote, other_source), None);
}

#[test]
fn ac2_scope_widening_is_not_a_match() {
    // The load-bearing anti-hallucination property: words the source does
    // not contain, or non-contiguous splices, break verbatimness.
    assert_eq!(
        locate_quote("sold three million copies worldwide", SOURCE),
        None
    );
    // "by 1986" without "in Japan" is a non-contiguous splice of the source.
    assert_eq!(
        locate_quote("sold three million copies by 1986", SOURCE),
        None
    );
    // Note: a quote that merely DROPS a trailing qualifier ("...in Japan")
    // still locates — it is a verbatim substring. Scope-scope match (B1) is
    // a Tier-1 judgment the reviewer makes against the rendered quote, not
    // a locator property.
    assert!(locate_quote("sold three million copies in Japan", SOURCE).is_some());
}
