# ADR-0002 — Base framework / agent runtime

Status: **Accepted** — option **(a)**: thin SP42-style pure-tool core + Vercel AI SDK
for the model layer only + our own persisted durable-HITL state machine. Owner-ratified
2026-06-02 (walkthrough). LangGraph.js retained as the low-cost escape hatch (option (b)),
isolated to the `agent` package.
Date: 2026-06-01 (ratified 2026-06-02)

## Context

The user's initial instinct was "probably based on the pi harness," with opencode
as an alternative, but explicitly asked that the choice be made on **architectural
merits, not language**, and that other open tooling be considered. The selection
rubric (weights): deep **unit-testability of tools (HIGH)**, **headless
deterministic loop testing with a fake model (HIGH)**, **one core serving CLI+web
over an API (HIGH)**, **durable human-in-the-loop confirm-each-edit (HIGH)**,
non-coding-domain fit (MED), maturity/license (MED), extend-without-fork (MED).

Candidates evaluated: pi (`earendil-works/pi`), opencode (`sst`/`anomalyco`),
Claude Agent SDK (TS), Vercel AI SDK (`ai`), Mastra, LangGraph.js, and "thin custom
loop on a low-level SDK."

## Scored summary (1–5)

| Candidate | A tool unit-test | B headless fake-model loop | C one core → CLI+web | D durable HITL | E non-coding | F maturity/license | G extend w/o fork |
|---|---|---|---|---|---|---|---|
| **Thin core + Vercel AI SDK** | **5** | **5** | 3 (you build server) | 3 (you build, full control) | 5 | 4 (churn v5→v6) | 5 |
| LangGraph.js | 4 | 4 | 4 | **5** (`interrupt()`+checkpointer) | 5 | 4 | 4 |
| Mastra | 4 | 4 | 4 | 4 (durable suspend/resume) | 5 | 4 | 4 |
| Claude Agent SDK | 3 | 2 (spawns CLI subprocess) | 3 | 4 (`canUseTool`+`updatedInput`; durability DIY) | 3 | 4 (Anthropic-only) | 4 |
| opencode | 2 | 2 | **5** (server/SDK/SSE) | 3 (**ephemeral** permissions) | 2 (coding core) | 4 | 3 |
| pi | 4 (TypeBox, injectable Model) | 4 (`agentLoop()`) | 3 (RPC; no HTTP server) | 2 (no pre-exec approval handshake) | 2 (coding) | 3 (open-core, acquired) | 4 |

## Decision (recommended)

**Own a thin, SP42-style pure-tool core with dependency-injected edges, and adopt
the Vercel AI SDK (`ai`) ONLY for the model-calling layer. Implement
human-in-the-loop as a persisted approval state machine we control.**

Rationale (maps to the highest weights):
- **A + B (testability):** Vercel AI SDK tools are plain objects with a Zod schema
  and an importable `execute()`; side effects are injected and mockable. `ai/test`
  provides **`MockLanguageModelV2` + `simulateReadableStream`**, letting us script
  tool-call chunks and assert on the event stream **with no API key and no
  server** — criterion B almost verbatim. No coding-shaped harness matches this.
- **D (durable HITL):** rather than rely on any framework's in-memory approval
  (opencode's events are *ephemeral*; pi's RPC has *no* pre-exec gate), we make
  durability explicit: read-only tools run freely; the single write tool persists
  a pending-approval row (Storage edge), streams the diff over SSE, and blocks;
  the MediaWiki write happens only after `approve`/`edit-approve` resume. This is
  the SP42 boundary we already want, and it survives reload by construction.
- **C (CLI+web):** we expose the loop behind a small Hono/Express server with an
  SSE event stream; both clients consume the same endpoints. We build this
  deliberately (it is small) rather than inherit opencode's coding-centric server.
- **E/G:** the runtime is domain-neutral and entirely ours; adding a tool is
  first-class, never a fork.

**Escape hatch / sanctioned alternative:** if hand-rolling durable pause/resume
proves costly, run the *same pure-tool core inside LangGraph.js nodes* and use its
`interrupt()` + checkpointer (`PostgresSaver`) + `Command({resume})` for
best-in-class durable HITL. This is a drop-in for the `agent` package only; `core`,
`tools`, and `edges` are unaffected — so the decision is reversible at low cost.

## Why not the others (as a base)

- **pi** — excellent minimalist DI-friendly MIT core, but **no pre-execution
  approval handshake** in RPC, no HTTP/SSE server, coding-shaped tools, and
  open-core uncertainty post-acquisition. *Borrow ideas; don't base on it.*
- **opencode** — best-in-class "one server, many clients" (mine it for server
  inspiration), but coding-agent to the core, tools not cleanly unit-testable in
  isolation, and **permission events are ephemeral / don't survive reload** — a
  direct hit on requirement D.
- **Claude Agent SDK** — `canUseTool` + `updatedInput` is the nicest confirm/edit
  ergonomics, but it **spawns the Claude Code CLI subprocess**, **locks to
  Anthropic models**, and provides **no durability or server**. Vendor- and
  subprocess-locked; bad fit for a deeply-testable, vendor-neutral, API-first
  harness. (We may still *offer* Claude as one model provider via the SDK layer.)
