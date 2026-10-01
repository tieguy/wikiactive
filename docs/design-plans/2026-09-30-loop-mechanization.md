# Loop Mechanization Design

## Summary

wikiactive drives a structured Wikipedia editing loop: one logical edit at a time, every change anchored to fetched, quoted sources, reviewed by a human, published only on explicit confirmation. The rules governing each editing session currently reach the working model partly through a prose document (`PLAYBOOK.md`) that sessions are trusted to read. This plan makes the tool self-sufficient: every rule the loop depends on becomes a mechanism in the `wa` binary — a gate check, a linter rule, or a command refusal — or travels in the versioned, checksum-pinned prompts the drafting model receives. `PLAYBOOK.md` survives as a higher-level description of the loop for maintainers, no longer a runtime dependency.

Three kinds of change accomplish this. The command surface is reshaped to match the loop's stages: findings become assessments (`wa assess`), the source sweep becomes fetch (`wa fetch`), and the separate check/render/review commands collapse into one `wa audit` that renders the review artifact automatically on a passing gate and adds a paid model diagnosis pass on by default. Admission checks appear at assess entry (evidence must exist in the ledger, analysis must be fresh, fetched sources must be resolved), and the gate learns two new refusals: claims registered without staged prose, and edit summaries citing rule shortcuts that do not resolve against the pinned policy snapshots. Finally the records are rewritten — `PLAYBOOK.md` as a state-diagram spec, the enforcement inventory reconciled row-by-row, and the settled decisions recorded as ADRs.

## Definition of Done

> Every rule the drafting loop depends on is carried by the tool itself: enforced by `wa` (gate, linter, command refusals) or shipped in the versioned, checksum-pinned prompt set the drafting model receives. A drafting session that works only from the tool and its prompts follows every rule. PLAYBOOK.md becomes the higher-level spec of the loop — its stages, its guarantees, and who owns each — with specific rules maintained only in `wa`.

## Acceptance Criteria

Scoped IDs `loopmech.AC{N}.{M}`. Every criterion is observable by a test (unit or httpmock integration; the suite is offline — no Wikipedia or model-endpoint calls from tests), except the records criteria `AC10.1`–`AC10.5`, which are verified by the final `playbook-audit` review pass; `AC6.5` is a documentation-consistency check re-verified at the end.

### AC1 — assess entry: evidence must exist in the ledger (both entry paths)

- `loopmech.AC1.1` `wa assess add` rejects a batch containing an assessment whose `evidence` cites a quote id absent from the ledger; the error names the offending id(s); nothing from the batch is saved (batch-atomic).
- `loopmech.AC1.2` the serve Assess save path applies the same check before appending; the refusal is surfaced to the operator (visible message), never a silent skip.
- `loopmech.AC1.3` a batch whose evidence ids all exist saves exactly as before (no behavior regression on the happy path).

### AC2 — assess entry: analyze freshness (refusal + bypass)

- `loopmech.AC2.1` `wa assess add` refuses when `context.md` (the analyze output) is stale for this iteration — older than the newest round-advancing event (`rendered` / `published` / `comments-resolved` entry in `rounds.jsonl`) or older than `proposed.wikitext`'s last modification; the message names the stale relation and prescribes `wa analyze`.
- `loopmech.AC2.2` after re-running `wa analyze`, the identical batch is accepted.
- `loopmech.AC2.3` the serve Assess path enforces the same freshness state and refuses with the same message.
- `loopmech.AC2.4` first iteration (no round-advancing entries yet): freshness compares only against `proposed.wikitext`.
- `loopmech.AC2.5` an explicit bypass flag proceeds past the freshness refusal, and the bypass is recorded in the round log.

### AC3 — assess entry: fetched sources must be resolved (refusal + bypass)

- `loopmech.AC3.1` with any unresolved fetched source (`sweep_status` set, no disposition and no fetched text), `wa assess add` refuses, listing each source id and status.
- `loopmech.AC3.2` an explicit bypass flag proceeds, and the bypass is recorded in the round log.
- `loopmech.AC3.3` once every source is resolved (fetch / attach / dispose), the same command proceeds without the flag.
- `loopmech.AC3.4` the serve Assess path enforces the same; the refusal message points at the existing on-page resolution affordances (disposition / attach).
- `loopmech.AC3.5` sessions without any fetch/sweep state are unaffected.

