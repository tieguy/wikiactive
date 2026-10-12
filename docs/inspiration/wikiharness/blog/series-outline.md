# Blog Series Outline: Harnesses for the Knowledge Commons

## Post 1: What is a harness?

- Define the term and why it matters: the LLM is the engine, the harness is what makes it useful.
- Component sketch: inner loop (plan/act/validate), outer scaffolding (memory, guardrails, observability, evals), and the outside world (substrate / promotion paths).
- Cross-cutting themes: provenance, heterogeneity, open incrementalism.
- Plant tests-vs-evals lightly and nod forward to Post 2; set up the series.

## Post 2: Why trusting AI in the commons is harder — and what software already knows

- Trust is *the* question for a skeptical commons audience; this is the verification deep dive (likely the most-cited piece for that audience).
- Two halves held together: it's harder (commons claims are eval-shaped, ground truth is rare) *and* software is decades ahead at the verification discipline we now need.
- Three beats: borrow the solved part (tests, CI, evals, harness-as-asset; why prompting forfeits all of it — harnesses >> prompting); confront the unsolved part (no ground truth for contested/normative/historiographic knowledge); why that matters for trust (orchestrated validation; "trust the system that checks the model," not the model).
- Bridges to the governed-factory idea (Post 5) and provenance. Pre- or post-Wikimania placement TBD after Post 4 drafts and the talk locks.

## Post 3: Design criteria for a commons harness

- Motivation: before choosing an MVP, establish what a good commons harness should satisfy.
- The eight criteria (verifiability-native; dual-readable; edit-level governance; native uncertainty and contestation; open substrate; incrementally improvable; legible; composable with scholarly and civic infrastructure).
- Design considerations deferred from the criteria list: federation, temporal integrity, extraction/reciprocity, resilience to disintermediation.

## Post 4: The boring MVP, and why it might be like building a better Britannica

- The task: verify and improve a Wikipedia article's claims and sourcing (cite-checking as the concrete, tractable case).
- **The skills-only version** (e.g. a `wikidata-enhance-and-check`-style SKILL.md): instructs the model through SIFT verification with generic tools. A real proof of method — and the right tool for prototyping the inner loop.
- **Why it falls short** as fielded infrastructure: a skill can only instruct the model to check its own work, so it's structurally confined to self-critique — the weakest validation strategy. No place for deterministic tests, independent/adversarial evals, durable state, real guardrails, observability, self-evals, or governance. Map each gap to the post-1 components.
- **What a harness adds**, using SP42 as the concrete example (not the spine): scoring/diffing/cite-checking as code, queues and contracts, localhost auth bridge and token isolation, ADRs/STATUS, test fixtures, multiple shells, and a CONSTITUTION (governance). The skill is where you discover what the harness must enforce; Luis's own skill-first → harness-contributor trajectory is the argument, lived.
- Score against the Post 3 criteria: what even a good harness satisfies well, partially, or poorly.
- The honest limit: improving/patrolling Wikipedia articles optimizes for an audience AI overviews are disintermediating — valuable as proof of method, but a better Britannica isn't the whole answer. Plus the Hill's-model tension: better cite-checking/patrol tooling could be more newcomer-humane (better at telling good-faith-but-clumsy from vandalism) or could deepen newcomer rejection — an open question SP42 makes concrete.

## Post 5: Who might be interested in building the commons?

- Disambiguate: the commons is *for* everyone (the prime objective stands); the open question is *who shows up to build it*, and what they need from a harness.
- The New Editor Experiences personas (Reactive Corrector, Knowledge Sharer, Audience Builder, Box Checker, Social Changer, Joiner Inner) and Stinson's argument that "anyone can edit" is not a strategy for the four personas that need more than tools.
- Each persona implies a different harness; the boring MVP implicitly serves the two personas already best served by current tooling.
- Plausible contributor pivots: learned contributors (Box Checkers + expert Knowledge Sharers), civic / journalistic Social Changers, WikiProject-style Joiner Inners. Agents-and-downstream-systems flagged as something the persona framework doesn't capture.
- Sets up Post 6: candidate hubs score differently depending on which contributor populations the harness commits to inviting and amplifying.

## Post 6: Criteria-meet-candidates — toward a bolder MVP

- Candidate hubs to evaluate: Wikidata (thickened), Abstract Wikipedia, Semantic Scholar / Asta lineage, verified-claims middleware, legal/normative knowledge hubs, something yet-unbuilt.
- Score each against the Post 3 criteria, *given the contributor commitments from Post 5*.
- What experiments would help choose — and why a harness is what lets us run them.
- Call to action: funders, editors, researchers who want to help decide.
