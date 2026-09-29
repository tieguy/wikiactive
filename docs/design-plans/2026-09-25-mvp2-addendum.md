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

**Decision:** `assess_paraphrase` detects quotation-marked spans (≤ ~200
chars) in the DRAFT and defers entirely to the quote-anchor gate, which
enforces the quoted span verbatim in the fetched source. Unmarked
quote-like text still flags (the author must choose quote vs rewrite).
CLOP governs our own prose, not our citations-as-quotes.

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
- Disclosure-log entry template: "(assisted editing session)" dropped as
  redundant (a21780f, operator catch on the first live publish); extracted
  to `disclosure_entry()` and pinned by test so the B.4 publish-core
  refactor cannot silently change on-wiki wording (the upsert regenerates
  the entry on every publish — hand edits on-wiki are clobbered by design).

## Phase-A retrospective (2026-09-29, at v0.5.0)

Twelve-plus live review rounds on Sarah Kidder (3 published edits:
1377121505, 1377498415, 1377500073) plus the Temple Fielding session drove
real product hardening. Closing record:

### Paraphrase gate — live behavior

Three mechanisms earned their keep, each tuned or scoped on live evidence:
the LCS threshold (2/5 → 1/2, A.2.3: name-dense short sentences), the
attributed-quote deferral (A.3: canonical-phrase facts), and the
inherited-claim skip (pre-existing text gaining a citation is not ours to
paraphrase-check; operator MOS catch). A fourth live case surfaced at edit
3: the SF Call obituary is token-dense enough that *any* natural English
sentence about the marriage matches ≥½ of its tokens in order (9/15 for
the article's own pre-existing sentence shape — the overlap was proper
nouns, a middle initial folding to "a", and "in 1874", not expression
copying). Resolution: no threshold change, no article rewording — the
claim was recorded as the mixed prose+quotation block the edit actually
is (sentence + efn quoting True West), which is exactly the A.3 deferral;
both quotes still verbatim-verify. Operational lesson recorded below
(staging order).

### Review-surface evolution

Rounds 1–14 of the Kidder artifact trace the surface's growth: deletion
anchors on old-pane blocks (comments on removed text resolve); enwiki
link affordances; Word-style single-column diff with a right-hand
evidence rail; evidence card speaking reviewer language (internal ids →
DOM attributes only); verbatim quote promoted above prose with a
locator-verified header; clickable source/archive citations carrying the
citation text; consulted-source manifest with fetch status; per-edit
card scoping (stale published-edit cards archived); self-archived
sources labeled "this link is the archived snapshot". The lavish-axi
0.1.78 idle-timeout (30 min, then a user-ended session needs
`--reopen`) bit once mid-publish-review; `wa poll` keep-alive is the
documented interim answer, `wa serve` is the fix.

### The 1874 → 1870 → 1874 episode — the case for the source sweep

The article said 1874 (uncited since 2019). Edit 1 changed it to 1870
because the *only fetchable* source (archived True West profile) said
1870, and the gate (correctly) would not let uncited 1874 stand. The
operator's browser capture of the contemporary SF Call obituary — the
best source, unfetchable by tooling (CDNC bot-blocks) — then showed 1874,
and edit 3 restored it with the discrepancy footnoted. Net: one
unnecessary on-wiki round-trip caused by making source-access decisions
*during* analysis instead of before it. This is the direct case for
Phase B's **source sweep**: fetch-or-dispose every cited source up front,
classify accessibility (incl. operator capture where tooling fails), and
let content decisions be made once against the full accessibility
picture.

### Source-accessibility taxonomy (observed live)

- **dead-live with Wayback snapshot** — fetchable via the archived copy
  (True West 2007): fetch ✓, Earwig ✓.
- **WAF/bot-blocked** — CDNC: 403 for the fetcher *and* for Earwig;
  operator browser capture + `wa ledger attach` is the working path
  (text-first ledger; no replay machinery needed).
- **Paywalled** — The Union: subscribe boilerplate; needs operator
  capture or exclusion.
