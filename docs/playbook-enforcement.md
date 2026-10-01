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

Reconciled 2026-10-01 against the loop-mechanization mechanisms
(docs/design-plans/2026-09-30-loop-mechanization.md, phases 1–6). Both
2026-09-30 VERIFY rows confirmed and flipped; the vocabulary is the ADR-0003
surface (assess/fetch/audit/review).

## Session lifecycle

| Playbook rule | Status | Mechanism / note |
|---|---|---|
| init pins base revid; moved base aborts publish | ENFORCED | publish path; AC.10 |
| init refuses existing slug | ENFORCED | `wa session init` refusal |
| `--review-since-user` drift diff in analyze bundle | ENFORCED | analyze bundle assembly |
| corpus missing → analyze fails loudly | ENFORCED | analyze |
| analyze is fresh for the iteration being assessed | ENFORCED | assess entry checks: `assess_entry_checks` (`src/session.rs`) refuses `wa assess add` and the serve Assess save when `context.md` predates `proposed.wikitext`'s mtime or the newest rendered/published/comments-resolved round entry; prescribes `wa analyze`; `--allow-stale-analyze` bypass recorded in the round log |
| fetch resolution precedes assessment | ENFORCED | assess entry checks refuse while any `sweep_status` source lacks text and disposition (both surfaces; `--allow-unresolved-fetch` bypass recorded); the gate's `SweepSourceUnresolved` remains the render/publish backstop |
| assessment evidence quotes must exist in the ledger | ENFORCED | three layers: driver step validation (`src/driver/steps.rs` assess step), assess entry checks (`UnknownQuote` refusal, no bypass), gate `UnknownQuoteId` |
| quotes that don't locate verbatim rejected at entry | ENFORCED | `wa ledger quote` |
| claims are registered only with staged prose | ENFORCED | gate `ClaimNotStaged` (loopmech.AC4): prose in neither base nor proposed wikitext is a NEEDS ANCHOR reason; inherited prose keeps its skip |
| propose ONE logical edit | JUDGMENT | scope discipline; reviewer/checks surface |
| audit report NEEDS ANCHOR vs HARD BLOCK grouping | ENFORCED | `wa audit` output (`format_reasons`) |
| a green audit renders the artifact; blocked writes nothing | ENFORCED | `audit_flow` (render-on-pass; `RenderError::GateBlocked` semantics preserved); retired `wa render`/`wa poll` fail with pointers |
| LLM diagnosis pass never blocks; default per fork config | ENFORCED | `wa audit --llm/--no-llm` + `[audit] llm_pass`; concerns cannot gate; unconfigured/unreachable endpoint degrades to a reported skip recorded as a `rule-review-failed` round entry with the artifact standing |
| evidence-card comments resolve manually; driver never splices them | ENFORCED | `Comment::is_evidence()` (`src/comments.rs`) + the serve apply-comments skip (`src/serve.rs`) |
| edit summaries resolve their rule attributions | ENFORCED | `summary_rule_check` (`src/checks/summary_rules.rs`): every `MOS:`/`WP:`/… token must resolve against the offline-built `ShortcutIndex` from `rules/canonical/`; refused at `wa audit --summary` (no artifact) and at `publish_core` (no edit request — both surfaces) |
| publish re-runs gate; non-empty summary; disclosure suffix appended | ENFORCED | publish |
| publish requires interactive tty/app confirmation; model never self-publishes | ENFORCED | pinned by tests/serve.rs |
| post-publish read-back VERIFY checks | ENFORCED | publish read-back |
| Earwig compare per new web source | ADVISORY | semi-manual; backlog: post-publish command |
| TALK note default-skip (2026-09-29 decision) | JUDGMENT | operator policy (ADR-0001) |
| disclosure-log append, idempotent per session | ENFORCED | disclosure log upsert |
| screenshots (operator, manual Commons upload) | JUDGMENT | manual by design |

## Conventions and guards

| Playbook rule | Status | Mechanism / note |
|---|---|---|
| resolvable shortcut cited for an edit it does not cover | ADVISORY | attribution truthfulness is judgment — resolution is enforced (row above), coverage stays with the reviewer |
| base:-prefixed vs plain anchor semantics | DESCRIPTIVE | comments queue mechanics |
| drafted-lines guards vs article defects scoping | ENFORCED | `rules/linter.toml` `applies = "added-lines"`; the analyze bundle's defect-candidates section is detection only and no gate reason can originate from it (ADR-0005; pinned by the rules-corpus suite) |
| per-loop rule-pack table | DESCRIPTIVE | duplicates analyze loading config |
| BundledConsent backs only disclosure-log upsert | ENFORCED | tests/serve.rs (`publish_requires_the_explicit_web_confirmation`, `declined_confirmation_never_edits`) |
| prompts are versioned and checksum-pinned | ENFORCED | `prompt_checksums_are_pinned` (`src/driver/prompts.rs`); every prompt edit re-pins with a reason in the commit |
| assessment admission is schema-validated (batch-atomic) | ENFORCED | `Assessment::validate` + `AssessmentsFile::parse_validated` (`src/session.rs`); a rejected batch saves nothing; ids are `AS<n>` |
| assessments are implicitly typed via `rules[]` | DESCRIPTIVE | no code branches on a "type" (ADR-0004; revisit triggers recorded there) |
| the model is called at exactly the three judgment points (+ the diagnosis rider) | ENFORCED | the only call sites are `src/driver/steps.rs` (`assess`, `draft_proposal`, `resolve_comments`, `review_draft`), each through a pinned prompt |
| resolve splices revised blocks once per group and records the note | ENFORCED | serve apply-comments (grouped splice + applied/rejected/reply notes persisted to `comments.jsonl`; tests/serve.rs) |
| review comments anchor exactly to the artifact's blocks | ENFORCED | the embedded anchor table (`src/render.rs`) + comment targets (`base:`-prefixed / plain / `ledger:Qn`); tests/serve.rs |
| retired command names fail with pointers, never dispatch | ENFORCED | hidden variants: `wa findings`/`wa sweep`/`wa check`/`wa review`/`wa render`/`wa poll` (tests/assess_cli, fetch_cli, audit_cli) |

## Move-to-code backlog (ADVISORY rows above)

1. Earwig post-publish check as a command
2. attribution-truthfulness aid (surface the cited shortcut's canonical text at publish confirmation — the judgment stays human)
3. quote-extraction judgment point between fetch and assess (found in the 2026-10-01 Gouldner/Night Watch smoke: quote registration has no owner in the driver flow, so a fresh session's Assess can only return []; the Phase-6 defect candidates never reach the Assess prompt; and the assess context embeds full fetched text — 5.7 MB on The Night Watch. Shape: model proposes verbatim spans per source (bounded per-source calls), the tool verbatim-verifies and registers Q ids; assessments then work off small verified evidence. The evidence invariant is unchanged. Session artifacts: sessions/the-night-watch, sessions/alvin-gouldner)
