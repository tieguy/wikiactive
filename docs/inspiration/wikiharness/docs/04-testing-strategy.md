# 04 — Testing Strategy ("deeply testable" is the prime directive)

> **Staging — HOW (tentative).** Architecture, contracts, and sequencing here are **exploratory, not locked** — a current best guess, revisable freely until M0 (base framework now settled — `adr/0002`, accepted 2026-06-02). The settled *what* lives in `00`–`02`/`11`. See [Staging](../README.md#staging). *(The **requirement** "deeply testable" is firm; the concrete tiers/mechanics below are the tentative part.)*

The user's top criterion: **every subcomponent must be deeply testable.** This
document defines what that means concretely and how the architecture guarantees
it. Testing is not a phase; it is the shape of the codebase.

> **"Deeply testable" has two halves.** This doc covers **code correctness** — *does
> the code do what we think it does* (deterministic, offline; Tiers 1–4). The harder
> half — **output quality**: *are the LLM-judgment outputs good, and do they regress
> when we tweak a prompt/model/policy* — is its own discipline in
> [`05-evaluation-and-benchmarking.md`](05-evaluation-and-benchmarking.md). Both are
> inputs to architecture; neither is a sub-tier of the other.

## The core principle: a pure, I/O-free core behind dependency-injected edges

Lifted directly from SP42's proven structure (`sp42-core` has *zero* networking;
every external dependency is a trait with stub/replay/memory/fixed
implementations). We adopt the same boundary in TypeScript:

```
packages/
  core/        # PURE. No fetch, no fs, no clock, no randomness, no model calls.
               # Only: types, pure logic, request builders, response parsers,
               # claim/diff/provenance logic, policy evaluation.
  edges/       # Adapters that implement the core's injected interfaces:
               # HttpClient, WikiClient, ModelClient, Clock, Storage, ObjectStore.
  tools/       # Each capability = a tool composing pure core + injected edges.
  enhancers/   # Registrable plugins (ADR-0005) composing tools toward an objective;
               # each is independently testable with injected edges + scripted model.
  outputs/     # Consumer-agnostic findings/proposals store; pure assembly is Tier-1.
  agent/       # The loop: model-calling + human-in-the-loop. (Base TBD: adr/0002)
  server/      # HTTP API exposing workflows (one core, many clients).
  cli/  web/   # Thin clients of the server/core.
```

Every injected dependency is an interface with at least two implementations:
a **real** one (in `edges/`) and a **test double** (stub / replay / in-memory).

```ts
interface HttpClient   { fetch(req: HttpRequest): Promise<HttpResponse>; }
interface ModelClient  { complete(req: ModelRequest): Promise<ModelResponse>; }
interface Clock        { now(): number; }
interface Storage      { get/put/append(...): ... }
```

## The `build_*Request` / `parse_*Response` split (testability without a network)

Also from SP42. Each external interaction is decomposed into **pure** functions:

```ts
buildEditRequest(params): HttpRequest          // pure → unit test the exact API call
parseCitoidResponse(bytes): Citation[]         // pure → unit test parsing on fixtures
executeEdit(http, params): Promise<EditResult> // thin glue: http.fetch(buildEditRequest(...))
```

Consequence: we can unit-test **"what edit/API call would the agent make"** and
**"how do we parse this response"** with *no network and no credentials*, which is
exactly where most real bugs live. `execute*` glue is covered by integration
tests with a replay/stub HTTP client.

## The test tiers (code correctness)

Four deterministic, offline tiers. The former "Tier 5 — behavioral evals" now lives in
[`05`](05-evaluation-and-benchmarking.md) as its own discipline (see the pointer after
Tier 4).

### Tier 1 — Pure unit tests (the bulk; fast, deterministic, offline)
Every function in `core/` and every tool's pure logic. No network, no model, no
clock, no randomness. Examples:
- claim extraction from REST HTML (port of `citation-checker/core/claim.js`),
- citation URL resolution incl. Harvard/`sfn` short-cites and archive handling
  (port of `core/urls.js`),
- verdict parsing incl. JSON / fenced / markdown-emphasis recovery (port of
  `core/parsing.js`) and verdict canonicalization (`core/verdicts.js`),
- Wikidata ontology pre-checks (P31/P279), ported from SIFT's `sift_precheck.py`,
- diff generation, provenance assembly, edit-summary formatting,
- policy evaluation (given a YAML policy + an edit, is it allowed?).

### Tier 2 — Tool tests with injected edges (recorded fixtures / "cassettes")
Each tool is tested by injecting a **replay HTTP client** and a **stub model**.
External responses are recorded once as fixtures (HTTP cassettes: request → saved
response) and replayed deterministically. Adapted from SIFT's fixture-driven
pytest suite (220 tests) and SP42's `ReplayEventSource`/`StubHttpClient`.
- A "verify citation" tool test feeds a fixed article HTML + a fixed source page +
  a stub model returning a scripted verdict, and asserts the proposed enhancement.
