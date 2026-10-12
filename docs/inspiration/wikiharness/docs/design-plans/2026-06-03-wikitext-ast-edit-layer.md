# Wikitext-AST Edit Layer Design (#9, adr/0009)

## Summary

This slice builds the structured wikitext editing foundation that the citation-repair tools have needed
since M1. Today, `repair_citation` detects bare-URL `<ref>` tags with a bounded regex and
`commit_confirmed_edit` applies edits with a `wikitext.replace` call — which forces a fail-closed ambiguity
guard whenever a page contains two textually-identical refs. The AST layer removes both limitations by
introducing a narrow `WikitextParser` edge (a `core` interface implemented in `edges` by a `wikiparser-node`
wrapper) that exposes exactly the two operations bare-URL repair requires: enumerate `<ref>` nodes in
document order and replace a single node by ordinal.

The key design move is the **ordinal locator** — an index into the document's ref list that is stable at a
fixed revision and is embedded in `ProposedEdit`, bound into the confirmation token, and re-verified at
apply time. This makes edit-application node-anchored rather than text-anchored: `commit_confirmed_edit`
re-parses the article, asserts that the ordinal-th node still matches the reviewed text, and replaces
precisely that node — eliminating both the ambiguity guard and the fragility of first-match string
replacement. The `wikiparser-node` library's global parser state and filesystem config-loading are
encapsulated entirely inside the edge; `core` stays deterministic and I/O-free. All existing invariants —
anti-hallucination diff grounding, confirmation-token binding, `baseRevision`/`baseTimestamp`, dry-run
default, and the test-wiki-only write gate — are preserved unchanged.

## Definition of Done

