# Eval-Harness Port Implementation Plan

> **For Claude:** This plan is executed **in-session** with TDD (per the owner's "write the plan,
> then execute" instruction) rather than the `executing-an-implementation-plan` `/clear` handoff.
> Each phase ends green (`npm run ci`-clean for what it touches) and is committed.

**Goal:** Port the `citation-checker-script` output-quality apparatus (prompt, verdict taxonomy,
body-usability classifier, voting, the labeled corpus) into wikiharness's `evals`/`core`, as
(a) an extended hermetic eval gate and (b) a new opt-in real-model benchmark — purging the
tranche / line-id / confidence anti-patterns (see `adr/0010`).

**Architecture:** Pure primitives (body-classifier, voting, verdict-normalize, enhanced prompt) land
in `core/citation/` (≥90% coverage). The `evals` package gains a benchmark runner that takes
**injected** core-interface clients (no `edges` runtime dep). The real composition (FetchHttpClient +
ModelClient panel + API keys) lives in `scripts/benchmark.ts`, **not** in `npm run ci`. The existing
hermetic gate (`evaluateCitationDataset`, replaying recorded model responses) is preserved and extended.

**Tech Stack:** TypeScript (Node strip-only via `tsx`), Vitest, Zod (dataset validation), `node:crypto`
(content-hash ids via `core/hash.ts`), the existing edge doubles (`ReplayHttpClient`, `InMemoryObjectStore`,
`ScriptedModelClient`, `makeTestContext`).

**Scope:** 7 phases.

**Codebase verified:** 2026-06-02 (five codebase-investigators: evals contracts, core verdict/prompt,
edges cassette, test/CI conventions, alex source schema).

---

## Grounded contracts this plan builds on (verified, not assumed)

- **Verdict (core):** `VERDICTS = ['SUPPORTED','PARTIAL','NOT_SUPPORTED','SOURCE_UNAVAILABLE']`
  (`core/src/types.ts:18`). `isVerdict(s)` guard. Title-case alex forms ("Partially supported") must be
  normalized. **No confidence anywhere** (enforced by comment + structure; confirmed).
- **Parser (core):** `parseVerdictResponse(text): ParsedVerdict | null`, `canonicalizeVerdict` (`core/src/citation/parsing.ts:76`); fails closed (returns `null`) on garbage.
- **Prompt (core):** `buildVerifyPrompt({claim, sourceText, sourceUrl}): ModelMessage[]` (`core/src/citation/prompts.ts`); already source-only, bans confidence, requires a verbatim `quote`, JSON `{verdict, quote}`.
- **Hash (core):** `sha256Hex(s)`, `canonicalize(v)` (RFC 8785), `hashProposedEdit(e)` (`core/src/hash.ts`).
- **Evals schema:** `LabeledCase { id, claim, sourceUrl, sourceText, expectedVerdict, cohorts?, modelResponse, modelResponseWithIdentity? }` (`evals/src/types.ts:9`). `parseDataset(raw)` Zod-validates, rejects dup ids + invalid verdicts + empty (`evals/src/dataset.ts:24`). `CaseOutcome { id, expected, actual, quoteLocated, hallucinated, cohorts }`.
- **Metrics:** `accuracy`, `confusionMatrix`, `perVerdict` (`evals/src/metrics.ts`); `support` is a CASE COUNT, not a model score.
- **Gates/governance:** `antiHallucinationGate`, `accuracyGate`, `identityInvarianceGate`, `regressionCompare`, `GATE`, `GOVERNANCE_FLOOR=['anti-hallucination','identity-invariance']`, `checkGovernance`, `evaluateCitationDataset` (`evals/src/{gates,governance,suite}.ts`).
- **Edges (test doubles):** `ReplayHttpClient(cassette)` matches `method+url`, throws on miss; `Cassette {entries:[{request:{method,url}, response:{status,headers,body}}]}`. `InMemoryObjectStore` (sha256 content-addressed). `makeTestContext(opts)` assembles a hermetic ctx (default `dryRun:true`, stub http, in-memory everything). `ScriptedModelClient([ModelResponse])` + `modelText`/`modelToolCalls`/`toolCall`. `FetchHttpClient({userAgent,...})` is live, GET/HEAD-only. `scripts/record-fixture.ts` records cassettes (PII-stripped).
- **CI:** `npm run ci` = format → typecheck → lint → boundaries → stubscan → coverage → eval. Coverage ≥90% lines/fns/stmts, 85% branches, on **core + tools only**. Tests are `*.test.ts` colocated, Vitest. Import rule: `evals: ['core','tools','policy']` (edges is a devDep — tests only). Stub-scan bans TODO/FIXME/XXX/HACK/"not implemented" in shipped src.
- **Alex source:** `dataset.json = {metadata, rows:[...]}`; row fields: `id (row_N), article_url, article_title, citation_number, occurrence, total_occurrences, claim_text, claim_container, source_url, source_text, ground_truth, dataset_version, extraction_status, needs_manual_review`. CSV header: `Citation number,Citation instance,Article,Ground truth,Dataset version,WMF claim text,WMF source URL,WMF provenance` (189 rows; ~42% have WMF overrides → `curated-import`). `body-classifier.js`: `classify(text) → {usable, reason}`, constants `SIGNATURE_LEN=500, SHORT_BODY_FLOOR=300, CHROME_LENGTH_CAP=600`, patterns: json_ld_leak, css_leak, anti_bot_challenge, wayback_redirect_notice, wayback_chrome, amazon_stub, short_body. `voting.js`: `computeNClassVote` (4-class plurality, skeptical tiebreaker rank Partial > NotSupported > SourceUnavailable > Supported) + `computeBinaryVoteN`.

---

## Design decisions (from `adr/0010`)

- **No tranches. Content-hash ids. No confidence.** (the three purges)
- **Facet taxonomy:** inherent (`inputProvenance`, stored) · computed (`sourceType`, body-usability — derived, not stored) · assessed (GT + `labelMethod`+`labelAsOf`, stored) · heuristic-assessed (compoundness — **deferred**).
- **GT:** `labelMethod` + `labelAsOf` stored; panel-vs-GT disagreement computed by the benchmark as an audit flag (flags for re-audit, never sets GT).
- **Two source modes:** cassette (frozen, primary) + live (real fetch). Body-classifier splits pipeline-attributed vs model-attributed.
- **Gate vs benchmark:** hermetic gate stays in CI (recorded responses); benchmark is opt-in (real models), composed in `scripts/`.

---

## Phase 1 — Pure core primitives: verdict-normalize, body-classifier, voting

*Functionality. TDD. All in `core/src/citation/`, exported from `core/src/index.ts`, ≥90% coverage.*

- **Task 1.1 — `verdict-normalize.ts`:** `normalizeExternalVerdict(raw: string): Verdict | null` mapping
  alex title-case ("Supported"/"Partially supported"/"Not supported"/"Source unavailable") and common
  variants → the core `Verdict` union; `null` on unrecognized. Tests cover each canonical form, case/space
  variants, and `null` for junk. (Reconcile with existing `canonicalizeVerdict`: reuse it; this is the
  port-facing adapter.)
- **Task 1.2 — `body-classifier.ts`:** `classifyBodyUsability(text: string): { usable: boolean; reason: string }`
  ported from `body-classifier.js` (constants + 7 patterns). Tests: the 10 real unusable audit samples →
  `{usable:false, reason:<pattern>}`; the 7 must-not-flag positives → `{usable:true, reason:'ok'}`; the 3
  edge cases. Pure; ReDoS-safe regexes (bounded).
- **Task 1.3 — `voting.ts`:** `nClassVote(verdicts: readonly Verdict[]): { winner: Verdict; agreement: number; counts: Record<Verdict, number> }` (plurality, skeptical tiebreaker) + `binaryVote(verdicts): { positive: boolean; agreement: number }` (Supported/Partial = positive). `agreement` = measured fraction backing the winner (NOT a model confidence). Tests: unanimous, plurality, tie→tiebreaker, empty→throws or abstains.

**Done when:** all three modules + tests green; exported from index; coverage ≥90%; `npm run lint`/`typecheck` clean. Commit per task.

---

## Phase 2 — Pure core: enhance the verify prompt with the gold framing

*Functionality. TDD. Modifies `core/src/citation/prompts.ts`.*

- **Task 2.1 — Enhance `buildVerifyPrompt`:** fold in the source's two-step framing (STEP 1 source/provenance
  check → `SOURCE_UNAVAILABLE` short-circuit; STEP 2 claim verification), the metadata-is-provenance-not-evidence
  rule, the transliteration-variants rule, the hedged-vs-definitive rule, and the worked examples — adapted to
  wikiharness's output contract: JSON `{verdict, quote}` (the verbatim span the grounding gate needs), **not**
  alex's `{verdict, comments}`, and keep the explicit no-confidence ban. Tests assert the system prompt contains
  the Step-1/Step-2 structure, the SU short-circuit, ≥1 worked example, the no-confidence ban, and still requires
  a verbatim `quote`; assert `parseVerdictResponse` round-trips the documented JSON shape.

