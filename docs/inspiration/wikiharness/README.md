# wikiharness

A design corpus and writing workspace for **LLM-assisted, human-confirmed quality improvements to Wikipedia / Wikidata / Commons**.

The central discipline: *suggest source-verified improvements, never edit autonomously, never hallucinate.* Every proposed citation must be grounded in a source the harness actually fetched — no generation from memory.

## What's here

### `blog/` — public writing

A six-post series on harnesses for the open knowledge commons. Drafted so far:

- [Post 1 — What Is a Harness?](blog/2026-06-what-is-a-harness.md) — LLMs are engines; harnesses are everything else. What they are and why they matter for open knowledge.
- [Post 2 — Why Trusting AI in the Commons Is Harder](blog/2026-06-trusting-ai-in-the-commons.md) — the trust/validation deep dive: borrow software's verification discipline, then confront where the commons runs out of ground truth. *(draft)*

Planning and drafting material:

- [`series-outline.md`](blog/series-outline.md) — the full six-post arc.
- [`sketches/`](blog/sketches/) — drafting sketches for later posts ([Post 4, boring MVP](blog/sketches/post-4-boring-mvp.md); [Post 5, who builds the commons](blog/sketches/post-5-who-builds-the-commons.md)).

### `docs/` — design corpus

Fifteen numbered design documents plus ADRs covering the full wikiharness design: policy constraints, architecture, tool catalog, evaluation strategy, roadmap, and more. Also home to analysis and collaboration notes as they accumulate.

| | |
|---|---|
| `docs/00`–`02`, `11` | What the harness must do (settled) |
| `docs/04`–`08` | How it works: testing, eval, architecture, tools, MVP-1 |
| `docs/09`–`10` | Roadmap and build plan |
| `docs/12`–`13` | Prior art and reference designs |
| `docs/14` | Design criteria for a commons harness (reference behind blog post 3) |
| `docs/adr/` | Architectural decision records (0001–0011) |
| `docs/analysis/` | Spike write-ups, prior-art surveys, and explorations (incl. the validation/eval-datasets survey backing blog Post 2) |

### TypeScript implementation — archived in git history

The MVP-1 citation-verification harness was fully built in TypeScript (M0–M5 + enhancer composition + wikitext-AST edit layer). It has been removed from the working tree so this stays a docs/writing repo, but it is preserved at the `code-archive-final` tag and recoverable with:

```
git checkout code-archive-final -- code-archive/
```

Active Rust work is in [SP42](https://github.com/schiste/SP42).

## The broader context

wikiharness started as a TypeScript proof-of-concept to answer: *can you build a harness rigorous enough to meet Wikipedia's community standards?* The short answer is yes for citation verification, with the right grounding gates, eval harness, and human-confirmation model.

That work is now feeding into SP42 governance and a series of posts working through what harnesses for the commons need to look like.
