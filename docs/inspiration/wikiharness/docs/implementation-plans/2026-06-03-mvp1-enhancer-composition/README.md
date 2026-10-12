# MVP-1 Enhancer Composition — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `ed3d-plan-and-execute:executing-an-implementation-plan`
> to implement this plan task-by-task. Execute phases in order (`phase_01` → `phase_06`); each
> phase ends with `npm run ci` green and a commit.

**Goal:** Turn `improve()` (the CLI/server product path) from a single hard-wired tool call into a
capability-gated **enhancer registry** feeding **one** enforcement pipeline and **two** output
channels (`ProposedEdit`s + `Finding`s) — closing chainlink **#7** (Phase 1) and **#13** (Phases 2–6).

**Design source:** `docs/design-plans/2026-06-03-mvp1-enhancer-composition.md`.

**Architecture:** A canonical post-hook chain `[groundingVerifier, policyHook]` (built by a single
`enforcementPipeline(policy)` factory) is consumed identically by the agent loop and `improve()`.
`Finding`s gain an `evidence` field and a `source-fetched` grounding profile, get an immutable
`FindingStore`, an API route, and a CLI rendering. **Two** enhancers ship: `bare-url-repair` (wraps
the existing `repair_citation` tool) and `citation-verify` (the model spine, one graded `Finding`
per citation). `wayback-archive` is **deferred to #9** (decided 2026-06-03 — attaching an
`archive-url=` needs the wikitext-AST). The model is env-gated (`resolveModelConfig`,
default-closed) like the write path. One new pure-`core` primitive — `mapWithConcurrency`
(`core/concurrency.ts`) — is extracted **once** and reused by both `citation-verify` (≤3 per
citation) and the existing benchmark runner, retiring a duplicate worker pool.

**Tech stack:** TypeScript (NodeNext ESM, `.ts` import specifiers), Vitest 3, Zod, a pure-`core` /
DI-`edges` monorepo. New runtime libs: none — every external dependency (`ai`, `defuddle`,
`linkedom`, `mwn`, `@ai-sdk/openai-compatible`) is already wrapped by an existing edge; Phase 4
only *reuses* `@ai-sdk/openai-compatible` (already a root devDep used by the benchmark) from the
server composition root.

**Scope:** 6 phases (the full design). Codebase **verified 2026-06-03** by 8 parallel
codebase-investigators (see "Verification" below).

---

## Phases

| # | File | Closes | Summary |
|---|------|--------|---------|
| 1 | `phase_01.md` | **#7** | Unified enforcement pipeline: `policyHook` + `enforcementPipeline(policy)` + `runEnforcement`; `Finding.evidence` + `source-fetched` grounding; `groundingVerifier` iterates findings; behavior-preserving `improve()` refactor (characterization-tested). |
| 2 | `phase_02.md` | #13 | `Finding` plumbed end-to-end: `FindingStore` (immutable), `SessionResult.findings`, `GET /api/sessions/{id}/findings`, CLI `printFinding`. |
| 3 | `phase_03.md` | #13 | Enhancer contract + `ENHANCERS` registry; `bare-url-repair` (wraps `repair_citation`); `improve()` becomes a capability-gated registry loop. Adds the `agent → enhancers` import boundary. (`wayback-archive` **deferred to #9**.) |
| 4 | `phase_04.md` | #13 | `resolveModelConfig(env)` (default-closed) + `NoModelClient`; capability set threaded into `SessionService`/`improve()`; server runnable key-free; startup banner `verification: on/off`. |
| 5 | `phase_05.md` | #13 | `citation-verify` enhancer: per-citation `resolve → fetch → verify`, one graded `Finding` each, `located-quote`/`source-fetched` grounding, ≤3 concurrency via a new pure `mapWithConcurrency` (also adopted by the benchmark — no duplicate pool). |
| 6 | `phase_06.md` | #13 | End-to-end `improve()` integration test (both channels populate, `appliedEdits === 0`); eval gate exercises a `Finding`; `npm run ci` green. |

