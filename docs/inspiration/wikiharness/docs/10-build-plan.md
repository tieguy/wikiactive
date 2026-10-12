# 10 — Build Plan (milestones & sequencing)

> **Staging — HOW (tentative).** Architecture, contracts, and sequencing here are **exploratory, not locked** — a current best guess, revisable freely until M0 (base framework now settled — `adr/0002`, accepted 2026-06-02). The settled *what* lives in `00`–`02`/`11`. See [Staging](../README.md#staging).

Concrete, testable increments. Each milestone has an explicit **exit gate**; no
milestone is "done" until its gate is green. Testability is built in from M0, not
retrofitted.

## How we build: the planning loop (ADR-0007)

Each milestone (and any non-trivial task within it) runs the ed3d-style loop:

```
design-doc  →  (fresh) implementation-plan  →  execute  →  end-of-stage bug-hunt
   │                    │                         │                │
 codebase-grounded   regenerated fresh        TDD per 04      adversarial review
 (explore real code)  against current tree                    + fix loop, 0 critical
```

- **Design before code; plan fresh.** The implementation plan is regenerated at
  execution time so it reflects the *current* tree, not stale design assumptions;
  phases are context-isolated.
- **End every stage with a bug-hunt** (ADR-0007): a reviewer distinct from the
  implementer hunts for bugs and the issues are fixed and re-reviewed until zero
  critical findings remain — *before* the exit gate is evaluated.
- Plans and design docs are **git-committed artifacts** (auditable, like everything
  else here).

## M0 — Skeleton & test harness (no features)
- Monorepo per `06-architecture.md` (`core`, `edges`, `tools`, `agent`, `policy`,
  `server`, `cli`, `web`, `evals`, `fixtures`).
- Define the core interfaces (`HttpClient`, `WikiClient`, `ModelClient`,
  `Storage`, `ObjectStore`, `Clock`) + their **test
  doubles** (replay/stub/in-memory/fixed) **first**.
- Wire `ModelClient` to the Vercel AI SDK; test the **adapter** with a
  `MockLanguageModelV2`-backed double. Drive the **headless loop** through a separate,
  SDK-agnostic **scripted `ModelClient` double** (not `MockLanguageModelV2` directly) so
  loop tests don't couple to the SDK's stream-chunk shape (`adr/0002` resolution, note 1).
- CI: lint, typecheck, import-boundary rule, coverage, hermetic test run.
- **`03` UX redline is an M0 input** (`adr/0005` D5; owner stance *UX trumps
  architecture*): confirm the redlined interaction model before finalizing the API
  surface / SSE event payload. Revisit `adr/0002` only if the redline forces a different
  runtime shape — current assessment: it won't, since the durable-HITL/SSE/one-API shape
  is required by every open `03` option.
- **Exit gate:** a "hello" tool runs in a headless loop test driven by a scripted
  `ModelClient` double, zero network, in CI; the `03` UX redline is confirmed (or the
  API surface is explicitly marked provisional pending it).

## M1 — MVP-1 read path (verify, no writes)
Port from `citation-checker-script` and SIFT:
- `get_article`, `extract_claim` (← `claim.js`), `resolve_citation_url` (←
  `urls.js`), `fetch_source` (← SIFT `web_fetch`), `verify_citation` (←
  `prompts.js`/`parsing.js`/`verdicts.js`), `locate_quote_in_source`.
- Provenance assembly; verdict + evidence output.
- **Exit gate:** Tier-1/2 tests + fixtures for every tool; coverage ≥ 90%; the
  anti-hallucination unit gate passes; verdict accuracy on the ported labeled set
  ≥ citation-checker baseline.

## M2 — Proposal + policy + HITL state machine (still dry-run)
- `enrich_citation` (Citoid), `repair_citation`, `find_archive_snapshot` (read-only
  Wayback), `add_archive_to_citation` (attaches an **existing** snapshot) → all emit
  `ProposedEdit`s only. (`archive_url` / Save-Page-Now is post-MVP-1 — no IA writes
  in MVP-1.)
- `evaluate_policy` + first YAML policy (`suggested`); `format_edit_summary`
  (AI-disclosure); `render_diff`.
- The **persisted approval state machine** (`AWAITING_CONFIRMATION` → approve/edit/
  reject) on the `Storage` edge; audit ledger append.
- **Exit gate:** Tier-4 contract tests — approve / edit-approve / reject /
  resume-after-reload / **no-write-without-token** — all green against the API.

## M3 — Server + CLI + web (one core, two clients)
- `server`: `/sessions`, SSE `/events`, proposal decisions, `/resume`, `/audit`;
  OAuth2 token custody server-side.
- `cli`: interactive verify+confirm loop.
- `web`: citation list + diff/evidence/provenance pane + approve/edit/reject.
- **Exit gate:** same Tier-4 suite passes through the HTTP boundary; a human
  completes an assisted edit on **`test.wikipedia.org`** from **both** CLI and web;
  provenance recorded in the ledger; production writes still disabled (kill-switch).

## M4 — Eval gates + governance + graduate MVP-1
- Eval harness (`05`: labeled datasets, metrics, gates) wired into CI;
  `check-governance` script enforces `suggested→candidate→active` with
  `required_gates`.
- Promote the MVP-1 citation policy to `candidate`, then `active` once gates pass.
- BRFA prep if cadence warrants; dedicated account.
- **Exit gate:** MVP-1 policy `active`; anti-hallucination gate = 0 violations on
  eval set; documented graduation checklist satisfied.

## M4b — External evidence harness + on-wiki outcome loop
The strategic deliverable from `11-evidence-and-policy-strategy.md` / `adr/0003`:
turn the internal eval machinery into **community-credible evidence.**
- Reproducible, public eval datasets with documented labeling protocol +
  inter-annotator agreement; pre-registered methodology & thresholds.
- Comparative reporting vs. baselines (experienced humans; Citoid/IABot/ORES;
  prior version) + adversarial/red-team suite.
- **On-wiki outcome measurement loop:** instrument committed assisted edits and
  *follow up* — acceptance (approve/edit/reject), reversion/durability vs. human
  baseline, reviewer effort — using the recent-changes/diff plumbing.
- A public evidence report generator (the artifact attached to any RfC/BRFA ask).
- **Exit gate:** a reproducible report exists that a third party can re-run;
  introduced-error and hallucinated-citation rates measured and published; on-wiki
  reversion tracked for a pilot batch.

## M5 — MVP-2 (IA book-scan sourcing)
- `search_books` (Open Library), `search_inside_book` (IA Search-Inside — confirm
  JSON shape against a live item; pin behind adapter + fixtures),
  `verify_book_support`, `propose_book_citation` (`{{cite book}}` w/ `page=` +
  deep link), optional `propose_book_edition_item` (Wikidata P648/P212/P957/P724).
- **Exit gate:** same testability bar; a page-accurate `{{cite book}}` proposed,
  confirmed, and committed to a test wiki, with the quote located in OCR.

## M6 — MVP-3: peacock/puffery enhancer — ADR-0007
A bounded demonstrator that **blocking hooks + multi-model voting** make small,
concise *content* improvements safely on **MOS:PEACOCK (puffery)** — a rule
traditional non-NLP bots can't evaluate (rule rationale + dataset: `01`, `12 §C-bis`).
- `detect_puffery` (NLP judgment over prose), `verify_puffery` (**N-of-M multi-model
  agreement** hook), `propose_puffery_fix` (flag `{{Peacock term}}` *or* minimal
  concise edit), behind a PreToolUse scope/policy gate + PostToolUse verification.
- Hard-gated as text-adjacent (`02 §1`): flag-first; any minimal text change is
  multi-model-agreed, human-confirmed, and disclosed.
- Reuse the `abertsch72/wikipedia-puffery-detection` data + distant supervision from
  `{{Puffery}}`/`{{Peacock}}` templates; **citation features** feed from the MVP-1
  core. Target: beat the ~0.963 F1 RoBERTa baseline at the agreement threshold.
- **Exit gate:** same testability bar; multi-model-agreement gate meets its declared
  precision threshold on the puffery labeled set; a human confirms a small, concise
  improvement on a test wiki with full provenance.

## M6b — MVP-3 follow-on: weasel enhancer (composability proof)
Swap `detect_weasel` / `{{Weasel inline}}` onto **the same** `verify`/`propose`
pipeline and hooks from M6. The point is to add a second rule with **minimal new
code** — demonstrating the enhancer framework (`adr/0005`) composes.
- Eval on **WikiWeasel/CoNLL-2010** (~20.7k sentence-level labels) vs. its
  shared-task baselines.
- **Exit gate:** weasel enhancer passes the same gates **reusing** M6's
  verify/propose/hook machinery (diff shows the rule is a parameter, not a rewrite).

## M7+ — Wikidata enrichment → GLAM media → text
Per `09-roadmap.md`, each as its own milestone with the same exit-gate discipline
and per-sink production graduation (`09` "graduating a write path").

## Definition of done (every milestone)
1. New tools have typed contracts + fixtures; Tiers 1–4 green & hermetic.
2. Coverage ≥ 90% on `core`/`tools`.
3. Any prompt/policy change passes its eval gates (`05`).
4. No write path enabled past its guards; kill-switch + dry-run defaults intact.
5. Docs/ADRs updated if a decision changed.
6. **End-of-stage bug-hunt passed** (ADR-0007): adversarial review by a non-author,
   fixes applied, **zero critical findings** remaining.
