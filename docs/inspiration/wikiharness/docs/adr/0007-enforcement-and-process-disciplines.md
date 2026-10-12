# ADR-0007 — Enforcement & process disciplines (blocking hooks, planning loop, stage-end bug-hunt)

Status: **Accepted** — owner-ratified 2026-06-02 (walkthrough). MVP-3 (MOS:PEACOCK)
ratified as a hard-gated demonstrator; *building it ≠ deploying past the evidence ladder*
(see Non-goals).
Date: 2026-06-01 (ratified 2026-06-02)

## Context

Mining Superpowers / ed3d-plugins / crosslink (`13`, "Process & harness-robustness
references") surfaced engineering-process patterns. They converge on one lesson:
**rigor comes from hard, ordered gates and behavior-intercepting hooks, not from
prompting the model nicely.** The owner adopted the following.

## Decisions

1. **Blocking hooks are a first-class enforcement layer (not advisory), threaded
   throughout.** The harness exposes pre-/post-action interceptors that *block*
   structurally, so compliance does not depend on the agent choosing to cooperate:
   - **PreToolUse** can `DENY` / `ASK` / `ALLOW` a tool call before it runs.
     Runtime examples: deny any write lacking a human-confirmation token; deny if
     `evaluate_policy` fails; deny if editor-identity signals are present in context
     (`02 §7`); deny opportunistic edits outside the invoked scope.
   - **PostToolUse** runs verification after a tool call: the anti-hallucination
     quote-locator, provenance completeness, and a **stub/placeholder scan** (no
     `TODO`/unimplemented content reaches a proposal).
   - This is the genuinely *new* idea from the mining pass; the rest reinforce
     existing gates (`02`, `06`, `04`).

2. **Mandatory end-of-stage bug-finding & fixing gate** (from ed3d's
   code-reviewer/test-analyst). Every **stage** — each build milestone (`10`) *and*
   each enhancer pipeline stage at runtime — ends with a dedicated **adversarial
   bug-hunt + fix loop** that must pass before the stage is "done." Severity-tiered:
   **critical findings block**; the loop re-runs until clean. Reviewer is distinct
   from implementer (fresh-context subagent in dev; a separate review pass at
   runtime).

3. **Adopt the ed3d planning loop for development:** **design-doc → (fresh)
   implementation-plan → execute**, codebase-grounded, with **context isolation**
   between phases (the implementation plan is regenerated fresh so it reflects the
   current tree, not stale design assumptions). Plans are git-committed artifacts.
   This governs how we *build* WikiHarness (`10`); a runtime "plan-before-act"
   enhancer is a possible later analogue, not required now.

4. **Multi-model voting (ensemble agreement) is the safety mechanism for
   higher-risk edit classes**, especially content edits. Already proven in SIFT
   (2-of-3 agreement → auto-accept at 96.6% precision). Require **N-of-M model
   agreement** before a content proposal is surfaced; disagreement → suppress or
   route to human with the split shown. Open models first (`adr/0006`).

5. **New scope — MVP-3: codify one NLP-requiring on-wiki rule as a demonstrator.**
   A *single* existing English-Wikipedia editing rule that **traditional, non-NLP
   bots cannot evaluate**, implemented as a bounded enhancer showing that **blocking
   hooks + multi-model voting** can make **small, concise content improvements
   safely**. A deliberate, hard-gated toe-dip into *text-adjacent* edits, consistent
   with the evidence ladder — explicitly **not** general prose generation.
   - **Rule chosen: MOS:PEACOCK (puffery)** — owner decision, gated on dataset
     availability (`12 §C-bis`): public code+data (`abertsch72/wikipedia-puffery-
     detection`, distant-supervised from `{{Puffery}}`/`{{Peacock}}` templates),
     a ~0.963 F1 RoBERTa baseline to beat, and a citation-feature synergy that
     reuses WikiHarness's own citation core.
   - **Weasel (MOS:WEASEL) is queued as the immediate follow-on enhancer**, reusing
     peacock's `detect → verify(vote) → propose` pipeline. This reuse is itself the
     intended demonstration that the enhancer framework (ADR-0005) *composes* —
     and weasel brings the larger WikiWeasel/CoNLL-2010 benchmark (~20.7k labels).

## Consequences

- **`06`** gains a hooks/enforcement layer (guiding principle + the agent-loop
  interception points + a package), plus a multi-model-voting note.
- **`04`** gains the end-of-stage bug-hunt gate and a multi-model-agreement gate.
- **`10`** gains the planning loop (build process) + stage-end bug-hunt in the
  Definition of Done, and a new **M6 = MVP-3** milestone.
- **`01`** sequencing gains **MVP-3** (after MVP-2, before the broader
  Wikidata→GLAM→text enhancers).
- Together these make the README's "structure forces robustness" claim concrete.

## Non-goals

- Hooks do **not** replace the pure, I/O-free, deeply-testable core (`04`); they are
  an *additional* structural enforcement layer, themselves unit-tested.
- MVP-3 is **not** open-ended prose rewriting: one codified rule, flag-or-minimal-
  edit, multi-model-gated, human-confirmed, hard-gated by hooks.
- **Building MVP-3 ≠ unlocking evidence-ladder rung 4 (`11`).** The milestone produces a
  hard-gated, flag-first, multi-model-voted demonstrator *and evidence*; deploying
  text-adjacent edits at scale still climbs the ladder via BRFA/RfC. Capability built is
  not boundary moved.
