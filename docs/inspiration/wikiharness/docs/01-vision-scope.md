# 01 — Vision & Scope

> **Staging — WHAT (converging).** Vision / scope / posture / findings: close to settled, changes deliberately. Architecture & mechanics (the *how*) live in `06`–`10` and stay tentative until base-framework sign-off — see [Staging](../README.md#staging).

## How to read this document (provenance)

This file keeps **Luis's vision** distinct from **planning decisions**, so the
vision stays yours, not the planner's. The two-part structure now carries that
split, so individual headers no longer need a provenance tag:

- **# Vision** — Luis's, drawn from `open-graph-next` (the `README`, the
  `all-the-opens` and `wikidata-SIFT` docs, and the WikiCredCon 2026 lightning
  talk). Quoted where possible. The few *planning decisions* that sit inside it are
  flagged inline _(planning decision)_.
- **# Scope** — choices made jointly in *this* planning process. Real decisions, but
  not pre-existing vision; **all revisable**.
- **_Claude's note_** — the planner's own analysis/inference, flagged inline so it
  can be challenged. Anything I was unsure reflected your intent is flagged inline
  rather than asserted as fact.

---

# Vision

_This part is Luis's vision (quoted where possible); the few planning decisions
inside it are flagged inline._

## North star — the aim

WikiHarness is one expression of the broader `open-graph-next` premise:

> *"LLMs are going to reshape how open knowledge is created, maintained, and
> connected... How can LLMs help make open knowledge more rigorous, more
> exhaustive, more interconnected, and more accessible — without losing the human
> collaboration that makes it a genuine monument to what humans can achieve."*
> — `open-graph-next/README`

And the cooperative-infrastructure half of that premise:

> *"Exploring what it would look like if open knowledge institutions worked
> together at internet speed."* — `all-the-opens/README`

So the goal *for this project, as it starts* is not to build "an AI that edits
Wikipedia." It is **human-centric, human-curated, LLM-assisted open knowledge** —
using LLMs to make the open knowledge graph more rigorous, more complete, more
interconnected, and more accessible, with humans at the center and the institutions
(Wikipedia, Wikidata, Commons, the Internet Archive, libraries, museums)
cooperating across their boundaries.

This is a *starting stance*, not a permanent wall — and it is worth being precise
about which part can move and which cannot:

- **The starting stance (now, and in every MVP): a human in the loop of *every
  edit* — the *inner* loop.** Nothing reaches article space without a person
  confirming that specific change.
- **The permanent wall (never crossed): WikiHarness stays *human-centric* — a human
  stays in the loop *somewhere*.** What is negotiable is *where*. With evidence, the
  human role may move from the **inner** loop (approving each edit) outward to the
  **outer** loop (developing and curating the rulesets, and evaluating outputs in
  aggregate). "No human anywhere" is never on the table.

The line between "an AI that edits" and "a human who edits with AI assistance" will
blur, and the same modular enhancers (citation-checking, cross-database enrichment,
…) could later be repurposed for a more autonomous bot — editing at a scale similar
to pre-LLM wikibots. We don't rule that out: it is a plausible **future mode**,
reached — if at all — only by climbing the same evidence ladder and earning
community approval through **BRFA** (Wikipedia's *Bot requests for approval*
process) that gates every other boundary (`11-evidence-and-policy-strategy.md`,
`docs/adr/0003`), never by quietly drifting there. What stays fixed is the
human-centric commitment; what may move — only on evidence — is whether the human
sits in the inner or the outer loop.

> _Claude's note — first articulation, expect iteration._ The inner-loop/outer-loop
> framing is my attempt to pin down the "starting stance vs. permanent wall"
> distinction you flagged as still hard to articulate; treat it as a draft to refine.

## What WikiHarness is: a modular enhancement framework

_(Planning decision — the modular-framework shape was chosen in planning; recorded
in `docs/adr/0005`.)_

