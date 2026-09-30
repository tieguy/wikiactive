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

## Plan-004 shakedown UX record (2026-09-30, P.5 in progress)

Session 1 (userspace page creation) published fully in-app: artifact →
comment UI → on-artifact publish → approval → disclosure-log upsert
(revid 1377614230). **Operator verdict: "still a lot of UX work to be
done but at least it all works now."** The verdicts that drove the
rework so far (each fixed the same day):

1. Session-page comment forms — "unusable": snippets not full text,
   unexplained two-field form, unstyled, "change ·" jargon. → Forms now
   live IN the served artifact, one per block, plain language.
2. Stale artifacts served as current (an old page-creation draft looked
   like the live review). → Round-log/mtime staleness guard; banner +
   form suppression + driver-resolve refusal.
3. Console "shows nothing useful". → Per-session review status
   (live/comments/done+why), attention-first sort.
4. Publish leg on a different page, named "approve" that didn't exist,
   greyed-out footer. → Publish + approval blocks on the artifact.

**Open UX debt (operator's call, not blocking P.5 sign-off):** general
polish and layout work across the artifact/session/console; whatever
surfaces in session 2's full comment → apply → re-render cycle.

### Overnight findings (plan-005 UI self-review, 2026-09-30)

Self-review on a realistic multi-block artifact: live **Laura de Force
Gordon** wikitext (revid 1336672250) with a scratch edit in the "Modern
speculation" section — one changed pair (rewritten Maupin sentence, the
link- and ref-rich block), one pure deletion (the flyleaf sentence), one
finding with a hand-written ledger quote (evidence card). Rendered via
cached Parsoid transforms; served from a scratch CWD with BOTH z.ai and
the wiki pointed at loopback mocks (publish walk decline-only, mock log
confirmed the override — zero edit POSTs). Every console state (live /
stale-published / stale-comments-applied / stale-text-changed / none),
comment queue, manual + driver resolve, splice, re-render, publish
approval block, and decline were exercised. Fixed the same night:

1. **Evidence card nested paragraphs** — the guidance line was
   `<p class="rules"><p class="rules">…</p></p>` (invalid HTML; browsers
   close the outer `<p>` early). Fixed in `evidence_card`; pinned
   (single rules paragraph, no nesting).
2. **Revisions registry duplicated on re-render** — re-rendering the
   SAME round appended a second registry entry ("Round 2" twice).
   `registry_with_round` now replaces the round-keyed entry; pinned
   (replace-on-rerender, append-on-new).
3. **tty wording leaked into the web confirmation** — the shared publish
   prompt said "This tty confirmation is the FINAL gate … when you type
   yes" on the session page and the artifact approval block. Reworded
   confirm-source-neutral ("This confirmation is … when you approve");
   the tty bracket hint `[type yes to publish]` still carries the typing
   instruction on the tty path. Pinned on the web pending page.
4. **OSC-8 escapes in the web serve log** (plan O.2) — the post-publish
   `check it:`/`session log updated:` prints emitted terminal links on
   the web path. Now Via-aware: tty keeps the clickable link, web logs
   the plain URL. Pinned (no ESC byte in the web log, plain permalink
   present).

CSS-only (no pins, per plan): `.wa-bar` and `.wa-resolve` gained
`flex-wrap` and session-page inputs/textarea `max-width: 100%` — the
~360px static-review pass found the top bar and resolve rows could
overflow horizontally; actual thin-client verification stays the
operator's morning pass (unattended agents cannot see pixels).

Test hardening shipped with the above (plan O.3): three-group
driver-resolve splice (two disjoint replacements + one pure insertion,
exact `proposed.wikitext` pinned), console `rank` extracted to module
level and unit-pinned (live+comments → live → history), and a
multi-block `block_insertions` position test (each form directly after
its own block's `</div>`, including the link-rich block whose old-side
`<del>` follows a `<span class="wl">` — the `</span>` truncation class).

**Deferred to the morning list (taste/product, not fixed):** the session
page's `Publish` h2 wraps the driver-findings/propose and render forms
too (heading mislabels the section); "archive: (archive pending)"
phrasing on the evidence card for never-archived sources; the fetch-
status icons (still operator-deferred from 2026-09-27). Note: the scratch
walk's round-2 re-render initially showed a stale Maupin sentence — that
was a stale HTML *fixture* passed to the offline render form (the test
surface), not an app defect: production renders transform
`proposed.wikitext` live. One extra Parsoid transform was spent
correcting it (4 total vs the planned ≤2 — read-only, recorded here).

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