**Done when:** prompt tests green; existing `prompts.test.ts` + `parsing.test.ts` still pass; coverage ≥90%. Commit.

---

## Phase 3 — Evals schema v2: content-hash ids, GT provenance, inputProvenance (backward-compatible)

*Functionality. TDD. Modifies `evals/src/types.ts`, `evals/src/dataset.ts`; adds `evals/src/case-id.ts`.*

- **Task 3.1 — Extend `LabeledCase`** with optional, backward-compatible fields: `rationale?: string`,
  `labelMethod?: string`, `labelAsOf?: string`, `inputProvenance?: 'wiki-mined' | 'curated-import'`, and make
  `modelResponse` **optional** (benchmark cases have no recorded response; gate cases do). Existing fixture stays valid.
- **Task 3.2 — `case-id.ts`:** `deriveCaseId(parts): string` = short `sha256Hex(canonicalize({...}))` over the
  identifying tuple (article/citation/occurrence for mined; claim+sourceUrl for imported). Stable under insertion.
- **Task 3.3 — Harden `parseDataset`:** still reject dup ids / invalid verdicts / empty; **additionally reject** any
  case carrying a banned key (`confidence`, `dataset_version`, `tranche`, `version`) — a guard so the purged concepts
  cannot re-enter. Accept the new optional fields. Tests for each new acceptance + each new rejection.