- **Mastra** — credible batteries-included alternative (Apache-2.0, durable
  suspend/resume, HTTP server, domain-neutral, Zod tools); co-runner-up to
  LangGraph. Hesitation: VC/open-core governance and more framework than a thin
  core needs.

## Risks & mitigations

- **Vercel AI SDK version churn (v5→v6):** pin versions; keep the model layer
  behind our own `ModelClient` interface so a breaking change touches one adapter.
- **We build the server + HITL state machine ourselves:** small, but real work;
  mitigated by the LangGraph.js escape hatch if it proves heavier than expected.
- **LangGraph.js resume/`thread_id` wiring bugs** (if we take the escape hatch):
  verify against known issues; ensure `thread_id`/`checkpoint_ns` are correctly
  threaded; keep wiki writes strictly post-interrupt (idempotency).

## Model-layer constraint (from ADR-0006)

The `ModelClient` interface must be **strictly provider-neutral with open-model
backends (OpenRouter/Together/Ollama/vLLM) first-class**; a **cheap open model is
pinned for CI/evals**; no code in `core`/`tools` may assume a frontier vendor's
features. Frontier models are a pluggable option, never a dependency. The Vercel AI
SDK satisfies this (multi-provider), but it is now a *requirement* on the model
layer, not just a nicety — and the evidence harness runs an open-vs-frontier panel
(`11`, `adr/0006`).

## Resolution (owner-ratified 2026-06-02)

The one call worth explicit sign-off — **(a) thin core + Vercel AI SDK with our own
durable HITL** vs **(b) the same core inside LangGraph.js** — is decided: **(a).**

The walkthrough surfaced *why* (a) is the stronger call, not merely the
lower-code/higher-control default: tracing what (b) actually *saves* shows the gap
is narrow. Option (b) does **not** save the HTTP server, the SSE event stream, the
approve/edit/reject endpoints, or the `confirmationToken`-binds-to-`ProposedEdit`-hash
logic — all of which we own in either option. Option (b) rents only the
persist-checkpoint-and-block-then-resume mechanism (`interrupt()` + `PostgresSaver` +
`Command({resume})`). That rented piece is small and well-isolated, and it brings a
whole graph/node programming model plus the JS-resume/`thread_id`/`checkpoint_ns`
caveats in Risks below. For the single most safety-critical path in the system — the
human-confirms-every-edit gate we most want to read line-by-line and Tier-4
contract-test — owning a small persisted state machine beats delegating to a
framework's interrupt semantics. (b) remains the sanctioned fallback, isolated to the
`agent` package, if hand-rolling pause/resume proves costlier than expected.

### Revisability — UX trumps architecture

This acceptance covers the **runtime / model layer**, and is held open to revision if
the pending `03` UX redline changes the API surface, data structures, or event model.
Project stance (owner, 2026-06-02): **UX trumps architecture.** The first UX is the
load-bearing input (`01`, `03`), so if redlining `03` demands a different runtime shape,
**this decision yields — not the UX.**

Current assessment is that it won't: the durable-resumable HITL state machine, the SSE
event stream, server-held credentials, and the single API surface that (a) commits to are
required by *all* of `03`'s open redline options (command-first vs chat-led, one-at-a-time
vs batched triage, web-in-MVP-1 or not). The redline is therefore expected to refine the
**event payload and screens**, not the framework. The `03` redline remains an M0 gate;
revisit this decision there if that assessment proves wrong.

**Outcome (2026-06-02):** the `03` redline is complete and **confirmed this assessment** —
none of the five UX decisions (command-first; one-at-a-time + fast sequential advance;
honeypots/throttling deferred to post-MVP; minimal web client in MVP-1; experienced-editor
audience) requires a different runtime shape. The revisability condition is **discharged**;
`0002` stands as option (a).

### Two precision notes recorded at ratification (M0 follow-ups)

1. **Tier-3 loop tests drive the loop through our own scripted `ModelClient` double,
   not `MockLanguageModelV2` directly.** Asserting on the Vercel AI SDK's stream-chunk
   shape would couple our headless loop tests to the SDK's streaming protocol, so the
   v5→v6 churn risk would hit our *test scaffolding*, not just one adapter — contradicting
   the mitigation below. `MockLanguageModelV2` is reserved for testing the `ModelClient`
   *edge adapter* itself; the loop (criterion B) is exercised against an SDK-agnostic
   scripted double. (`06`'s edges table — which currently equates the `ModelClient` test
   double with `MockLanguageModelV2` — is tightened to reflect this.)

2. **The one seam where the (a)→(b) escape hatch actually bites is Storage/checkpoint.**
   "`core`/`tools`/`edges` unaffected" is true except for proposal persistence: option (a)
   persists a pending-approval *row* via our `Storage` edge, whereas LangGraph holds graph
   state in its own checkpointer (`PostgresSaver`). Taking the escape hatch means re-homing
   proposal persistence from our row model to LangGraph's checkpoint model. The reversal is
   still cheap and `agent`-local, but it is not literally zero-cost.
