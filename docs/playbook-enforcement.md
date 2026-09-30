# PLAYBOOK enforcement inventory

Governance baseline for `PLAYBOOK.md`. Every playbook rule is classified by
enforcement status; `playbook-audit` (Polytoken subagent) diffs PLAYBOOK and the
code against this file and reports drift. Prescriptive-but-unenforced items are
the "should be in code" backlog — each enters through the plan facet as ordinary
work; when one lands, its status here flips and the playbook line becomes
descriptive.

Statuses:
- **ENFORCED** — mechanism exists and is named (gate / command / refusal / test pin)
- **VERIFY** — claimed enforced; audit must confirm the mechanism exists
- **ADVISORY** — prescriptive but unenforced → backlog candidate
- **JUDGMENT** — stays with operator/model by design (corpus, reviewer, human gate)
- **DESCRIPTIVE** — documents existing mechanics; rot check only

Baselined 2026-09-30 from PLAYBOOK.md by the Polytoken session; first
`playbook-audit` run should confirm every VERIFY row.

## Session lifecycle

| Playbook rule | Status | Mechanism / note |
|---|---|---|
| init pins base revid; moved base aborts publish | ENFORCED | publish path; AC.10 |
| init refuses existing slug | ENFORCED | `wa session init` refusal |
| `--review-since-user` drift diff in analyze bundle | ENFORCED | analyze bundle assembly |
| corpus missing → analyze fails loudly | ENFORCED | analyze |
| analyze is step 0 of EVERY iteration | ADVISORY | backlog: `wa findings add` refuses/warns without fresh analyze this iteration |
| sweep before analysis (ordering) | ADVISORY | gate blocks render/publish on unresolved sources, but nothing orders sweep *before* findings; backlog: findings-add guard |
| gate blocks render/publish while swept source unresolved | ENFORCED | gate |
| findings evidence quotes must exist in ledger | VERIFY | claimed; confirm in `wa findings add` / gate |
| quotes that don't locate verbatim rejected at entry | ENFORCED | `wa ledger quote` |
| claim sequencing (register when staging) | ADVISORY | gate punishes early claims by blocking; backlog: distinct gate report category "claims with no staged prose" |
| propose ONE logical edit | JUDGMENT | scope discipline; reviewer/checks surface |
| `wa check` NEEDS ANCHOR vs HARD BLOCK grouping | ENFORCED | `wa check` output |
| render pre-flight gate; blocked = no artifact | ENFORCED | render; AC.11 |
| evidence-card comments resolve manually; driver never splices them | VERIFY | confirm apply-comments skips `ledger:Qn` |
| publish re-runs gate; non-empty summary; disclosure suffix appended | ENFORCED | publish |
| publish requires interactive tty/app confirmation; model never self-publishes | ENFORCED | pinned by tests/serve.rs |
| post-publish read-back VERIFY checks | ENFORCED | publish read-back |
| Earwig compare per new web source | ADVISORY | semi-manual; backlog: post-publish command |
| TALK note default-skip (2026-09-29 decision) | JUDGMENT | operator policy |
| disclosure-log append, idempotent per session | ENFORCED | disclosure log upsert |
| screenshots (operator, manual Commons upload) | JUDGMENT | manual by design |

## Conventions and guards

| Playbook rule | Status | Mechanism / note |
|---|---|---|
| edit summaries name a rule only when verified vs `rules/canonical/` | ADVISORY | mechanically checkable; backlog: gate resolves `per MOS:X` in summaries against canonical corpus |
| house rules never attributed to MOS | ADVISORY | same backlog item as above (distinct house-vs-MOS namespace check) |
| base:-prefixed vs plain anchor semantics | DESCRIPTIVE | comments queue mechanics |
| disposition ladder (use → efn → talk → drop) | JUDGMENT | corpus/cards |
| drafted-lines guards vs article defects scoping | ENFORCED | `rules/linter.toml` drafted-lines scope |
| per-loop rule-pack table | DESCRIPTIVE | duplicates analyze loading config |
| BundledConsent backs only disclosure-log upsert | ENFORCED | tests/serve.rs |

## Move-to-code backlog (ADVISORY rows above)

1. findings-add requires fresh analyze per iteration
2. sweep-resolution guard before findings authoring
3. gate report category: claims with no staged prose
4. edit-summary rule resolution against `rules/canonical/` (+ house-vs-MOS namespace)
5. Earwig post-publish check as a command
