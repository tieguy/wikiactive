//! Paraphrase gate — shingle similarity between added prose and its claimed
//! source. WP:V mechanically enforced in both directions:
//!
//! - **Too-close** (CLOP): the draft keeps the source's expression (high
//!   n-gram containment or a long verbatim word-run). Hard-blocks render
//!   exactly like an anchor-gate failure; proceeding requires revising the
//!   prose, never waiving the check.
//! - **No-support**: the draft shares almost nothing with its claimed source;
//!   the claimed anchor does not actually support this prose. Requires anchor
//!   resolution before render (find the real quote, or the text goes).
//! - **Ok**: clean paraphrase in the support band.
//!
//! Integer-ratio comparisons only (shared·D ≥ draft·N) — no float ever
//! decides a verdict.

/// Word-shingle length. 4-grams are the standard close-paraphrase signal:
/// short enough to catch copied phrase structure, long enough that honest
/// paraphrase in the same language about the same facts rarely matches.
pub const SHINGLE_N: usize = 4;

/// Too-close when ≥ 60% of the draft's shingles occur in the source.
const TOO_CLOSE_NUM: usize = 6;
const TOO_CLOSE_DEN: usize = 10;
/// Too-close when any unquoted verbatim word-run of this length appears.
const TOO_CLOSE_RUN_WORDS: usize = 10;
/// Too-close when ≥ 40% of the draft's tokens match the source in order
/// (LCS) — catches synonym-substitution that keeps the sentence
/// architecture, which shingles miss.
const TOO_CLOSE_LCS_NUM: usize = 2;
const TOO_CLOSE_LCS_DEN: usize = 5;
/// No-support when < 5% shingle containment AND < 8% content-token LCS — the
/// claimed source shares nothing substantive with the prose. Content tokens
/// exclude function words so 1–2 stray "the"/"and" matches cannot defeat the
/// check.
const NO_SUPPORT_NUM: usize = 1;
const NO_SUPPORT_DEN: usize = 20;
const NO_SUPPORT_LCS_NUM: usize = 2;
const NO_SUPPORT_LCS_DEN: usize = 25;

/// English function words excluded from the no-support LCS signal.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "of", "in", "on", "at", "to", "was", "is", "were", "are",
    "that", "which", "who", "for", "with", "by", "from", "as", "it", "its", "their", "his", "her",
    "after", "before", "during", "into", "over", "than", "then", "be", "been", "has", "had",
    "have",
];

/// Verdict for one draft block against one claimed source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParaphraseVerdict {
    /// In the support band: neither too close nor unsupported.
    Ok,
    /// Close paraphrase: revise the prose (hard block).
    TooClose,
    /// The claimed source does not contain this prose's substance: resolve
    /// anchors before render.
    NoSupport,
}

/// Measured assessment (counts, not derived ratios — display computes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParaphraseAssessment {
    pub verdict: ParaphraseVerdict,
    /// Shingles of the draft present in the source.
    pub shared_shingles: usize,
    /// Total shingles in the draft.
    pub draft_shingles: usize,
    /// Longest contiguous verbatim word-run (after folding) shared with the
    /// source.
    pub longest_common_run: usize,
    /// Longest-common-subsequence token count (order-preserving overlap).
    pub lcs_tokens: usize,
}

/// Assess one block of added prose against the claimed source text.
///
/// Both sides are folded exactly as the quote locator folds them (NFC,
/// whitespace collapse, curly→straight quotes, dash unification, case) —
/// see [`crate::checks::quote_anchor`]. Empty draft or empty source is `Ok`
/// with zero counts (nothing to assess; the anchor gate handles missing
/// quotes separately).
#[must_use]
pub fn assess_paraphrase(draft: &str, source: &str) -> ParaphraseAssessment {
    let draft_tokens = fold_tokens(draft);
    let source_tokens = fold_tokens(source);

    if draft_tokens.is_empty() || source_tokens.is_empty() {
        return ParaphraseAssessment {
            verdict: ParaphraseVerdict::Ok,
            shared_shingles: 0,
            draft_shingles: 0,
            longest_common_run: 0,
            lcs_tokens: 0,
        };
    }

    let draft_shingles: std::collections::HashSet<&[String]> =
        draft_tokens.windows(SHINGLE_N).collect();
    let source_shingles: std::collections::HashSet<&[String]> =
        source_tokens.windows(SHINGLE_N).collect();
    let shared = draft_shingles.intersection(&source_shingles).count();
    let total = draft_shingles.len();
    let run = longest_common_run(&draft_tokens, &source_tokens);
    let lcs = lcs_len(&draft_tokens, &source_tokens);

    let verdict = if run >= TOO_CLOSE_RUN_WORDS
        || (shared * TOO_CLOSE_DEN >= total * TOO_CLOSE_NUM)
        || (lcs * TOO_CLOSE_LCS_DEN >= draft_tokens.len() * TOO_CLOSE_LCS_NUM)
    {
        ParaphraseVerdict::TooClose
    } else if (shared * NO_SUPPORT_DEN < total * NO_SUPPORT_NUM)
        && no_support_lcs(&draft_tokens, &source_tokens)
    {
        ParaphraseVerdict::NoSupport
    } else {
        ParaphraseVerdict::Ok
    };

    ParaphraseAssessment {
        verdict,
        shared_shingles: shared,
        draft_shingles: total,
        longest_common_run: run,
        lcs_tokens: lcs,
    }
}