**Dependencies:** 1 → 2 → 3 → 4 → 5 → 6 (each phase builds on the prior; 3 needs 1+2, 5 needs 1+3+4).

---

## Anti-duplication ledger (the #1 review lens for this work)

The owner's explicit priority: **do not reinvent the wheel — internal or external.** #7 itself is
a de-duplication (two enforcement paths → one). Every new module below **reuses** an existing one;
the executor must call these, not re-implement them:

| New code | MUST reuse (import path) | Instead of hand-rolling |
|---|---|---|
| `policyHook` | `evaluatePolicy` (`@wikiharness/policy`) | policy logic |
| `enforcementPipeline`/`runEnforcement` | `groundingVerifier` (`@wikiharness/hooks`), the loop's hook-runner shape | a second grounding/policy gate |
| `groundingVerifier` (findings + `source-fetched`) | `locateQuoteInSource` (`@wikiharness/core`), `ctx.objects.has` | substring search / store probing |
| `FindingStore` | `ProposalStore` pattern + shared key helpers (`@wikiharness/outputs`), the `Storage` edge | a bespoke store / new key scheme |
| `bare-url-repair` | `repairCitationTool` (`@wikiharness/tools`) | bare-URL detection/templating |
| `citation-verify` | `resolveCitationUrlTool`/`fetchSourceTool`/`verifyCitationTool` (`@wikiharness/tools`), `buildVerifyPrompt`/`parseVerdictResponse` (`@wikiharness/core`) | the verify spine / prompt / parser |
| `mapWithConcurrency` | extracted to `@wikiharness/core`, **then** adopted by `evals/benchmark.ts` | a second worker pool / `pLimit` |
| `resolveModelConfig` | `resolveWriteConfig` pattern (`packages/server/src/write-config.ts`) | a new env-gating style |
| real model wiring | `VercelModelClient` (`@wikiharness/edges`) + `@ai-sdk/openai-compatible` (as in `scripts/benchmark.ts`) | importing `ai` directly / a new adapter |
| `GET …/findings` route | the `apiRoutes` pattern (`packages/server/src/app.ts`) | a new routing mechanism |
| `printFinding` | `CliDeps` + `printProposal` style (`packages/cli/src/main.ts`) | a new output abstraction |
| all tests | `makeTestContext`, `ScriptedModelClient`+`modelText`/`modelToolCalls`, `StubHttpClient`, `ReplayHttpClient`, `InMemoryWikiClient.appliedEdits`, `InMemoryObjectStore` (`@wikiharness/edges`) | bespoke doubles/mocks |

---

## Verification — corrections to the design doc's assumptions (found 2026-06-03)

Investigation found the codebase *ahead of* a few design assumptions and *behind* others. The plan
is written to the **actual** state:

- ✗ **`Finding.evidence` does not exist yet.** Design Phase 1 adds it — confirmed required.
  (`packages/core/src/types.ts:148-155`.)
- ✗ **`GroundingAssertion` has no `source-fetched` variant** (only `located-quote`,
  `in-article-span`, `diff`). Phase 1 adds `{ kind: 'source-fetched'; sourceHash: string }`.
  (`types.ts:80-102`.)
- ✓ **`ToolResult.findings` ALREADY exists** (`packages/core/src/tool.ts:73`). No change needed;
  `groundingVerifier` just needs to iterate it.