**Feature:** Wikitext-AST edit layer (#9) — the AST foundation, proven by migrating `repair_citation` /
`commit_confirmed_edit` onto node-anchored editing. No enhancer is built in this slice; the migration
*is* the first real consumer (it makes bare-URL edit-application node-precise), satisfying `adr/0009`'s
YAGNI rule without a speculative layer.

**Primary deliverables**
1. **A wikitext-parser edge** — a new pure `core` interface (`WikitextParser`) implemented in `edges` by
   a `wikiparser-node` wrapper (GPL-3.0-or-later, owner-accepted), injected via `ToolContext.wikitextParser`
   (mirroring the `HtmlExtractor` edge). The library's global config + fs-config-loading stays inside the
   edge; `core` stays deterministic/I-O-free.
2. **`repair_citation` migrated onto the AST** — bare-URL `<ref>` **detection** moves from the bounded
   regex to AST queries (find `<ref>` nodes whose body is a bare URL); the proposal is built node-anchored.
3. **`commit_confirmed_edit` made node-anchored** — application replaces the exact AST node instead of
   `wikitext.replace(before, after)` (first-match), **retiring the fail-closed-on-duplicate-`<ref>`
   ambiguity guard**. Identical duplicate refs are disambiguated and applied precisely.

**Success criteria**
- A page with **two identical bare-URL `<ref>`s** (the case that fails closed today) now yields correct,
  precise per-node proposals that apply without the ambiguity refusal.
- All existing single-bare-URL behavior is preserved (characterized by the current tests, updated only
  where the ambiguity-guard behavior intentionally changes from "refuse" → "precise replace").
- Invariants intact: anti-hallucination (diff) grounding, confirmation-token binding,
  `baseRevision`/`baseTimestamp`, dry-run default, write-allowlist / test-wiki-only gates.
- The wikitext-edge is unit-tested (parse→serialize idempotence, node location, node-anchored replace);
  `npm run ci` green; import boundaries hold.

**Explicitly out of scope** (documented as candidate future consumers, not built here)
- `wayback-archive` enhancer — the IABot-redundancy / preemptive-live-link-niche question is unresolved
  (see the 2026-06-03 research note on #9); deferred.
- `malformed-cite-repair` enhancer — rare; deferred.
- Save-Page-Now / any external archive write.

## Glossary

- **AST (Abstract Syntax Tree)**: a tree representation of parsed wikitext in which each node (e.g. a `<ref>`
  tag, a template) is a typed object that can be queried and replaced individually, rather than treated as
  raw text.
- **wikiparser-node**: the third-party Node.js library (GPL-3.0-or-later) that parses wikitext into an AST
  and re-serializes it; hidden behind the `WikitextParser` edge so `core` never imports it directly.
- **Edge / DI (Dependency Injection)**: a pattern in this codebase where impure or third-party functionality
  is placed behind a `core` interface and injected via `ToolContext`, keeping `core` pure and all external
  dependencies swappable in tests.
- **Ordinal locator**: the zero-based index of a `<ref>` node in document order, stored in
  `ProposedEdit.locator`; it identifies which exact node to replace without relying on string matching.
- **Node-anchored editing**: replacing a specific AST node identified by its ordinal (and verified by its
  exact text) rather than performing a first-match string substitution on raw wikitext.
- **Ambiguity guard (fail-closed-on-duplicate-`<ref>`)**: the existing `indexOf !== lastIndexOf` check in
  `commit_confirmed_edit` that refuses to apply an edit when the target string appears more than once in the
  wikitext; retired by the ordinal locator.
- **Diff grounding**: the `diffGroundingHash(before, after)` mechanism that binds a proposed edit's exact
  before/after text into the confirmation token, ensuring the human approved precisely the change that will
  be applied.
- **Confirmation token**: a cryptographic hash bound to the exact `ProposedEdit` (including the `locator`)
  that `commit_confirmed_edit` requires before writing; prevents a proposal from being tampered with between
  approval and application.
- **`baseRevision` / `baseTimestamp`**: the MediaWiki revision ID and timestamp of the article at the time
  it was reviewed; sent with the write request so MediaWiki can detect concurrent edits (TOCTOU protection).
- **`<ref>` / ext-tag**: a wikitext extension tag (`<ref>...</ref>`) used to inline citations;
  `wikiparser-node` exposes these as `ext#ref` nodes queryable by `querySelectorAll`.
- **FCIS (Functional Core, Imperative Shell)**: the codebase's architecture where pure, deterministic logic
  lives in `core` and all I/O / side effects live in injected edges (the shell); here, bare-URL
  classification stays in the functional core while ref-boundary parsing moves to the edge.
- **`makeTestContext`**: the factory that constructs a fully-wired `ToolContext` for tests; for
  `wikitextParser` it provides the real `wikiparser-node` wrapper (not a fake) because the library is
  offline/deterministic and a hand-rolled fake would risk diverging from it.
- **GPL-3.0-or-later**: the strong-copyleft license of `wikiparser-node`; accepted by the owner after a
  separate license analysis and recorded in `adr/0009`.

## Architecture

A narrow, operation-oriented **wikitext-parser edge** wraps `wikiparser-node` and exposes only the two
operations bare-URL repair needs. `repair_citation` and `commit_confirmed_edit` migrate onto it, making
edit-application **node-anchored** (replace the exact AST node, not the first string match) and retiring
the fail-closed ambiguity guard. `wikiparser-node` is fully hidden behind the edge; the rest of the system
sees only the `core` interface.

### The edge contract (`core`)

```ts
interface WikitextRef {
  ordinal: number;   // index among <ref> nodes in document order — the stable locator at a fixed revision
  text: string;      // the node's full wikitext, e.g. '<ref>https://a/x</ref>'
  inner: string;     // the inner body, e.g. 'https://a/x' — input to pure bare-URL classification
  attrs: string;     // ref attributes, e.g. ' name="n"' (empty if none)
}

interface WikitextParser {
  /** Enumerate <ref> nodes in document order; ordinal == array index. */
  findRefs(wikitext: string, wiki: string): WikitextRef[];
  /**
   * Replace the ordinal-th <ref> with `newNodeText`, ONLY if that node's current wikitext === `expectedText`
   * (anti-drift / re-grounding at apply time). Returns the new full wikitext, or null if the ordinal is
   * out of range or the node text no longer matches.
   */
  replaceRefNode(
    wikitext: string, wiki: string, ordinal: number, expectedText: string, newNodeText: string,
  ): string | null;
}
```

The empirical spike (2026-06-03) confirmed in our Node/ESM runtime: `querySelectorAll('ext#ref')` returns
refs in document order; ordinals are reproducible across a re-parse of identical wikitext; `safeReplaceWith`
on the ordinal-th node replaces exactly one of two textually-identical refs; `String(root)` re-serializes
losslessly except the edited node.

### The locator (`core/types.ts`)

`ProposedEdit` gains one extensible, hash-bound field; `before`/`after` and the `diff` grounding are
unchanged:

```ts
locator?: { kind: 'ext-ref'; ordinal: number };
```

Because the confirmation token binds to `hashProposedEdit(proposal)`, the ordinal is tamper-evident.

### Components

- **`WikitextParser` interface + `WikitextRef`** — `packages/core/src/edges.ts` (pure signatures).
- **`ProposedEdit.locator`** — `packages/core/src/types.ts` (new optional field).
- **The `wikiparser-node` wrapper** — `packages/edges/src/` (new). Owns `Parser.config`; sets it and parses
  synchronously (no `await` between → no interleave window); maps `wiki` codes → wikiparser configs
  (`'en'` → `'enwiki'`, default `enwiki`). `wikiparser-node` added to `packages/edges/package.json`.
- **`ToolContext.wikitextParser`** — `packages/core/src/tool.ts` (new **required** edge, like `clock`/
  `storage`); provided by `makeTestContext` (the real wrapper — it is offline/deterministic, so hermetic)
  and by the composition root `packages/server/src/main.ts`.
- **Pure bare-URL classification** — `packages/core/src/citation/repair.ts`, refactored to classify a ref's
  `inner` body (is it a bare URL?) and build the wrapped `{{cite web…}}` text, instead of scanning raw
  wikitext for ref boundaries.
- **`repair_citation`** — `packages/tools/src/citation/repair-citation.ts`: `edge.findRefs()` → pure
  classification → `ProposedEdit` with the ordinal `locator`.
- **`commit_confirmed_edit`** — `packages/tools/src/commit-confirmed-edit.ts`: replaces `wikitext.replace`
  + the `indexOf !== lastIndexOf` ambiguity guard with `edge.replaceRefNode(...)`.

### Data flow

- **Proposal:** `improve` → `repair_citation` → `edge.findRefs(wikitext, wiki)` → pure classification of
  each `inner` → for each bare-URL ref, a `ProposedEdit { before: ref.text, after: wrapped,
  locator: { kind: 'ext-ref', ordinal: ref.ordinal }, evidence: diff-grounded, hash }`.
- **Apply:** `commit_confirmed_edit` → token binding (`hashProposedEdit`, now covering `locator`) →
  `baseRevision` guard (guarantees apply-time wikitext == reviewed wikitext) →
  `edge.replaceRefNode(current.wikitext, wiki, locator.ordinal, before, after)` (re-parse → assert
  `String(node) === before` → `safeReplaceWith` → serialize) → `applyEdit` with
  `baseRevision`/`baseTimestamp`, dry-run, write-allowlist, test-wiki-only gates (all unchanged).

### System boundaries

`WikitextParser` is a `core` interface (no dependencies); its impl lives in `edges` (`edges → [core]`
holds); `tools` consume it via `ctx.wikitextParser` (injected — no new boundary). **No boundary-rule
changes.** `core` stays pure: the parser's I/O-impurity (global state, fs-config-load) is isolated in the
edge, exactly per `adr/0009`.

## Existing Patterns

Investigation (2026-06-03) confirmed the design follows established codebase patterns:

- **DI edge pattern (`adr/0011`):** `HtmlExtractor` (interface in `core/src/edges.ts`) → `DefuddleHtmlExtractor`
  (impl in `edges`) → injected via `ToolContext`. `WikitextParser` mirrors this exactly. **One deliberate
  divergence:** `htmlExtractor` is *optional* (graceful degradation); `wikitextParser` is *required* (the
  node-anchored path is the only path — the DoD retires the regex/text-replace fallback). Because the edge
  is offline/deterministic, `makeTestContext` provides the real wrapper rather than a fake (a hand-rolled
  fake would re-implement ref-scanning and risk diverging from the library — the fragility this work
  escapes).
- **Import-boundary layering** (`scripts/lib/boundaries.ts`): `core: []`, `edges: ['core']`. Unchanged.
- **Confirmation-token + diff grounding** (`commit_confirmed_edit.ts`, `hooks/grounding-verifier.ts`,
  `core/diff.ts`): `hashProposedEdit` binds the token; `diffGroundingHash(before, after)` grounds the edit
  and `verifyDiff` re-checks it. Preserved verbatim; the apply-time `String(node) === before` check is an
  additional, strictly-stronger anti-drift gate.
- **Pure-core / FCIS:** the bare-URL *pattern* logic stays pure in `core` (now operating on ref inner text);
  only ref-boundary *parsing* moves to the edge.
- **Characterization-test discipline** (as used for the #7/#13 migrations): pin current `repair_citation` /
  `commit_confirmed_edit` behavior before refactoring; keep single-bare-URL tests green; intentionally flip
  the ambiguity-guard test.

## Implementation Phases

### Phase 1: The `WikitextParser` edge
**Goal:** A working, unit-tested wikitext-parser edge over `wikiparser-node`, standalone.

**Components:**
- `WikitextParser` + `WikitextRef` interface — `packages/core/src/edges.ts`.
- The wrapper impl (`findRefs`, `replaceRefNode`, wiki-code→config map, synchronous config+parse) —
  `packages/edges/src/` (new file); `wikiparser-node` added to `packages/edges/package.json`.
- Edge unit tests — `packages/edges/src/`: parse→serialize idempotence (mixed whitespace), `findRefs`
  count + document-order ordinals, `replaceRefNode` success, anti-drift `null` (expectedText mismatch),
  out-of-range `null`, and the **two-identical-`<ref>`s disambiguation**.

**Dependencies:** None (first phase).

**Done when:** `npm install` succeeds; edge unit tests pass; `npm run boundaries` green (`edges → core`).

### Phase 2: Wire the edge into `ToolContext`
**Goal:** `wikitextParser` is available everywhere via DI; nothing consumes it yet; existing tests stay green.

**Components:**
- `ToolContext.wikitextParser: WikitextParser` (required) — `packages/core/src/tool.ts`.
- `makeTestContext` provides the real wrapper — `packages/edges/src/`.
- Composition root provides the wrapper — `packages/server/src/main.ts`.
- Update any other `ToolContext` construction sites surfaced by investigation.

**Dependencies:** Phase 1.

**Done when:** `npm run typecheck` clean; the full existing suite stays green; `npm run ci` green.

### Phase 3: Migrate `repair_citation` detection onto the AST + the locator
**Goal:** Bare-URL detection runs through the edge; proposals carry the ordinal locator.

**Components:**
- Pure bare-URL classification refactored to operate on ref `inner` text — `packages/core/src/citation/repair.ts`
  (+ tests).
- `ProposedEdit.locator` field — `packages/core/src/types.ts`.
- `repair_citation` uses `edge.findRefs()` + classification, emits the `locator` —
  `packages/tools/src/citation/repair-citation.ts` (+ updated tests).

**Dependencies:** Phases 1–2.

**Done when:** `repair_citation` tests pass (single-bare-URL behavior preserved; proposals carry
`locator: { kind: 'ext-ref', ordinal }`); diff grounding unchanged.

### Phase 4: Node-anchored application in `commit_confirmed_edit`
**Goal:** Apply via the exact node; retire the ambiguity guard; duplicates handled precisely.

**Components:**
- `commit_confirmed_edit` uses `edge.replaceRefNode(...)`; remove `wikitext.replace` + the
  `indexOf !== lastIndexOf` guard — `packages/tools/src/commit-confirmed-edit.ts`.
- Tests: the ambiguity-guard test flips (refuse → precise application); add the **duplicate-identical-`<ref>`
  success-criterion** test; anti-drift refusal (`String(node) !== before`) test —
  `packages/tools/src/commit-confirmed-edit.test.ts`.

**Dependencies:** Phase 3 (the locator).

**Done when:** A page with two identical bare-URL `<ref>`s applies the correct one precisely; all
invariant tests (token binding, `baseRevision`/`baseTimestamp`, dry-run/allowlist/test-wiki gates) green.

### Phase 5: Integration, CI, and ADR reconciliation
**Goal:** Prove the whole path and close out.

**Components:**
- A hermetic end-to-end test: `improve` → bare-URL `ProposedEdit` (with locator) → `commit_confirmed_edit`
  applies node-anchored, no write beyond the gated path — `packages/agent/src/` (sibling integration test).
- `adr/0009` updated to **Built** (record the confirmed GPL-3.0-or-later license + the spike evidence).
- Confirm CI Node engine satisfies `wikiparser-node` (`>=24.11`); bump if needed.

**Dependencies:** Phases 1–4.

**Done when:** the end-to-end test passes; `npm run ci` green; `adr/0009` reflects the built state.

## Additional Considerations

- **Node engine:** `wikiparser-node` requires Node `^20.19 || ^22.13 || >=24.11`. Confirm the project's CI
  Node version satisfies this (local dev is on 26); bump the CI matrix if needed (Phase 5).
- **License:** `wikiparser-node` is **GPL-3.0-or-later** (strong copyleft). The owner is running a separate
  license analysis; `adr/0009`'s "GPL-family/permissive" note is corrected to the confirmed license at
  adoption (Phase 5).
- **Global config concurrency:** `Parser.config` is global/static. The edge sets it and parses
  synchronously (no `await` in between), so concurrent async callers cannot interleave at the config
  boundary. A `wiki`-code → wikiparser-config map (default `enwiki`) lives in the edge.
- **Future extensibility:** the narrow edge widens when the next consumer lands — `wayback-archive` adds a
  template-param operation (`setParam`), `malformed-cite-repair` adds template detection. The `locator`
  union (`{ kind: 'ext-ref'; ordinal }`) extends to other node kinds (e.g. `{ kind: 'template'; ordinal }`)
  without disturbing existing edits.
- **Memory:** `wikiparser-node` documents rare memory leaks; low risk here (one parse per session), but
  worth watching if the long-running server parses many large articles.
