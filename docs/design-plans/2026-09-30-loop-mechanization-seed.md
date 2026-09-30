# Loop mechanization — seed for plan-facet authoring

Status: SEED — input for `/facet plan`, not a design plan. The plan facet
investigates, refines, and writes the real design plan; delete or supersede this
file when that plan lands. Full discussion: Polytoken session `0cdf23-ruin`
(2026-09-30).

## Proposed Definition of Done (operator to confirm/edit verbatim)

A coding session never loads PLAYBOOK to do its work: every rule the drafting
loop depends on is either enforced by `wa` (gate, linter, command refusals) or
carried in the versioned prompt set the drafting model receives. PLAYBOOK
remains only as the loop's behavior spec, maintained through plan-facet pairing.

## Phase sketch (refine in the plan facet; investigation required)

1. **Audit & relocate drafting-facing content.** Inventory every PLAYBOOK line
   aimed at the drafting model (disposition ladder, summary rule-attribution,
   one-logical-edit scoping, comment-resolution conventions). Move into the
   `prompts/` judgment-point prompts; update checksums; the disclosure page's
   "exact code including model prompts" promise must stay true.
2. **Analyze-freshness guard.** `wa findings add` refuses/warns without a fresh
   analyze this iteration.
3. **Sweep-before-findings guard.** Findings authoring blocked while swept
   sources are unresolved (explicit operator bypass allowed).
4. **Claim-sequencing report.** Distinct gate NEEDS-ANCHOR category for claims
   with no staged prose.
5. **Summary rule resolution.** Gate resolves `per MOS:X`-style attributions in
   edit summaries against `rules/canonical/`, with the house-vs-MOS namespace
   distinction; unknown attributions block.
6. *(deferred candidate)* Earwig post-publish check as a command.
7. **PLAYBOOK restructure + inventory flip.** Remaining playbook prose becomes
   spec-form; `docs/playbook-enforcement.md` rows ADVISORY→ENFORVED; pairing
   deliverable per the plan spec.

## Pointers the plan must read

- `docs/harness-layering.md` — the two-model-contexts rule (drafting rules live
  in `prompts/` or the gate, never in the coding harness); invariant table.
- `docs/playbook-enforcement.md` — the enforcement inventory; phases 2–5 are its
  ADVISORY backlog.
- `docs/decisions/README.md` — ADR conventions; decisions this plan makes that
  future plans would otherwise re-litigate get records (alternatives mandatory).
- Constraints: prompts are checksum-pinned and disclosed; the gate is the single
  enforcement point for drafted output; `wa` owns the loop (serve UI ordering can
  complement, not replace, mechanical refusals).