- ✓ **The `enhancers → [core, tools, outputs, policy, hooks]` import boundary ALREADY exists**
  (`scripts/lib/boundaries.ts:16`) **and** `packages/enhancers/package.json` already declares those
  deps. The design's Phase 6 "add `enhancers → [core, tools]`" is therefore **already satisfied** —
  Phase 6 only *verifies* boundaries stay green (one investigator's claim that it was "not
  configured" was wrong; the file was read directly). **New** edge added by **Phase 3**:
  `agent → enhancers` (so `improve()` can import the `ENHANCERS` registry) — no cycle.
- ⊘ **No Wayback snapshot *lookup* exists** (only `rewriteWaybackUrl`/`isArchiveUrl`/
  `resolveCitationUrl`/`recoverWaybackBody` — handlers for *known* archives). `wayback-archive`
  would be **new** work, and attaching an `archive-url=` to an existing citation needs the
  wikitext-AST (#9). **Decision (2026-06-03): defer `wayback-archive` to #9.** This work ships
  `bare-url-repair` + `citation-verify` only.
- ✗ **`hitl.test.ts` has no suppression test today** — the `improve()` suppression path
  (`reasons`, `proposal_suppressed`, `suppressed` audit) is uncharacterized. Phase 1's first task
  writes characterization tests *before* refactoring.
- ✓ `ScriptedModelClient(script: readonly ModelResponse[])` with `modelText`/`modelToolCalls`/
  `toolCall` helpers (one investigator's `readonly string[]` was wrong).

## Key planning decisions (resolved ambiguities — flag any you want changed at review)

1. **Module home for the chain:** `packages/agent/src/enforcement.ts` (`makePolicyHook` +
   `enforcementPipeline` + `runEnforcement`) — **not** `hooks`. Reason: `policyHook` needs
   `evaluatePolicy`, but the import boundary keeps `hooks → [core, outputs]` (policy-free, pure
   predicates over `core`). Both consumers — the agent loop (`loop.ts`) and `improve()` (`hitl.ts`)
   — already live in `agent`, which **is** allowed to import `policy` (`boundaries.ts:17`). So the
   composition lives in `agent`; `groundingVerifier` is still imported from `@wikiharness/hooks`.
   This needs **no** boundary or package.json change. (The design/investigators suggested `hooks`;
   the boundary makes `agent` the correct home.)
2. **`policyHook` needs the policy** → it is a **factory** `makePolicyHook(policy)` closing over the
   policy (the `PostToolUseHook.verify(call,result,ctx)` signature has no policy slot, and threading
   it through `ctx` would touch every edge). `enforcementPipeline(policy)` returns the per-session
   array; both `improve()` and (future) loop wiring obtain their hooks from this one factory.
3. **Enforcement chain order = one declarative decision in the factory.** The order of the chain is
   set in exactly one place — `enforcementPipeline(policy)` — and nowhere else; `improve()` and the
   loop both consume that array verbatim. This is the *real* reason this is clean: if the order ever
   becomes load-bearing, it changes in **one line**, not scattered across two enforcement sites.
   - **Chosen value: `[groundingVerifier, policyHook]` (grounding-first)** — matching the design doc
     and the project's identity (the anti-hallucination gate is the most fundamental invariant, so it
     leads the reasons). `runEnforcement` aggregates → suppression `reasons = [groundingReason,
     ...policyReasons]`.
   - This is **not** chosen to preserve the legacy `improve()` order (`[...policyReasons,
     groundingReason]`, `hitl.ts:104-107`). Pre-MVP, preserving existing behavior is **not** a goal;
     correctness/clarity wins. The order is **cosmetic + untested today** (no code depends on it; no
     ripple effects), so aligning with the design doc is free.
   - **Recorded tradeoff (for when order matters):** the one argument for policy-first is *fail-fast*
     — the loop's short-circuiting `runPostHooks` would skip the (async) grounding re-verification on
     a policy-failing edit. That delta is negligible (one `ObjectStore.get`, and only on the
     not-yet-wired loop path; `improve()` aggregates and runs both regardless). If that ever matters,
     flip the one line in `enforcementPipeline`.
4. **`PostVerification` gains optional `reasons?: readonly string[]`** (backward-compatible) so
   policy's *multiple* reasons survive as separate array elements (no info loss); `runEnforcement`
   falls back to `reason ? [reason] : []`. The loop's single-`reason` path is untouched.
5. **`source-fetched` grounding shape:** `{ kind: 'source-fetched'; sourceHash: string }`, verified
   by `ctx.objects.has(sourceHash)` (the source was really fetched this session). `groundingVerifier`
   gets an **explicit** branch — today's `return { ok: true }` fallthrough would silently pass an
   unfetched source.
6. **Empty-quote guard:** `groundingVerifier`'s `located-quote` branch rejects an empty quote (an
   empty string locates at offset 0 and would falsely "ground"). This closes the degenerate
   "supporting verdict with no quote" case at the gate.
7. **Finding suppression vs surfacing for `citation-verify`:** the **gate** is the single authority.
   SUPPORTED/PARTIAL with a located quote → reuse `verify_citation`'s `located-quote` evidence
   (surfaces). SUPPORTED/PARTIAL whose quote did **not** locate (or is absent) → attach
   `located-quote` evidence built from the model's *claimed* quote so `groundingVerifier` re-checks
   and **suppresses** it (`finding_suppressed`). NOT_SUPPORTED/SOURCE_UNAVAILABLE → `source-fetched`
   evidence (no quote needed; surfaces).
8. **`FindingRecord` shape:** `{ finding: Finding; sessionId: string; surfacedAt: number }` —
   immutable, **no status machine** (mirrors `ProposalRecord` minus `status`/`confirmationToken`).
   Finding audit entries (`finding_surfaced`/`finding_suppressed`) go on the **same** session audit
   stream (`audit:{sessionId}`) as proposals, interleaved by time.
9. **Concurrency:** extract a pure `mapWithConcurrency(items, limit, fn)` into `@wikiharness/core`,
   unit-test it, **refactor `evals/benchmark.ts` to use it**, and use it in `citation-verify` (≤3).
   One pool, two consumers — no duplication.
10. **Capabilities:** a `ReadonlySet<string>` (`'model'`, `'htmlExtractor'`) passed to the
    `SessionService` **constructor** (built once per server at the composition root). `improve()`
    skips enhancers whose `requires ⊄ capabilities`, appending a `enhancer_skipped` audit entry.
11. **Model provider wiring (Phase 4):** `resolveModelConfig(env)` reads `OPENROUTER_API_KEY` (the
    same key the benchmark uses) + optional `WIKIHARNESS_MODEL`/`WIKIHARNESS_MODEL_BASE_URL`
    overrides, default-closed. The server builds a `VercelModelClient` over
    `createOpenAICompatible(...)` (mirroring `scripts/benchmark.ts`) when enabled, else
    `NoModelClient`.
12. **Phase 6 eval-gate-covers-findings:** add a focused hermetic test in `packages/evals` that runs
    a **hallucinated finding** through `antiHallucinationGate` semantics (proving the 0-violation
    gate extends to findings), plus the end-to-end `improve()` integration test. The labeled
    citation dataset is left unchanged (findings are orthogonal to the verdict corpus).
13. **`SessionService` constructor uses a trailing `opts` object** (`{ enforcement?, capabilities?,
    enhancers? }`), introduced in Phase 1 and extended in Phases 3–4 — so growing the seam never
    churns positional args. `new SessionService(ctx, policy)` callers are unaffected.
14. **`wayback-archive` deferred to #9** (your call, 2026-06-03). Recorded on chainlink in Phase 3.
    Ships: `bare-url-repair` (Phase 3) + `citation-verify` (Phase 5).
15. **`WikipediaArticleSubject` is built incrementally:** Phase 3 = `{ title, wiki, wikitext,
    revision }` (all `bare-url-repair` needs); Phase 5 **adds** `article: ParsedArticle` (and the
    `getArticleTool` fetch) for `citation-verify` — a non-breaking extension that avoids an unused
    REST fetch in Phase 3.

## Test commands (every phase ends green)

- Single file: `npx vitest run <path-or-substring>`
- Package/area run: `npm run test`
- Full gate (definition of done): `npm run ci` — runs `format → typecheck → lint → boundaries →
  stubscan → coverage (≥90% lines/fns/stmts, ≥85% branches on `core`/`tools`) → eval`.
- Coverage note: only `core` and `tools` are coverage-gated (`vitest.config.ts:15-22`), so new
  `core`/`tools` code (the `source-fetched` branch, `mapWithConcurrency`, any tool changes) needs
  thorough tests; `hooks`/`agent`/`enhancers`/`server`/`cli`/`evals` are not coverage-gated but
  **must** still be tested (stub-scan + the behaviors below).