/// Fold text into comparison tokens using the quote locator's normalization.
pub(crate) fn fold_tokens(text: &str) -> Vec<String> {
    crate::checks::quote_anchor::normalize_for_match(text)
        .split(' ')
        .map(super::quote_anchor::clean_token_pub)
        .filter(|token| !token.is_empty())
        .collect()
}

/// Content tokens: non-stopwords only.
fn content_tokens(tokens: &[String]) -> Vec<&String> {
    tokens
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// No-support LCS test: over content tokens only, below the
/// `NO_SUPPORT_LCS` ratio.
fn no_support_lcs(draft_tokens: &[String], source_tokens: &[String]) -> bool {
    let draft_content = content_tokens(draft_tokens);
    if draft_content.is_empty() {
        return false; // no content words: nothing to support-check
    }
    let source_content = content_tokens(source_tokens);
    let lcs = lcs_len(&draft_content, &source_content);
    lcs * NO_SUPPORT_LCS_DEN < draft_content.len() * NO_SUPPORT_LCS_NUM
}

/// Longest common subsequence length of two token sequences (order-
/// preserving overlap; quadratic DP with rolling rows). Generic over borrowed
/// or owned tokens compared by their string content.
fn lcs_len<T: AsRef<str>>(a: &[T], b: &[T]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            curr[j] = if a[i - 1].as_ref() == b[j - 1].as_ref() {
                prev[j - 1] + 1
            } else {
                prev[j].max(curr[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.fill(0);
    }
    prev[b.len()]
}

/// Longest contiguous run of identical tokens, in order, anywhere in both.
/// Quadratic worst case is acceptable: draft blocks are single paragraphs and
/// sources are bounded by ledger fetch caps.
fn longest_common_run(a: &[String], b: &[String]) -> usize {
    // Classic dynamic programming on runs; rolling rows to bound memory.
    let mut prev = vec![0usize; b.len() + 1];
    let mut curr = vec![0usize; b.len() + 1];
    let mut best = 0usize;
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            if a[i - 1] == b[j - 1] {
                let run = prev[j - 1] + 1;
                curr[j] = run;
                if run > best {
                    best = run;
                }
            } else {
                curr[j] = 0;
            }
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.fill(0);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::{ParaphraseVerdict, assess_paraphrase};

    const SOURCE: &str = "The Golden Gate Bridge was completed in 1937 after a decade of \
                          construction delays and cost overruns that strained municipal \
                          finances during the Depression.";

    #[test]
    fn ac3_near_verbatim_flags_too_close() {
        // Synonym substitution, same architecture — the canonical CLOP shape.
        let draft = "The Golden Gate Bridge was finished in 1937 following ten years of \
                     building setbacks and budget excesses that stretched city finances \
                     during the Depression.";
        let a = assess_paraphrase(draft, SOURCE);
        assert_eq!(a.verdict, ParaphraseVerdict::TooClose, "{a:?}");
    }

    #[test]
    fn ac3_clean_paraphrase_with_support_passes() {
        // Restructured: different sentence architecture, same anchored facts.
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

    #[test]
    fn verbatim_copy_is_too_close_via_run_length() {
        let draft = "The ledger records that the bridge was completed in 1937 after a decade of \
                     construction delays, and the overruns dominated later budgets.";
        let a = assess_paraphrase(draft, SOURCE);
        assert_eq!(a.verdict, ParaphraseVerdict::TooClose, "{a:?}");
        assert!(a.longest_common_run >= super::TOO_CLOSE_RUN_WORDS);
    }

    #[test]
    fn quoted_passage_is_still_too_close_this_gate_is_about_prose() {
        // The paraphrase gate measures prose against source; whether the text
        // is marked as a quotation is an authoring concern — the gate still
        // reports too-close so the author decides quote vs rewrite.
        let a = assess_paraphrase(
            "As the history puts it: completed in 1937 after a decade of construction \
             delays and cost overruns.",
            SOURCE,
        );
        assert_eq!(a.verdict, ParaphraseVerdict::TooClose, "{a:?}");
    }

    #[test]
    fn empty_draft_or_source_is_ok() {
        let a = assess_paraphrase("", SOURCE);
        assert_eq!(a.verdict, ParaphraseVerdict::Ok);
        let b = assess_paraphrase("Some prose.", "");
        assert_eq!(b.verdict, ParaphraseVerdict::Ok);
    }

    #[test]
    fn shared_entities_alone_stay_in_the_support_band() {
        // Names and dates shared, nothing else — must be Ok (this is what a
        // well-anchored sentence looks like), not no-support and not close.
        let draft = "Municipal budgets suffered elsewhere too in that period, and the \
                     bridge authority's bonds were refinanced twice before 1937.";
        let a = assess_paraphrase(draft, SOURCE);
        assert_eq!(a.verdict, ParaphraseVerdict::Ok, "{a:?}");
    }
}
