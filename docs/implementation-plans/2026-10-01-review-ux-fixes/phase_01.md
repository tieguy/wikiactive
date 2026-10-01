# Phase 1 — Reject control, Process-comments placement, cannot-verify prompt

Plan: `docs/design-plans/2026-10-01-review-ux-fixes.md`. Code state
re-verified 2026-10-01 (all anchors below read, post-`355429d`):

- `ServeState::resolve(&id, approve)` serve.rs L128; `pending_for(&slug)` L168.
- `back_to` L299; `StaleKind` L842 (three arms + `short`/`sentence`);
  `next_round` L876 (TextChanged ⇒ same round; wildcard ⇒ +1);
  `artifact_state` L890 (rendered rposition, then published /
  comments-resolved checks, then strict-`>` mtime leg).
- `stale_banner` L1087 (renders kind.sentence + a re-render form);
  serve.rs L786 flow comment; L1107 "apply-comments action" comment.
- `comments_bar` L1682 — the "Apply comments" primary button (L1698)
  posting `/driver/resolve?from=review` from the top bar.
- `run_publish` L2564 (only the empty-text refusal at L2571);
  `publish_core` (cli.rs L1185) has no base-identical refusal.
- `AssessContext` (steps.rs L53; `max_assessments` L66); the step's user
  message renders slots; `run_driver_assess` filters sources by
  `fetched_text.is_some()` (~L1780).
- `prompts/assess.md` pin `aa8491f8…`; harness: `spawn_serve`,
  `audit_offline`, `reap_child`, `mtime_gap`, httpmock wiki
  (`mock_wiki`, counted edit POST) + zai override.

## Tasks (failing test first, every one)

1. **Reject endpoint — staged case.** Test: render offline; POST
   `/sessions/<slug>/reject`; assert proposed == base on disk, rounds
   gains an `aborted` entry, outcome surfaces. Implement handler + route
   (decline-pending loop first, restore, append). `Verifies: AC1.1`
2. **Reject endpoint — no-op case.** Test: proposed == base; POST
   reject; no new round entry, files untouched, "nothing staged"
   outcome. `Verifies: AC1.4`
3. **StaleKind::Rejected.** Test: render, mtime-gap, POST reject; the
   review page shows the rejected banner (names the rejection; NO
   re-render form) and rounds carries `aborted` (this is also the
   aborted-phase-inertness pin). Implement the enum arm,
   `artifact_state` aborted check (after the rendered rposition, before
   published/comments-resolved), `next_round` (+1), `short`/`sentence`.
   `Verifies: AC1.2`
4. **Race closure.** Test: mock wiki; POST publish (parks); POST
   reject; GET/POST the confirmation approve → "Approval not found /
   nothing written" page; edit mock calls == 0. `Verifies: AC1.6`
5. **Nothing-staged publish refusals.** Tests: serve — run_publish
   outcome on base-identical proposed; CLI — publish_core errors on
   base-identical (readback-style scenario). Implement both refusals.
   `Verifies: AC1.5`
6. **Reject controls.** Tests: review page renders "Reject this edit"
   inside the publish section; session page renders it under the Audit
   form; `?from=review` honored. Implement. `Verifies: AC1.3`
7. **Process comments.** Tests: label + decline-with-reasons meta;
   control renders at end-of-diff above the publish section and in the
   Comments section; the top bar carries no form. Implement
   `comments_bar` split + placements + the serve.rs L786/L1107 comment
   rewording. `Verifies: AC2.1, AC2.2`
8. **fetch_summary.** Tests: driver_steps (hand ctx — summary line and
   rule marker in the sent prompt); serve layer (httpmock zai; ledger
   N=4/M=2 fixture; exact counts on the wire body). Implement
   `AssessContext.fetch_summary`, the user-message line, the
   `run_driver_assess` count (predicate: `fetched_text` absent).
   `Verifies: AC3.2, AC3.3`
9. **Prompt craft rule.** Edit `prompts/assess.md` (cannot-verify rule
   per AC3.1); re-pin with reason; the task-8 marker assertion covers
   presence. `Verifies: AC3.1`
10. **Records.** PLAYBOOK (review stage reject; resolve wording);
    inventory (ENFORCED rows for reject + process-comments; cannot-verify
    as prompt-carried; backlog 4–6 removed); README + serve.rs
    doc-comment wording sweep (hyphen/case-tolerant; staleness list gains
    `rejected`; design-plans archives excluded). `Verifies: AC4.1–AC4.3`
11. **Gate.** fmt/clippy/test fresh; sonar secrets per commit; push;
    CI green; `sonar list issues --new-code` zero BLOCKER/CRITICAL;
    code-reviewer → test-analyst → playbook-audit. `Verifies: None`