## B.6 shakedown verdict + the plan-004 decision (2026-09-29, operator's call)

**Verdict: the lavish-interlinked loop was not usable.** Every failure in
the B.6 live shakedown lived in the lavish interlink — never in the
artifact, the gate, the ledger, or the driver steps:

- **Idle death**: lavish 0.1.78 idles out after 30 min; the review leg
  silently expired mid-session.
- **Ended-session links**: the state file keeps URLs of ended sessions;
  the session page linked a dead URL as if live (probe + re-open was a
  patch, not a fix).
- **Wrong-session polls**: `wa poll` resolved against whatever session
  the state file last recorded — comments from another article's review
  surfaced in the wrong loop.
- **"Agent not listening"**: the push model required the agent to be
  long-polling at the moment the operator commented; the review was
  gated on the tool's lifecycle, not the operator's.

**Decision (plan-004, operator's call): cut lavish out of the default
loop.** The review leg becomes in-app: block-anchored comments on the
session page, persisted to a session-local queue
(`sessions/<slug>/comments.jsonl`), resolved by a driver action that
groups comments per changed block (both sides of a pair in ONE model
call), splices the revised blocks, and writes applied/rejected/reply
notes back to the queue. The whole loop — sweep → findings → proposal →
render → review comments → resolution → publish approval — runs in ONE
process (`wa serve`), zero external server lifecycles, no Node.

Trade-off accepted: comment fidelity drops from text-selection to block
anchors + a free-text "words you mean" field (mitigated by resolution
notes quoting the acted-on span) — acceptable for a single-reviewer
tool. `wa render`'s lavish open and `wa poll` remain working legacy CLI
commands (tests intact); the documented loop and the app UI no longer
touch them.

What survived from B.6 (prerequisites, not waste): the publish-
confirmation wait, gate-in-web, the in-app artifact link, config-based
z.ai, and `--tsnet`.

## Plan-005 overnight record (2026-09-30, operator asleep)

Unattended work per plan-005: UI self-review on a realistic multi-block
artifact (both z.ai and the wiki mocked, loopback only), small-nit fixes,
test hardening, v0.9.0 release prep, wm-fetch lint, and three read-only
research items. Hard guardrails held: no wiki/Wikidata writes of any
kind; no session-2 prep under the live `sessions/`; no z.ai calls; the
running `wa-serve` service untouched until the single gated final
restart; conservative UI changes only.

### O.0 — Baseline snapshot (night's first action, 2026-09-30T05:19:30Z)

- **Wiki boundary (read-only queries via wm-fetch):** disclosure log
  `User:LuisVilla/wikiactive/log` at revid **1377614233**
  (2026-09-30T04:52:45Z); top contributions exactly the session-1 pair —
  1377614233 (log upsert) and 1377614230 (publish to
  `User:LuisVilla/wikiactive/shakedown`) — nothing newer. Matches the
  plan's stated boundary verbatim.
- **`sessions/` manifest:** 51 files across 7 session dirs
  (`sarah-kidder`, `temple-fielding`, `user-luisvilla-wikiactive`,
  `-shakedown`, `-shakedown-b6`, `-smoke`). Manifest digest (sha256 of
  the sorted `find sessions -type f | sort | xargs sha256sum` output):
  `682ac42e4b32302c5e45df4452200b0d701fbcd3c945d9274e52240707c64203`.
  Reproduce with that command and compare digests (recorded at teardown
  as AC.3).

### O.1 defect log — see "Overnight findings" appended to the Plan-004
shakedown UX record above (this section holds the audit trail; the UX
record holds the findings).

### O.6b — Wikidata marriage-date prep (L5 design note; NO writes made)

Read-only investigation (wbsearchentities/wbgetentities, 2026-09-30):

- **Sarah Kidder = Q7422487**; her **P26 (spouse) = John Flint Kidder
  (Q6233465)** statement exists with **no qualifiers and no
  references** (bare). The reverse statement on Q6233465 (P26 → Q7422487)
  is equally bare. Her P569 (1839) carries only a P143 "imported from
  enwiki" reference — the item is essentially unsourced.
- The article (post edit 3) reads "married civil engineer John Flint
  Kidder in 1874", with the True West 1870 reading footnoted as a
  discrepancy. **The qualifier value is 1874** (contemporary SF Call
  obituary, the better source per the Phase-A episode); True West's 1870
  must NOT be cited as support for 1874.

**Ready-to-apply morning checklist** (apply WITH the operator; requires
an EditData token + `assert=user`; nothing below was executed tonight):

1. Fetch the statement id: `wbgetentities ids=Q7422487 props=claims` →
   the P26 statement id (`Q7422487$<hash>`).
2. Add the marriage-year qualifier (precision 9, year only — the
   sources give no month/day):
   `wbsetqualifier claim=<statement-id> property=P580 snak-type=value
   value={"time":"+00000001874-01-01T00:00:00Z","precision":9,
   "calendarmodel":"http://www.wikidata.org/entity/Q1985727"}`
   with a reference on the statement:
   `P854` (reference URL) =
   `https://cdnc.ucr.edu/?a=d&d=SFC19010411.2.43` (the ledger's S2, the
   contemporary obituary) + `P813` (retrieved) = apply date. Optionally
   also `P248` (stated in) = a suitable item for the San Francisco Call
   if the operator wants a bibliographic anchor, and the S1 True West
   snapshot URL as a SECOND reference carrying the conflicting 1870 —
   operator's call whether to cite the conflict on-wiki or keep it to
   the efn.
3. Mirror the same P580 qualifier on Q6233465's P26 statement (symmetry;
   the marriage date is a property of the union, both directions
   should carry it).
