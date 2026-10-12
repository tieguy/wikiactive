# ADR-0006 — Open-model posture, and the open-vs-frontier benchmark as a carrot

Status: **Accepted** — owner-ratified 2026-06-02 (walkthrough). Already binding via
`0002`'s model-layer constraint and `02 §8`; ratification confirms it, no daylight to revisit.
Date: 2026-06-01 (ratified 2026-06-02)

## Context

Resolving open question #3 from `01-vision-scope.md`. Luis's lightning-talk value:
*"open weights, open code, runnable on community infrastructure; no dependency on a
frontier vendor for credibility tooling,"* and the SIFT result that the "no fetch,
no verdict" discipline *"works with any cheap open model"* because the constraint
is in the prompt, not the model.

Luis chose the "open-capable + open-default + open-tested; frontier optional"
posture, and added a strategic dimension:

> *"this should be a 'carrot' — eventually I'd like to say 'IBM Granite / Mistral /
> AI2 OLMo is almost as good as Sonnet, IBM/Mistral/AI2 please commit engineers to
> make it better' — this would be unreasonable for most organizations but is doable
> for Wikipedia."*

## Decisions

1. **Open-capable (hard).** WikiHarness MUST always work on cheap open-weight
   models. The model layer is strictly **provider-neutral** behind the
   `ModelClient` interface (ADR-0002), with open-model backends
   (OpenRouter/Together/Ollama/vLLM/etc.) as **first-class**, not afterthoughts.

2. **Open-default + open-tested (hard).** Open models are the **default**, and CI
   eval gates run against a **pinned cheap open model**, so "runnable on community
   infrastructure" is *demonstrated, not claimed*. The published evidence
   (`11`) is generated on open models.

3. **Frontier optional (never a dependency).** Frontier models are a permitted,
   pluggable option for users who want them; nothing in the system *requires* a
   frontier vendor. No credibility claim depends on a closed model.

4. **The open-vs-frontier benchmark is a first-class, published artifact (the
   carrot).** The evidence harness (`11`) routinely runs the *same* WikiHarness
   tasks across a panel of open **and** frontier models and **publishes the gap**.
   The purpose is twofold:
   - **Demonstration** — show that cheap open models are good enough for credibility
     work on community infrastructure.
   - **Incentive / leverage** — make the explicit, public ask to open-model orgs
     (IBM/Mistral/AI2/…): *here is exactly how close you are to frontier on a task
     the Wikipedia community cares about; commit engineering to close it.* This ask
     is uniquely credible coming from the open-knowledge ecosystem.

## Consequences

- **ADR-0002 (model layer):** reinforced — `ModelClient` must support open backends
  first-class; a cheap open model is pinned for CI/evals; no API in `core`/`tools`
  assumes a specific vendor's features. (The "no fetch, no verdict" constraint stays
  prompt-level, so it ports across models — the SIFT finding.)
- **Evidence harness (`11`):** the **open-vs-frontier model panel** becomes a
  standing comparison dimension (alongside human/tool/prior-version baselines), with
  a public leaderboard-style report framed as the carrot. Per-model cost is reported
  (the SIFT "under a cent per verdict" framing).
- **Eval taxonomy:** the `hard-patrol-problems.md` cases (qualifier semantics,
  ambiguous dates, accents, Q-id mapping) are exactly where open-vs-frontier gaps
  are most diagnostic — they become the benchmark's sharp edges.

## Non-goals

- Not "open vs. frontier" as a culture-war stance — it's an engineering scoreboard
  meant to *recruit* open-model investment, reported honestly (including where
  frontier wins).
- Not banning frontier models (that would be the rejected "open-weight ONLY"
  option); they remain a pluggable option.
