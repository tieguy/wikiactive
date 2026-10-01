# Phase 7 — Records: PLAYBOOK rewrite, inventory, ADRs, layering, final audit

Plan Phase 7. State re-verified 2026-10-01 (`93ea25b`): PLAYBOOK.md (289
lines, MVP-era protocol), `docs/playbook-enforcement.md` (baselined
2026-09-30, 2 VERIFY rows, 5 backlog items), `docs/harness-layering.md`,
`docs/decisions/{README,ADR-0001}`, `is_evidence` at
`src/serve.rs:2200`/`src/comments.rs:81` (the evidence-comment skip).

**ACs:** `loopmech.AC10.1`–`AC10.5` (+ AC6.5 re-verification).

## Decisions

- PLAYBOOK leads with the flow state diagram (fetch → analyze → assess →
  propose → audit → review → resolve → re-audit → publish → post-publish),
  then per-stage one-liners (what enters, what leaves, who acts, the
  guarantee), then the serve console walkthrough, end-of-session
  artifacts, per-loop packs, and limitations. REMOVED per AC10.1:
  comment-resolution mechanics (step 9's anchor-semantics prose), the
  claim-sequencing prescription (§3's live-session lesson), the
  summary-attribution conventions block, and the disposition ladder
  (deleted outright). The drafting-style-guards section stays (scoping
  guarantee) and gains the defect-scan pointer. Legacy-CLI limitation
  note replaced by the retirement.
- Inventory: flip both VERIFY rows ENFORCED (assess entry checks +
  gate `UnknownQuoteId` + `steps.rs` validation; `is_evidence()` +
  serve skip); reword the two boundary ADVISORY rows as ENFORCED
  (freshness + fetch resolution at assess entry); delete the
  disposition-ladder row and the claim-sequencing ADVISORY (now
  `ClaimNotStaged`); add ENFORCED rows for every AC1–AC9 mechanism;
  keep ADVISORY only for Earwig (deferred) and attribution
  truthfulness; the backlog list shrinks to those two.
- ADRs (each with alternatives + rejection reasons; cited by ID in
  PLAYBOOK/inventory/layering):
  - ADR-0002 loop layering (specific rules only in `wa`; PLAYBOOK is
    the higher-level spec);
  - ADR-0003 loop vocabulary (stages are verbs; one word one meaning;
    ledger/assessment split; the 2×2);
  - ADR-0004 assessments implicitly typed via `rules[]` (tentative,
    with revisit triggers);
  - ADR-0005 detection vs enforcement (base scan detects; only
    drafted-lines enforce).
- Layering: PLAYBOOK row becomes "higher-level loop spec (stages,
  guarantees, ownership)"; drafting-context bullet cites ADR-0002; the
  prompts bullet drops the disposition ladder (deleted); the loop-shape
  rows cite ADR-0003; invariant rows cite ADR-0002/0005 where they
  apply; the two-model-contexts wording keeps step names current
  (assess/audit).
- AC6.5 re-verify: grep CLAUDE.md/README/PLAYBOOK for stale command
  names.

## Tasks

1. ADRs 0002–0005 in `docs/decisions/`. `Verifies: loopmech.AC10.3`
2. `docs/harness-layering.md` update. `Verifies: loopmech.AC10.4`
3. `docs/playbook-enforcement.md` reconciliation.
   `Verifies: loopmech.AC10.2`
4. PLAYBOOK rewrite. `Verifies: loopmech.AC10.1`
5. AC6.5 consistency grep + fixes; phase gate (fmt/clippy/test); commit.
6. `playbook-audit` subagent run; fix/rebut until zero Critical and
   zero Important. `Verifies: loopmech.AC10.5`
