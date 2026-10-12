# ADR-0005 — WikiHarness is a modular enhancement framework

Status: **Accepted** — owner-ratified 2026-06-02 (walkthrough). Decision 3 revised
2026-06-02 (co-equal "modes" demoted). Decision 5 records the "UX trumps architecture"
stance and the flipped dependency to the `03` redline.
Date: 2026-06-01 (ratified 2026-06-02)

> **Revision (2026-06-02) — "co-equal modes" demoted.** On review we retired the
> "improvement and patrol/triage as co-equal modes" framing (Decision 3, and the
> enhancer list in Decision 2). Rationale: *recent-changes / vandalism patrol is
> already well-covered* (ORES, Huggle, SWViewer, …); what WikiHarness uniquely
> offers — citation-level verification ("does this source support this claim?") — is
> the **same shared core pointed at a diff**, not a distinct mode or a separate
> enhancer. The shared-core, plugin, and consumer-agnostic-outputs decisions are
> unchanged; only the "two co-equal modes" framing is retired. Luis's original "both
> co-equal" quote below is kept as the historical record of what was revised.

## Context

Resolving open questions #1, #2, #4, #5 from `01-vision-scope.md`. Luis clarified
the shape of the project: it is not a single citation tool that later grows, but a
**framework** that hosts many enhancement capabilities, spanning more than one use
case, emitting outputs that more than just Wikipedia/Wikidata can consume.

Key quotes (Luis, this planning conversation):
- *"an umbrella over two (citation-checker/SIFT + open-graph) plus a framework that
  can include other ways to enhance articles + the open knowledge graph, as we
  discover them."*
- On supporting both improvement and patrol: *"(both co-equal), because we're going
  to add even more later, so the earlier we think through a modular architecture and
  UX, the better. One possible parallel is ... a code quality improvement tool:
  first TDD, then static analysis for memory problems, then ... complexity, then a
  fuzzer."*
- *"The rabbit hole visualization is out of scope, but we should think of that sort
  of thing as a possible consumer of our outputs, reminding us that not all our
  consumers will be Wikipedia articles or Wikidata."*
- *"the initial front-end is a CLI driven at least in part by the conversational
  chat style (though let's not over-lock-in on that UX!)."*

## Decisions

1. **WikiHarness is a modular enhancement framework**, an umbrella over the
   verification/editing lineage (`wikidata-SIFT` + citation-checker) and the
   open-graph work, **plus** an extensible host for new enhancers over time.
   Mental model: **a code-quality tool suite** — independent, composable passes
   over a shared core.

2. **Enhancers are first-class plugins.** Each capability (citation verification,
   book-scan sourcing, Wikidata enrichment, open-GLAM linkage, …) is a registrable
   unit with a typed contract, independently testable, runnable alone or composed.
   This is the structural form of "every subcomponent deeply testable."

3. **One verification core, two targets — not two modes** *(revised; see note above)*.
   The same shared core (fetch/verify with "no fetch, no verdict", provenance,
   human-in-the-loop, write paths) runs whether the objective is *improving an
   article* or *checking someone else's incoming edit*. Checking a diff is that core
   pointed at a diff — **not** a separate "patrol" mode, and **not** a bid to compete
   with well-covered recent-changes/vandalism patrol. **Autonomous
   moderation/reverting remains out of scope** — we surface verdicts for human action
   (consistent with human-confirm-each).

4. **Outputs are consumer-agnostic.** The harness emits structured, addressable
   *findings/proposals* that multiple consumers can ingest: the write paths to
   Wikipedia/Wikidata/Commons are *one class* of consumer; external surfaces
   (discovery/"rabbit-hole" viz, dashboards, other tools) are others. We design the
   outputs layer for this from the start; we do **not** build those external UIs.

5. **UX is pluggable *in principle*, but v1 must commit to one concrete UX.** The
   core is consumer-agnostic, so many front-ends *can* sit on it — that is not the
   same as deferring the choice. An API is not a product: we owe **one** shipped UX
   (initially a CLI, partly conversational/chat-driven), and that first UX is a
   **locked-in** decision that shapes the API surface, data structures, and event
   model (a load-bearing input to `adr/0002`). The layer stays thin (generalized
   "compose these enhancers" UIs are hard), but the first interaction style is not
   free or freely reversible.
   **Update (2026-06-02):** `adr/0002` is now ratified (option (a)), but its acceptance
   is held open to revision pending the `03` UX redline — **UX trumps architecture**: if
   redlining `03` changes the API/event model, `0002` yields, not the UX. Current
   assessment is that it won't (the durable-HITL/SSE/one-API shape `0002` commits to is
   required by every open `03` redline option), so the dependency arrow has effectively
   flipped: `0002` is locked and the **`03` redline is now the downstream M0 gate**, not
   an input `0002` is waiting on. Ratifying this ADR commits to the *principle* of one
   locked-in UX; it does **not** by itself make the UX *choice* (that is the `03` redline).
   **Update (2026-06-02):** the `03` redline is now complete (see `03` "Redline outcomes") —
   it confirmed `0002` with no runtime-shape change.

6. **accords stays separate / TBD.** Not adopted as a WikiHarness mechanism now;
   may be revisited as a guardrail experiment later (`adr/0004` reuse posture
   already covers guardrails; accords is not a dependency).

## Consequences

- **Architecture (`06`)** gains two first-class concepts: an **enhancer registry**
  (plugins over the shared core) and a **consumer-agnostic outputs/results layer**
  (findings/proposals as durable, addressable artifacts; edit-commit is one
  consumer). The agent/tool design already supports modular tools; this elevates
  "enhancer" and "outputs store" to named architecture. Each output carries an
  **enhancer-declared evidence envelope** (`07`, decided 2026-06-02): a required common shape
  (primary evidence shown before any verdict + a machine-checkable `grounding` assertion) plus
  a per-kind extension/renderer — the structural form of evidence-before-verdict (`03 §3`),
  enforced by a PostToolUse hook (`adr/0007`).
- **Scope** widens conceptually but the **MVP sequencing is unchanged** — the first
  enhancers are still citation verification (MVP-1) then book-scan sourcing
  (MVP-2). Checking an incoming edit reuses the shared core wherever it is useful; it
  is not a separately sequenced enhancer or mode.
- **Non-goals** updated: *building* end-consumer UIs and *autonomous reverting* are
  non-goals. (Checking incoming edits is neither a non-goal nor a separate mode — it
  is an application of the core.)
- Naming: "harness" is apt — a harness that hosts and composes many enhancers.

## Non-goals / guardrails (unchanged or reinforced)

- No autonomous editing or reverting (carried from ADR-0001/0004).
- We build the framework + enhancers + the outputs layer; we do **not** build the
  external consumer surfaces (e.g. the rabbit-hole viz).
