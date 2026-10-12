# 03 — UX & Interaction Model (redlined 2026-06-02)

> **Status: redlined by the owner 2026-06-02.** Drafted from `01`/`06`/`08`/`11`; the five
> open points were resolved in the UX walkthrough — see **"Redline outcomes"** at the end.
> Decided positions below are no longer strawman; a few items stay marked **STRAWMAN ▸**
> where they were not redlined (architecture-facing and uncontested, e.g. the
> enhancer-declared evidence payload and the "save my position" affordance). The redline
> **did not disturb the base framework** (`adr/0002`) — the durable-HITL / SSE / one-API
> shape serves every decision below, discharging `0002`'s revisability condition.
>
> **Why this sits before `06`.** `01` says the first UX is *"a load-bearing input to
> the architecture… it feeds back into API design, data structures, and the event
> model."* If UX is an *input* to architecture, the interaction model should be
> reasoned about *before* the architecture that serves it. This doc is that input;
> `06` is the response. Concrete screens live in `08`; this doc is the model above
> them, not a re-sketch.

## North star for the interaction

The editor stays in charge and stays fast. WikiHarness does the reading, fetching,
and cross-checking; the human does the judging and commits every edit. The
interaction's job is to make *"is this proposed change actually right?"* a
**fast, well-evidenced** decision — never a rubber-stamp, never a wall of text.

## 1. Shape: command-first, conversation-capable

`01` commits to a CLI "partly conversational/chat-driven" but warns *"let's not
over-lock-in on that UX."* The strawman reads that as a spectrum and picks a point:

