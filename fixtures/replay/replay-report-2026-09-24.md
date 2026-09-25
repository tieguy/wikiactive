# Replay soft report — 2026-09-24 (Phase 6.3)

Offline PLAYBOOK replay over the two frozen revisions (publishing disabled).
Mechanical findings come from `wa lint` (asserted by `tests/replay.rs`);
judgment findings below are model-surfaced, for **operator grading** against
the plan's expected-findings checklist — graded, never prose-matched
(training-data contamination caveat).

## Temple Fielding @ 1372827284

### Mechanical (asserted)

- `refname-autonumber`: 12 occurrences of VisualEditor-style `:N` auto ref
  names (`:0` ×3, `:1` ×5, `:2` ×4).
- `heading-spacing`: `==Publications==`, `==Bibliography==`,
  `==External links==` unspaced.
- `named-ref-with-pinpoint`, `italic-mismatch` ('Time' both plain and
  italicized), `semicolon-prose`.

### Judgment (graded)

- **Bare-URL ref / empty citation (L2)**: `<ref>An estimated 2.5 million
  were printed.</ref>` — a footnote with assertion text but **no citation
  at all**. Either find the source for the 2.5M print run (the Time 1969
  cover story?) or remove the number. This is the strongest V finding on
  the page.
- **Publications list formatting (Tier 3 + judgment)**: mixed title-case/
  capitalization across entries (''Fielding's Travel Guide to Europe:
  1954-55'' vs ''Fielding's travel guide to Europe 1967''); the Fort Bragg
  handbook's inclusion under "Publications" is defensible (he wrote it) but
  the ref style differs from the guide entries.
- **Bibliography section of one item (MOS:LAYOUT)**: a "Bibliography"
  section containing a single Time 1969 citation is arguably a
 Further-reading/References concern; consider merging into References or
  expanding with actual bibliography.
- **External links (NOT/ELNE)**: the single Time-cover link is directly
  relevant; fine to keep. No spamlink found at this revid.
- **Plan expectation check**: the plan's checklist mentions a
  "Nudgewise/see-also spamlink" — **not present at revid 1372827284** (no
  "Nudgewise" match; no See-also section at all). Either it landed in a
  later revision or the checklist's memory predates the freeze. Grading
  note for the operator: expected-finding miss, reason recorded.

## Commitment device @ 1343452323

### Mechanical (asserted)

- `heading-spacing`: 8 unspaced level-2 headings (whole article).
- `national-variety-mix`: 'honour' and 'honor' both appear (image caption
  `[[Honour|honor]]` vs body) — ENGVAR consistency needed.
- `see-also-duplication` (warn): [[precommitment]] linked in both body and
  See also.
- `named-ref-with-pinpoint` (the `|name=CD` JAMA ref carries page
  pinpoints; house style wants `{{rp}}` at use sites).
- `italic-mismatch` ('et al.' both forms), `semicolon-prose`.

### Judgment (graded)

- **RECENTISM / pop-culture UNDUE (L3)**: "Other examples" gives 2020
  (Alice Wu) and 2024 (Nathan Fielder, cited to a **YouTube re-upload**
  dated 2024, accessed 2025) roughly the weight of the game-theory
  literature discussion. The Fielder item's source is a Comedy Central
  Africa YouTube channel — weak sourcing for content weight; the primary
  broadcast source or coverage about the episode would be better if the
  example stays.
- **Tense/WIKIVOICE (Cluster A)**: "Behavioral economist Daniel Goldstein
  describes…" — Goldstein's TED talk is the sole source (tier: caution —
  lecture platform). The paragraph carries his argument in close to
  wikivoice across three sentences. Attribute tightly or condense.
- **Beggs blog in Further reading** (RS-TIERS: personal blog) — as
  further-reading (not content support) it's tolerable; flag for the
  operator's call.
- **Plan expectation check**: "game-theory/UNDUE absence" — game theory IS
  present (Overview mention; Other examples: honor/emotions per Arslan
  2011, Ross & Dumouchel 2004) though thin relative to the literature;
  "Robert Frank synthesis" — **no Robert Frank reference exists at this
  revid** (0 matches). Both expectations unverified at the freeze; grading
  note recorded.
- **Lead/body near-duplication (sub-threshold)**: exactly one shared
  6-gram ("with their backs to a river") — the Cortés/Han Xin story told
  in both lead and body in nearly the same words. Below the 8-gram
  duplication error by design (summary-style overlap is legitimate);
  recorded as an editorial tightening candidate, not a finding.

## Verdict for the operator

Mechanical: complete agreement between the deterministic checks and the
fixtures (see `tests/replay.rs`). Judgment: 3–4 gradeable findings per
article above, plus two plan-expectation misses with recorded reasons
(Nudgewise absent at freeze; Robert Frank absent at freeze).