4. The ledger's provenance chain (claims → quotes → sources) is exactly
   the L5 hook `PLAYBOOK` anticipates: `wa`-side, a future
   `wa wikidata` command would read it to build these payloads
   mechanically. Designing that command is post-0.9.0 work; tonight's
   payloads are the schema sketch.

### O.6c — SonarQube probe: recorded deferral

`bws secret list` (keys only, values never read): 17 secrets, **no
SonarQube/sonarcloud token** present (closest are unrelated API keys;
the wiki OAuth material the repo already uses is there). With no token
and an undetermined host (sonarcloud.io vs self-hosted), no
token-authenticated API check is possible. Deferral passes AC.5's
environmental-deferral clause: to unblock, the operator adds a token
(e.g. `SONAR_TOKEN`) to bws and names the host; morning-list item 7
(analysis upload / scanner run) stays gated on that anyway per the
plan's no-external-writes guardrail.

### O.6d — "Claude Code UI review function": honestly not available

No browser or screenshot tooling is exposed in this harness, so no
agent-side visual review of the served artifact was possible beyond the
static CSS review recorded above. If the operator wants a Claude-Code
side review, run it against the served artifact URL directly (the
console lists every session; the artifact route is
`/sessions/<slug>/review`). Morning-list item.

### O.5 — wm-fetch: clean lint + green suite at the final cutover state

Scope note: the repo was rewritten to Rust **v2.0.0** during this
session's evening (a07e8e8, 22:41 local) and then **cut over** while
tonight's work ran (690b580, 22:50 local: the v1 bash script and
`tests/smoke.sh` retired from the tree, symlink re-pointed to
`target/release/wm-fetch`, contact config created). Verified at HEAD:

- **shellcheck 0.11.0** (throwaway venv `shellcheck-py`; no system
  installs): zero findings on the only tracked shell script,
  `install.sh` — and zero findings had the now-retired bash `wm-fetch` /
  `tests/smoke.sh` when linted at a07e8e8 mid-cutover. Nothing to fix
  or waive; no version bump (nothing changed by tonight).
- **`cargo test`**: 30 unit + 25 wiremock integration green; the 3 live
  tests correctly ignored (need `WM_FETCH_LIVE_CONTACT`).
- **Live legs through the installed v2.0.0 binary**: the AC.3/AC.4
  teardown queries (disclosure-log revid + usercontribs) round-tripped
  cleanly with pacing honored — the modern equivalent of the retired
  smoke test's live legs.
- **Symlink**: resolves to the release binary; config present. No
  morning action needed.


