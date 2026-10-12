# 14 — Design Criteria for a Commons Harness

*Corpus reference doc — the reference material behind **blog post 3** ("Design criteria for a commons harness"); also the measuring stick used in posts 4 and 6. A working set of standards against which any commons-oriented knowledge harness — and the knowledge artifacts it produces — can be evaluated. Developed without regard to near-term financial or institutional constraints, to give experiments a shared target.*

## The criteria

1. **Verifiability-native.** The atomic unit of knowledge carries its provenance: claim, source, and context together. Not text with footnotes bolted on.

2. **Machine-and-human dual-readable.** Both an LLM and a person can consume and contribute productively. Neither human-first with machine-readable afterthoughts (Wikipedia) nor machine-first with weak narrative (Wikidata) is sufficient.

3. **Governable at the edit level.** Dispute, versioning, and deprecation attach to individual claims, not just to pages. Talk-page-equivalent discussion is possible at the claim level.

4. **Represents uncertainty and contestation natively.** Claims can carry confidence, competing positions, and evidence weights. Forced consensus is not required; contestation can be first-class data. (Prior art worth drawing on includes Google's Knowledge Vault work on probabilistic fact confidence, and scholarship on knowledge construction and disputation more generally.)

5. **Open at the substrate layer.** CC0 or CC-BY licensing, open-weights-friendly, no single-vendor dependency required for the system to function. Open is incrementally achievable — components can be swapped as open alternatives mature.

6. **Incrementally improvable.** Small contributions — one verified claim, one citation traced, one disambiguation — are individually valuable. Not just valuable in aggregate, and not requiring monolithic regeneration.

7. **Legible.** Contributors can understand why a claim is there, what it rests on, and what would change it. Any reader who wants to dig deeper can follow the chain — to sources, to prior versions, to the reasoning behind a particular formulation. Legibility is about affordance for action, not just readability.

8. **Composable with scholarly and civic infrastructure.** Citations resolve to real, canonical identifiers — DOI, OCLC, Wikidata Q-IDs, court docket numbers, persistent archival IDs. Natural partners include academic publishing, Free Law Project and legal open-data communities, and public-service news organizations.

## Design considerations (not criteria)

These matter but are choices to be made during design, not standards against which to evaluate:

- **Federation and network architecture.** A single hub, a federated network, or something stranger (the "DOI × atproto" provocation). Probably a network in the long run; probably a single instance for MVP.
- **Temporal integrity.** Claims have timestamps; knowledge evolves; superseded is distinct from deleted. Wikidata's rank system and point-in-time qualifiers cover significant ground. Abstract Wikipedia is too early to evaluate.
- **Extraction and reciprocity mechanisms.** How the commons interacts with commercial extractors. Deliberately deferred from criteria because the shape is heavily financial-constraint-driven; worth imagining the ideal system free of those constraints first.
- **Resilience to disintermediation.** Related to extraction. The question of what sustains a commons when pageview funnels collapse is real but follows once we know what the commons *is*.

## How to use this list

- **In blog writing**, as the measuring stick for any concrete MVP — boring or bold (posts 3, 4, and 6 in the series).
- **In project planning**, as a checklist for definition-of-done and for scoping tradeoffs honestly.
- **In funder conversations**, as evidence that the project is not vibes-based: there are stated standards, scored candidates, and a path for revising both as the work progresses.

## Provenance of this doc

Developed in the harness-series planning work as the canonical statement of the eight criteria. The blog series' post 3 ("Design criteria for a commons harness") is the prose treatment of this material; this doc is the reference version it draws from.