- A "find archive" tool test replays canned CDX / Availability responses.
- A "search open data" tool test replays canned SPARQL / museum-API responses.

Each tool ships a **fixture set** under `tools/<tool>/__fixtures__/`. A recorder
script (`scripts/record-fixture`) can refresh fixtures against the live API for
maintenance; CI runs only against recorded fixtures (hermetic, offline).

### Tier 3 — Headless agent-loop tests (deterministic, no real model)
Run the **entire agent loop in-process** with a **scripted model** that returns a
predetermined sequence of tool calls, and assert on the emitted event stream and
the resulting proposed-edit set. This is a hard requirement on the base framework
(`docs/adr/0002`, criterion B): the loop must be drivable headlessly with an
injected fake model and no server. SIFT already proves the two-phase
investigate→verdict loop is testable; SP42 proves the runtime loops are testable
via injected event sources.

### Tier 4 — Human-in-the-loop / API contract tests
The pause → surface proposal → resume-on-confirm cycle is tested at the API
boundary: a test drives the server API as a client would, receives a
"pending-confirmation" event for a proposed edit, sends an approve/edit/reject,
and asserts the durable state transition. Because CLI and web share this API,
**one contract-test suite covers both clients.** Includes: resume-after-reload
(durability), reject path, edit-then-approve path, and the "no write without
confirmation token" guard.

### Output quality (behavioral evals) → `05`
LLM output is non-deterministic, so we do **not** assert exact strings on real models.
That entire discipline — labeled datasets, precision/recall metrics, the per-policy eval
gates, and the **regression gate** that blocks any prompt/model/policy change which
worsens output quality — now lives in
[`05-evaluation-and-benchmarking.md`](05-evaluation-and-benchmarking.md). It is the
project's hardest testing problem and the engine behind the external evidence harness
(`11`, `adr/0003`); it was extracted from here so it reads as a first-class concern, not
a fifth tier. The CI definition of done below still references its gates.

## Anti-hallucination, specifically

This deserves its own gate because it is *the* risk (Section 1 of
`02-constraints-policy.md`). The rule: **a proposed citation is only valid if its
supporting quote can be found, by exact/normalized string match, in a document the
harness actually fetched in this session.** This is a pure function
(`locateQuoteInSource(quote, fetchedText) → offset | null`) — Tier-1 unit-tested —
and a hard eval gate (`05`; must be 0 violations on the eval set). It converts
"trust the model" into "verify against retrieved bytes."

**Generalized across enhancers (decided 2026-06-02).** `locateQuoteInSource` is the
**grounding predicate for the `located-quote` evidence kind**; the evidence envelope (`07`)
requires *every* kind to carry its own pure, Tier-1 grounding predicate (`in-article-span` →
span exists in the cited revision + valid rule id; `diff` → shown diff equals the real diff),
which a PostToolUse hook (`adr/0007`) re-checks pre-human. So "verify against retrieved bytes"
holds for enhancers that have **no external source** (e.g. MOS:PEACOCK), not just citations.

## Determinism boundary

- The **tool layer and core are fully deterministic and replayable** (hermetic
  fixtures, injected clock/rng). This is a hard, enforced property.
- The **model layer is non-deterministic**; it is tested *behaviorally* (`05`),
  never by exact match. We relax SP42's literal "same input → same output"
  constitution to the non-LLM parts only — but keep its spirit everywhere else.

## CI gates (definition of done for any change)

- Lint + typecheck clean; formatter clean.
- Tiers 1–4 green, hermetic (no network in CI).
- Coverage threshold on `core/` and `tools/` (target ≥ 90%, per SP42 precedent).
- Eval gates (`05`) pass for any prompt/policy change (run against recorded model
  outputs or a **pinned open eval model**; results compared to declared thresholds).
- Governance check: no policy promoted past its passing gates; no write path
  enabled without its confirmation/provenance guards present.
- **End-of-stage bug-hunt (`adr/0007`):** before any stage (build milestone or
  enhancer pipeline stage) is "done," a dedicated **adversarial bug-finding + fix
  loop** — run by a reviewer distinct from the implementer — must complete with
  **zero critical findings**. Severity-tiered; the loop re-runs until clean.
- **Hook coverage:** every PreToolUse/PostToolUse hook (`06`, `adr/0007`) is a pure
  predicate with Tier-1 tests; the loop's hook dispatch (DENY blocks the call;
  failed PostToolUse rejects pre-human) is covered by Tier-3.