**Done when:** schema + dataset tests green; existing `dataset.test.ts` passes unchanged; the in-CI fixture still loads. Commit per task.

---

## Phase 4 — Dataset port: alex `dataset.json` → wikiharness labeled corpus (re-audited, no tranches)

*Infrastructure + transform. Verified operationally. Adds `scripts/port-citation-dataset.ts` + the output fixture.*

- **Task 4.1 — `scripts/port-citation-dataset.ts`:** read the source `dataset.json` (path arg), for each
  `extraction_status==='complete'` row: map `ground_truth` via `normalizeExternalVerdict`; derive a content-hash
  `id`; set `inputProvenance` from WMF-override presence; set `labelMethod`/`labelAsOf` (default
  `'ported:alex-cite-checker'` / the source extraction date); **drop** `dataset_version`, `confidence`,
  `extraction_status`, `needs_manual_review`; keep `claim`/`sourceUrl`/`sourceText`/`expectedVerdict`. Apply the
  known GT corrections from a small inline correction map (the reversed row + the "claim not in source" rows,
  keyed by content of the row, not by alex id). Emit `packages/evals/fixtures/citation-benchmark.dataset.json`.
- **Task 4.2 — Verify operationally:** run the script; assert the output `parseDataset`-validates, has the expected
  case count, contains **zero** banned keys, all ids are unique 16-hex, and every `expectedVerdict ∈ VERDICTS`.
  A small `*.test.ts` asserts these invariants over the produced fixture (so CI guards the port output).

**Done when:** fixture produced + invariant test green; `npm run boundaries`/`stubscan` clean. Commit.

---

## Phase 5 — Benchmark runner (evals, injected clients): panel × cases, cassette/live, attribution + agreement

*Functionality. TDD with `ScriptedModelClient` + `ReplayHttpClient` (hermetic test of the runner logic). Adds `evals/src/benchmark.ts`.*