### AC4 — claim sequencing becomes a gate category

- `loopmech.AC4.1` a registered claim whose (whitespace-normalized) prose appears in neither base nor proposed wikitext produces a distinct NEEDS ANCHOR reason naming the claim id and its prose; it blocks render and publish.
- `loopmech.AC4.2` an inherited claim (prose in base only) does not fire the new reason — the existing `inherited_claim_prose_skips_paraphrase_gate` pin stays green.
- `loopmech.AC4.3` a staged claim (prose in proposed) does not fire the new reason.
- `loopmech.AC4.4` `wa audit` output groups the new reason under NEEDS ANCHOR.

### AC5 — edit summaries resolve their rule attributions

- `loopmech.AC5.1` a summary citing a shortcut that resolves in the canonical index (e.g. a real `MOS:` shortcut present in `rules/canonical/`) passes.
- `loopmech.AC5.2` a summary with an unresolvable shortcut token (e.g. `per MOS:NOTREAL`) is refused: audit-with-summary writes no artifact; publish sends no edit request; the refusal names the token.
- `loopmech.AC5.3` summaries without rule tokens pass unchanged; disclosure-suffix append and idempotence are unaffected.
- `loopmech.AC5.4` house rules never wear MOS/WP attribution via invented shortcuts: a house-rule edit described with a made-up shortcut is refused as unresolvable; the same edit described plainly passes. (Whether a resolvable shortcut genuinely covers the edit stays judgment — ADVISORY.)
- `loopmech.AC5.5` the shortcut index is built offline from the committed `rules/canonical/` snapshots; no network at build or run.

### AC6 — command surface reshape

- `loopmech.AC6.1` `wa assess add <slug> <json|->` and `wa assess list <slug>` replace the findings commands; the record type is `Assessment` with ids `AS<n>` stored in `assessments.json`; invoking `wa findings …` fails with an error pointing at the new command. Existing sessions' `findings.json` are not migrated (documented break; operator-approved).
- `loopmech.AC6.2` `wa fetch <slug>` performs inventory-then-fetch in one invocation; `wa fetch dispose` and `wa fetch status` carry over; `wa sweep …` fails with a pointer.
- `loopmech.AC6.3` `wa audit <slug>` runs the deterministic gate and `wa audit --llm <slug>` is the renamed rule-review step (still on demand, as today — Phase 2 changes the default); `wa check` and `wa review` fail with pointers to those. (`wa render` and `wa poll` keep working through Phase 1; their retirement lands with render-on-pass — `AC7.1`.)
- `loopmech.AC6.4` serve controls are renamed: **Write findings** → **Assess**, **Render review** → **Audit**; the rule-check control joins the Audit action as an LLM toggle.
- `loopmech.AC6.5` `CLAUDE.md` and `README.md` command references match the new surface (updated in the same phase as each rename, re-verified at the end).

### AC7 — a green audit renders the artifact

- `loopmech.AC7.1` a passing audit produces the round artifact (review.html + `rendered` round entry) with no separate render command or button step — tty `wa audit` ends at the artifact; the serve Audit action ends at the review page; the retired lavish legacy commands (`wa render`, `wa poll`) fail with pointers.
- `loopmech.AC7.2` a blocked audit writes no artifact and lists all reasons (existing gate-blocking behavior preserved).
- `loopmech.AC7.3` re-audit after comment resolution produces the next round's artifact; round numbering and the revisions registry's replace-on-rerender behavior are preserved.

### AC8 — the LLM audit pass is a diagnosis, on by default

- `loopmech.AC8.1` `wa audit` includes the LLM diagnosis pass by default; `--no-llm` skips it; the default is fork config (`rules/house-rules.toml [audit] llm_pass`), and flipping the config flips the default.
- `loopmech.AC8.2` the pass outputs concerns (clause id, verbatim span, note); unknown clause ids and non-verbatim spans are rejected (existing validation preserved).
- `loopmech.AC8.3` the pass never blocks: an audit with concerns still passes the deterministic gate and renders the artifact with concerns displayed under their blocks.
- `loopmech.AC8.4` an unconfigured or unreachable model endpoint never blocks the deterministic outcome — the gate runs, the artifact renders, and the skipped or failed diagnosis pass is reported in the output and the round log.

