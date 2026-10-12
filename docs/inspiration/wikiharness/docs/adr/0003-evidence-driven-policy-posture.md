# ADR-0003 — Evidence-driven policy posture

Status: **Accepted** — owner-ratified 2026-06-02 (walkthrough). Refines (does not reverse)
`0001`'s conservative defaults; the on-wiki outcome loop lands as milestone M4b (`10`).
Date: 2026-06-01 (ratified 2026-06-02)

## Context

The owner is a long-term Wikipedian whose explicit goal is to **move the window of
what AI-assisted editing Wikipedia allows** — by demonstrating that the work is
useful, reliable, and safe. Pushing on current boundaries/rules is therefore an
intended outcome, **conditioned** on having firm, empirically grounded data that
the community can independently scrutinize.

This refines (does not reverse) ADR-0001. The conservative defaults there remain
the *starting* operating posture; they are not treated as a permanent ceiling.

## Decision

1. **Boundary-pushing is an explicit project goal**, gated strictly on
   community-credible empirical evidence of quality. Evidence first; ask second.
2. **The measurement subsystem is strategically central**, not just CI hygiene. We
   distinguish:
   - **Internal eval gates** — block bad releases (`04` Tier 5).
   - **External evidence harness** — produces reproducible, pre-registered,
     comparative, community-facing quality reports that justify policy/scope asks.
3. **We measure real on-wiki outcomes**, not only lab metrics: human acceptance
   rate of proposals, reversion/durability of committed edits vs. human baselines,
   reviewer effort, and audited introduced-error and hallucinated-citation rates.
   This requires a follow-up measurement loop on committed edits.
4. **We foreground the metrics the community fears**: introduced-error /
   false-positive rate (target: better than human baseline) and hallucinated-
   citation rate (target: zero, by construction *and* by audit). Precision is
   prioritized over recall.
5. **We always report against baselines** (experienced human editors; existing
   tools like Citoid/IABot/ORES; our own prior version) and include adversarial /
   red-team results — never absolute numbers alone.
6. **We change policy through the community's own processes** (BRFA, RfC,
   WikiProjects), with data as the argument *inside* those processes, transparently
   published and independently re-runnable. We do not route around process.
7. **Evidence-gated escalation ladder** (`11`) replaces "permanent caution": each
   rung of expanded scope is unlocked by pre-declared thresholds met on the rung
   below plus the relevant community consensus. No rung is pre-committed; the
   *method* is the commitment, and every step is reversible.
8. **Honest negative results are mandatory.** If a boundary's data is insufficient,
   we do not ask to cross it. Credibility is the asset that makes the strategy work.

## Consequences

- Elevates the eval/measurement work from "definition of done" to a first-class,
  externally-facing deliverable with its own quality bar (reproducibility,
  pre-registration, inter-annotator agreement, comparative baselines).
- Adds an **on-wiki outcome measurement loop** (follow committed edits for
  acceptance/reversion/durability) to the `agent`/`server` scope and the build plan
  (realized as milestone **M4b**, `10`). Collection reuses SP42's recent-changes/diff
  plumbing; the comparison against the human-baseline reversion rate adopts citation-checker
  `ccs compare`'s **bias-control reporting discipline** (intersection-only, post-hoc slicing,
  declare-what-changed — `05`/`11`/`12 §C`).
- Reframes `09` graduation criteria and the text phase as evidence-gated
  escalation; reframes `02`'s stance from "fixed constraints" to "current operating
  rules we engineer to exceed and then move."
- The safety/human-in-the-loop machinery is retained and *reinforced* — it is
  precisely what makes boundary-pushing safe and what the evidence documents.

## Non-goals / guardrails

- This is **not** a license to outrun review or ship un-evidenced automation. The
  posture is *more* rigorous, not less: we hold ourselves to a higher-than-human
  standard and prove it before asking for trust.
