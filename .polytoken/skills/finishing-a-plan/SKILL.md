---
description: Use when executing an approved handoff plan in this repo - governs just-in-time per-phase implementation, bite-sized tasks with AC traceability, the code-reviewer and test-analyst gates, and what "done" means before the work may be declared complete or published.
---
<!-- Execute-side loop ported from ed3dai/ed3d-plugins writing-implementation-plans /
     executing-an-implementation-plan / requesting-code-review (CC-BY-SA 4.0; portions
     via obra/superpowers, MIT), adapted to Polytoken's execute facet and wikiactive. -->

# Finishing a Plan (execute-side loop)

The handoff plan is directional. Implementation detail is generated just-in-time,
from the codebase as it exists now.

## First task of every execution

Commit the approved plan verbatim to `docs/design-plans/YYYY-MM-DD-<slug>.md`
(exact naming from the plan) unless it is already committed. Create
`docs/implementation-plans/YYYY-MM-DD-<slug>/` for phase files.

## Per phase

1. **Re-verify codebase state before writing the phase's tasks.** The plan's paths
   were true at planning time; confirm they are still true (grep/read/lsp). Never
   write "update X if exists" — investigate, then write a definitive instruction.
2. **Write `phase_NN.md** with bite-sized tasks (one action each: write failing
   test / run it / implement minimally / run tests / commit). Every functionality
   task names the AC cases it verifies: `Verifies: <slug>.AC2.1`. Infrastructure
   tasks verify operationally and say `Verifies: None`.
3. **Execute the tasks.** Test behavior, not wiring. Every phase ends with the
   tree building and that phase's tests passing — a phase is never left red for
   the next phase to fix. Commit at task granularity with `cargo fmt` run first.
4. Track phases with todos so the loop survives compaction.

## Proportionality

The gate is not negotiable; the paperwork scales. Small fixes that touch no loop
behavior never enter the plan facet at all — do them directly under
`verification-before-completion` (fresh commands, evidence, commit). Plans of
≤2 phases may write a single combined phase file instead of one per phase. The
final gate (steps 1–5 below) runs exactly once per effort, not per commit.

## Final gate — in order, no skipping

1. `cargo fmt` applied; `cargo clippy --all-targets` clean; `cargo test` green —
   run fresh, read output (see the verification-before-completion skill).
2. **code-reviewer subagent** over the whole change set. Fix or rebut every
   finding; re-run until zero Critical and zero Important. Rebuttals go to the
   operator, not around the reviewer.
3. **test-analyst subagent** to validate every AC is genuinely covered. Gaps →
   write the missing tests, re-run.
4. **SonarQube**: `sonar analyze secrets` over the changed files before
   committing (local, always available). After push, CI analyzes the branch —
   read `sonar list issues -p tieguy_wikiactive --new-code --format toon` on the
   next gate pass: BLOCKER/HIGH are must-fix before the work counts as done.
   (Vortex pre-push analysis is not on the current SonarCloud plan.)
5. Only after 1–4 pass may the work be called done — and only then proceed to
   anything operator-facing (`wa render`, `wa publish` is always the operator's
   explicit confirmation regardless).

## Rationalizations that mean STOP

- "The plan mentioned this file, so it exists" → verify now.
- "Tests can come at the end of the phase set" → tests are per-task deliverables.
- "code-reviewer found only Important issues, close enough" → the gate is zero.
- "test-analyst will pass, the tests look right" → run it; it reads test bodies.
- "Phase 4 will fix Phase 3's failures" → no phase ends red.