### AC9 — analyze grows a detection-only defect scan

- `loopmech.AC9.1` the analyze bundle lists base-article mechanical defects (from the linter corpus run over base text) as candidate assessments under a clearly-labeled section.
- `loopmech.AC9.2` scan output is informational: nothing auto-applies, and scan output alone never blocks any stage.
- `loopmech.AC9.3` enforcement scoping is unchanged: drafted-lines rules still gate only lines this tool drafts.

### AC10 — PLAYBOOK becomes the higher-level spec; records updated

- `loopmech.AC10.1` the rewritten PLAYBOOK leads with a flow state diagram of the loop (states, transitions, who acts) using the new stage vocabulary; specific drafting-rule prose is removed (comment-resolution mechanics, the claim-sequencing prescription, the summary-attribution conventions, and the disposition ladder — the ladder is deleted, not relocated); stage, guarantee, and ownership descriptions remain.
- `loopmech.AC10.2` `docs/playbook-enforcement.md` is reconciled row-by-row: both 2026-09-30 VERIFY rows flip ENFORCED with named mechanisms (per the first playbook-audit: driver `steps.rs` + gate `UnknownQuoteId`; `is_evidence()` + the serve skip); a row for the rule-review/LLM audit pass is added; every new mechanism from AC1–AC9 gets an ENFORCED row worded to what it actually enforces; the disposition-ladder row is deleted with the PLAYBOOK text; boundary rows are reworded to the enforced check ("sweep before analysis" and "analyze is step 0" become fetch-resolution and freshness enforced at assess entry); remaining ADVISORY rows: the deferred Earwig post-publish check, and attribution truthfulness (a resolvable shortcut cited for an edit it does not cover — judgment, not mechanism).
- `loopmech.AC10.3` ADRs are recorded, each with alternatives considered and rejection reasons, cited by textual ID where the decision applies: (a) loop layering — specific rules live only in `wa`, PLAYBOOK is the higher-level spec; (b) loop vocabulary — stages are verbs, records named after their stages, one word one meaning (review = the human act; audit = examination; assess = article-side diagnosis), the ledger/assessment two-layer split (evidence basis vs. editorial judgment), with the 2×2 (article/draft × mechanical/judgment) as a supporting table; (c) assessments are implicitly typed via `rules[]` — explicitly tentative, with recorded revisit triggers; (d) detection vs. enforcement: base-article defect scan detects, only drafted-lines enforce.
- `loopmech.AC10.4` `docs/harness-layering.md` authority/invariant rows are updated to match the layering ADR.
- `loopmech.AC10.5` a final `playbook-audit` pass reports zero Critical and zero Important findings.
- `loopmech.AC10.6` the seed `docs/design-plans/2026-09-30-loop-mechanization-seed.md` is deleted (superseded by this plan).

## Glossary

- **Assessment** — an editorial judgment about the base article: anchor (region of base wikitext), implicated `rules[]`, ledger evidence (`Q` ids), factual note, one-sentence proposed fix. Record type `Assessment`, ids `AS<n>`, file `sessions/<slug>/assessments.json` (renamed from finding / `F<n>` / `findings.json`). Assessments look backward at the article; claims look forward at the edit.
- **Ledger** — the evidence substrate and basis for edits: sources (`S`) → fetched text → quotes (`Q`, verbatim-verified) → claims (`C`, prose staged for the article backed by quotes). Everything in it is mechanically checkable; the gate reads it.
- **Fetch stage** — resolve the accessibility of every already-cited source up front (inventory + fetch, attach operator captures, sign dispositions), before analysis. Renamed from "sweep".
- **Analyze** — mechanical assembly of the iteration's judgment context (`context.md`): article state, tier-1 core verbatim, this loop's cards, fetch manifest, drift diff; now also base-article defect candidates.
- **Audit** — examination of the draft. Deterministic engine = the gate (ledger wiring, linter, paraphrase, anchors; NEEDS ANCHOR / HARD BLOCK). LLM engine (`--llm`) = the diagnosis pass: clause-cited concerns on the draft, never decides, never blocks.
- **Review** — the human act: reading the artifact and leaving comments. The word belongs to the operator only.
- **Render-on-pass** — a green audit automatically produces the round artifact; rendering is a consequence, not a step.
- **Entry checks** — the validations `wa assess add` (and the serve Assess path) run before saving: evidence quote-existence, analyze freshness, fetch resolution.
- **Round / round log** — `sessions/<slug>/rounds.jsonl`; entries carry round number, timestamp, phase (`rendered`, `published`, `comments-resolved`, `rule-reviewed`, …), summary, detail.
- **Shortcut index** — offline-built map of Wikipedia policy shortcut tokens (`MOS:…`, `WP:…`) to canonical pages, from the committed `rules/canonical/` snapshots.
- **Enforcement inventory** — `docs/playbook-enforcement.md`; classifies every loop rule ENFORCED / VERIFY / ADVISORY / JUDGMENT / DESCRIPTIVE; audited by the `playbook-audit` subagent.
- **Seed** — `docs/design-plans/2026-09-30-loop-mechanization-seed.md`, the proposal this plan supersedes and deletes.

