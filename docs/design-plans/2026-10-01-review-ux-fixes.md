# Review-UX Fixes Design (reject, process-comments, cannot-verify)

## Summary

The 2026-10-01 Alvin Gouldner smoke test drove the full loop-mechanization surface end-to-end on a live article and surfaced three findings from the operator's review of the artifact (recorded as backlog items 4–6 in `docs/playbook-enforcement.md`, commit `355429d`). First, the review page offers comment-or-publish with no way to reject the staged edit — the operator killed a wrong proposal by hand (restoring `proposed.wikitext` to base and appending an `aborted` round entry), proving the mechanics but exposing the missing affordance. Second, the control that sends comments to the drafting model is labeled "Apply comments" (it processes them — the model may apply, reject with reasons, or reply) and lives in the top status bar, far from the comment fields it acts on. Third, the staged edit was wrong in a way the prompt permitted: with 18 of 31 cited sources unreachable, the model converted "the fetched evidence nowhere mentions it" into a deletion proposal against a sentence the operator knows to be verifiable from paywalled sources — absence in reachable evidence was treated as unsupportedness.

This plan adds the missing affordances and closes the judgment gap: a Reject-this-edit control at review (restore-to-base + `aborted` round entry + artifact staleness), an honest relabel and relocation of the comments control ("Process comments", placed with the queue), and an assess-prompt craft rule — carried with a fetch-manifest summary line in the model's context — that flags "cannot verify from available sources" instead of proposing content removal. One phase, full gate, playbook pairing included.

## Definition of Done

