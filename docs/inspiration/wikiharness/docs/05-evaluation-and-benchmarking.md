# 05 — Evaluation & Benchmarking (output quality)

> **Status: extracted from `04` Tier 5.** Output quality was buried as the fifth tier
> of a code-testing doc; it is the project's hardest problem, so it gets its own home.
> Like `04` (code correctness) it is an **input to architecture** — it shapes the
> `ModelClient`/ensemble edges and the promotion/governance machinery — so it sits
> with `04`, **before** architecture (`06`).
>
> **Staging — HOW (tentative).** Concrete datasets, metrics, and thresholds are a best
> guess, revisable until M0 (base framework now settled — `adr/0002`, accepted 2026-06-02). The **requirement**
> — that output quality is measured and gated against regression — is firm.

## The hard problem this doc owns

`04` answers *"does the code do what we think it does."* This doc answers the harder,
fuzzier question:

> **Are the LLM-judgment outputs actually good — and when we tweak a prompt, swap a
> model, or change a policy, did quality silently regress?**

That regression question is the real day-to-day risk. Prompts are edited constantly;
without a labeled benchmark and a gate, a "small wording improvement" can quietly tank
precision on a cohort and no unit test would ever notice. Because the model layer is
non-deterministic (`04` "Determinism boundary"), we **never assert exact strings on real
models** — we measure behavior against labeled data and gate on the metrics. This adapts
SP42's scoring-evaluation harness + SIFT's benchmark/metrics.

## Two audiences, one harness

The same machinery serves two purposes:

1. **Internal CI gates** that block bad releases (a prompt/model/policy change that
   regresses cannot ship — see "No promotion without passing").
2. **The external evidence harness** — reproducible, pre-registered, comparative,
   community-facing quality reports that justify policy/scope expansion. This is a
   first-class *strategic* deliverable; it lives in `11-evidence-and-policy-strategy.md`
   and `docs/adr/0003`. It additionally measures **on-wiki outcomes** (acceptance,
   reversion/durability vs. human baselines, reviewer effort) that pre-commit evals
   cannot.

The **named, external benchmarks and labeled datasets** to adopt (ALCE, Citation Needed,
EUFCC-340K, WikiWeasel/CoNLL-2010, and baselines such as ProVe / SIDE / Add-a-Fact) are
catalogued in `12-prior-art-and-reuse.md §evidence harness`; this doc defines how they're
used as gates.

## Labeled datasets (ground truth)

- Claim/citation pairs with **verdict ground truth**. SIFT and citation-checker both ship
  starter sets (citation-checker ~189 labeled entries; SIFT evaluated 500 edits); these
  are thin on their own, so the named external benchmarks (`12`) supply scale and cohort
  coverage.
- Held-out cohorts for robustness (paywalled, non-English, PDF-page, short-cite) so a gate
  can catch *cohort-specific* regressions, not just aggregate drift.
- For codified-rule enhancers (MVP-3+), distant-supervision labels from Wikipedia's own
  inline cleanup templates (`{{Peacock term}}`, `{{Weasel inline}}`, …) — see `09`/`12`.

## Metrics

- Precision/recall, PR-AUC / ROC-AUC, confusion matrices **per verdict**, measured **per
  model and per ensemble** (reuse SIFT's scikit-learn metrics + the citation-checker
  benchmark harness).
- The model panel is **open-first**: a cheap open model is the pinned default and the CI
  gate target, and the harness additionally runs a published **open-vs-frontier
  comparison** (`02 §8`, `docs/adr/0006`, `11`).

## Eval gates (declared per policy as `required_gates`)

A change ships only if every declared gate passes:

- **regression** — accuracy on the labeled set ≥ threshold. *This is the direct guard for
  "I tweaked a prompt; did outputs get worse?"* Port citation-checker's **`ccs compare`**
  (`comparing-benchmark-runs.md`; `12 §C`): a two-run control-vs-treatment diff that
  classifies every changed cell as **improvement** (wrong→correct) / **regression**
  (correct→wrong) / **lateral** (wrong→different-wrong), compares only the **intersection**
  of cells present in both runs (dropped/errored cells can't flatter the delta), and flags
  deltas below a noise floor. The flip taxonomy is the gate's natural diff output.
- **anti-hallucination** — 0 proposed citations whose supporting quote is **not
  string-locatable** in the fetched source (hard gate — must be exactly zero). The
  underlying check is a pure, Tier-1-unit-tested function in `04`; here it is enforced as a
  hard gate **across the eval set**.
- **no-unsourced-claim** — 0 proposed edits that introduce an uncited assertion.
- **fairness/robustness** — no regression on the hard cohorts above.
- **identity-invariance** (`02 §7`, `06`) — verdicts must not change when editor-identity
  metadata (anon / account age / edit count / group / IP) is injected; hard gate (assess
  the edit, not the editor).
- **multi-model agreement** (`adr/0007`) — for higher-risk classes (content edits, MVP-3),
  require N-of-M model agreement before surfacing; the gate measures precision *at the
  agreement threshold* against the labeled set.
- **cost/latency** budgets.
- **model-bias / political-neutrality** *(future, post-MVP — does not block the MVP)* — on
  curated adversarial sets of politically contested or censorship-sensitive topics (`02 §1`,
  consequence 5 — e.g. Tiananmen / Tank Man for Chinese-origin models; January 6 and
  climate change for US-origin models), a backend must not silently omit, soften, or skew
  *well-sourced* content. Measured **per model** (bias profiles differ by backend origin,
  `02 §8`); reported via the open-vs-frontier panel (`11 §4b`). Building the sets is future
  work.

## No promotion without passing

A capability advances `suggested → candidate → active` only by passing its declared gates.
**A prompt or policy change that regresses a gate cannot go live.** This is enforced in CI
by a governance script (cf. SP42's `check-scoring-governance.sh`) and is part of the
definition of done in `04` "CI gates."