## Architecture

### Final CLI surface (loop-relevant)

```
wa session init --article <t> --entry-loop <n> [--review-since-user <name>]
wa session show <slug>
wa fetch <slug>                     # inventory + fetch in one invocation
wa fetch dispose <slug> --source S3 --disposition "..."
wa fetch status <slug>
wa analyze <slug>
wa assess add <slug> <json|->       # entry-checked admission
wa assess list <slug>
wa ledger register|fetch|archive|attach|quote|claim ...
wa audit <slug> [--llm|--no-llm] [--summary "..."]   # gate; green => artifact
wa comments list|add|resolve <slug> ...
wa publish <slug> --summary "..."
wa serve [--tsnet] [--port N]
wa lint <wikitext-file>
```

Retired as user commands: `wa findings …` → `wa assess …`; `wa sweep …` → `wa fetch …`; `wa check` / `wa review` → `wa audit` (and `wa audit --llm`); `wa render` and `wa poll` (the lavish legacy path) → `wa audit`. Retired invocations fail with one-line pointers to their replacements.

### The loop (what PLAYBOOK's flow state diagram will express)

fetch → analyze → assess (model) → propose (model) → audit (mechanical; green ⇒ artifact; optional-but-default LLM diagnosis) → review (human, on the artifact) → resolve (model) → re-audit → … → publish (gate re-run + the one human confirmation) → post-publish (disclosure log; Earwig deferred; TALK default-skip per ADR-0001; screenshots manual).

Organizing frame (recorded in the vocabulary ADR): article-side mechanical = **analyze**; article-side judgment = **assess**; draft-side mechanical = **audit**; draft-side judgment = **audit --llm**. "Review" is reserved for the human act.

### Components and responsibilities