- **Lending/registration-gated** — Levinson via archive.org
  registration; NYT Times Machine PDF.
- **Print, no web text** — not observed in this session; the sweep
  auto-dispositions this class at inventory time.

### Driver latitude — pipeline vs tool-call

Phase A ran under a full-latitude driver (Polytoken+GLM with general
tool use). Observed: the latitude was almost never used for judgment
outside the three designed points (finding authorship, proposal
drafting, comment resolution) — the rest was orchestration the pipeline
owns. The two process errors of the session were both *latitude*
errors, not judgment errors: registering edit-3 claims before edit 2
published (poisoning edit 2's paraphrase gate — the gate correctly
assessed claims it could not know were premature), and choosing
sentence wording before checking it against the gate. Both argue for
the Phase-B pipeline's enforced ordering (sweep → findings → proposal →
gate → review → publish): same judgment points, less rope. Tool-call
fallback remains the pre-agreed escape hatch if the pipeline proves
rigid at B.6.

### Phase-B carry-over items

- `EarwigClient::compare` parses `result.verdict`/`result.ratio`, but
  the live copyvios API returns `best.violation`/`best.confidence`
  (observed 2026-09-29; the compare results above were obtained by
  calling the API directly). Re-record fixtures and fix the parse in
  Phase B.
- Claim sequencing is now PLAYBOOK material: register a claim only when
  staging the edit whose wikitext carries its prose.
- lavish `--reopen` UX wraps into `wa serve`'s session console.

### Plan-002 AC mapping (superseded by plan-003)

At v0.5.0: old AC.1–AC.5 done — their tests remain in-suite; old AC.4's
session-use clause was satisfied by the Kidder drift pin. Old AC.6/AC.9
fold into plan-003 AC.10 (live shakedown). New AC.2–AC.4 (sweep) are new
scope; new AC.5–AC.9 correspond to old AC.7–AC.11; new AC.11 ⊃ old
AC.13.

## Phase-B record (2026-09-29, toward 0.9.0)

### Pipeline decision record (B.1/B.2)

**Decision: a pipeline, not a tool-calling agent.** The model is called
at exactly three judgment points (`author_findings`,
`draft_proposal`, `resolve_comments` — `src/driver/steps.rs`); loop
control, the ledger, the gate, and the publish confirmation are
deterministic Rust. Supporting evidence: the Phase-A driver-latitude
assessment above (the session's two process errors were both latitude
errors), plus the milestone review's finding that the mechanical
guarantees (schema-validated admission, ledger-quote-id existence,
retry-once-then-block) are exactly what the pipeline encodes. The
corrective retry carries the rejected output as an assistant turn — a
stateless model must see what it got wrong. Tool-call fallback remains
the pre-agreed escape hatch, to be reconsidered only at a B.6-style
retrospective if the pipeline proves rigid.

### z.ai contract (B.0, fixtures/zai/)

The operator's key is a **Coding-Plan key**: the coding endpoint
(`https://api.z.ai/api/coding/paas/v4`) serves it; the standard base
returns HTTP 429 with terminal code 1113 ("no resource package") —
recognized and never retried. Wire shape is OpenAI chat completions with
`content` + a separate `reasoning_content` field; content arrives raw or
json-fenced (the parser handles a preamble before the fence). B.6
operator adjustments: the endpoint lives in `rules/house-rules.toml
[zai]` (fork config — "why am I doing shell exports?"), and `wa serve
--tsnet` binds the tailnet interface for thin-client operators (default
stays loopback-only; the LAN is never bound).

### lavish state.json note (B.4/B.6)

lavish-axi 0.1.78's state file (`~/.lavish-axi/state.json`) keeps
session URLs after the server idles out (30 min) or the user ends the
session — a stale URL presented as a live link was a B.6 operator catch
(the old smoke session). `wa serve` now probes before linking and
offers a one-click re-open (the serve-side wrap of `render --reopen`);
`wa poll` remains the keep-alive pattern for tty sessions.
