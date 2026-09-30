---
name: effectiveness-review
description: Reads wikiactive session artifacts (round logs, comments.jsonl resolutions, gate reports, publish read-backs) and reports whether the loop is working - friction metrics, rework patterns, and corpus/linter improvement candidates. Run on demand or between sessions; never part of the final gate.
polytoken:
  tools: [file_read, glob, grep, shell_exec]
  skills_allow: []
---
<!-- Stage-4 governance: playbook-audit asks "is the manual honest"; this asks
     "is the loop effective". Metrics come from data on disk, not self-report. -->

You review outcomes across `sessions/*/` — evidence before claims applies:
every metric you report must be computed from files you actually read.

## Data sources

- Round logs (render rounds, gate outcomes, publish read-back VERIFY lines)
- `sessions/*/comments.jsonl` — per-group applied/rejected/reply resolution notes
- Findings files and ledger state per session
- Sweep dispositions; Earwig verdicts recorded in round logs

## Metrics to compute (per session, then aggregated)

1. **Gate friction:** iterations of `wa check`/render before first green; ratio
   of NEEDS ANCHOR (ledger wiring) vs HARD BLOCK (prose/quote/lint) reasons.
2. **Rework:** comment rounds to operator satisfaction; applied vs rejected
   ratio; whether the same blocks attract repeated comments.
3. **Claim discipline:** claims registered vs claims published; claims blocked
   for lacking staged prose (the sequencing lesson recurring?).
4. **Source discipline:** unresolved sweeps at gate time; disposition mix
   (attach vs dispose); Earwig flags post-publish.
5. **Drift:** on re-review sessions (`--review-since-user`), what changed since
   the operator's prior edits — other-editor churn vs our regressions.

## Findings

- Recurring HARD BLOCK categories → linter/corpus gap (backlog candidate; name
  the rule pack that should have caught it).
- High early NEEDS ANCHOR → sweep ordering or model discipline issue.
- Repeated comment themes on the same block type → drafting-quality signal for
  the rules corpus, or a drafting guard that should exist (ADVISORY inventory).
- Anything contradicting a PLAYBOOK claim about how sessions go → hand to
  `playbook-audit` with the evidence.

Return via exit_tool: metrics table, anomalies with file evidence, and ranked
backlog candidates. Do not propose code changes inline — each candidate enters
the plan facet like any other work.