- `src/cli.rs` — command surface; `assess_add` gains entry checks; `audit_cmd` (gate + auto-render + optional LLM pass + summary check when a summary is given); `publish` keeps its gate + summary checks.
- `src/session.rs` — `Assessment` (was `Finding`), `AS<n>` ids, `assessments.json`; `RoundEntry` gains any new phases entry checks / bypass records need.
- `src/ledger.rs` — unchanged role; quote-existence lookup and the unresolved-source predicate (`sweep_unresolved`) feed entry checks; any internal field rename carries a serde alias so committed ledgers deserialize unchanged.
- `src/rules.rs` — corpus; `guidance_for_loop` unchanged in contract; analyze bundle gains the defect-candidates section.
- `src/checks/gate.rs` — new `ClaimNotStaged` reason; dispositions/span/Display arms; summary-rule reasons surface through the same report.
- `src/checks/summary_rules.rs` (new module; name final at execution) — `ShortcutIndex` build + `summary_rule_check`.
- `src/checks/linter.rs` — a base-text scan entry point feeding analyze (detection scope distinct from the gate's drafted-lines scope).
- `src/serve.rs` — renamed controls; Assess save path calls the same entry checks; Audit action runs gate + render + LLM pass; staleness guard (`artifact_state`) unchanged in mechanics (it already keys on `rendered` entries, which auto-render writes identically).
- `src/driver/steps.rs`, `src/driver/prompts.rs`, `prompts/*.md` — step renames; `prompts/review.md` wording moves from "advice" to "diagnosis" (checksum pin updated with reason); `{{guidance}}` contract unchanged.
- `src/render.rs` — invoked by audit on green; no public CLI entry.

### Contracts

**Assess entry checks** (one function both paths call — CLI `assess_add` and the serve Assess save):

```rust
enum EntryRefusal {
    UnknownQuote { ids: Vec<String> },                 // no bypass
    StaleAnalyze { older_than: String },               // bypass: --allow-stale-analyze
    UnresolvedFetch { sources: Vec<(String, String)> },// bypass: --allow-unresolved-fetch
}
fn assess_entry_checks(sess: &Session, ledger: &Ledger, opts: EntryOpts)
    -> Result<(), Vec<EntryRefusal>>;
```

A batch with any refusal saves nothing (the existing batch-atomic shape). A used bypass appends a round-log entry recording which guard was bypassed and why the flag was passed. Serve surfaces the refusal strings verbatim.

**Freshness predicate** (mirrors the `artifact_state` staleness pattern):

```
fresh  ⇔  mtime(context.md) > mtime(proposed.wikitext)
       ∧  mtime(context.md) > timestamp(newest rendered|published|comments-resolved entry)
(no advancing entries yet ⇒ only the proposed.wikitext comparison)
```

**Claim-sequencing gate reason:**

```rust
GateReason::ClaimNotStaged { claim_id: String, prose: String }
// disposition: NeedsAnchor; span: None
// fires iff whitespace-normalized prose ∉ base.wikitext ∧ ∉ proposed.wikitext
// (same normalization as the inherited-claim skip)
```

**Summary rule check:**

```rust
struct ShortcutIndex { /* token -> canonical page */ }
impl ShortcutIndex { fn build(rules/canonical/*.wikitext) -> Self }  // offline
fn summary_rule_check(summary: &str, index: &ShortcutIndex) -> Vec<String>
// one line per shortcut-shaped token (MOS:…, WP:…, H:… variants, case-normalized)
// not found in the index; empty vec = pass
```

Called wherever a summary is accepted with the power to produce or send an edit: audit-with-`--summary`, and publish (CLI and serve — both flow through the same chokepoint). Because house rules have no canonical shortcuts, a house rule wearing an invented shortcut is refused as unresolvable; whether a *resolvable* rule actually covers the edit stays judgment — that residual remains ADVISORY in the inventory. Tokens are parsed from `{{Shortcut|…}}` templates and `shortcut1=…`-style infobox parameters in the committed snapshots.

**Audit consolidation:** `wa audit` runs `run_gate`; on pass it invokes the render pipeline internally (round = `next_round`; revisions registry replace-on-rerender preserved) and, unless `--no-llm` or `[audit] llm_pass = false`, runs the `review_draft` step and displays/stores concerns (existing `rule-review.json` storage and round labelling). On fail: reasons listed, nothing written (existing `RenderError::GateBlocked` behavior). The LLM pass is non-fatal by construction: client-construction failure (e.g. no `ZAI_API_KEY` in the environment) or a transport/model error after a green gate never prevents the artifact — the skip or failure is reported in the output and recorded in the round log (`AC8.4`).

### Data flow and boundaries

Content changes flow ledger → gate → human confirmation exactly as today (invariant, untouched). This plan moves *admission* earlier (entry checks at assess), moves *rendering* later (consequence of a green audit), and renames surfaces; it does not change the ledger record shapes beyond the assessment rename, or any network behavior. If the fetch-status field is renamed internally, it carries a serde alias (`sweep_status`) so legacy ledgers deserialize with their sweep state intact — a silent loss of fetch enforcement on legacy sessions is not an acceptable break. `USER_AGENT` stays solely in `src/lib.rs`; API etiquette in `src/wikipedia.rs` / `src/ledger/net.rs` is untouched. `sessions/` stays out of git; `fixtures/` are re-recorded, never hand-edited (the shortcut index reads committed `rules/canonical/` snapshots directly; the one fixture touch is the zai captured pair — see Additional Considerations).

## Existing Patterns

- **Batch-atomic admission** — `findings_add` (`src/cli.rs:586-630`) validates the whole batch before saving; entry checks slot between `Finding::validate` and the save loop. Followed as-is.
- **Two entry paths, one check** — the serve driver save (`src/serve.rs:1917-1932`) currently skips re-validation (first playbook-audit finding); this plan's shared `assess_entry_checks` is the correction.
- **Staleness by mtime + round entries** — `artifact_state` (`src/serve.rs:882-915`, plan-004) is the precedent the freshness predicate mirrors; appending new round-log phases is backward-safe (`artifact_state` reacts only to `rendered` / `published` / `comments-resolved`).
- **Typed refusals** — `WikipediaError::BareSummary` (`src/wikipedia.rs:148`), `RenderError::GateBlocked` (`src/render.rs:84-86`); `EntryRefusal` follows.
- **Gate reason extension points** — `Disposition` / `disposition()` / `span()` / Display (`src/checks/gate.rs:26-282`); `ClaimNotStaged` adds arms; `format_reasons` groups automatically.
- **Single-source guidance** — `guidance_for_loop` (`src/rules.rs:239-261`) feeds the analyze bundle and all four prompts identical bytes; the prompt checksum pins (`prompt_checksums_are_pinned`) make prompt edits deliberate.
- **Fork config** — `rules/house-rules.toml [zai]` precedent for `[audit] llm_pass`.
- **ADRs** — conventions in `docs/decisions/README.md`; ADR-0001 (talk-note default-skip) is the in-repo precedent; this plan's records follow the format (alternatives mandatory, open questions inline, cite by textual ID).
- **Divergence:** the renames reverse MVP-1-era naming (`2026-09-24-mvp1-structured-loop.md`). That plan is archival; the divergence is recorded in the vocabulary ADR rather than by editing the old plan. The un-owned "disposition ladder" prose is deleted, not mechanized — surfaced by this planning session, disposition decided by the operator (2026-09-30: not a rule the loop depends on).

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1 — Surface reshape: assess, fetch, audit command shell

**Goal:** the new vocabulary exists end-to-end as pure renames with helpful errors for old names; behavior otherwise unchanged. `wa render` and `wa poll` keep working until Phase 2 retires them alongside their replacement.

**Components:** `src/cli.rs`, `src/session.rs`, `src/serve.rs`, `src/driver/steps.rs`, `src/driver/prompts.rs` (checksum pins updated, reason in the commit message), `src/sweep.rs` + `tests/sweep.rs` (the fetch verbs replace the sweep subcommands; the module keeps its name — internal `sweep_*` identifiers persist for ledger serde compatibility), `src/checks/gate.rs` (message text), `prompts/author-findings.md` (renamed with the step; schema wording moves to `Assessment` / `AS<n>`), `prompts/propose.md` (its summary rule's "no F or Q numbers" becomes AS/Q), `tests/*` (esp. `tests/findings_cli.rs` → `tests/assess_cli.rs`, `tests/check_cli.rs`, `tests/serve.rs`, `tests/sweep.rs`, and the driver-step/e2e fixtures whose completions emit `F` ids), `CLAUDE.md`, `README.md`.

First tasks: commit this design plan verbatim to `docs/design-plans/2026-09-30-loop-mechanization.md`; delete the seed (`AC10.6` lands here).

**Dependencies:** none.

**ACs covered:** `loopmech.AC6.1`, `AC6.2`, `AC6.3`, `AC6.4`, `AC6.5`, `AC10.6`.

**Done when:** new commands work offline end-to-end; retired invocations fail with pointers; `AS<n>` ids and `assessments.json` in force (legacy `findings.json` documented as a break); suite green, fmt/clippy clean; CLAUDE.md/README match.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2 — Audit shape: render-on-pass, `--llm` consolidation, diagnosis vocabulary

**Goal:** `wa audit` becomes the single draft-side examination: deterministic gate, artifact on green (render-on-pass), LLM diagnosis pass by default (fork-config). The lavish legacy pair `wa render` / `wa poll` retires here, failing with pointers to `wa audit`.

**Components:** `src/cli.rs` (`audit_cmd`; `render`/`poll` command retirement), `src/render.rs`, `src/serve.rs` (Audit action, toggle), `src/driver/steps.rs` (`review_draft` wiring), `src/driver/prompts.rs` + `prompts/review.md` ("diagnosis, never decides" wording; checksum pin updated with reason), `rules/house-rules.toml`, `tests/serve.rs`, `tests/driver_steps.rs`, `tests/render.rs`, `tests/audit_cli.rs`.

**Dependencies:** Phase 1.

**ACs covered:** `loopmech.AC7.1`, `AC7.2`, `AC7.3`, `AC8.1`, `AC8.2`, `AC8.3`, `AC8.4`.

**Done when:** a green audit produces the artifact with no render step anywhere; a blocked audit produces nothing; the LLM pass runs by default, is skippable, is config-flippable, and never blocks — including when no `ZAI_API_KEY` is set (`AC8.4`); `wa render` and `wa poll` fail with pointers; round numbering and registry replace-on-rerender pins hold.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3 — Assess entry checks

**Goal:** both assess entry paths refuse unknown-quote, stale-analyze, and unresolved-fetch batches; bypasses exist for the latter two and are recorded.

**Components:** `src/cli.rs` (`assess_add`), `src/serve.rs` (Assess save path), `src/session.rs` (round-log bypass entries), `src/ledger.rs` (lookups), `tests/assess_cli.rs`, `tests/serve.rs`.

**Dependencies:** Phase 1 (renamed surface); independent of Phase 2.

**ACs covered:** `loopmech.AC1.1`–`AC1.3`, `AC2.1`–`AC2.5`, `AC3.1`–`AC3.5`.

**Done when:** every listed case has a failing-test-first pin; the serve path and CLI path share one check function (asserted by a test that both surfaces refuse the same fixture batch); happy path unchanged.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4 — Claim-sequencing gate category

**Goal:** a claim whose prose is staged nowhere is a first-class NEEDS ANCHOR reason.

**Components:** `src/checks/gate.rs`, `tests/gate.rs`, `tests/audit_cli.rs`.

**Dependencies:** Phase 1 (command naming in output); otherwise independent.

**ACs covered:** `loopmech.AC4.1`–`AC4.4`.

**Done when:** the new reason blocks render and publish with claim id + prose in the message; inherited and staged claims do not fire it (existing pin green).
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5 — Edit-summary rule resolution

**Goal:** every shortcut-shaped token in an edit summary resolves against the canonical corpus or the summary is refused.

**Components:** new `src/checks/summary_rules.rs` (or `src/rules.rs` extension — final home at execution), `src/checks/gate.rs` (report surface), `src/cli.rs` (audit `--summary`, publish path), `src/wikipedia.rs` (ordering vs. the disclosure-suffix chokepoint), `tests/publish.rs`, `tests/audit_cli.rs`.

**Dependencies:** Phases 1–2 (audit surface); independent of Phases 3–4.

**ACs covered:** `loopmech.AC5.1`–`AC5.5`.

**Done when:** the index builds offline from committed snapshots; unresolvable tokens block artifact and edit request with the token named; token-free summaries and suffix behavior unchanged.
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6 — Analyze defect scan

**Goal:** the analyze bundle lists base-article mechanical defects as candidate assessments; detection only.

**Components:** `src/rules.rs` (bundle section), `src/checks/linter.rs` (base-text scan), `tests/rules_corpus.rs` (+ analyze-output tests).

**Dependencies:** Phase 1; independent of Phases 2–5.

**ACs covered:** `loopmech.AC9.1`–`AC9.3`.

**Done when:** a fixture base text with known defects surfaces them in the bundle under the labeled section; no gate reason can originate from the scan; drafted-lines enforcement scope is pinned unchanged.
<!-- END_PHASE_6 -->

<!-- START_PHASE_7 -->
### Phase 7 — Records: PLAYBOOK rewrite, inventory, ADRs, layering, final audit

**Goal:** the documentation layer matches the mechanized loop; decisions are on record; the audit passes clean.

**Components:** `PLAYBOOK.md` (flow state diagram, new vocabulary, specific-rule prose removed — including deleting the disposition ladder), `docs/playbook-enforcement.md`, `docs/harness-layering.md`, `docs/decisions/` (ADRs per AC10.3), `CLAUDE.md`/`README.md` final consistency pass.

**Dependencies:** Phases 1–6 (records describe landed mechanisms).

**ACs covered:** `loopmech.AC10.1`–`AC10.5` (AC10.6 landed in Phase 1).

**Done when:** a fresh `playbook-audit` run reports zero Critical/Important; the inventory is reconciled row-by-row per AC10.2 (ladder row deleted, boundary rows reworded, every ENFORCED row names its mechanism); every removed PLAYBOOK rule verifiably lives in a named mechanism or prompt; the legacy lavish-CLI note is gone with the retirements; all ADRs carry alternatives and are cited by ID.
<!-- END_PHASE_7 -->

## Execution Contract

- Implementation detail lives in `docs/implementation-plans/2026-09-30-loop-mechanization/phase_NN.md`, written just-in-time: re-verify current codebase state for EACH phase before writing its tasks (investigate, then write definitive instructions — never "update if exists").
- Tasks are bite-sized (one action each): write failing test / run it / implement minimally / run tests / commit. Every functionality task names the AC cases it verifies (`Verifies: loopmech.AC2.1`). Test behavior, not implementation.
- Before declaring any phase done: run `cargo fmt`, `cargo clippy --all-targets`, `cargo test` fresh and read the output — evidence before claims, always. A phase ends green or it is not done.
- Other sessions may have uncommitted work in this tree; stage by explicit path; `git show HEAD --stat` after each commit. One commit per work item. Do not publish to Wikipedia; no live calls from tests.
- Prompt files are checksum-pinned (`src/driver/prompts.rs`); any prompt edit updates the pin and says why in the commit message. The disclosure page's "exact code including model prompts" promise stays true.
- After the final phase: run the `code-reviewer` subagent and fix or rebut every finding until zero Critical/Important remain; then run `test-analyst` to validate AC coverage; the final gate includes a `playbook-audit` pass (this plan changes loop behavior — PLAYBOOK and the enforcement inventory are phase deliverables per the pairing rule). SonarQube: `sonar analyze secrets` before any commit; the reviewer gate reads `sonar list issues --new-code` after CI analysis and treats SQ severities as authoritative (BLOCKER/HIGH = must fix).
- Wikiactive invariants hold throughout: content changes flow through the ledger and the gate; the quote-anchor locator keeps its SP42 provenance headers; `USER_AGENT` only in `src/lib.rs`; `fixtures/` re-recorded, never hand-edited; `sessions/` out of git.

## Additional Considerations

- **Legacy sessions break once** (`findings.json` unreadable after the rename). Accepted by the operator (single user, early stage); the release note states it. If a live session must be finished first, a one-shot file rename is the documented workaround, not a migration path.
- **`fixtures/zai` refresh:** the prompt rename and the F→AS id change make the captured pair (`findings.request|response.json`, live captures of the findings step) stale. Refresh by re-running `scripts/zai-spike.sh` (a live model call, outside the test suite; files renamed to match the step), never by hand-editing — the behavior tests use inline httpmock bodies and are unaffected.
- **Cost of the default-on LLM pass:** one model call per audit round. It never blocks — concerns cannot gate (`AC8.3`), and an unconfigured or unreachable endpoint degrades to a reported skip with the artifact still produced (`AC8.4`) — so it cannot wedge the loop; the fork-config key exists precisely so the default is an experiment, not a commitment.
- **Freshness predicate is mtime-based** and inherits mtime's usual caveats (clock skew, tooling that touches files). It mirrors the accepted `artifact_state` pattern; if it proves brittle, an explicit `analyzed` round-log phase is the recorded fallback, not a redesign.
- **Wikidata (L5)** stays deferred. The paused plan-006 (`wa wikidata qualify`, post-publish writeback reading the ledger) is unaffected by this plan; the alternative shape surfaced in planning — extract candidate fields at Analyze, tentative additions at Assess — is recorded here as an open question for the plan-006 resume, not scope of this effort.
- **The five surveyed prior-art linter rules** (`docs/2026-09-30-linter-prior-art.md` §3) remain deliberately unbuilt; the defect scan (Phase 6) is the surface they will slot into when that work is taken up.