- **Task 5.1 — Types:** `BenchmarkResult` (per case×model: `{caseId, model, predicted, expected, quoteLocated, hallucinated, bodyUsable, attribution: 'model'|'pipeline'}`), `BenchmarkReport` (per-model accuracy/lenient/binary + confusion + the ensemble vote per case + measured agreement + the panel-vs-GT disagreement flag).
- **Task 5.2 — `runBenchmark(dataset, { models, http, objects, clock, sourceMode })`:** for each case, obtain source
  bytes — **cassette**: from inline `sourceText` (pre-seed `objects`); **live**: via injected `http` fetch spine —
  run `classifyBodyUsability`; if unusable, record a *pipeline-attributed* `SOURCE_UNAVAILABLE` (no model call);
  else call each injected `ModelClient` with `buildVerifyPrompt`, parse via `parseVerdictResponse`, check the quote
  with `locateQuoteInSource` (hallucination signal). Aggregate accuracy + `nClassVote`/`binaryVote` agreement +
  model-vs-pipeline split + the GT-disagreement flag. Takes only **core interfaces** (no `edges` import).
- **Task 5.3 — Tests:** scripted-model + replay-http drive: a usable-source case where the panel agrees → correct +
  high agreement; an unusable-source case → pipeline-attributed SU, zero model calls; a hallucinated quote →
  flagged; a case where the whole panel disagrees with GT → disagreement flag set. Hermetic, deterministic.

**Done when:** runner + tests green; `evals` import boundary clean (no edges runtime dep). Commit per task.

---

## Phase 6 — Composition + report: `scripts/benchmark.ts` (real edges, opt-in) + GT-audit flag report

*Infrastructure + a pure-report unit test. Adds `scripts/benchmark.ts` + `scripts/lib/benchmark-report.ts`.*

- **Task 6.1 — `scripts/lib/benchmark-report.ts`:** pure `renderBenchmarkReport(report): string` (markdown) + the
  pure `auditFlags(report): string[]` (case ids where ≥ threshold of the panel disagrees with GT). Unit-tested.
- **Task 6.2 — `scripts/benchmark.ts`:** composition root — load the ported fixture, build a real `ModelClient`
  panel + (live mode) `FetchHttpClient` from env (API keys; refuses to run real models without keys), call
  `runBenchmark`, write `report.md` + `report.json` + the audit-flag list. **Not** added to `npm run ci`. A
  hermetic `*.test.ts` exercises the script's pure pieces (report, audit flags) with a scripted panel; the real
  run is documented as `tsx scripts/benchmark.ts --mode cassette` (keys via env).

**Done when:** report/audit unit tests green; script runs end-to-end against a scripted/fake panel in the test; documented invocation. `boundaries`/`stubscan` clean. Commit per task.

---

## Phase 7 — Wire-up, docs, and the stage-end bug-hunt

*Infrastructure + docs. Operational verification (`npm run ci` green) + ADR-0007 bug-hunt.*

- **Task 7.1 — Activate the enhanced prompt** in the live verify path (confirm `buildVerifyPrompt` is the one the
  `verify_citation` tool uses) and re-run the hermetic gate (`npm run eval`) to confirm the existing fixture still passes.
- **Task 7.2 — Docs:** finalize `adr/0010`; update `CLAUDE.md` doc-map (ADRs `0001`–`0010`) and the reuse-sources note
  (prompt/classifier/voting/dataset ported); add a short `packages/evals/README.md` describing gate-vs-benchmark + how to run the benchmark.
- **Task 7.3 — ADR-0007 stage-end bug-hunt:** run the multi-agent adversarial bug-hunt over the new code (prompt,
  classifier, voting, dataset port, benchmark runner, composition). Fix all Critical + Important + Minor before done.
- **Task 7.4 — Full `npm run ci` green**; commit; (owner) merge to `main`.

**Done when:** `npm run ci` green; bug-hunt findings resolved; docs updated; branch ready to merge.

---

## Out of scope (explicit)

- Compoundness / fragmentariness / difficulty facets (heuristic-assessed; pipeline being rebuilt — `adr/0010`).
- Inter-human GT agreement (needs labeling infra we don't have).
- Citoid bibliographic augmentation, query-aware extraction, Wayback `id_` rewrite (source-fetch robustness — later).
- Adding the benchmark to CI (it needs keys + network by nature).