**Decided (redline 2026-06-02) ▸ command-first, conversation-*capable* — not a free-form chat agent.**
The spine is explicit commands (`wikiharness verify "Einstein" --wiki en`), so the
tool is scriptable, deterministic to invoke, and CI-friendly. A conversational layer
rides *on top* for the parts where it earns its keep — follow-ups ("why this
verdict?", "find a better source"), disambiguation, and natural-language scoping
("check only the citations added since 2024"). The core workflow does **not** depend
on the chat layer; it degrades to plain commands + prompts.

- _Rationale:_ a pure chat agent is harder to test (`04`'s prime directive),
  non-deterministic to drive, and invites the unbounded "do anything" surface we
  explicitly don't want. Command-first keeps the load-bearing constraint legible.
- _The alternative to weigh:_ a more agentic chat-led loop is more inviting to
  non-technical editors and closer to how people already use LLMs. **STRAWMAN ▸**
  resolves this by putting the *chat affordance in the web client* (below) and
  keeping the CLI command-first. Redline if you want chat as the CLI's primary mode.

## 2. The approval loop is the product

Everything else is plumbing around one repeated moment: a proposed change is shown,
the human decides. This is the durable state machine in `06` and the
`approve / edit / reject / skip / why` controls in `08`. The UX commitments around
it:

- **One proposal at a time, by default** *(confirmed; redline 2026-06-02)*. Match `08`'s
  user story ("one at a time"). It keeps each decision deliberate and is the honest pace for
  high-stakes edits.
- **Decided ▸ fast sequential advance is an MVP requirement, not a later affordance.** After
  deciding proposal *N*, getting to *N+1* must be near-frictionless (a keystroke), so a long
  article's proposals flow quickly *without* becoming a bulk action. This is **not** true
  batch — there is no "tick many boxes, submit once"; every proposal stays an individual,
  evidence-forward decision. A skim/triage *queue view* (verdict chips, open any one) may
  still come later, but bulk submit is exactly the rubber-stamping surface (§3) we avoid —
  speed comes from frictionless N→N+1, not from deciding many at once.
- **Five verbs as a shared starting vocabulary:** `[a]pprove` · `[e]dit` ·
  `[r]eject` · `[s]kip` · `[?]why`. `edit` opens the proposed wikitext for human
  modification before commit (`08`). `skip` defers without judgment. These are a
  common starting point, **not a parity mandate** — each client is free to diverge
  as we learn what its surface needs (see §5).
- **Edit, don't just accept/reject.** The high-value path is *human-corrected*
  proposals; the UI must make editing-then-approving as cheap as a bare approve, so
  good-but-imperfect proposals get fixed rather than rejected or rubber-stamped.

## 3. Evidence-forward, anti-rubber-stamping (carried from `11`)

`11` makes acceptance-without-correctness the central failure (the CAT lesson). The
UX is the first line of defense, so these are interaction requirements, not nice-to-haves:

- **Decided (2026-06-02) ▸ evidence before verdict — the *principle* is general; the *rendering*
  is per-enhancer.** *(Architecture: a hybrid enhancer-declared evidence envelope, `07`/`06`;
  contract fixed at M1, kinds added as enhancers land.)* Lead with the basis for the decision and place the **graded categorical verdict**
  (fully/partial/not/unavailable) beside it, never as the headline — the human reads the
  evidence, not just a green chip. **No model-generated confidence/support number appears
  anywhere** — a manufactured "0.94" is false precision and exactly the authoritative-looking
  thing that invites rubber-stamping. The honest signals are the graded verdict, **measured
  ensemble disagreement** (show the split when models differ — `adr/0007`), and abstention. But **"evidence"
  does not reduce to one "highlighted source quote" field** — it differs by enhancer
  (`adr/0005`: WikiHarness hosts many), and notably *not every enhancer has an external
  source*:
  - *citation verification (MVP-1):* the located quote in the fetched external source,
    highlighted, + provenance;
  - *book-scan sourcing (MVP-2):* the located quote in the fetched OCR/page text, +
    provenance;
  - *codified-rule enhancers (MVP-3, e.g. MOS:PEACOCK):* **no external source at all** —
    the evidence is the offending in-article passage plus the rule it cites;
  - *diff-checking (any enhancer pointed at an edit):* the diff plus whatever signal
    that enhancer produces.

  So the architecture handoff is a **generic, enhancer-declared evidence payload** with
  a per-enhancer renderer — not a hardcoded source-quote locator. *(Decided 2026-06-02 as a
  **hybrid envelope**: required common shape — primary evidence + a machine-checkable
  `grounding` assertion — plus a per-kind extension; `grounding` generalizes the
  anti-hallucination gate. Contract in `07`; fixed at M1.)*
- **Progressive disclosure.** Default view is compact (claim, evidence quote,
  proposed diff, decision). `[?]why` expands the full reasoning, alternative sources
  considered, and the raw fetched context. Depth on demand, not by default.
- **Throttle to genuine review capacity** (`11`) — *post-MVP (possibly long post-MVP);
  redline 2026-06-02.* Active throttling is **deferred**: MVP volume is naturally bounded by
  one-at-a-time review by a single experienced editor (§2/§6), which is the interim pace.
  Once we have quality data the throttle is **calibrated to it** ("if the quality is good,
  set the throttle appropriately") rather than guessed up front. Lands with the evidence
  harness at **M4b** (`10`).
- **Reviewer-diligence honeypots** (`11`) — *post-MVP; redline 2026-06-02.* **Deferred**
  with throttling above (both belong to the evidence harness, M4b). The *principle*
  (anti-rubber-stamping) stays; the honeypot **mechanism** is not an MVP-1 deliverable. When
  built, planted known-good/known-bad items must be indistinguishable from real proposals.

## 4. Durability is a UX property, not just an implementation detail

From `06`/`08`: `AWAITING_HUMAN_CONFIRMATION` is a durable checkpoint. The
*experienced* promise: **a web user can close the tab, a CLI user can `^C`, and pick
up exactly where they left off** — pending proposals re-surface on return. No lost
work, no "start over." This is why the approval state is a persisted row, not memory
(and a primary base-framework selection criterion in `adr/0002`).

**STRAWMAN ▸ "save my position" is a first-class affordance, not just crash-recovery.**
A long run — especially the `review` meta-command (`09`), which composes several
enhancers and so produces *many* proposals — will exceed one sitting. Review volume
there is **inherent; we don't engineer it away**, we let the human stop and resume
deliberately. Two granularities, both backed by the same durable proposal store:

- **Save position (session-level):** stop the run here and resume later exactly where
  you left off — a deliberate bookmark, not just an accidental disconnect.
- **Pin / save-for-later (item-level):** flag a specific proposal to come back to.
  Distinct from `[s]kip` (which passes without judgment and need not resurface) — a pin
  *will* resurface in a "pinned" set. This is just another proposal status in the store.

## 5. Two clients, one core

`01`/`06`/`08`: the CLI and web are thin clients of the same API; the server holds
OAuth tokens server-side and streams events over SSE.

- **CLI is the primary, reference client** (the one shipped UX `01` owes). It is the
  testbed that shapes the API.
- **Decided ▸ a minimal web client is in MVP-1** *(redline 2026-06-02)* — the `08` sketch
  (citation list + detail pane + approve/edit/reject), nothing more. Beyond serving
  CLI-averse editors (`01`), its primary near-term value is that **it exercises the shared
  API/contract** (Tier-4, `04`/`06`) — a second client built early keeps the server honest.
  Kept deliberately minimal; it is also the natural home for the conversational affordance (§1).
- The two clients share **one server contract** — Tier-4 contract tests target the
  *server* (`06`, `04`), so the underlying API and behavior stay honest. But their
  **UX surfaces are deliberately free to diverge.** Trying an idea in one client
  without the other is the *whole point* of prototyping on two surfaces, not a defect
  to stamp out. We converge later only where a pattern has clearly earned it. (What's
  enforced is the shared contract, **not** identical screens.)

## 6. Who the user is

The MVP user is an **experienced editor** who already understands citations and
policy — not a newcomer, and not a passive approver. The UX optimizes for *their*
speed and judgment: dense-but-scannable, keyboard-driven, evidence-first. (Newcomer
onboarding and gentler surfaces are explicitly out of scope for now.) *(Confirmed, redline
2026-06-02: experienced editors are the audience right now.)*

## UX principles (the rules architecture must honor)

1. **The human commits every edit.** No interaction path writes without an explicit,
   per-edit confirmation bound to that proposal (`02 §2`).
2. **Evidence outranks verdict** on screen, always (§3); the verdict is the **graded
   categorical** label, and **no model-generated confidence/support number is ever shown**
   (false precision invites rubber-stamping — §3).
3. **A shared decision vocabulary** (approve/edit/reject/skip/why) as a *starting
   point*, with clients free to diverge as we learn — shared contract, not identical
   screens (§2/§5).
4. **Editing is first-class**, as cheap as approving (§2).
5. **Resumable by construction** — losing the tab or the terminal loses nothing (§4).
6. **Paced to attention** — throughput is bounded by real review capacity (§3). *(MVP: the
   pacing mechanism is one-at-a-time single-editor review; active throttling + honeypots are
   post-MVP — redline 2026-06-02.)*
7. **Command-first, chat-optional** — the core never depends on the chat layer (§1).

## What this hands to architecture (`06`) and the API (`07`/`08`)

The interaction model above *is* the input `01` promised. Concretely it requires:

- a **durable, resumable proposal/approval state machine** with a `confirmationToken`
  bound to a proposal hash (drives `06`'s state machine + `Storage` edge);
- an **event stream** rich enough to render evidence-before-verdict and live progress
  (drives the SSE event shape — proposal + diff + rationale + a **generic,
  enhancer-declared evidence payload**, not a hardcoded source-quote locator; see §3,
  `06`/`08`/`adr/0005`);
- **server-held credentials** so neither client touches OAuth tokens (`06`);
- a **single API surface** both clients consume, so parity is structural (`06`/`08`);
- a **"next pending proposal" navigation** so sequential review is ~one keystroke — fast
  N→N+1 without bulk submit (drives a cheap advance in the API/event model; the durable
  proposal store already supports it — §2);
- **honeypot + throttle hooks** in the proposal-surfacing path (drives where `11`'s
  anti-rubber-stamping lives in the loop) — *post-MVP (M4b, `10`): reserve the seam now,
  ship the mechanism later (redline 2026-06-02).*

## Redline outcomes (resolved by owner, 2026-06-02)

The five open points are decided. The redline **did not disturb the base framework**
(`adr/0002`) — every decision is served by the existing durable-HITL / SSE / one-API shape.

- **§1 — command-first, chat-optional:** confirmed as drafted.
- **§2 — one-at-a-time + fast sequential:** one decision at a time stands; **fast N→N+1
  advance is an MVP requirement**; **true batch (multi-select + one submit) is not the
  model**. A skim/triage queue view may come later.
- **§3 — anti-rubber-stamping mechanisms:** **honeypots + active throttling are post-MVP**
  (possibly long post-MVP), calibrated once quality data exists; MVP is naturally paced by
  one-at-a-time single-editor review. *(Evidence-forward rendering + progressive disclosure
  stay in MVP — they are the load-bearing anti-rubber-stamping UX.)*
- **§5 — minimal web client:** **in MVP-1**, primarily to exercise the shared API/contract;
  kept minimal.
- **§6 — audience:** **experienced editors** now; newcomer surfaces out of scope.

Still open / unredlined (architecture-facing, uncontested): the **"save my position" / pin**
affordances (§4) remain as drafted, pending `08` detailing. *(The enhancer-declared **evidence
payload** shape — §3 — was decided 2026-06-02 as a hybrid envelope; see `07`.)*
