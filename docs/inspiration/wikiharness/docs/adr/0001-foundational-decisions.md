# ADR-0001 — Foundational decisions

Status: **Accepted** — owner-ratified 2026-06-02 (walkthrough). The ADR-0002 dependency
is resolved: base = option (a) (thin core + Vercel AI SDK + own durable HITL).
Date: 2026-06-01 (ratified 2026-06-02)

## Context

WikiHarness is a greenfield LLM agent harness to help editors *improve* Wikipedia
articles and the open-knowledge graph. The defining requirement is that **every
subcomponent be deeply testable**. The project draws on prior art: the user's own
`wikidata-SIFT` (Python), `citation-checker-script` (JS), and — for architecture
and governance, not LLM design — `SP42` (Rust, no LLM). The current English
Wikipedia LLM guideline (RfC closed 2026-03-20) near-bans LLM-generated article
*text*, which strongly shapes scope.

## Decisions

1. **Goal is improvement, not patrolling.** We optimize "what is most improvable /
   highest value," not "what is most likely harmful." We do not carry over
   patroller identity signals (anon/new/temp account).

2. **Metadata & non-text first.** Roadmap leads with citations, archiving,
   Wikidata, and structured GLAM media; article *prose* is last and hardest. This
   is both product sense and the path of least policy resistance.

3. **Language: TypeScript.** Chosen for the agent/harness ecosystem fit and a
   single end-to-end language for core + tools + server + both clients. The user
   confirmed porting existing JS/Python prior art to TS is acceptable, and that
   the *framework* decision must be made on architectural merits, **not** chosen
   to match a language (see ADR-0002).

4. **Edit delivery: assisted editing, human confirms every edit.** No autonomous
   article-space editing in any MVP. OAuth2; tokens held server-side only. Higher
   automation, if ever, requires a BRFA and a dedicated bot account.

5. **Architecture: own a thin, pure, dependency-injected core** (SP42-style),
   with `build_*Request`/`parse_*Response` splits, so tools are unit-testable with
   no network/model/server. (Realized in `docs/06-architecture.md`.)

6. **Human-in-the-loop is a durable, persisted approval state machine**, not an
   in-memory promise — must survive page reload/restart.

7. **API-first; one core, many clients.** CLI and web are thin clients of one HTTP
   API. This is a non-negotiable product requirement and a base-selection input.

8. **Policy-as-data with eval gates** (SP42 governance model): editorial
   thresholds, disclosure templates, rate limits, domain allow/block lists, and
   per-sink write enablement live in versioned YAML; promotion
   `suggested→candidate→active` requires passing declared gates.

9. **Anti-hallucination is a subsystem, not a prompt.** No unsourced claims may be
   introduced; every proposed citation's supporting quote must be string-located
   in a document actually fetched this session (`locate_quote_in_source`).

10. **Provenance + append-only audit ledger on every proposed edit**; honest edit
    summaries that disclose AI assistance.

11. **Safety defaults:** dry-run by default, test wikis in dev, global kill
    switch, scope guard.

## Consequences

- The codebase shape is dictated by testability (pure core + DI edges), not by an
  off-the-shelf agent framework's conventions.
- We reuse prior art as *ported components* (see reuse map in `07`), not as
  dependencies, except for the model-calling layer (ADR-0002).
- Several constraints become *code* (guards, schemas, gates), not documentation.

## Alternatives considered

- *Python harness reusing SIFT/pywikibot directly* — maximizes reuse of the user's
  own code, but weaker off-the-shelf agent/testing ergonomics for the
  API-centric CLI+web requirement, and a split language story across the JS
  citation-checker. Rejected in favor of one TS codebase; SIFT logic is ported.
- *Suggestions-only or sandbox/talk-page delivery* — lower policy footprint, but
  the user explicitly chose assisted confirm-each-edit. We keep suggestions-only
  as the safe default *mode* (dry-run) within the assisted model.
