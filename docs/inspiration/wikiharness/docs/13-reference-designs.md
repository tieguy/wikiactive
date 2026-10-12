# 13 — Reference Designs (for review: what to borrow, what to skip)

> **Staging — WHAT (converging).** Vision / scope / posture / findings: close to settled, changes deliberately. Architecture & mechanics (the *how*) live in `06`–`10` and stay tentative until base-framework sign-off — see [Staging](../README.md#staging).

Prior *systems* worth studying as design references (distinct from the *tools we
reuse as infrastructure*, which live in `12 §A`). This doc exists so Luis can
decide, per system, **what is actually worth bothering with.** Each entry is
framed for that decision, not as a settled conclusion.

**Confidence note (honest):** several primary PDFs (Nature/SIDE, WINELL, ProVe
internals) blocked automated fetch during research; those entries rest on
abstracts + secondary write-ups and are marked **[thin]**. WikiChat and Sphere
were verified from their GitHub repos **[verified]**. Numbers attributed to a
paper are paper-reported, not independently reproduced.

## At-a-glance

| System | One-line | Verdict (proposed) | Confidence |
|---|---|---|---|
| **Meta SIDE / Sphere** | Flag a weak citation; suggest a better source | **Borrow the *loop*; skip the corpus** | [thin] on results, [verified] on code/license |
| **Stanford WikiChat** | Grounded generation: keep only retrieved-supported facts | **Borrow the discipline; maybe reuse the endpoint** | [verified] |
| **WMF "Add a Fact"** | LLM posts a structured citation to a talk page | **Borrow the pattern wholesale** | [thin] |
| **WMF "Add a Link/Image"** | Reversible one-click accept/reject micro-tasks | **Borrow the UX contract** | medium |
| **WINELL** | Multi-agent suggest-edits, trained on edit history | **Study; don't depend** | [thin] |
| **ProVe** | Verify a Wikidata claim against its reference | **Study as the cautionary precedent** | [thin] |
| **SIFT (your own)** | Investigate→verdict, "no fetch no verdict" | **This *is* the core; formalize it** | [verified, your repo] |
| **CheckPlease** | Existing fact-checking prompt set | **Skim; low priority** | low |

---

## Meta SIDE + Sphere — "Improving Wikipedia verifiability with AI"
[Nature MI 2023](https://www.nature.com/articles/s42256-023-00726-1) ·
[code (archived)](https://github.com/facebookresearch/Sphere)

- **What:** A pipeline (not an agent): claim → generated search query → retrieve
  candidates from **Sphere** (134M-doc web corpus; BM25 + DPR indices) → a
  **Verifier** ranks the *existing* citation against retrieved alternatives → if the
  original ranks low, flag the claim and suggest a better source.
- **Borrow:** the **loop shape** — `query-gen → retrieve → rank existing-vs-
  alternatives → flag + suggest`. This is essentially MVP-1's spine, and it is a
  *published, peer-reviewed* validation that the shape works.
- **Skip:** the **Sphere corpus itself** — frozen ~2021 snapshot, repo archived Oct
  2023, CC-BY-NC. Use live search + IA/GLAM instead. Also skip the pre-LLM
  Verifier; an LLM verifier (which Sphere predates) reasons about *support*, not
  just retrieval rank.
- **Relevance:** high — it is the closest peer-reviewed precedent to MVP-1.
- **Open question:** worth pulling the exact human-eval numbers (blocked in
  research) to cite as an MVP-1 baseline in `11`? I can target the PDF specifically.

## Stanford WikiChat — grounded generation
[repo](https://github.com/stanford-oval/wikichat) · [paper](https://arxiv.org/abs/2305.14292) · Apache-2.0 **[verified]**

- **What:** Generate a draft, then **keep only the facts it can ground** against
  retrieved Wikipedia passages, retrieve more, recompose. Reported 97.3% factual
  accuracy in simulated conversation; v2.1 covers 25 Wikipedias with a **free
  hosted retrieval endpoint**.
- **Borrow:** the **"keep only grounded facts" discipline** — it is the generative
  analogue of your "no fetch, no verdict." And potentially **reuse the hosted
  multilingual Wikipedia retrieval endpoint** as a grounding backend rather than
  standing up our own index.
- **Skip:** the chatbot/conversation framing — we're not building a chatbot.
- **Relevance:** medium-high (discipline + a possible free dependency).
- **Open question:** is depending on a Stanford-hosted endpoint acceptable, or does
  Principle 6 (community infrastructure / no external dependency) rule it out for
  anything but experiments?

## WMF "Add a Fact" — the closest cousin to our write path
[Future Audiences experiment](https://meta.wikimedia.org/wiki/Future_Audiences/Experiment:Add_a_Fact) **[thin]**

- **What:** Highlight a claim in a secondary source → an LLM (Llama3-70B) finds the
  best target Wikipedia article and posts a **pre-filled talk-page template with a
  structured citation.** Guardrails: ~10 facts/day cap, unreliable-source warnings,
  spam-blocklist hard block. Outcome: some productive contributions, **community
  pushback on AI ethics, low retention.**
- **Borrow:** the **pattern wholesale** for any not-yet-trusted edit class —
  route the proposal to a **talk page / review queue** with a structured citation,
  plus **reliable-source + spam-blocklist checks** and a **rate cap.** This is a
  WMF-blessed shape that stays out of mainspace.
- **Skip / learn-from:** the *retention* failure — a standalone "highlight on the
  web" gadget didn't stick. The lesson (also `12 §E`): **design for editor adoption
  inside existing workflows, and model accuracy is not the bottleneck — trust,
  disclosure, reversibility, retention are.**
- **Relevance:** high — it is the de-risked version of "assisted edit" before full
  confirm-each-edit in mainspace.

## WMF Growth "Add a Link" / "Add an Image" — the UX the community already trusts
[Add an Image](https://www.mediawiki.org/wiki/Growth/Personalized_first_day/Structured_tasks/Add_an_image)

- **What:** Algorithm-backed **reversible, single-decision** micro-tasks
  (accept/reject/skip) with provenance, used to add images to 90k+ articles. "Add
  an Image" deliberately avoids CV — it recycles *existing* P18/P373/interwiki links.
- **Borrow:** the **UX contract** — one decision at a time, reversible, provenance
  shown, easy to skip. Matching this format is how we inherit community acceptance;
  it is also exactly our human-confirm-each-edit model.
- **Skip:** its *coverage ceiling* — recycling existing links can't serve the
  unlinked long tail (that gap is our niche, `12 §D`), and its models are pre-LLM.
- **Relevance:** high for UX; low for the matching engine.

## WINELL — Wikipedia Never-Ending Updating with LLM agents
[arXiv 2508.03728](https://arxiv.org/abs/2508.03728) (WWW 2026) **[thin]**

- **What:** Multi-agent `analyze structure → iteratively web-search → select
  important new facts → aggregate → generate fine-grained edit suggestions for human
  review`; editor models **conditioned on Wikipedia revision history** so
  suggestions resemble human diffs; reported to beat GPT-4o on coverage/efficiency.
- **Borrow (as ideas):** (a) **condition edit phrasing on real revision history** so
  proposals look like human edits; (b) the **search→select→aggregate→suggest**
  decomposition with human-in-loop.
- **Skip / caution:** no public code surfaced; it leans toward *adding facts*
  (closer to content generation) — we must keep our stricter "no unsourced claim /
  no fetch no verdict" line. Don't take a dependency; mine the ideas.
- **Relevance:** medium — closest *published* cousin, useful as a reference
  architecture and a literature citation, not a building block.
- **Open question:** worth extracting its exact method/metrics (blocked PDF) to
  position WikiHarness against it in writing?

## ProVe — reference verification on Wikidata
[Wikidata:ProVe](https://www.wikidata.org/wiki/Wikidata:ProVe) **[thin]**

- **What:** A 3-model pipeline (T5 + 2×BERT) that checks whether a Wikidata
  statement's **reference actually supports it** — and surfaces a score. Crucially:
  **display-only; it never crossed into editing.**
- **Borrow:** it is the **direct precedent** for `verify_citation` /
  `propose_statement`, and the closest existing thing to your "does the source
  support the claim" verdict on Wikidata.
- **Study as the cautionary precedent:** ProVe is exactly the "academic prototype
  that never crossed the deployment gap" your own survey names. The lesson is *why*
  — and crossing that gap (assisted, human-confirmed, evidence-first) is WikiHarness's
  whole reason to exist.
- **Relevance:** high as motivation/positioning; medium as design.

## SIFT (your own `wikidata-SIFT`) — the actual core
[`config/sift_prompt_openrouter.md`, `run_verdict_fanout.py`, `tool_executor.py`]

- **What:** Two-phase **investigate (web_search/web_fetch tool loop) → structured
  verdict**, with "no fetch, no verdict," verified-vs-reported provenance, no editor
  identity signals, and a cheap open-model ensemble (2-of-3 agreement → 55% of queue
  auto-accepted at 96.6% precision on the 500-edit eval).
- **Borrow:** *this is not a reference to borrow from — it is the spine to port and
  formalize.* The verdict schema, the provenance discipline, the ensemble/agreement
  idea, the failure taxonomy (`hard-patrol-problems.md`), and the eval harness all
  carry forward (already reflected in `04`, `07`, `11`).
- **Diff-checking framing (accepted via `adr/0005`, 2026-06-02):** SIFT's verification machinery (agreement
  thresholds, the verdict schema) carries forward and can be **pointed at an incoming
  edit**, not just at an article we're improving — but this is the same core aimed at
  a diff, **not** a separate "patrol" mode, and not an attempt to compete with
  well-covered recent-changes/vandalism patrol (`01`, `docs/adr/0005`). The one line
  that stays out is autonomous reverting: WikiHarness surfaces verdicts for human
  action.

## CheckPlease — existing fact-checking prompt set
[checkplease.neocities.org](https://checkplease.neocities.org)

- **What:** A community fact-checking prompt collection (noted in your SIFT docs as
  prior art to adapt for Wikidata sourcing norms).
- **Verdict:** **skim, low priority** — possibly a source of prompt patterns;
  unlikely to be load-bearing given the SIFT prompt already exists and is tuned.

---

## Process & harness-robustness references (a different category)

The systems above are *domain* references (verification/citation work). This section
is about **how to build the harness so it forces rigor on itself** — the
"structure forces robustness" philosophy the README leads with. **Adopted in
`docs/adr/0007`** (blocking hooks, the planning loop, end-of-stage bug-hunt,
multi-model voting; and they motivate MVP-3). These three Claude
Code skill/plugin frameworks were mined for engineering-process patterns, not
domain logic. They converge remarkably: rigor comes from **hard, ordered gates and
behavior-intercepting hooks**, not from asking the model nicely.

| Project | One-line | What to borrow |
|---|---|---|
| [**Superpowers**](https://github.com/obra/superpowers) (obra) | Mandatory, ordered "skills" pipeline (brainstorm → plan → execute) injected at session start | Hard-gate markers; ordered skill/enhancer pipeline with terminal states; TDD "iron law"; verification-before-completion; red-flag rationalization tables |
| [**ed3d-plugins**](https://github.com/ed3dai/ed3d-plugins) (ed3dai) | Plugin marketplace: phase-gated plan-and-execute, house-style, security/skill-reinforcement hooks | Phase gates with context isolation; severity-tiered code-review **as a hard merge gate**; pre/post-tool-use hooks that *block* (secret-leak guard, skill reinforcement); project-scoped guidance files |
| [**crosslink**](https://github.com/forecast-bio/crosslink) (forecast-bio) | Rust/Git persistence + coordination layer for AI agents (event-sourced, signed) | **Event-sourced append-only audit log** with deterministic replay; mandatory typed comments at every decision; tiered enforcement (strict/normal/relaxed); property-based + fuzz testing in CI; design-doc-driven decomposition |

**The transferable disciplines (what we adopt, and where):**

1. **Hard gates over good intentions.** Encode non-negotiable barriers as explicit,
   machine-checkable gates — not prose the model can rationalize past. We already
   have this shape (anti-hallucination gate, policy-as-data promotion gates, the
   no-write-without-confirmation rule); these three confirm the pattern and push us
   to make every guardrail a *gate*, not a guideline (`02`, `04`). _New idea worth
   adopting:_ **behavior-intercepting hooks** (pre/post-tool-use) that *block* a bad
   action structurally, so compliance doesn't depend on the agent cooperating.
2. **Ordered pipeline with terminal states (the enhancer DAG).** Superpowers'
   brainstorm→plan→execute ordering maps directly onto our enhancer model (`06`,
   `adr/0005`): each enhancer declares preconditions it requires and postconditions
   it establishes, and illegal orderings are rejected. This makes "modular
   enhancers" a *contract*, not just packaging.
3. **Verification-before-completion / "no claim without fresh evidence."** The exact
   runtime sibling of "no fetch, no verdict": no enhancer reports success without a
   fresh, recorded verification artifact. Reinforces the eval/evidence gates (`05`/`11`) and
   the replayable-audit posture (`04`, `11`).
4. **Event-sourced, replayable audit (crosslink).** Strong validation of our
   append-only audit-ledger + provenance design (`02 §4`, `06`): an immutable event
   log with deterministic replay is exactly how we make verdicts auditable and
   disputes resolvable. Worth adopting crosslink's `(timestamp, actor, seq)`
   ordering discipline for the ledger.
5. **TDD iron law + severity-tiered review as merge gates** — for *building*
   WikiHarness itself (our own dev process, `10`), not just its runtime: tests-first
   with deletion-on-violation, and code review that *blocks* on critical issues.

_Skip / not-for-us:_ crosslink's multi-agent distributed locking and SSH-signed
trust model (we're not orchestrating an agent swarm); the marketplace/packaging
machinery (we have our own package layout, `06`). Mine the disciplines, not the
infrastructure.

---

## What I'd actually bother with (Claude's recommendation, for your call)

- **Definitely:** SIDE *loop*, Add-a-Fact *pattern*, Add-a-Link/Image *UX contract*,
  WikiChat *grounding discipline*, and ProVe as *positioning*. These are cheap to
  adopt and each de-risks a specific part of MVP-1.
- **Ideas only, no dependency:** WINELL.
- **Probably skip / low priority:** CheckPlease; the Sphere corpus.
- **Process disciplines (definitely, cheap):** from Superpowers / ed3d-plugins /
  crosslink — hard gates over guidelines, behavior-intercepting hooks, an ordered
  enhancer DAG, verification-before-completion, and an event-sourced replayable
  audit log. Most reinforce designs we already have (`02`/`06`/`04`); the genuinely
  *new* idea to adopt is **blocking pre/post-tool-use hooks**.
- **Needs your decision:** whether to depend on WikiChat's hosted endpoint
  (Principle 6 tension); whether to chase the blocked SIDE/WINELL/ProVe numbers
  to use as written baselines in `11`; and whether the "process disciplines" above
  warrant their own ADR or just thread into `04`/`10`.