WikiHarness is not a single tool; it is **a framework that hosts many composable,
independently-testable enhancement capabilities** over the open knowledge graph —
and an **umbrella** over two existing lineages: the verification/editing lineage
(`wikidata-SIFT` + the citation-checker work) and the broader open-graph work,
**plus** room for new enhancement types as they're discovered.

The governing analogy (Luis's): **a code-quality tool suite.** A CI harness
composes independent passes — TDD, then memory static-analysis, then complexity
analysis, then a fuzzer — each separately useful and separately testable.
WikiHarness composes the knowledge-quality analogues — citation verification, then
book-scan sourcing, then Wikidata enrichment, then open-GLAM linkage, then
more — over a **shared core** (fetch/verify with "no fetch, no
verdict", provenance, human-in-the-loop, write paths). Implications:

- **Enhancers are plugins.** Each capability is a first-class, registrable unit
  with a typed contract; the harness runs one, or composes several. This is the
  concrete form of "every subcomponent deeply testable."
- **One core, two targets — not two modes.** The same verification core ("does this
  source support this claim?") can be pointed at an article you're improving *or* at
  someone else's incoming edit; checking a diff is just that core aimed at a diff, not
  a separate "patrol" mode. (Recent-changes/vandalism patrol is already well-covered
  by ORES/Huggle/SWViewer; the uncovered gap is citation-level verification — our
  core.)
- **Outputs are consumer-agnostic.** Not every consumer is a Wikipedia article or a
  Wikidata item. The harness emits structured, addressable *findings/proposals*
  that external surfaces can ingest — e.g. a discovery/"rabbit-hole" visualization
  is a plausible **consumer of our outputs** (out of scope to *build*, in scope to
  *feed*). The write paths to Wikipedia/Wikidata/Commons are one class of consumer,
  not the only one.
- **UX is pluggable *in principle* — but v1 must still commit to one concrete UX.**
  The core is consumer-agnostic, so many front-ends *can* sit on it; that is not the
  same as deferring the choice. **An API is not a product:** we owe at least **one**
  shipped UX (initially a CLI, partly conversational/chat-driven). That first UX is a
  real, **locked-in** decision that will shape the API surface, data structures, and
  event model — a load-bearing *how* decision (`06`/`adr/0002`), not a thin
  afterthought. Generalized "compose any enhancer" UIs are genuinely hard, so we keep
  the layer thin — but we do not pretend the first interaction style is free or
  freely reversible.

## The architectural thesis: why a *harness*, not a chatbox

If the **North star** (above) is *what* WikiHarness builds toward, the **thesis** is
*why it takes this form*. The bet is architectural:

> **A harness — deterministic code + LLMs + systematic validation, composed — is far
> more robust than asking a chatbox.** A bare model guesses; a harness wraps the
> model in code that fetches the real inputs, constrains what the model is allowed to
> assert, validates its output deterministically, and measures the whole pipeline
> against labeled data. The LLM does the one thing it is good at (reading at scale);
> everything load-bearing around it is checkable code.

*(The **commitment** to a harness is firm vision; the specific composition — which
runtime, which edges — is the tentative HOW in `06`–`10`, see
[Staging](../README.md#staging). This is the same "structure forces robustness"
philosophy stated in the README.)*

This is what lets us make the deliberately provocative WikiCredCon 2026 claim true
*in practice*:

> *"The hallucination machines can help with credibility."*

**The flagship example of the pattern is the one load-bearing rule** (lightning
talk, slide 2):

> *"No fetch, no verdict. Every 'verified' source must be a URL the model actually
> fetched in this session."*

That rule is not a *hope* about model behavior — it is **enforced by the harness**:
code resolves and fetches the source, the prompt forbids using anything but the
fetched text, and a deterministic layer parses/canonicalizes the verdict and caps it
when the source was never actually retrieved. The `citation-checker-script` lineage
we're porting (`00`) already implements this shape — e.g. live-over-archive URL
fetching (`#221`), deterministic verdict parsing/canonicalization (`#214`, `#215`),
and pipeline-side detection of structurally unusable source bodies *before* the
model is trusted (`#217`) — all measured against a labeled benchmark. (That is the
thesis in miniature: an LLM in the middle, deterministic code and validation on
every side.)

Consequences, in your framing:
- A source the model only "remembers" is marked **reported**, never **verified**;
  the schema caps the verdict accordingly. Hand-waving with a remembered URL is
  **"citation laundering"** and invalidates the verdict.
- Therefore **every verdict is a replayable audit trail**: *"you don't need to
  trust the model — you re-run its fetches and read the sources yourself. It's the
  same thing Wikipedia editors already do when they audit each other."*
- Because the constraint lives in the **prompt and the surrounding code, not the
  model's goodwill**, it works on **cheap, open-weight models** — no dependency on a
  frontier vendor.

## Division of labor: humans and LLMs each do what they are good at

The point of the tool is to put scarce human attention where it matters, not to
replace it (lightning talk, slide 3):

- LLMs are used for **"lateral reading at scale" and nothing else** — taking the
  *volume* of routine verification/matching off the queue.
- *"The hard problems stay human. They are also the interesting problems."* When
  the models are wrong, they are wrong about Wikidata-specific judgment (qualifier
  semantics, ontological mapping, source-hierarchy disputes, character-level
  fidelity), not about inventing facts — *"the failure mode maps perfectly onto
  the value of your expertise."*
- This is also a **burnout** stance: the `wikidata-SIFT` guidelines name the
  community's fear that AI turns editing into *"an increasingly investigative and
  adversarial process."* WikiHarness must **reduce** that burden — surface fewer,
  better-evidenced things for humans to decide — never add to it.

## Principles: the rules we hold ourselves to

Where the **North star** is the *aim* and the **thesis** is the *method*, these are
the **operating commitments** — the short, citable list we (and reviewers) hold the
project to. Carried from `wikidata-SIFT/docs/llm-guidelines.md` and the lightning
talk. The first two restate the vision above as testable rules rather than re-argue
it; the distinctive commitments are **3–6**.

1. **LLMs as research assistants, not content generators** — identify, suggest,
   interpret, and draft for review; never generate claims, fabricate references, or
   edit unsupervised. *(Argued under Division of labor.)*
2. **Source-first / verifiability is non-negotiable** — every claim traces to an
   actual fetched source; LLM "knowledge" is never a source. *(Enforced by "no
   fetch, no verdict" — see the thesis.)*
3. **Human responsibility and mandatory review.** *"As with all edits, an editor is
   fully responsible for their LLM-assisted edits."* "The AI told me" is not a
   defense. All output is reviewed by a human before any edit acts.
4. **Transparency / disclosure.** AI assistance is disclosed in edit summaries;
   every session is logged in machine-readable form (model(s) used, sources consulted,
   graded verdict + ensemble agreement, reasoning, human decision).
5. **No editor identity signals — assess the edit on its merits, not the editor's
   reputation.** The fairness pivot that distinguishes the approach from ORES / the
   Sarabadani-2017 baseline, which weight anon/account-age/curator-group features
   the authors themselves call *"not fair to anonymous and new editors."*
6. **Open weights, open code, community infrastructure** — must run on cheap
   open-weight models (open-capable / open-default / open-tested; frontier
   optional). *(Hard model-layer constraint in `docs/adr/0006`; open-vs-frontier
   "carrot" in `11`.)*

_Not a vision principle: **accords.** The `open-graph-next` `accords/` subproject
("reusable orientation documents giving humans and LLM agents common language for
working together on a domain") is, per Luis, **an experiment in guardrails — not
necessarily part of the vision.** It is noted here only so it isn't mistaken for a
commitment; whether WikiHarness draws on it as one possible guardrail mechanism is
**TBD / out of scope for now**, not a principle._

## Mission: move the window, with evidence

Your stated goal as a long-term Wikipedian: **expand what the community allows
AI-assisted editing to do, by demonstrating it is useful, reliable, and safe** —
*"as long as we also have firm, empirically grounded data to show the quality of
our work."* Evidence first; ask second; through the community's own processes.
This is the same posture as the lightning talk (publish the numbers, the prompt,
the verdict logs; *"I want you to argue about it"*). Detailed in
`11-evidence-and-policy-strategy.md` and `docs/adr/0003`.

## Open models & community infrastructure

A first-class commitment, not a footnote: WikiHarness must run on **cheap,
open-weight models on community infrastructure** — *open-capable* (it always works
on open models), *open-default* (open is the default), *open-tested* (CI eval gates
and the published evidence run on a **pinned open model**). Frontier models are an
optional, pluggable backend, never a dependency; **no credibility claim rests on a
closed model**. This is what makes the tooling *"runnable on community
infrastructure,"* and it is only possible because the load-bearing constraint lives
in the prompt and surrounding code, not in any one vendor's model (see *the
architectural thesis*).

It is also a **strategy, not just a constraint** — the *"carrot."* The evidence
harness publishes an honest **open-vs-frontier scoreboard** on WikiHarness's own
tasks, which (a) demonstrates open models are good enough for credibility work, and
(b) hands the Wikipedia ecosystem a public, concrete lever to recruit open-model
orgs (IBM / Mistral / AI2 / …) to close any remaining gap — *"doable for Wikipedia"*
in a way it isn't for most projects. (Hard constraint: `docs/adr/0006`, `02 §8`; the
published panel: `11 §4b`.)

## Knowledge and institutions as first-class

The target is *knowledge*, of which article prose is only one surface — and the
distinctive move is treating cooperating institutions as a connected graph, à la
the `all-the-opens` "rabbit hole" (Internet Archive ↔ Wikipedia ↔ Commons ↔
OpenLibrary ↔ Smithsonian ↔ …). Equal targets:

- **Citations** — verify a source actually supports its claim; repair malformed
  cites; enrich bare URLs into full templates; find *better* sources (incl. scanned
  books) for weakly-cited claims.
- **Wikidata** — add missing, *sourced* statements, identifiers, and sitelinks;
  link a subject across the identifier network (VIAF/GND/GBIF/…) and to open
  collections.
- **Internet Archive** — cite specific scanned book pages; archive live citation
  URLs; surface full-text evidence. The IA is both a *source* and a *sink*.
- **Open GLAM media & data** — openly-licensed (ideally CC0/PD) metadata and media
  from sources that accept open contribution or are CC0 (Commons, plus the Met,
  Rijksmuseum, Smithsonian, etc.), linkable from Wikidata.

**Sources vs. sinks vs. consumers.** Sources that accept open contribution are
first-class *write targets* (sinks), not just read sources: Wikipedia (assisted,
human-confirmed), Wikidata, Commons, the Internet Archive. Read-only enrichment:
CrossRef, museum APIs, SPARQL endpoints, search. **And a third role —
*consumers*:** because outputs are consumer-agnostic (see "What WikiHarness is"),
the findings/proposals the harness produces can also be ingested by non-edit
surfaces (a discovery/"rabbit-hole" viz, dashboards, other tools). Every tool is
tagged `read`/`read+write` (`07`); every write path is governed by the
human-confirmation + provenance rules.

---

# Scope

_Everything in this part is a **planning decision** made jointly in *this* process —
real, but revisable; not pre-existing vision._

## Scope decisions

- **Scope shape:** a **modular enhancement framework** (umbrella over the
  SIFT/citation-checker + open-graph lineages, plus new enhancers over time), with a
  **shared verification core that applies equally to improving an article and to
  checking an incoming edit**, and **consumer-agnostic outputs**. (See "What
  WikiHarness is" above; proposed in `docs/adr/0005`, pending your review.)
- **Edit delivery:** assisted editing — **the human confirms every edit** (OAuth2).
  No autonomous article-space editing anywhere in the planned roadmap — though we
  expect to learn more about the real strengths and weaknesses of autonomous editing
  over time, and would revisit the line if the evidence warrants. (Applies equally
  when the target is someone else's incoming edit: we surface a verdict for human
  action; we do **not** auto-revert.)
- **Product surfaces:** API-first; **CLI and web are two clients of one core**, so
  editors uncomfortable with a CLI can drive the same workflows. The core is
  consumer-agnostic, but **v1 must still ship at least one concrete UX — an API is
  not a product.** That first UX (initially a CLI, partly conversational/chat-driven)
  is a **locked-in** choice that feeds back into API design, data structures, and the
  event model, so it is a load-bearing input to the architecture, not a deferred
  detail. (Primary input to `docs/adr/0002`.)
- **MVP sequencing** (the first enhancers, in order):
  1. **MVP-1 — interactive citation verification** (architectural testbed): a TS
     evolution of `citation-checker-script` from a *static report* into an
     *interactive, human-confirms-each-edit* enhancement workflow.
  2. **MVP-2 — Internet Archive book-scan sourcing**: page-accurate `{{cite book}}`
     for weakly-/un-sourced claims. (Chosen over dead-link repair, which IABot
     covers.)
  3. **MVP-3 — codify a "words to watch" rule** (`docs/adr/0007`): a bounded
     demonstrator that **blocking hooks + multi-model voting** can make small,
     concise *content* improvements safely on a rule traditional non-NLP bots can't
     evaluate. Flag-first; any minimal text change is multi-model-agreed +
     human-confirmed — **not** general prose generation. **Rule chosen:
     MOS:PEACOCK (puffery)** — gated on dataset availability (`12 §C-bis`: public
     code+data, ~0.963 F1 baseline, citation-feature synergy). **MOS:WEASEL** is the
     immediate follow-on enhancer, reusing the same `detect → vote → propose`
     pipeline — the reuse *is* the proof the enhancer framework composes.
  4. **Later enhancers — Wikidata enrichment → open GLAM media (text/metadata only,
     no image parsing) → general text.** General text is sequenced
     *last because it is hardest to prove safe*, not because it is permanently
     barred; the bar is empirical (`11`). (MVP-3 above is a single-rule exception,
     not general text.) Verifying an incoming edit is the same core pointed at a
     diff — available wherever it is useful, not a separately sequenced enhancer.

### Non-goals (initially — "initially" is load-bearing)
- Autonomous / unattended editing of article space — including **auto-reverting an
  edit we have flagged** (we surface verdicts; humans act). _Not categorical: autonomy is a
  possible far-rung future mode gated by the evidence ladder + BRFA (see North star
  and `11`); it is out of scope **now and for every MVP**, not forever._
- Generating or rewriting article *prose* with an LLM (out of scope *now*;
  revisited via the evidence ladder, not a permanent ban — `02`, `11`).
- **Building** a discovery/"rabbit-hole" visualization or other end-consumer UI —
  out of scope to build, but a first-class *consumer* of our outputs to design for.
- Mass / high-throughput bot-style editing — pushing large volumes of edits without
  per-edit human judgment. (Making an individual editor's own workflow *faster and
  more efficient* is squarely a goal; what stays out is automated volume that bypasses
  the "human confirms every edit" rule.)
- Non-English Wikipedias first (architecture stays i18n-capable).

> _Note: checking someone else's incoming edit is **not** a separate "patrol" mode —
> it is the verification core pointed at a diff. What stays out is **autonomous
> moderation/reverting**: we assess edits and surface verdicts for human review, we
> never auto-revert. (Recent-changes/vandalism patrol is well-covered elsewhere.)_

### Success criteria
- Every subcomponent is an independently, **deeply unit-testable** tool with a
  typed contract, no hidden I/O, and recorded fixtures (no network/model needed).
- A human runs MVP-1 end-to-end from *either* CLI *or* web, reviewing each proposal
  with rationale + provenance and confirming edits one at a time.
- No edit reaches article space without per-edit human confirmation, a disclosed
  summary, and a recorded, **replayable** provenance trail.
- The work runs on **open-weight models / community infrastructure** (Principle 6).
- The project produces **community-credible, reproducible quality evidence**
  (introduced-error and hallucinated-citation rates, acceptance/reversion vs. human
  baselines) strong enough to justify expanding what is permitted (`11`).