> **Formal statement.** The review surface makes both operator decisions explicit and mechanically safe, and the assess judgment point treats evidence-absence as unverifiability:
>
> 1. **Rejection is a first-class operator act with the same safety guarantees as publication.** A single control on the review artifact (and the session page) rejects the staged edit. A rejection: restores `proposed.wikitext` to the base text; appends a round-log entry with phase `aborted` recording the rejection; declines every pending publish confirmation for the session; and marks the previous artifact stale under a rejection-specific banner that offers no re-render. After a rejection, no code path — CLI or serve — can write the rejected text to Wikipedia.
> 2. **The comment-resolution control states and sits where it acts.** It is labeled "Process comments" with wording that the drafting model revises each commented block and may decline with reasons; it renders at the point of the comments it acts on (end of the review diff; the session page's Comments section), not in the top status bar.
> 3. **Evidence-absence is unverifiability, not refutation.** When the fetch manifest shows cited sources without fetched text, the assess step may flag a claim as unverifiable from available sources, and must not propose deleting or altering cited content on the ground that the fetched evidence does not mention it. The rule is carried by the pinned assess prompt, together with the fetch-manifest summary the model needs to apply it.
>
> Provenance (operator's review of the Gouldner artifact, 2026-10-01): "It's not clear how to say 'nope, don't do this'"; "the 'apply comment' button is (1) wrong - it isn't 'apply', it is 'process' - that should go back to the model with context"; "(2) is not near or below the comment fields, so easy to miss"; "this edit simply isn't right, I think because of the missing sources."

## Acceptance Criteria

Scoped IDs `revux.AC{N}.{M}`. All criteria are observable by offline tests (unit or httpmock; no live calls), except the records criteria in AC4, verified by inspection.

### AC1 — Reject the staged edit, from the review page

- `revux.AC1.1` `POST /sessions/{slug}/reject` on a session with a staged edit (proposed ≠ base) first declines every pending publish approval for the session (parked confirmations vanish — approving a stale rendered block does nothing), then restores `proposed.wikitext` to the base text, appends a round-log entry with phase `aborted` whose summary records the operator rejection, and reports the outcome on the session page.
- `revux.AC1.2` After a reject, the prior artifact is stale with a **rejected** banner (new `StaleKind::Rejected`: names the operator rejection, offers no re-render) — not the hand-edit wording. The AC1.2 test pins the banner kind AND that `rounds.jsonl` carries the `aborted` entry — this test is also the pin that an `aborted` phase is inert to the published/comments-resolved staleness legs (no existing test pins that today).
- `revux.AC1.3` A **Reject this edit** control renders adjacent to the publish form on the review page and in the session page's Review section; the comments queue and admitted assessments are untouched by a reject. The handler honors `?from=review` via the shared `back_to`, like every sibling POST.
- `revux.AC1.4` Rejecting when nothing is staged (proposed == base) is a visible no-op — outcome "nothing staged to reject", no round entry, files untouched.
- `revux.AC1.5` After a reject, publish refuses: `run_publish` (serve) and `publish_core` (CLI) gain an explicit up-front refusal when proposed == base — "nothing staged to publish" — pinned at both surfaces. (Today no such refusal exists: base-identical is the gate-clean state and publish would otherwise proceed to confirmation and an approved null edit.)
- `revux.AC1.6` The race is closed end-to-end: publish → (confirmation parks) → reject → approve the still-rendered confirmation → **no edit request reaches the wiki** (httpmock wiki, edit call count 0; the declined flow's outcome surfaces).

### AC2 — The comments control is honest and where the comments are

- `revux.AC2.1` The control is labeled **Process comments** (with a short meta line that the drafting model revises each commented block and may decline with reasons); the string "Apply comments" no longer appears on rendered pages, in PLAYBOOK.md, in README.md, or in the `src/rules.rs` doc comment that names the control — archival `docs/design-plans/` records are explicitly out of scope (history is not edited).
- `revux.AC2.2` On the review page the control renders at the end of the diff — after the last block's comment thread, immediately above the publish section — and on the session page within the Comments section; the top status bar keeps at most the open-count text, not the button.

### AC3 — Cannot-verify instead of deletion-on-absence

- `revux.AC3.1` `prompts/assess.md` gains a craft rule: when the fetch summary shows unreachable/dispositioned sources, NEVER propose deleting or altering cited content on the ground that the fetched evidence does not mention it — such a claim is flagged in `factual_note` as `cannot verify from available sources: <claim>` with `proposed_fix` stating the verification need (e.g. "needs a source in reach or an operator capture"), not a content change; checksum re-pinned with reason in the commit message.
- `revux.AC3.2` The assess step's context carries a fetch-manifest summary line built from the ledger — predicate: `sources` entries with `fetched_text` **absent** count as without-text — pinned at BOTH layers: a driver-step test (hand-built ctx) asserts the summary line and the craft rule's marker text ride the sent prompt, and a serve-layer test (httpmock zai, ledger fixture with N sources / M without text) asserts the exact counts ("N total, M without fetched text") ride the request body — the serve test is what pins the construction in `run_driver_assess`, where the counting actually happens.
- `revux.AC3.3` The serve Assess action and the step's validation are unchanged otherwise (schema, quote-existence, entry checks; existing tests stay green).

### AC4 — Records (playbook pairing)

- `revux.AC4.1` PLAYBOOK.md: the Review stage documents the reject affordance (explicit rejection mirrors explicit publish confirmation); the Resolve stage wording says Process comments.
- `revux.AC4.2` `docs/playbook-enforcement.md`: ENFORCED rows for the reject control and the process-comments control; the assess cannot-verify rule recorded as prompt-carried (enforced by the checksum pin + the prompt-content test, per ADR-0002's layering); backlog items 4, 5, and 6 removed as resolved/absorbed.
- `revux.AC4.3` No stale "Apply comments" or missing-reject references remain in README.md (the loop-flow narrative line "the comment → apply → re-render cycle"; the staleness-kind list, which gains `rejected`) or in the `src/serve.rs` doc comments (the "apply-comments action" comment and the flow comment "read → comment → apply → re-render → publish → approve"). Grep is hyphen/case-tolerant (`apply[- ]comments` / `apply` in flow phrases); archival `docs/design-plans/` records remain out of scope.

## Glossary

- **Reject (the edit)** — the operator's explicit "no" at review: restore `proposed.wikitext` to the base text, append an `aborted` round-log entry, stale the artifact. Assessments and the comment queue survive (they may seed the next proposal).
- **Process comments** — the resolve judgment point's control: the open queue entries go to the drafting model grouped per changed block; it revises, may decline with reasons, and replies. "Apply" overstated it.
- **Cannot-verify flag** — the assess prompt's honest output for a claim that neither the fetched evidence nor the article's other in-reach material settles, when the fetch manifest shows unreachable sources: a flag for the operator, not a content change.
- **Fetch summary** — one line in the assess context: total cited sources and how many are without fetched text (unreachable, dispositioned, or no-text).
- **`aborted`** — the pre-existing round-log phase for a rejected round (session.rs `RoundEntry` doc; inert to `artifact_state`).

## Architecture

### Reject (serve)

```
POST /sessions/{slug}/reject   (honors ?from=review via back_to, like siblings)
  known_session? → 404
  for (id, …) in state.pending_for(&slug): state.resolve(&id, false)
      // decline parked publish confirmations FIRST, in both branches:
      // publish_core reads proposed.wikitext before parking on WebConfirm,
      // so a still-rendered Approve must find nothing to approve
      // (memory-state only — touches no file, so the no-op branch's
      // "files untouched" holds)
  base := read base.wikitext; proposed := read proposed.wikitext
  base == proposed → outcome "nothing staged to reject", redirect (no file change)
  else:
    fsio::write(proposed.wikitext, base)          // restore
    round entry { round: <last rendered round or 0>, phase: "aborted",
                  summary: "edit rejected by operator at review",
                  detail: ["proposed.wikitext restored to base"] }
    fsio::append_line(rounds.jsonl, …)
    outcome "edit rejected — proposed restored to base; the review is out of date"
```

Staleness: `artifact_state` gains a `StaleKind::Rejected` arm — an `aborted`
entry after the last render maps to it (before the published/comments-resolved
checks), and the mtime leg (restore rewrites `proposed.wikitext`, so its mtime
exceeds the artifact's) remains the fallback. The rejected banner names the
operator rejection and points back to the session page — no re-render button
(re-rendering a base-identical pair is pointless). `next_round` maps
`Rejected` to round + 1 (the rejected round is done; the NEXT edit starts a
fresh round). The reject control renders inside the publish block on the
review page (`inject_comment_ui`'s `wa-publish` section, a secondary button
beside `publish_form`) and in the session page's `review_section` under the
Audit form.

Publish refusal: `run_publish` (serve) and `publish_core` (CLI) both gain an
up-front `proposed == base` refusal ("nothing staged to publish") — closing
AC1.5 at both surfaces rather than relying on the wiki layer's null-edit
outcome.

### Process comments (serve)

`comments_bar` (the `wa-bar` at the top of the review) keeps only the
open-count sentence. The form — relabeled **Process comments**, meta line
"the drafting model revises each commented block; it may decline with
reasons" — moves into the `wa-publish` section's head on the review page
(end of the diff, where the operator lands after commenting) and into the
session page's Comments section. Same route (`/driver/resolve`), same
handler; only rendering and label change.

### Cannot-verify (prompt + context)

- `src/driver/steps.rs`: `AssessContext` gains `fetch_summary: String`
  ("cited sources: 31 total, 18 without fetched text (unreachable or
  dispositioned) — verify what you can, flag what you cannot"); the step's
  user message gains the line. The serve Assess action builds it from the
  ledger (counts; no new ledger API — iterate `sources` filtering
  `fetched_text`).
- `prompts/assess.md` craft rule (AC3.1 wording); `src/driver/prompts.rs`
  pin updated; `tests/driver_steps.rs` matcher asserts the rule text and
  the summary line ride the prompt.

### Contracts

The reject handler follows the publish-handler shape (`State`, `Path`,
`known_session`, outcome, redirect) — a new route
`.route("/sessions/{slug}/reject", post(reject))`. No ledger, gate, or
prompt-schema changes beyond the additions above.

## Existing Patterns

- **Handler shape** — `src/serve.rs` `publish`/`audit` handlers: known_session gate, `note_outcome`, redirect; the reject handler copies the pattern.
- **Round entries + phases** — `fsio::append_line` + `RoundEntry` (phase strings free-form; `aborted` already documented in `src/session.rs`). No existing test pins that `artifact_state` ignores the `aborted` phase — the AC1.2 test becomes that pin.
- **Staleness by mtime** — `artifact_state`'s `TextChanged` (serve.rs L882+): the manual reject on sessions/alvin-gouldner proved restore-to-base trips the mtime leg (strict `>` on mtimes — the AC1.2 test renders before rejecting, reusing the `mtime_gap` pattern if needed).
- **Prompt discipline** — checksum pins (`prompt_checksums_are_pinned`, `src/driver/prompts.rs`); the assess pin was last updated for the AS-schema rename (loop-mechanization Phase 1). Prompt-carried rules are the ADR-0002 layer for drafted-output rules.
- **Context slots** — `AssessContext` fields render into the step's user message (`max_assessments`, `quote_ids`); `fetch_summary` follows the same shape. The step's text-digest filter at serve.rs ~L1783 uses `fetched_text.is_some()` — the summary's without-text predicate is its complement.
- **Serve test harness** — `spawn_serve` (env-stripped), `audit_offline`, `reap_child`, httpmock zai override; the review-page assertions follow the existing page-grep style.
- **Divergence:** none — this extends the surfaces the loop-mechanization plan built.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1 — Reject control, Process-comments placement, cannot-verify prompt

**Goal:** the operator can say "no" at review with one click; the comments control is honest and adjacent to the comments; the assess step flags unverifiable claims instead of proposing their removal.

**Components:** `src/serve.rs` (reject handler + route; decline-pending step; reject button in `inject_comment_ui`'s publish section and `review_section`; `StaleKind::Rejected` + `artifact_state`/`next_round` arms; Process-comments label + relocation; the `run_publish` nothing-staged refusal; the fetch-summary construction in the Assess action), `src/cli.rs` (`publish_core` nothing-staged refusal), `src/driver/steps.rs` (`AssessContext.fetch_summary` + user-message line), `src/serve.rs` Assess action (build the summary from the ledger), `prompts/assess.md` + `src/driver/prompts.rs` (craft rule, re-pin), `src/rules.rs` (doc-comment wording for the control's new name), `PLAYBOOK.md`, `docs/playbook-enforcement.md`, `README.md` (grep), `tests/serve.rs`, `tests/driver_steps.rs`, `tests/driver_e2e.rs` (AssessContext constructor), `tests/publish.rs` (nothing-staged refusal pin).

**Dependencies:** none (lands on `master` at `355429d` or later).

**ACs covered:** `revux.AC1.1`–`AC1.6`, `revux.AC2.1`–`AC2.2`, `revux.AC3.1`–`AC3.3`, `revux.AC4.1`–`revux.AC4.3`.

**Done when:** every listed AC case has a failing-test-first pin that passes; a fresh `cargo fmt`/`cargo clippy --all-targets`/`cargo test` run is green; the prompt pin is updated with reason; the stale-surface greps are clean; `sonar analyze secrets` before each commit.
<!-- END_PHASE_1 -->

## Execution Contract

- Implementation detail lives in `docs/implementation-plans/2026-10-01-review-ux-fixes/phase_01.md`, written just-in-time: re-verify current codebase state for the phase before writing its tasks (investigate, then write definitive instructions).
- Tasks are bite-sized (one action each): write failing test / run it / implement minimally / run tests / commit. Every functionality task names the AC cases it verifies (`Verifies: revux.AC2.1`). Test behavior, not implementation.
- Before declaring the phase done: run `cargo fmt`, `cargo clippy --all-targets`, `cargo test` fresh and read the output — evidence before claims, always.
- Prompt files are checksum-pinned; any prompt edit updates the pin and says why in the commit message.
- After the final phase: run the `code-reviewer` subagent and fix or rebut every finding until zero Critical/Important remain; then run `test-analyst` to validate AC coverage; then run a `playbook-audit` pass confirming PLAYBOOK.md, the enforcement inventory, and the code agree (no stale Apply-comments wording, reject rows present, backlog items 4–6 cleared) — serve changes are loop behavior and the pairing rule requires it. SonarQube: `sonar analyze secrets` before any commit; the reviewer gate reads `sonar list issues --new-code` after CI analysis (BLOCKER/HIGH = must fix).
- Wikiactive invariants hold: content changes flow through the ledger and the gate; `USER_AGENT` only in `src/lib.rs`; `fixtures/` re-recorded, never hand-edited; `sessions/` out of git; live model calls only via the pinned-prompt steps; no live Wikipedia calls from tests.

## Additional Considerations

- **What a reject does not do:** it does not delete admitted assessments or the comment queue — the operator's Gouldner review kept AS2/AS3 alive for a future round after rejecting AS1's edit, and open comments may inform the next proposal. If queue-clearing on reject is ever wanted, that is a new decision.
- **The prompt rule's enforcement ceiling:** the craft rule is carried by the pinned prompt and its presence is test-asserted, but whether the model obeys it is judgment (ADR-0002 layering: prompts carry drafted-output rules; ADR-0005's detection-vs-enforcement split — the flag is judgment surfaced, not a gate reason; the gate still catches mechanically-checkable failures). The inventory row words it as prompt-carried, not gate-enforced.
- **A reject bumps `proposed.wikitext`'s mtime**, so the assess entry checks will refuse the next Assess with StaleAnalyze until `wa analyze` re-runs — by design (the text did change) and the refusal names it; worth knowing at the console.
- **AC1.2's mtime ordering:** `artifact_state` uses strict `>`; the test renders the artifact before POSTing reject (the `mtime_gap` pattern if the clock is too close).
- **`fetch_summary` vs backlog item 3:** the one-line summary is deliberately minimal — it does not attempt the quote-extraction judgment point, the defect-candidate flow into assess, or per-source text bounds (backlog item 3 remains the fuller fix).
- **Cost:** one additional line of prompt tokens per Assess call; the diagnosis-pass economics are unchanged.
