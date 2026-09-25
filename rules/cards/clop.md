# Card: CLOP — close paraphrasing is plagiarism with a thesaurus

**Canonical:** [[WP:CLOSEPARAPHRASE]] (`rules/canonical/close-paraphrasing.wikitext`)

## Operative clause

Adequate paraphrase means *restructuring the expression*, not synonym
substitution. If your sentence keeps the source's word order and only swaps
words, it is close paraphrase even with a citation — and it is copyright-risk
plus plagiarism. Quote verbatim (with quotation marks) or rewrite the
*sentence's architecture*: different clause order, different constructions,
your own connective tissue. The paraphrase gate (`checks/paraphrase.rs`)
measures shingle overlap both ways: too-close blocks render; no-support
blocks require anchor resolution.

## Load-bearing specifics

- Short verbatim quotes are fine *marked as quotes*; unmarked verbatim is not.
- Restructure + attribute: "Writing in the Guardian, X described …" makes
  the borrowed structure unnecessary.
- Public-domain sources may be copied verbatim but should still be
  restructured for encyclopedic tone.

## Failure examples

- Source: "The bridge was completed in 1937 after a decade of construction
  delays and cost overruns." Draft: "The bridge was finished in 1937
  following ten years of building setbacks and budget overruns." → same
  architecture, swapped words; gate flags too-close.
- Upstream example (CLOSEPARAPHRASE): synonym-swap paraphrase of
  "The company was found guilty of discrimination" → "The firm was judged
  culpable of prejudice" — flagged as close.
