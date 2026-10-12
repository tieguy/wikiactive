# ADR-0010 — Porting the citation-verification eval corpus (no tranches; facet taxonomy; gate vs benchmark)

Status: **Accepted** — owner-directed 2026-06-02 (in-session design walkthrough).
Date: 2026-06-02

## Context

`citation-checker-script` (the `alex-cite-checker` workspace) is wikiharness's MVP-1 reuse
source (`00`, `12`): its prompt, verdict taxonomy, eval dataset, voting, and a body-usability
classifier are battle-tested over a 189-row labeled corpus and a multi-provider benchmark. We
are porting that output-quality apparatus into the `evals` package (`05`) — it is the long-pending
labeled-eval gate, and the part of the program that fights the project's *actual* failure modes
(garbage-in sources, LLM instability), which a pure core does nothing about.

The source eval carries three design flaws we will **not** carry across — each an instance of the
same anti-pattern, *accidental position masquerading as meaning*:

1. **`v1`/`v2`/`v3` "tranches"** — a single token overloaded with three unrelated jobs (row origin,
   extraction mode, reproducibility-freeze identity). As a metric slice it is meaningless.
2. **`row_<csv_line>` ids** — physical CSV line number as identity; inserting a row mid-file shifts
   every downstream id (the +33 remap PR #155 had to do).
3. **a model `confidence` field + calibration metrics** — banned here by ratified invariant
   (`02`/`03`/`07`/`11`, CLAUDE.md): no model-generated confidence/support numbers anywhere.

Grounding (codebase investigators, 2026-06-02) established that wikiharness **already** has an M4
hermetic eval gate (`packages/evals`) that replays *recorded model responses* through the real
`verify_citation` pipeline. That is a deterministic pipeline/regression gate, **not** a model-quality
benchmark. The two must not be conflated.

## Decisions

1. **Two distinct deliverables.**
   - **Hermetic eval GATE (CI):** the existing small curated fixture + suite, which replays recorded
     model responses; keeps testing pipeline correctness + the anti-hallucination and
     identity-invariance hard gates deterministically. Stays in `npm run ci`.
   - **Model-quality BENCHMARK (opt-in, NOT in CI):** the ported ~180-case labeled corpus run against
     a real open-model panel; measures accuracy + measured ensemble (dis)agreement against GT. Its
     composition root (real `FetchHttpClient` + real `ModelClient`s + API keys) lives in `scripts/`
     so the `evals` package keeps its import boundary (`evals: [core, tools, policy]`; no runtime
     `edges`). The benchmark runner logic lives in `evals` and takes **injected** core-interface
     clients.

2. **Purge the three accidental-identity anti-patterns.** Tranches deleted. Case ids are
   **content-hash-derived** (stable under insertion). `confidence` dropped entirely — the only honest
   signals are the graded categorical **verdict**, **measured ensemble (dis)agreement**, and
   **abstention** (`SOURCE_UNAVAILABLE`).

3. **Facet taxonomy — classify each facet by how its value is established:**
   - **inherent** (given at creation; cannot be wrong) → stored on the case: `inputProvenance`
     (`wiki-mined` | `curated-import`, the one real signal rescued from the v3 tranche).
   - **computed** (pure function of the case/run; reproducible) → **not stored** as truth; derived at
     analysis time: `sourceType` (from URL), body-usability (from bytes), claim features.
   - **assessed** (fallible judgment; the only facets that can be *wrong*) → stored **with
     provenance**: the GT verdict + `labelMethod` + `labelAsOf`.
   - **heuristic-assessed** (model-labeled, e.g. compoundness) → **deferred entirely** (that pipeline
     is being rebuilt; re-derive later under measured agreement, not imported on faith).
   - Gating may rest only on inherent / computed / **audited**-assessed facets — never on a
     model-labeled facet (that would smuggle an unverified model judgment into a gate).

4. **GT label provenance + a free audit flag.** GT carries `labelMethod` + `labelAsOf` (assessed
   provenance, stored). Panel-vs-GT disagreement is **computed-of-run** (not stored): the benchmark
   emits "rows where most/all of the panel disagrees with GT" as an audit-targeting flag. It may
   **flag** rows for human re-audit; it may **never set** GT (circularity guard — models cannot
   define the truth they are graded against).

5. **Two source modes.** **cassette** (frozen source bytes — isolates citation-completeness / model
   quality; the primary, deterministic mode) and **live** (real fetch spine — adds scraper-completeness).
   The ported (pure) **body-usability classifier** separates *pipeline-attributed* outcomes (unusable
   body → honest abstention) from *model-attributed* outcomes, so a fetch failure is never scored as a
   model error.

6. **Reuse the gold prompt.** Enhance core `buildVerifyPrompt` with the source's two-step
   provenance/SU framing, worked examples, and transliteration + hedge rules — keeping wikiharness's
   `{verdict, quote}` JSON output and the no-confidence ban.

## Consequences

- The ported dataset is **re-audited**: the known GT corrections found in the source's both-wrong
  audit (e.g. the reversed row, the "claim not in source" rows) are carried in; fresh content-hash
  ids mean there is no reproducibility tie to the source's row ids to protect.
- New **pure core** modules: `body-classifier`, `voting` (measured agreement), `verdict-normalize`.
- The benchmark is **not** in CI (needs keys + network); a stage-end **ADR-0007 bug-hunt** runs before
  it is considered done.
- Compoundness / difficulty facets are intentionally absent until their labeling pipeline is rebuilt.
