# 06 — Architecture

> **Staging — HOW (tentative).** Architecture, contracts, and sequencing here are **exploratory, not locked** — a current best guess, revisable freely until M0 (base framework now settled — `adr/0002`, accepted 2026-06-02). The settled *what* lives in `00`–`02`/`11`. See [Staging](../README.md#staging).

> The base-framework choice behind this architecture is recorded, with the full
> bake-off, in `docs/adr/0002-base-framework.md`. Summary: **own a thin,
> dependency-injected pure-tool core; adopt the Vercel AI SDK (`ai`) only for the
> model-calling layer; implement human-in-the-loop as a persisted approval state
> machine.** LangGraph.js is the sanctioned escape hatch for durable HITL.

> **Serves the interaction model in `03`.** The UX/interaction model is a
> load-bearing *input* to this architecture (`01`). Much of what follows — the
> approval state machine, the SSE event shape, server-held credentials, one API for
> two clients — exists to satisfy `03-ux-and-interaction.md`. That doc was **redlined by
> the owner 2026-06-02**, and the redline **confirmed** this architecture — the
> durable-HITL / SSE / one-API shape serves every UX decision — so the pieces here are
> correspondingly firmed up (the enhancer-declared evidence payload is the one piece still
> being detailed).

## Guiding principles (where they come from)

1. **Pure, I/O-free core behind dependency-injected edges** — *from SP42.* The
   core has no network, fs, clock, or randomness; every external dependency is an
   interface with a real impl and a test double. This is what makes "every
   subcomponent deeply testable" structurally true rather than aspirational.
2. **`build_*Request` / `parse_*Response` split** — *from SP42.* Pure request
   construction + pure response parsing; thin `execute*` glue. Test the *exact*
   API call and the *exact* parsing without a network.
3. **Own the agent loop; rent only the model layer** — *bake-off conclusion.* The
   loop, the tool surface, and the HITL state machine are ours (small, testable,
   vendor-neutral). The Vercel AI SDK provides `generateText`/`streamText`,
   tool-call orchestration, and — crucially for tests — `MockLanguageModelV2`.
4. **Human-in-the-loop is a persisted state machine, not an in-memory promise** —
   *bake-off conclusion (D).* Durable across reload/restart; no write without a
   recorded confirmation token.
5. **API-first; one core, many clients** — CLI and web are thin clients of one
   HTTP API. No logic lives in a client.
6. **Policy-as-data with eval gates** — *from SP42.* Behavior thresholds live in
   versioned YAML; promotion requires passing declared gates.
7. **Provenance and audit on every proposed edit** — *from SIFT + SP42.*
8. **Modular enhancers over a shared core; consumer-agnostic outputs** — *ADR-0005.*
   Each enhancement capability is a registrable plugin (`enhancers/`) composing
   tools toward an objective. The same verification core runs whether the target is
   an article being improved or an incoming edit being checked — checking a diff is
   not a separate "patrol" mode (`adr/0005`). Findings/proposals are durable,
   addressable artifacts (`outputs/`) that
   many consumers can ingest — edit-commit is one consumer; external surfaces (viz,
   dashboards) are others. Each proposal/finding carries an **enhancer-declared evidence
   envelope** (`07`, decided 2026-06-02): a required common shape (the primary evidence shown
   *before* any verdict + a machine-checkable `grounding` assertion) plus a per-kind
   extension/renderer. A PostToolUse hook (`adr/0007`) re-verifies `grounding`, generalizing
   the anti-hallucination gate across enhancers.
9. **Provider-neutral, open-first model layer** — *ADR-0006.* `ModelClient` keeps
   the system open-capable/open-default/open-tested (a cheap open model is pinned
   for CI/evals); frontier models are an optional, pluggable backend, never a
   dependency.
10. **Assess the edit, not the editor** — *Principle 5 (`01`), `02 §7`.* Editor-
   identity signals never enter model context or any scoring/ranking input; the
   architecture provides no field path that carries them into a verdict.
11. **Blocking hooks enforce structurally, not by cooperation** — *ADR-0007.*
   Pre-/post-action interceptors `DENY`/`ASK`/`ALLOW` tool calls and run
   post-action verification, so guardrails hold even if the model "forgets" them.
   Hooks are themselves pure-logic + unit-tested.
12. **Multi-model voting for higher-risk classes** — *ADR-0007, SIFT.* Content
   proposals require N-of-M model agreement before they are surfaced; disagreement
   suppresses or escalates with the split shown.

## Layered package structure

```
wikiharness/
├─ packages/
│  ├─ core/        PURE. types; claim/diff/provenance logic; request builders;
│  │               response parsers; policy evaluation; locate_quote_in_source.
│  │               No fetch, no fs, no Date.now(), no Math.random(), no model.
│  ├─ edges/       Adapters implementing core interfaces:
│  │               HttpClient, WikiClient, ModelClient (Vercel AI SDK),
│  │               Clock, Storage, ObjectStore.
│  ├─ tools/       Each capability (07-tool-catalog.md) as a pure-ish Tool that
│  │               composes core + injected edges. Importable & unit-testable.
│  ├─ enhancers/   Registrable PLUGINS (ADR-0005): each enhancer (citation-verify,
│  │               book-source, wikidata-enrich, …) composes tools toward an
│  │               objective. Runnable alone or composed; the same core runs
│  │               whether the target is an article or an incoming diff.
│  ├─ outputs/     Consumer-agnostic results store (ADR-0005): findings/proposals
│  │               as durable, addressable artifacts. Edit-commit is ONE consumer;
│  │               external surfaces (viz, dashboards) are others.
│  ├─ agent/       The loop: model-calling (Vercel AI SDK) + the HITL approval
│  │               state machine. Drivable headlessly with a MockLanguageModel.
│  ├─ policy/      YAML policies + the (pure) policy engine + governance scripts.
│  ├─ hooks/       Blocking enforcement (ADR-0007): PreToolUse DENY/ASK/ALLOW +
│  │               PostToolUse verifiers (quote-locator, provenance, stub-scan).
│  │               Pure predicates over (toolCall, ctx); registered on the loop.
│  ├─ server/      HTTP API (Hono/Express): sessions, SSE event stream, proposal
│  │               approve/edit/reject, audit, and outputs feed. OAuth2 server-side.
│  ├─ cli/         Thin client of server/core (initial UX: partly conversational).
│  └─ web/         Thin browser client of the same API.
├─ evals/          Labeled datasets + metrics + gate definitions (`05`).
├─ fixtures/       Recorded HTTP cassettes + scripted model transcripts.
└─ docs/ , docs/adr/
```

Dependency rule (enforced by lint/import boundaries): `core` depends on nothing;
`tools` depend on `core` + interfaces; `edges` implement interfaces; `agent`
orchestrates `tools`; `server` exposes `agent`; `cli`/`web` consume `server`.
Nothing imports "downhill" into a client.

## The agent loop (own it; thin)

```
                 ┌───────────────────────────────────────────────┐
 ModelClient ◀──▶│  agentLoop(messages, tools, ctx)               │
 (Vercel AI SDK) │   • ask model for next step                    │
                 │   • model emits tool call(s)                   │
                 │   • PreToolUse hook → DENY / ASK / ALLOW        │
                 │   • READ tools  → execute immediately          │
                 │   • WRITE tool  → create ProposedEdit,         │
                 │                   PAUSE (persist checkpoint),   │
                 │                   await human confirmation      │
                 │   • PostToolUse hook → verify (quote-locate,    │
                 │                   provenance, stub-scan)        │
                 │   • feed results back; repeat until done       │
                 └───────────────────────────────────────────────┘
```

- In tests, `ModelClient` is a **scripted `ModelClient` double** returning a
  predetermined sequence of tool calls → the whole loop is deterministic and offline
  (Tier 3). The double is SDK-agnostic so loop tests don't couple to the Vercel AI SDK's
  stream-chunk shape; `MockLanguageModelV2` is used to test the `ModelClient` *adapter*
  itself, not to drive the loop (`adr/0002` resolution, note 1).
- Read vs. write is a property of the tool (`access` in the contract), so the loop
  decides "run now" vs. "pause for confirmation" structurally.
- **Hooks wrap every tool call (ADR-0007).** A `DENY` from a PreToolUse hook stops
  the call before any effect; a failed PostToolUse verifier rejects the result
  pre-human. Hooks are pure predicates, so each is Tier-1 unit-tested and the loop's
  hook-dispatch is covered by Tier-3 headless tests.

## Human-in-the-loop: the persisted approval state machine

The single most important runtime mechanism (see `02 §2`, `08`). Implemented as
durable state, not a dangling promise:

```
write tool proposed
        │
        ▼
 persist ProposedEdit  ──►  status = AWAITING_CONFIRMATION
 (Storage edge: SQLite/Postgres)        │
        │  emit over SSE: {proposal, diff, rationale, provenance, evidence}
        ▼
 ── client (CLI or web) shows it; user decides ──
        │
   approve / edit-then-approve / reject  (+ confirmationToken)
        │
        ▼
 verify token binds to ProposedEdit hash
        │  approve/edit → status = CONFIRMED → commit_confirmed_edit (OAuth2)
        │  reject        → status = REJECTED  → no write
        ▼
 append to audit ledger
```

Why durable: a web user can close the tab; the pending proposal is a row, not
memory, so a later `GET /sessions/:id/proposals` re-surfaces it and resume works.
This is exactly the gap that disqualified opencode (ephemeral permission events)
and that the Vercel-AI-SDK + own-state-machine approach closes cleanly. If we
prefer not to hand-roll it, the LangGraph.js `interrupt()` + checkpointer hybrid
provides the same durability (ADR-0002).

**Idempotency rule (carried from the LangGraph HITL caveat, true for us too):**
all read-only investigation happens *before* the pause; the actual MediaWiki write
happens *only after* resume, in `commit_confirmed_edit`. A resumed/retried step
must never double-write.

## Edges (the dependency-injection seam)

| Interface | Real impl | Test double |
|---|---|---|
| `HttpClient` | fetch + retry/backoff + maxlag + UA + ≤3 concurrency | replay cassette / stub |
| `WikiClient` | MediaWiki Action+REST, OAuth2, CSRF tokens (SP42 port) | in-memory fake wiki |
| `ModelClient` | Vercel AI SDK provider(s); multi-provider (citation-checker port) | scripted `ModelClient` double (drives the loop, Tier 3); `MockLanguageModelV2` to test the adapter |
| `Storage` | SQLite (dev) / Postgres (prod): sessions, proposals, audit | in-memory |
| `ObjectStore` | fetched-source cache (content-addressed by hash) | in-memory |
| `Clock` | system | fixed (epoch 0) |

A search edge (SIFT `tool_executor` port) and an `Rng` edge were dropped as premature
(zero consumers); discovery search is **MVP-4** work and will likely go through `HttpClient`
rather than a dedicated edge, so it is (re)introduced only when that milestone needs it.

Central cross-cutting concerns (UA, rate limit, concurrency cap, maxlag, retry)
live in `HttpClient`, **not** in individual tools — so policy compliance is
enforced in one tested place.

## API surface (server; one core, two clients)

See `08-mvp1-citation-verification.md` for the concrete MVP-1 endpoints
(`/sessions`, SSE `/events`, proposal decisions, `/resume`, `/audit`). The server:

- holds OAuth2 tokens **server-side only** (SP42 rule; browser never sees them),
- streams session events via SSE (web shows live progress; CLI consumes the same),
- persists sessions/proposals/audit via `Storage`,
- is the single surface the Tier-4 contract tests target (covering both clients).

## Security & operational guards (enforced, not documented)

- **Kill switch:** one config flag disables all write paths.
- **Dry-run default:** write tools emit proposals; committing needs `--commit` +
  confirmation token.
- **Test-wiki default in dev:** writes target `test.wikipedia.org` /
  `test.wikidata.org` until a path is graduated.
- **Scope guard:** the agent touches only the invoked article/entity + the claims
  under review.
- **Policy gate before surfacing:** `evaluate_policy(proposedEdit)` must pass
  before a proposal is shown to a human.
- **Anti-hallucination guard:** `locate_quote_in_source` must succeed for any
  proposed citation, or the proposal is rejected pre-human.
- **Identity-blindness guard (`02 §7`):** no editor-identity signal (anon / account
  age / edit count / curator group / IP) is included in model context or proposal
  scoring. Enforced structurally — the verdict-input type has no such field — and
  checked by an invariance eval gate (`05`).
