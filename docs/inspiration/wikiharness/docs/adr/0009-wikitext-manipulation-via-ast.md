# ADR-0009 — Wikitext manipulation via a parser AST (not regex), in the I/O layer

Status: **Accepted + Built** — accepted (owner-prompted) 2026-06-02; **built 2026-06-03 (#9)**.
The audit's "evaluate-spike" on `wikiparser-node` is resolved: a wikitext-AST layer is the
foundation for wikitext manipulation. The first real consumer arrived — making `repair_citation`
detection and `commit_confirmed_edit` application node-anchored — so the deferred build (YAGNI)
was done as #9, ahead of the M6 prose enhancer.
Date: 2026-06-02

## Context

The re-audit flagged that we parse + edit wikitext with regex — the same class as the
infamous regex-HTML anti-pattern that bit `htmlToText`. The first spike conclusion ("keep the
regex") was **scoped too narrowly**: it judged only the *current* bare-URL `<ref>` detection.
The owner corrected this — wikiharness's roadmap is full of wikitext manipulation **beyond
`<ref>`** (M2 enrich/repair/archive, M5 `{{cite book}}`, **M6 MOS:PEACOCK prose edits**,
Wikidata/GLAM). So the right lens is the program, not one function.

Two honest findings:
- For the **narrow bare-URL `<ref>` *detection***: regex offers no correctness loss — it's a
  bounded, ReDoS-safe, regular micro-pattern that already correctly excludes `{{templates}}`
  and `[url label]` links. Regex is the right tool *there*.
- For **the broader program**: a real wikitext parser (AST) is the right foundation, for two
  recurring correctness needs regex cannot serve well:
  1. **Edit *application*** — replacing the *exact* node, not the first string match (the
     duplicate-`<ref>` ambiguity we currently fail-closed on; it generalizes to every enhancer).
  2. **Prose editing (M6 puffery)** — finding/minimally-editing words in *running wikitext*
     while avoiding templates/refs/links is irregular-markup territory: the regex-HTML trap.

## Decision

1. **Wikitext manipulation will be built on a parser AST** (`wikiparser-node`, actively
   maintained, TS) — robust detection + **node-anchored edit-application**, not regex/string
   `.replace`.
2. **It lives in the I/O-permitted tool/edge layer, NOT pure `core`.** `wikiparser-node`
   lazily loads config (fs) and carries global parser state, which would violate core's
   "deterministic + I/O-free" rule. A thin wikitext-parsing edge/utility wraps it; pure core
   stays pure.
3. **Build is deferred to the first real consumer** (the M6 prose enhancer, or whenever we
   make edit-application node-precise) — per the YAGNI discipline this project keeps relearning
   (SeededRng, the unused edges). Building a wikitext-AST layer now, with only the
   regex-adequate `<ref>` path as a consumer, would be premature.
4. **Until then, the bounded `<ref>` regex stays** (it's correct + ReDoS-safe + fits pure
   core), and `commit_confirmed_edit` keeps its fail-closed ambiguity guard. New enhancers
   that touch arbitrary wikitext structures **build on the AST layer**, not new regexes — so
   we don't sleepwalk into the regex-markup trap one pattern at a time.

## Consequences

- No code change now beyond recording this; `wikiparser-node` is not yet a dependency.
- When M6 lands: introduce the wikitext-edge, migrate `repair_citation` detection +
  edit-application onto it, and make `commit_confirmed_edit`'s application node-anchored
  (turning today's fail-closed-on-ambiguity into precise replacement).
- License note: `wikiparser-node` is **GPL-3.0-or-later** (strong copyleft) — confirmed at
  adoption (v1.43.0, 2026-06-03). The owner is comfortable with copyleft (cf. `adr/0008` on
  mwn's LGPL).

## Built (2026-06-03, #9)

The wikitext-AST layer was implemented as a narrow, operation-oriented edge — exactly the two
operations bare-URL repair needs — proven by migrating `repair_citation` / `commit_confirmed_edit`
onto node-anchored editing (the migration IS the first consumer; no speculative layer).

- **Edge:** `WikitextParser` + `WikitextRef` (pure interface, `packages/core/src/edges.ts`),
  implemented by `WikiparserNodeParser` (`packages/edges/src/wikitext-parser.ts`) over
  `wikiparser-node` v1.43.0. The library's global `Parser.config` + fs-config load is isolated
  in the edge (set-config-then-parse synchronously — no async interleave); `core` stays pure.
  Injected as a **required** `ToolContext.wikitextParser` (the node-anchored path is the only
  path — no string-replace fallback to keep optional).
- **Detection:** bare-URL `<ref>` detection runs through `edge.findRefs()`; pure `core`
  (`classifyBareUrlInner`) classifies each ref's **inner body** with a bounded, ReDoS-safe
  regex. (The regex did not vanish — it moved from scanning raw wikitext to classifying inner
  bodies; ref-boundary parsing is the AST's job now.)
- **Locator:** `ProposedEdit.locator = { kind: 'ext-ref'; ordinal }` (the document-order index
  among all `<ref>` nodes), hash-bound via `hashProposedEdit` (tamper-evident).
- **Application:** `commit_confirmed_edit` dispatches on the locator — node-anchored
  `edge.replaceRefNode(...)` (re-verifying `String(node) === before`, an extra anti-drift gate)
  for located proposals, retiring the duplicate-`<ref>` ambiguity guard for them. The
  text-anchored fallback (presence check + ambiguity guard) is retained for locator-less
  proposals (e.g. `propose_book_citation`'s `{{cite book}}`).
- **Spike evidence (our Node/ESM runtime):** `querySelectorAll('ext#ref')` returns refs in
  document order; ordinals reproduce across a re-parse; `safeReplaceWith` on the ordinal-th node
  replaces exactly one of two identical refs; `String(root)` re-serializes losslessly except the
  edited node.
- **Engine:** root `package.json` `engines.node` tightened to `^20.19.0 || ^22.13.0 || >=24.11.0`
  to match `wikiparser-node`. No GitHub Actions workflow exists; CI is `npm run ci`.
- **Out of scope (deferred):** `wayback-archive` (needs a template-param op), `malformed-cite-repair`,
  Save-Page-Now. The `locator` union extends to other node kinds when those land.
