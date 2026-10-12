# Wikitext-AST Edit Layer — Phase 2: Wire the edge into `ToolContext`

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** `wikitextParser` is available everywhere via dependency injection. Nothing consumes it yet; the entire existing suite stays green.

**Architecture:** Add `wikitextParser` as a **required** field on `ToolContext` (the design's one deliberate divergence from `htmlExtractor`, which is optional — the node-anchored path will be the only path, so there is no graceful-degradation fallback to keep). Because the field is required, **every** site that constructs a `ToolContext` value must provide it. Investigation found exactly **three** construction shapes repo-wide, and only two need edits:
1. `makeTestContext` (`packages/edges/src/context.ts`) — the single factory **every** test uses (directly or via helpers). Provides the **real** `WikiparserNodeParser` (the edge is offline/deterministic, so a hand-rolled fake would just risk diverging from the library).
2. The composition root (`packages/server/src/main.ts`) — provides the real `WikiparserNodeParser`.
3. `packages/agent/src/hitl.ts:391` builds `{ ...this.ctx, confirmation: … }` — a **spread**, so it inherits the new field automatically. **No edit needed.**

**Tech Stack:** TypeScript (strict, ESM), Vitest, npm workspaces.

**Scope:** Phase 2 of 5. Depends on Phase 1.

**Codebase verified:** 2026-06-03 (exhaustive `ToolContext`-construction-site enumeration: only the three shapes above exist; **zero** test files build a `ToolContext` literal directly).

---

## Task 1: Make `wikitextParser` a required `ToolContext` field and wire both real construction sites

This is one cohesive change across three files: adding a required interface field necessarily breaks `tsc` until its two literal construction sites provide it. Do all three edits, then verify once. (The `hitl.ts` spread site needs nothing.)

**Files:**
- Modify: `packages/core/src/tool.ts` (add the required field + import the type)
- Modify: `packages/edges/src/context.ts` (provide the real wrapper in `makeTestContext`; add an optional override to `TestContextOptions`)
- Modify: `packages/server/src/main.ts` (provide the real wrapper in the composition-root `ctx` literal)

**Step 1: Add the field to the `ToolContext` interface**

In `packages/core/src/tool.ts`:

(a) Extend the type import (currently lines 8–17) to include `WikitextParser`. The import block is:

```typescript
import type {
  Clock,
  HtmlExtractor,
  HttpClient,
  ModelClient,
  ModelToolCall,
  ObjectStore,
  Storage,
  WikiClient,
} from './edges.ts';
```

Add `WikitextParser` (keep the list alphabetical):

```typescript
import type {
  Clock,
  HtmlExtractor,
  HttpClient,
  ModelClient,
  ModelToolCall,
  ObjectStore,
  Storage,
  WikiClient,
  WikitextParser,
} from './edges.ts';
```

(b) Add the **required** field to the `ToolContext` interface. Insert it immediately after the `clock: Clock;` line (line 36) — grouping it with the other always-present edges (`clock`/`storage`), and **before** the optional `htmlExtractor?` block:

```typescript
  clock: Clock;
  /**
   * Structured wikitext editing edge (#9, `adr/0009`). REQUIRED (unlike `htmlExtractor`):
   * bare-URL detection and node-anchored edit-application have no string-replace fallback,
   * so every context — tests (`makeTestContext`) and the composition root — provides the
   * real `wikiparser-node` wrapper. The edge is offline/deterministic, so this stays hermetic.
   */
  wikitextParser: WikitextParser;
```

**Step 2: Provide the real wrapper in `makeTestContext`**

In `packages/edges/src/context.ts`:

(a) Add the import (the file already imports its other doubles from `./…` modules). Add after the existing `./model/scripted.ts` import:

```typescript
import { WikiparserNodeParser } from './wikitext-parser.ts';
```

(b) Add the type to the `@wikiharness/core` type import block (which currently imports `Clock, HtmlExtractor, HttpClient, ModelClient, ObjectStore, Storage, ToolContext, WikiClient`). Add `WikitextParser`:

```typescript
import type {
  Clock,
  HtmlExtractor,
  HttpClient,
  ModelClient,
  ObjectStore,
  Storage,
  ToolContext,
  WikiClient,
  WikitextParser,
} from '@wikiharness/core';
```

(c) Add an optional override to `TestContextOptions` (so a test could inject a fake if it ever needed to), after the `htmlExtractor?` line:

```typescript
  htmlExtractor?: HtmlExtractor;
  wikitextParser?: WikitextParser;
```

(d) In the `makeTestContext` return object, provide the **real** wrapper as the default. Add after the `clock:` line and before `htmlExtractor:`:

```typescript
    clock: opts.clock ?? new FixedClock(0),
    wikitextParser: opts.wikitextParser ?? new WikiparserNodeParser(),
    htmlExtractor: opts.htmlExtractor,
```

**Step 3: Provide the real wrapper in the composition root**

In `packages/server/src/main.ts`:

(a) Add `WikiparserNodeParser` to the `@wikiharness/edges` import block (currently imports `DefuddleHtmlExtractor, FetchHttpClient, …, VercelModelClient, buildUserAgent, type MwnBot`). Add `WikiparserNodeParser` (keep ordering consistent):

```typescript
import {
  DefuddleHtmlExtractor,
  FetchHttpClient,
  InMemoryObjectStore,
  InMemoryStorage,
  InMemoryWikiClient,
  MwnWikiClient,
  NoModelClient,
  SystemClock,
  VercelModelClient,
  WikiparserNodeParser,
  buildUserAgent,
  type MwnBot,
} from '@wikiharness/edges';
```

(b) In the `ctx` literal (lines 95–106), add the field after the `clock:` line and before `htmlExtractor:`:

```typescript
  clock: new SystemClock(),
  wikitextParser: new WikiparserNodeParser(), // structured wikitext editing (adr/0009, #9)
  htmlExtractor: new DefuddleHtmlExtractor(), // main-content extraction (adr/0011)
```

**Step 4: Confirm there are no other construction sites**

Run this guard to confirm the investigator's finding. Match **construction literals** (`ToolContext = {`), not parameter annotations — `grep ": ToolContext"` would also match every `ctx: ToolContext` parameter (~30 hits) and is the wrong guard:

Run: `grep -rn "ToolContext = {" packages scripts --include=*.ts | grep -v "\.test\.ts"`
Expected output (exactly these two literals, both already handled):
- `packages/server/src/main.ts` — the composition-root literal you just edited.
- `packages/agent/src/hitl.ts` — the `{ ...this.ctx, confirmation }` **spread** literal, which inherits `wikitextParser` from `this.ctx` (no edit needed).

`makeTestContext` (`packages/edges/src/context.ts`) builds its context via `return { … }` (no `: ToolContext = {` annotation), so it won't appear here — it's the third site, covered by Step 2. If any **other** literal appears, add `wikitextParser: new WikiparserNodeParser()` to it (importing the wrapper from `@wikiharness/edges`).

**Step 5: Typecheck**

Run: `npm run typecheck`
Expected: clean. A missing construction site would surface here as `Property 'wikitextParser' is missing` — fix any that appear per Step 4.

**Step 6: Run the full suite**

Run: `npm test`
Expected: the entire existing suite stays green. Nothing consumes `wikitextParser` yet, so behavior is unchanged; tests just now carry a real parser in their context.

**Step 7: Commit**

```bash
git add packages/core/src/tool.ts packages/edges/src/context.ts packages/server/src/main.ts
git commit -m "feat(core): inject required wikitextParser edge into ToolContext (#9)"
```

---

## Task 2: Verify the full CI gate

**Files:** none (verification only).

**Step 1: Run CI**

Run: `npm run ci`
Expected: green — format, typecheck, lint, boundaries, stubscan, coverage (≥90% on `core`/`tools` unchanged — no new runtime logic in those packages this phase), eval.

**Step 2: Confirm boundaries still hold**

Run: `npm run boundaries`
Expected: green. `core` gained only a type field; `edges` is imported by `server` (already allowed: `server → edges`) and by `context.ts` within `edges` itself.

(No commit — verification only. If CI surfaces a missed construction site or a lint/format nit, fix it and amend or add a follow-up commit.)

---

## Phase 2 Done When

- `npm run typecheck` clean.
- `npm test` — the full existing suite green (no behavior change).
- `npm run ci` green.
- `wikitextParser` is a required field on `ToolContext`, provided by `makeTestContext` (real `WikiparserNodeParser`) and by the composition root; `hitl.ts`'s spread inherits it; no other construction site exists.
