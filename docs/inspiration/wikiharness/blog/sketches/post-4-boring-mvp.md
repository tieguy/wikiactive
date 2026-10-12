# Post 4 Sketch: The Boring MVP, and Why It Might Be Like Building a Better Britannica

*Sketch for drafting. Parallels the post 5 sketch in form. Pre-Wikimania priority. Centers the skills-only-vs-harness contrast, with SP42 as a concrete example (not the spine).*

## The task

The boring MVP: verify and improve a Wikipedia article's claims and sourcing. Cite-checking is the concrete, tractable entry point — "does this source actually support this claim?" — and it's the near-term MVP exemplar (SP42). It's also the natural bridge between patrol-side and authoring-side work: a citation that doesn't support its claim is both a patrol problem (flag the bad edit) and an authoring problem (fix my own sourcing).

## Beat 1: The skills-only version

What the minimum looks like: a SKILL.md (e.g. a `wikidata-enhance-and-check`-style skill) that walks the model through SIFT verification, using generic tools (WebSearch, WebFetch, pywikibot) and progressive disclosure.

- It's a real thing that does real work. It proved SIFT-style verification is tractable.
- It's the *right* tool for one job: prototyping the inner loop and proving a method fast.
- Be explicit and generous here — skills are necessary, the entry point, the way you discover what the harness must enforce. Not a straw man.

## Beat 2: Why it falls short as fielded infrastructure

The sharp version: a skill can only *instruct the model to check its own work*, so it's structurally confined to **self-critique — the weakest validation strategy**. It has nowhere to put the rest of the toolbox.

Map each gap to the post-1 components:
- No **tests** — deterministic checks the model can't talk its way around (DOI resolves, ISBN checksums, the quoted sentence is actually present in the fetched source). The model can hallucinate that a source supports a claim; nothing catches it.
- No **independent/adversarial evals** — a separate scoring pass, possibly a different model.
- No durable **memory/state** — no queue of claims, no record of what's been checked, no contracts; re-running is ad hoc.
- No real **guardrails/substrate** — a misconfigured skill calling pywikibot can hit production; no staged promotion path.
- No **observability** a community could audit beyond a chat transcript.
- No **self-evals** — can't tell whether editing the prompt made it better or worse.
- No **governance** — one person's prompt file, not a multi-maintainer artifact.

The throughline to post 2: the skill confines you to the bottom of the validation toolbox exactly when commons work needs the top of it.

## Beat 3: What a harness adds (SP42 as concrete example)

Use SP42 as the worked example of filling those gaps — not as the post's spine, just the illustration.

- scoring / diffing / cite-checking as **code** → tests *and* independent evals, not vibes
- patrol queueing, contracts → durable **state**
- localhost auth bridge, token isolation, staged path to live Wikimedia → **guardrails / substrate**
- ADRs, STATUS → **observability**
- test fixtures → **self-evals**
- shared Rust core with browser/CLI/desktop shells → **UI is just one component** (domain logic separated from interface)
- CONSTITUTION.md → **governance**

The lived argument: the skill is where you *discover* what the harness must enforce. Luis's own trajectory — skill first (`wikidata-enhance-and-check`), now porting cite-checking into a governed, multi-maintainer harness (SP42) — is the maturation path the post describes, told in the first person.

## Beat 4: Score against the post-3 criteria

Walk the eight criteria; mark what even a good harness satisfies well / partially / poorly. Honest scoring sets up the limit and post 5. (Likely strong on verifiability-native, legible, composable; partial on dual-readable, edit-level governance; weak/uncertain on native uncertainty representation, open substrate depending on model choice.)

## Beat 5: The honest limit

- Improving/patrolling Wikipedia articles optimizes for an audience AI overviews are disintermediating. Valuable as proof of method, but a better Britannica (or a better small-town newspaper) isn't the whole answer.
- Reframe carefully (per post 5): the deeper limit isn't the vanishing reader — it's that the boring MVP implicitly serves the contributor personas (Reactive Correctors, Knowledge Sharers) already best served by current tooling. That's *why it feels boring*.

## The Hill's-model tension (thread, or its own section)

SP42 makes patrolling better — but Hill's lifecycle model says quality-control tooling is exactly what drives away good-faith newcomers as collateral damage. So: is a better patrol/cite-checking harness part of the problem or the cure?

- Optimistic reading: a well-designed one could be *more* humane — better at distinguishing the good-faith-but-clumsy newcomer from the vandal, the discrimination current tooling fails at.
- Genuinely open question; SP42 is a concrete place to ground it.
- Connects forward to post 5 (who we design for) and back to the governed-factory idea.

## What this post is NOT

- Not "skills are bad." Skills are the right prototyping tool and the discovery mechanism.
- Not an SP42 advertisement. SP42 is one concrete example of the harness pattern; the argument is about the pattern.
- Not a claim that the boring MVP is worthless — it's a proof of method and a credible starting point. The limit is about ambition, not validity.

## Dependencies / sequencing

- Harvests post 2: the "confined to self-critique" critique relies on the tests-vs-evals and validation-toolbox material established there.
- Hands off to post 5: the "personas already best served" reframe is the bridge.
- Static/dynamic context + progressive disclosure: introduce the vocabulary here if not already spent, since skills are the protagonist (a skill *is* dynamically-loaded procedural knowledge).

## References / inputs to develop

- SP42 repo (README, CONSTITUTION.md, STATUS.md, ADRs) — confirm current framing; docs may lag the code (cite-checking is moving it from patrol-side toward authoring-side)
- Luis's `wikidata-enhance-and-check` skill as the skills-only exemplar
- SIDE (Petroni et al. 2023) as the closest cite-checking prior art (see eval-datasets analysis doc)
- Hill / Halfaker / TeBlunthuis peer-production lifecycle research for the newcomer-rejection tension
- Post 1 (components), post 2 (validation toolbox), post 3 (criteria), post 5 (personas) — internal links
