# MVP-2 addendum — decisions and records (2026-09-25 →)

Running record for the MVP-2 (0.9) work: Phase A proves the L2/L4 content
loops live with the current Polytoken+GLM session driver; Phase B ports the
loop to `wa serve` (axum, loopback-only) with a direct z.ai (GLM) driver.
The MVP-1 design plan (`2026-09-24-mvp1-structured-loop.md`) remains the
base reference; this file records post-MVP-1 decisions.

## A.1.2 — Review-surface link affordances (decided 2026-09-25)

Operator backlog note (2026-09-25): the dotted-underline wikilink marker
was an MVP expedient and read like many systems' misspelling marks; use the
wiki's own link affordances as the reference styling.

**Decision: option (b) — vendor a pinned snapshot of the deployed enwiki
(Vector 2022) link-affordance rules** into the renderer
(`vendor/enwiki-link-affordances.css`, inlined into the artifact's style
block, checksum-pinned by test, provenance in `vendor/PROVENANCE.md`).

Values snapshot from the live deployed CSS
(`en.wikipedia.org/w/load.php?…modules=skins.vector.styles&skin=vector-2022`,
2026-09-25): progressive `#36c`, hover `#3056a9` + underline, active
`#233566` + underline, visited `#6a60b0`, no underline at rest; redlink
family `#bf3c2c`/`#9f5555`/`#9f3526`/`#612419` (included for the future
page-existence case). The Vector external-link icon is omitted (externals
keep the progressive color, as on enwiki).

Alternatives evaluated and rejected:

- **(a) Link enwiki ResourceLoader CSS into the artifact** — rejected: the
  artifact is static and must render offline (fixtures/golden tests,
  re-review of saved sessions); load.php output is an unpinned moving
  target; and a remote <link> makes the review record network-dependent.
- **(c) Reuse VisualEditor components (diff machinery / link rendering)** —
  rejected: VE's machinery is a ResourceLoader/OO.ui runtime for the
  editing surface, with no static-artifact packaging path; the artifact is
  built from Parsoid HTML + our own diff, and only the *affordance values*
  needed to match. Re-evaluate only if the artifact ever becomes an
  interactive editing surface.

License note: the snapshot derives from GPL-2.0-or-later sources
(mediawiki/core `content.links.less`, Vector, Codex tokens); one-way
compatible with this repo's GPL-3.0-only.

## A.2.3 — Paraphrase LCS threshold tuned on live evidence (2026-09-25)

The first live L2 session (Sarah Kidder) produced a clean paraphrase —
0/14 shingles shared, longest run 3 words — that the gate blocked on the
LCS signal alone: 8/17 tokens in order, of which 7 were unavoidable proper
nouns and the date (Kidder, John, in, 1870, Grass, Valley, California).
Short factual sentences about a named subject are proper-noun-dense by
nature; at 40% the LCS check was measuring name overlap, not expression
overlap.

**Decision: `too_close_lcs` 2/5 (40%) → 1/2 (50%).** The shingle and
verbatim-run signals (unchanged) carry the close-paraphrase load — every
unit-test true positive still trips on run/shingles, not LCS. Both
`ParaphraseConfig::default()` and `rules/paraphrase.toml` updated together
(enforced by `rules_paraphrase_toml_matches_default_thresholds`).

## A.3 — Attributed quotes exempt from the paraphrase gate (2026-09-27)

The Sarah Kidder lead's superlative ("first female railroad president in
the world") is a canonical-phrase fact: every faithful paraphrase shares
most tokens with the source's phrasing ("the first woman in the world to
ever head a railroad"), so the gate blocked all honest wordings — while
the *correct* encyclopedia form is an attributed, quotation-marked short
quote with the citation.

**Decision:** `assess_paraphrase` strips quotation-marked spans (≤ ~200
chars) from the DRAFT side before assessment; unmarked quote-like text
still flags (the author must choose quote vs rewrite), and when a marked
quote is present the no-support leg is skipped — the quote-anchor gate
separately verifies the quoted span verbatim in the fetched source. CLOP
governs our own prose, not our citations-as-quotes.

## Review-surface backlog (operator, 2026-09-27, live L2 session)

- Fetch-status icons for sources ("fetched + relied on" / "fetched but not
  relied on" / "fetch failed") — deferred at operator request; the card
  currently uses placement (Source: vs Also consulted:) + a text status.

## Housekeeping

- The MVP-1 design plan's repo-layout block originally annotated LICENSE as
  "GPL-3.0-or-later"; corrected to **GPL-3.0-only** to match `Cargo.toml`
  and `CLAUDE.md` (pre-existing inconsistency, fixed 2026-09-25).
- Temple Fielding round 3 (MOS:TITLES de-italicization) was rejected in
  review ("the original form is correct — they bought the *title*") and is
  **abandoned**; the session is closed with edits 1–2 published (operator
  call, 2026-09-25). Nothing owed on-wiki (TALK note and disclosure log
  cover the published edits).
