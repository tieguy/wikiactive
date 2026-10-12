# 11 — Evidence & Policy Strategy (moving the window)

> **Staging — WHAT (converging).** Vision / scope / posture / findings: close to settled, changes deliberately. Architecture & mechanics (the *how*) live in `06`–`10` and stay tentative until base-framework sign-off — see [Staging](../README.md#staging).

## Thesis

WikiHarness is not built to sit *within* the current limits of what Wikipedia
allows AI to do. It is built to **move those limits** — to expand the window of
accepted AI-assisted editing — by demonstrating, with firm, empirically grounded
data, that our work is **useful, reliable, and safe.**

Pushing on the rules is therefore an explicit goal, **conditioned on one
non-negotiable**: we may only ask the community to extend a boundary once we can
*show*, with reproducible evidence the community can independently scrutinize, that
our work at that boundary meets or exceeds the quality bar the community cares
about. Evidence first; ask second. No evidence, no ask.

This is the reason the entire codebase is shaped around measurement
(`04-testing-strategy.md`): the eval machinery is not just CI hygiene — it is the
**instrument that earns the right to expand scope.**

## Why this is the right posture for *this* community

The 2026 LLM guideline is restrictive for good, specific reasons: hallucinated
statements and fabricated citations, and the difficulty of *scrutinizing*
high-speed AI edits. Those are **empirical claims about quality and
reviewability.** They are answerable with data — and they are exactly the claims
our measurement subsystem is designed to answer. The way to change a
quality-based rule is not rhetoric; it is to retire the underlying fear with
evidence. We engineer to make the fear *false*, then we show our work.

A long-term-Wikipedian framing also dictates *how* we push: from inside the
community's norms, not around them. Transparency, consensus, reversibility,
humility, and a higher-than-human standard for ourselves are the levers — not
clever automation that outruns review.

## What "firm, empirically grounded data" means here

Internal CI gates (`05`) are necessary but **not sufficient** to move
policy. Community-credible evidence is a higher bar with distinct properties:

1. **Ground truth the community trusts.** Human-labeled datasets with documented
   labeling protocols and **inter-annotator agreement** reported; where possible,
   labeled by experienced editors, not just us. Seeded from the existing
   citation-checker (~189 labeled pairs) and SIFT (500 edits) datasets and grown.
2. **The metrics Wikipedians actually fear-test**, foregrounded:
   - **Introduced-error / false-positive rate** — how often a *confirmed* edit was
     actually wrong. This is *the* number that retires the hallucination fear; our
     target is **better than the human-editor baseline**, and we report it that way.
   - **Hallucinated-citation rate** — must be **0** by construction
     (string-location gate), and *audited* to confirm the construction holds.
   - **Precision over recall** — it is far worse to propose a bad edit than to miss
     a good one; we optimize and report accordingly (PR-AUC, per-verdict confusion
     matrices — reuse SIFT's scikit-learn harness).
3. **Real on-wiki outcomes, not just lab metrics:**
   - **Human acceptance rate** — of surfaced proposals, how many editors approve
     (vs. edit / reject). A low accept rate is a quality signal we must not hide.
   - **Reversion/durability** — do our committed (assisted) edits *survive*?
     Reversion rate vs. the baseline reversion rate of comparable human edits.
     (We can measure this with the same recent-changes/diff plumbing SP42 uses.)
   - **Reviewer effort** — time-to-review per proposal; addresses the
     "scrutinizability" objection directly.
   - **Anti-rubber-stamping (the CAT lesson; see `12 §E` / `adr/0004`).**
     Acceptance is **not** a success metric — Computer-Aided Tagging was
     human-accepted and still mass-reverted. The metric that matters is
     **acceptance *correctness***: of *accepted* proposals, what share are
     independently correct and durable. **High acceptance + low independent
     correctness is a red flag we surface loudly**, not bury. We also run
     **reviewer-diligence honeypots** (planted known-good/known-bad gold items in
     the review stream); a reviewer who accepts planted errors is rubber-stamping,
     and that rate is itself a reported metric. Review UI is evidence-forward
     (quote/record/graph context shown first), and surfaced volume is throttled to
     genuine review capacity. **No model-generated confidence/support number is shown** —
     false precision is itself a rubber-stamping lure; the honest signals are the graded
     categorical verdict and measured ensemble (dis)agreement (`03`/`07`).
4. **Comparative baselines.** We never report an absolute number alone. Always vs.:
   (a) experienced human editors doing the same task, (b) existing tools where they
   exist (Citoid for citation fill, IABot for archiving, ORES/LiftWing, and prior
   research systems — **ProVe**, **SIDE**, **Add-a-Fact** — as published baselines),
   and (c) the prior version of our own system (regression tracking). Named reusable
   benchmarks/datasets (see `12 §C`): **ALCE** (citation precision/recall),
   **Citation Needed** (triage), **EUFCC-340K** (GLAM metadata matching),
   **Mix'n'match** unmatched queues, and the owner's `hard-patrol-problems.md`
   disagreement taxonomy.

   **Reporting discipline (ported from citation-checker `ccs compare`; `12 §C`/`05`):**
   every comparison — eval-run regressions *and* the on-wiki outcome deltas (§3, our
   reversion vs. the human baseline) — is computed on the **intersection** of comparable
   cases (dropped/errored cases never flatter a delta), **sliced post-hoc rather than
   pre-filtered** (no confirmation bias), and annotated with **what changed** between
   conditions. Same anti-cherry-picking method as the `05` regression gate, applied to
   the community-facing numbers — reinforcing pre-registration (§6).
4b. **Open-vs-frontier model panel — a first-class published comparison (the
   "carrot"; `adr/0006`).** Run the *same* WikiHarness tasks across a panel of cheap
   **open** models (Granite / Mistral / OLMo / DeepSeek / …) **and** frontier models,
   and publish the gap + per-model cost (cf. SIFT's "under a cent per verdict").
   Purpose is dual: **demonstrate** open models are good enough for credibility work
   on community infrastructure, and **create leverage** — a public, honest scoreboard
   that lets the Wikipedia ecosystem credibly ask open-model orgs to commit
   engineering to close the gap. The `hard-patrol-problems.md` cases (qualifier
   semantics, ambiguous dates, accents, Q-id mapping) are the benchmark's sharpest,
   most diagnostic edges. Reported honestly, including where frontier wins. The panel
   is also the natural place to surface **model-origin bias**: a curated set of
   politically contested / censorship-sensitive topics (`02 §1`, consequence 5;
   gate sketched in `05` — e.g. Tiananmen for Chinese-origin models, January 6
   and climate change for US-origin models) reported **per model**, showing where a
   backend silently omits or skews *well-sourced* content. *(Building these sets is
   post-MVP and does not block the MVP.)*
5. **Adversarial / red-team evaluation.** We actively try to make the system
   propose a hallucinated or unsupported edit, and report how often we succeed.
   A boundary is not "safe" until it survives hostile testing, not just the happy
   path.
6. **Reproducibility & pre-registration.** Eval datasets, methodology, and code are
   public and runnable. Where we intend to make a quality claim to support a policy
   ask, we **pre-register** the methodology and thresholds *before* running, so the
   result cannot be accused of being cherry-picked. (Methodology borrowed in spirit
   from SIFT's label-leakage controls and held-out evaluation.)
7. **Honest negative results.** If a boundary's data is not good enough, we say so
   and do **not** ask to cross it. The credibility that lets us move the window is
   built on never overclaiming.

## What grounding guarantees — and where the burden shifts (the fact→judgment gradient)

The hallucinated-citation rate is **0 by construction** (§2.2) because citation verdicts rest
on a *deterministic* check: the supporting quote must string-locate in a source we actually
fetched (`locateQuoteInSource`, `02 §1`). It is tempting to read the harness's "grounding
generalizes across enhancers" property (`07`/`adr/0007`) as if *every* enhancer enjoyed that
same construction-proof safety. It does not, and pretending otherwise would be the kind of
overclaiming §7 forbids.

**Grounding establishes fabrication-resistance; it does not establish judgment-correctness.**
The grounding predicate proves the model pointed at real retrieved bytes / a real article span /
a real rule id — not that its *evaluation* of them is right. And that proof **weakens along a
fact→judgment axis**:

- **Strongly groundable (citations).** "This quote supports this claim" decomposes into a
  *factual* sub-claim ("quote Q appears in source S") that the machine verifies exactly, which
  anchors the verdict to retrieved text. The deterministic gate does most of the safety work.
- **Weakly groundable (codified judgment rules, e.g. MOS:PEACOCK puffery).** There is no
  external source and nothing to string-match: the machine can confirm the flagged span exists
  and the cited rule id is real, but "is this span actually puffery?" is an irreducibly
  *semantic* judgment with no deterministic oracle. The gate confirms locus and rule-reference
  integrity, and stops.

For the judgment end, grounding is **necessary but not sufficient**. The community-credible
safety there comes from a **second, distinct layer**: **measured ensemble agreement** (N-of-M
independent models concurring — a *measured* signal, the SIFT 2-of-3 → 96.6% precision
precedent) plus **human confirmation**, with disagreement surfaced rather than hidden
(`adr/0007 §4`). This is exactly why no model-generated confidence number is ever shown (§2.3,
`03`/`07`): the one thing the machine cannot self-certify is its own judgment, so we refuse to
fake a number for it and instead report the **graded verdict + measured agreement +
abstention**, and route the residual to a human.

**Consequence for this document.** The escalation ladder below is therefore not one ladder but
two interleaved evidence regimes. The lower rungs (citation-class edits) are unlocked largely by
the *construction-proof* metric (0 audited hallucinated citations). The higher, **text-adjacent**
rungs (rung 4) cross into the weakly-groundable regime, so they require a *different* evidence
basis — ensemble-agreement precision, adversarial judgment evals, and acceptance-*correctness*
(not acceptance) — because no string-location gate can carry them. Honest evidence means naming
which regime a claim lives in.

## The evidence-gated escalation ladder

Scope expands one rung at a time; each rung is unlocked by *pre-declared* evidence
thresholds met on the rung below, and (for on-wiki behavior changes) by a
community ask backed by that evidence. The conservative defaults in
`02-constraints-policy.md` are the **starting rung**, not the ceiling.

| Rung | Behavior | Evidence required to *propose* the next rung |
|---|---|---|
| 0 | Dry-run / suggestions only, on test wikis | Internal Tier-1–5 gates green |
| 1 | Assisted edits, human confirms each, production, low volume | Lab precision ≥ baseline; **0** audited hallucinated citations; positive editor acceptance on a pilot |
| 2 | Higher assisted volume / more edit types | Rung-1 on-wiki data: reversion ≤ human baseline; acceptance ≥ threshold; reviewer effort acceptable — filed as/with a **BRFA** |
| 3 | Reduced per-edit confirmation for the *narrowest, best-proven* edit classes (e.g. adding an archive-url) | Sustained rung-2 data on that class + explicit community **RfC** consensus, with our data attached |
| 4 | Text-adjacent assistance (e.g. bounded copyedit), strictly flagged | Everything above + adversarial evals on text + a dedicated community proposal |

We do not pre-commit to reaching any particular rung. We commit to the *method*:
each step is earned by data and consensus, and is reversible.

## Engaging the community as the mechanism of change

The evidence exists to be *used*, transparently:

- **Publish, don't lobby.** Results go on-wiki (talk pages, WikiProject pages, the
  relevant policy talk pages), in a public repo, and — where warranted — as a
  write-up/paper. Invite the community to re-run our evals.
- **Use the real processes.** BRFA for automation; RfC for policy/scope changes;
  WikiProjects for domain buy-in. The data is the argument *inside* these
  processes; we do not route around them.
- **Audit trail as evidence.** The per-edit provenance + append-only audit ledger
  (`02 §4`) is itself a transparency artifact: every claim we make about quality is
  backed by an inspectable record of what we did and why.
- **Hold ourselves to a higher standard than humans.** The ask is not "let AI be as
  good as the average editor"; it is "here is data showing this is *better and
  safer* than the status quo for this specific task." That asymmetry is what makes
  the ask persuasive.

## How this changes the build (cross-references)

- `05-evaluation-and-benchmarking.md` is split conceptually into **(i) internal CI
  gates** (block bad releases) and **(ii) the external evidence harness** (produces
  community-facing, reproducible, comparative, pre-registered reports). Same
  machinery, two audiences.
- The **on-wiki outcome metrics** (acceptance, reversion/durability, reviewer
  effort) require instrumenting the assisted-edit flow to *follow up* on committed
  edits — a measurement loop, not just a one-shot. Add to the `agent`/`server`
  design and the build plan (`10`).
- `09-roadmap.md` graduation criteria and the text phase are reframed as
  **evidence-gated boundary-pushing**, not permanent caution.
- Accepted in `docs/adr/0003-evidence-driven-policy-posture.md` (owner-ratified 2026-06-02).
