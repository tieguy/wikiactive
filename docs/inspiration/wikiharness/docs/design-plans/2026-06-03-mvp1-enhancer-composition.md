# MVP-1 Enhancer Composition Design

> Resolves chainlink **#13** (compose the full MVP-1 enhancer set into `improve()`),
> with **#7** (enforcement-pipeline reconcile) as its first phase. Brainstormed
> 2026-06-03. The grounding fact→judgment gradient surfaced here is recorded in
> `docs/07` + `docs/11`.

## Summary

`improve()` is the function the CLI and server both call to analyze a Wikipedia article and
surface improvements. Until now it has been a single hard-wired tool call — essentially one trick.
This design makes it extensible: `improve()` becomes a loop over a **registry of enhancers**, each
of which can propose edits or flag issues independently. Three enhancers ship with this work:
`bare-url-repair` (fix unformatted citation URLs), `wayback-archive` (attach existing Wayback
Machine snapshots to citations), and `citation-verify` (run the model-assisted verification spine
and produce a graded verdict for every citation). Malformed-citation repair is deferred because it
requires a wikitext parse tree that is not yet built (#9).

The other major structural change is unifying the enforcement pipeline. Currently, grounding
checks (the anti-hallucination gate that verifies a model's supporting quote can actually be found
in the fetched source) and policy checks run in two separate places, creating a risk that one path
drifts from the other. This design consolidates them into a single post-hook chain —
`[groundingVerifier, policyHook]` — used identically by the agent loop and by `improve()`. A second
output kind, `Finding`, is also plumbed end-to-end: unlike `ProposedEdit`s (which require a human
confirmation token before any wiki write), findings are immutable read-only verdicts — they are
stored, served over the API, and rendered in the CLI, but they never enter the write path.
Together, these changes close chainlink issues #7 and #13.

## Definition of Done

- `improve()` (the product/CLI/server path) composes a **registry of Wikipedia-article
  enhancers** rather than hand-wiring a single tool. #13 ships three: `citation-verify`,
  `bare-url-repair` (existing), `wayback-archive`. Malformed-cite repair is **deferred to #9**
  (needs the wikitext-AST).
- `citation-verify` runs the full read spine (`resolve_citation_url → fetch_source →
  verify_citation`) per citation and surfaces **one `Finding` per citation** carrying the graded
  verdict — every citation, not only problems.
- `Finding` is plumbed **end-to-end**: it gains an `evidence` field, has its own immutable
  (no-confirmation) store, an API route, and a CLI rendering. The confirmation-token commit path
  is untouched — findings never write.
- The **anti-hallucination invariant holds for findings**: a verdict whose supporting quote is
  not locatable in the fetched source is suppressed pre-human by `groundingVerifier`. A correct
  `NOT_SUPPORTED` verdict (which has no supporting quote) is **not** suppressed — it grounds on
  source-provenance instead.
- **One enforcement pipeline** (#7 Option B): grounding + policy live in a single canonical
  post-hook chain consumed by both the agent loop and `improve()`, running over both
  `ProposedEdit`s and `Finding`s. The #7 drift risk is closed.
- The model is **env-gated**: with no API key the server runs key-free and skips verification
  with a notice; with a key, verification runs against the pinned open model. Mirrors the
  write-path's `resolveWriteConfig` gating.
- `npm run ci` is green (format, typecheck, lint, import-boundary incl. a new `enhancers`
  entry, stub-scan, coverage ≥90% on `core`/`tools`, hermetic eval gate now covering a finding).
  No test touches the network; nothing writes to a real wiki.

## Glossary

- **`improve()`**: The top-level function called by both the CLI and server to analyze a Wikipedia
  article and produce suggested improvements. Currently hard-wired; this design makes it
  registry-driven.
- **Enhancer / `WikipediaArticleEnhancer`**: A plugin that inspects an article and returns zero or
  more `ProposedEdit`s and/or `Finding`s. Each declares a `requires` set of capabilities;
  `improve()` skips it if the runtime cannot satisfy them.
- **Enhancer registry (`ENHANCERS`)**: The ordered list of active enhancers `improve()` iterates.
  Currently an empty placeholder in `packages/enhancers/`; this design populates it for the first
  time.
- **`ProposedEdit`**: A structured, human-confirmable suggestion to change a Wikipedia article.
  Requires a confirmation token bound to the exact edit hash before any write can occur.
- **`Finding`**: A read-only, immutable verdict about an article element (e.g., a citation's
  verification status). Never enters the write path; has its own store, API route, and CLI
  rendering.
- **Enforcement pipeline**: The canonical post-hook chain `[groundingVerifier, policyHook]` that
  every output (edit or finding) passes through before being surfaced. Defined once and shared
  across orchestration sites.
- **`groundingVerifier`**: A post-hook that enforces the anti-hallucination invariant. For outputs
  claiming a supporting quote, it checks that the quote string-locates in a source actually fetched
  this session; outputs that fail are suppressed and logged to the audit stream.
- **Grounding assertion / `grounding.kind`**: A machine-checkable claim attached to an output
  describing what evidence backs it. Two profiles used here: `located-quote` (a specific substring
  must exist in the fetched source) and `source-fetched` (the source was fetched; no quote
  required — used for `NOT_SUPPORTED` verdicts).
- **`policyHook`**: An adapter that wraps `evaluatePolicy` (the existing rules engine from
  `packages/policy`) into the shared post-hook shape. A no-op for `Finding`s, which never trigger
  policy.
- **`NOT_SUPPORTED` verdict**: A graded verdict meaning the fetched source does not support the
  claim being cited. Legitimately has no supporting quote; uses `source-fetched` grounding so it is
  not wrongly suppressed.
- **Capability gating**: The mechanism by which `improve()` advertises available runtime
  capabilities (e.g., `'model'`, `'htmlExtractor'`) and skips enhancers whose `requires` set is not
  satisfied. Mirrors the write-path's `resolveWriteConfig` pattern.
- **`resolveModelConfig`**: A pure, env-derived function (mirroring `resolveWriteConfig`) that
  decides whether a real `ModelClient` is wired. Default-closed: without an API key, the server
  starts key-free and skips model-dependent enhancers with a notice.
- **`ScriptedModelClient`**: A test double for the model layer that replays a scripted sequence of
  responses, allowing the full verification spine to be exercised hermetically.
- **Cassette / `ReplayHttpClient`**: A recorded-HTTP test double that replays previously captured
  responses so tests are hermetic (no live network).
- **`FindingStore`**: A new simple store (mirroring `ProposalStore`) for persisting `Finding`s. No
  status machine — findings are immutable once written.
- **`citation-verify` enhancer**: The model-dependent enhancer that walks every citation through
  `resolve_citation_url → fetch_source → verify_citation` and emits one `Finding` per citation
  carrying the graded verdict.
- **`bare-url-repair` enhancer**: A deterministic (no model required) enhancer that detects
  unformatted citation URLs and proposes properly templated replacements.
- **`wayback-archive` enhancer**: A deterministic enhancer that looks up existing Wayback Machine
  snapshots for citations and proposes attaching them. Read-only (does not trigger Save-Page-Now).
- **`DefuddleHtmlExtractor` / `HtmlExtractor`**: An HTML main-content extractor (`defuddle` +
  `linkedom`) that strips boilerplate from a fetched page before passing text to the model;
  implements the `HtmlExtractor` core interface (`adr/0011`).
- **`PostToolUseHook`**: The hook interface (a function over `(call, result, ctx)`) the agent loop
  runs after each tool invocation. The enforcement pipeline is composed of these.
- **HITL (Human-in-the-Loop)**: The confirmation-token state machine ensuring a human approves
  every `ProposedEdit` before any wiki write. Findings bypass this entirely.
- **`locateQuoteInSource`**: A pure function in `core/` checking whether a substring appears in a
  fetched source body — the foundation of the `located-quote` profile and the anti-hallucination
  invariant.
- **`adr/0005`**: Architecture Decision Record for treating enhancers as first-class, registrable
  plugins — the design contract this work implements for the first time.

## Architecture

`improve()` changes from a single hand-wired tool call into a **capability-gated enhancer
registry feeding one enforcement pipeline and two output channels**.

**Enforcement pipeline (one place).** A canonical post-hook chain `[groundingVerifier,
policyHook]` is defined once in a shared composition module and consumed by both the agent loop's
post-hook runner and `improve()`. It runs over **both** output kinds: `policyHook` (wrapping the
existing `evaluatePolicy`) is a no-op for `Finding`s (they never write); `groundingVerifier`
always applies. This is #7 Option B — it eliminates the two-orchestration-sites drift risk.

**Grounding is fabrication-resistance, not judgment-correctness.** `groundingVerifier` dispatches
on `grounding.kind`. #13 uses two profiles: `located-quote` (a supporting quote must string-locate
in a source fetched this session) and a new no-quote `source-fetched` profile (the source must be
in the object store; no quote required) for `NOT_SUPPORTED`/`SOURCE_UNAVAILABLE` verdicts. The
judgment regime (puffery, multi-model voting) is out of scope — see `docs/11` and `adr/0007 §4`.

**Enhancer registry.** Each enhancer implements `WikipediaArticleEnhancer` — `analyze(subject,
ctx) → { proposedEdits, findings }`, with a `requires` capability declaration. `improve()`
iterates the registry, runs only enhancers whose `requires ⊆ ctx capabilities` (logging the
skipped ones), wraps each in try/catch (a failing enhancer is skipped, partial results survive),
then routes every output through the pipeline before persisting surfaced ones.

**Two output channels.** `ProposedEdit`s (bare-url, Wayback) keep the durable
`AWAITING_CONFIRMATION` → confirmation-token → commit lifecycle. `Finding`s (verdicts) are
**immutable read-only flags** with a separate, simpler store; they are surfaced to the human and
never enter the commit path.

**Model is env-gated.** A pure `resolveModelConfig(env)` (beside `resolveWriteConfig`) decides
whether a real `ModelClient` is wired; the `'model'` capability — decoupled from "real vs
scripted" — gates `citation-verify`. Tests wire a `ScriptedModelClient` and advertise `'model'`;
production without a key advertises no `'model'` and skips verification.

## Existing Patterns

This design follows established repo patterns; investigation (codebase-investigator, 2026-06-03)
confirmed current state with file:line references.

- **Pure-config gating** follows `packages/server/src/write-config.ts` (`resolveWriteConfig` —
  default-closed, unit-tested, env-derived). `resolveModelConfig` mirrors it exactly.
- **Post-hook enforcement** follows `packages/hooks/src/grounding-verifier.ts` (a
  `PostToolUseHook` over `(call, result, ctx)`). `policyHook` adapts the existing
  `evaluatePolicy` (`packages/policy`) into the same shape.
- **The outputs store** follows `packages/outputs/src/proposal-store.ts` (`ProposalStore` over
  the `Storage` edge, session-prefixed keys, shared audit stream). `FindingStore` mirrors it,
  minus the status machine (findings are immutable). The package is already named
  "findings/proposals store" — this fills the intended half.
- **The enhancer contract** realizes `adr/0005` (enhancers as first-class plugins) and
  `docs/06`/`07`. `packages/enhancers` is currently an empty placeholder (`ENHANCERS = []`);
  this is its first real content.
- **Composition over the full read spine** follows the existing
  `packages/agent/src/mvp1-pipeline.integration.test.ts`, which already wires `get_article →
  resolve → fetch → verify → ground` with `ScriptedModelClient` + cassette doubles.
- **`improve()` already reuses `groundingVerifier`** inline (`packages/agent/src/hitl.ts:95–116`);
  Phase 1 moves that reuse into the shared chain rather than introducing it.

**Divergence:** the enhancer contract is intentionally **concrete-Wikipedia** (no
`Enhancer<TSubject>` generic) despite `adr/0005`'s multi-domain framing — strict YAGNI, matching
the #9 wikitext-AST deferral. Generalizing to Wikidata (`09` Phase 3) is a known, bounded future
refactor of this interface + the registry + the session entry point; downstream
(`EnhancerResult`/`ProposedEdit.sink`/`Finding`/gate/HITL/store) stays neutral and unaffected.

## Implementation Phases

### Phase 1: Unified enforcement pipeline (closes #7)

**Goal:** One canonical post-hook chain over both edits and findings; the foundation everything
else routes through.

**Components:**
- Core types in `packages/core/src/types.ts` — add `Finding.evidence: readonly Evidence[]`; add
  the no-quote `source-fetched` variant to `GroundingAssertion`.
- `groundingVerifier` in `packages/hooks/src/grounding-verifier.ts` — verify the `source-fetched`
  predicate (source present in object store, no quote required); iterate `result.findings[].evidence`
  in addition to `result.evidence` and `result.proposedEdits[].evidence`.
- `policyHook` in `packages/hooks/` — adapt `evaluatePolicy` to `PostToolUseHook`; no-op for findings.
- A shared composition module exporting the canonical chain `[groundingVerifier, policyHook]`,
  consumed by the agent loop's runner and by `improve()`.
- `improve()` in `packages/agent/src/hitl.ts` — replace the inline policy+grounding block with the
  shared chain runner (behavior-preserving for edits).

**Dependencies:** None (first phase).

**Done when:** Characterization tests pin current `improve()` suppression (policy-fail,
grounding-fail, pass) and stay green; a test proves a hook added to the shared chain is honored by
**both** the loop and `improve()`; the `source-fetched` predicate and `policyHook` are unit-tested.

### Phase 2: Finding plumbing end-to-end

**Goal:** A `Finding` can be persisted, served, and rendered — independent of any enhancer.

**Components:**
- `FindingStore` in `packages/outputs/src/` — `save`/`get`/`list` over `Storage`, key
  `finding:{sessionId}:{id}`; `FindingRecord { finding, sessionId, surfacedAt }`. Export from
  `packages/outputs/src/index.ts`.
- `SessionResult.findings` in `packages/agent/src/` and a `GET /api/sessions/{id}/findings` route
  in `packages/server/src/app.ts` (proposals/decide/audit routes unchanged).
- `printFinding` in `packages/cli/src/main.ts` — verdict chip, claim, located quote (or
  `(no supporting quote — NOT_SUPPORTED)`), source URL. No `decide`/`edit` verb for findings.

**Dependencies:** Phase 1 (`Finding.evidence`).

**Done when:** Tests show a fixture finding persists, returns over the API, and renders in the
CLI; findings carry no confirmation affordance.

### Phase 3: Enhancer registry + deterministic enhancers + improve() rewrite

**Goal:** `improve()` composes a registry; the two deterministic enhancers produce proposals.

**Components:**
- `packages/enhancers/src/` — `WikipediaArticleEnhancer`/`WikipediaArticleSubject`/`EnhancerResult`
  types; the `ENHANCERS` registry; `bare-url-repair` (wraps the existing `repairCitationTool`);
  `wayback-archive` (existing-snapshot lookup, read-only, `diff` grounding + Wayback-availability
  provenance).
- `improve()` in `packages/agent/src/hitl.ts` — build `WikipediaArticleSubject` (`getWikitext` +
  `get_article`/`parseArticle`), iterate `ENHANCERS` (capability-gated, try/catch), route outputs
  through the Phase-1 chain, persist edits → `ProposalStore`, findings → `FindingStore`.

**Dependencies:** Phase 1 (chain), Phase 2 (`FindingStore`).

**Done when:** `improve()` over an article with a bare URL surfaces a bare-url `ProposedEdit`;
over a citation with an existing Wayback snapshot surfaces a `wayback-archive` edit; a throwing
enhancer is skipped without aborting the session; tests green.

### Phase 4: Env-gated model wiring

**Goal:** Verification turns on by configuration; the server stays runnable key-free.

**Components:**
- `resolveModelConfig(env)` in `packages/server/src/` (beside `write-config.ts`) — default-closed
  `ModelConfig`.
- `NoModelClient` in `packages/edges/src/` — throws if `complete` is called (safety net).
- `packages/server/src/main.ts` — construct a real `VercelModelClient` when `modelEnabled`, else
  `NoModelClient`; build the `capabilities` set; pass it to `SessionService`. Startup banner gains
  a `verification: on/off` line.
- `SessionService`/`improve()` — accept and apply `capabilities` (filter enhancers).

**Dependencies:** Phase 3 (registry + `improve()` capability filtering).

**Done when:** `resolveModelConfig` is unit-tested (key→enabled, absent→disabled); the capability
gate filters enhancers; the server starts both with and without a key.

### Phase 5: citation-verify enhancer

**Goal:** The verification spine — a graded `Finding` per citation, anti-hallucination enforced.

**Components:**
- `citation-verify` in `packages/enhancers/src/` — per citation `resolve_citation_url →
  fetch_source → verify_citation`; emit one `Finding` (`kind:'citation-verdict'`, verdict in
  `detail`) with `located-quote` (SUPPORTED/PARTIAL) or `source-fetched` (NOT_SUPPORTED/
  SOURCE_UNAVAILABLE) grounding; `requires: ['model','htmlExtractor']`; ≤3 per-citation
  concurrency. Register in `ENHANCERS`.

**Dependencies:** Phase 3 (registry), Phase 4 (model capability), Phase 1 (grounding profiles).

**Done when (Tier 2/3, hermetic):** SUPPORTED-with-locatable-quote surfaces a `located-quote`
finding; NOT_SUPPORTED surfaces a `source-fetched` finding and is **not** suppressed for lacking
a quote; a **hallucinated quote** (not in source) is **suppressed** by `groundingVerifier` with a
`finding_suppressed` audit entry; with the `'model'` capability absent the enhancer is skipped and
`NoModelClient` is never called.

### Phase 6: End-to-end integration + CI gates

**Goal:** The full path proven hermetically; CI green.

**Components:**
- Extend `packages/agent/src/mvp1-pipeline.integration.test.ts` (or a sibling) — `improve()` over
  the real recorded article with `ScriptedModelClient` + cassette, `capabilities={model,
  htmlExtractor}`, asserting **both** channels populate and `wiki.appliedEdits === 0`.
- `scripts/check-boundaries.ts` — add `enhancers → [core, tools]`.
- The hermetic eval gate (`packages/evals`) — exercise a finding so the 0-hallucination gate
  covers findings, not only edits.

**Dependencies:** Phases 1–5.

**Done when:** `npm run ci` is green end-to-end; the e2e test asserts two channels and zero writes.

## Additional Considerations

**Running the server now makes model calls.** With a key configured, `improve()` issues model
requests (network) on top of the read-only source fetches it already makes. This is orthogonal to
the write gate, which stays default-closed. Keys live where the benchmark's do
(`../alex-cite-checker/.env`, owner-authorized).

**Sequential enhancers, concurrency inside.** Enhancers run sequentially for deterministic
proposal ordering; per-citation concurrency (≤3, `08`) lives inside `citation-verify`. If
verification latency becomes a problem, parallelizing enhancers is a later refinement, not a
contract change.

**Future extensibility (deferred, not built):** generalizing to Wikidata (`09` Phase 3) reopens
the enhancer interface (→ `Enhancer<TSubject>`), the registry (per-subject-type), and the session
entry point; Wikidata's "no unsourced statements" invariant becomes a new `statement-referenced`
grounding kind, slotting in like `in-article-span` for puffery. Recorded on #13/#9.
