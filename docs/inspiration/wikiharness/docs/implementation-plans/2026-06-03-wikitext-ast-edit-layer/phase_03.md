# Wikitext-AST Edit Layer — Phase 3: Migrate `repair_citation` detection onto the AST + the locator

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Bare-URL detection runs through the `wikitextParser` edge; pure `core` classifies a ref's **inner body** (not raw wikitext); each proposal carries an `ext-ref` **ordinal locator**.

**Architecture:** `repair_citation` calls `ctx.wikitextParser.findRefs(wikitext, wiki)` (the edge built in Phases 1–2) to get every `<ref>` node with its document-order ordinal, then asks pure `core` (`classifyBareUrlInner`) whether each ref's `inner` body is a bare URL, and builds the `{{cite web}}` replacement with `wrapBareUrl(attrs, url, date)`. The proposal gains `locator: { kind: 'ext-ref', ordinal }` where `ordinal` is the **full-list** document index (so Phase 4's apply-time re-parse indexes the same node). The diff grounding is unchanged.

**Two ordering-critical facts (do not skip):**
1. **The locator must survive zod parsing.** `commit_confirmed_edit` re-validates the proposal through a structural `proposedEditSchema` and **re-hashes the parsed object** to check the confirmation token. zod `.object()` **strips unknown keys by default**, so if the schema does not model `locator`, the re-hash drops it → the token no longer binds → every locator-bearing proposal would be refused (this would break the existing `hitl.test.ts` approve→commit test the moment Task 2 lands). Task 1 adds `locator` to that schema **first**.
2. **The ordinal is the FULL-list index, not the filtered index.** `findRefs` returns ALL `<ref>` nodes (templated ones included); only some classify as bare URLs. The `locator.ordinal` must be `ref.ordinal` from the edge — the index among *all* refs — because `replaceRefNode` re-parses and indexes the full list. Using a filtered loop counter would point at the wrong node.

**Tech Stack:** TypeScript (strict), Vitest, fast-check (property tests), zod.

**Scope:** Phase 3 of 5. Depends on Phases 1–2.

**Codebase verified:** 2026-06-03. Current `repair.ts` exports `findBareUrlRefs` / `BareUrlRef` / `wrapBareUrl`; consumers are `repair-citation.ts`, `repair.test.ts`, and `property.test.ts` (re-exported via `core/src/index.ts`). `hashProposedEdit` canonicalizes the whole object (RFC 8785 JCS), so a new optional field is hashed automatically — **no `hash.ts` change**.

---

## Task 1: Plumb the `locator` field (type + commit schema) — no emitter yet

This task is self-contained and stays green: it adds an **optional** field that nothing emits yet, plus the schema preservation that Task 2 will depend on. A regression test proves the token still binds when a locator IS present.

**Files:**
- Modify: `packages/core/src/types.ts` (add `ProposedEdit.locator`)
- Modify: `packages/tools/src/commit-confirmed-edit.ts` (preserve `locator` through `proposedEditSchema`)
- Modify: `packages/tools/src/commit-confirmed-edit.test.ts` (regression: a locator-bearing proposal's token binds in dry-run)

**Step 1: Write the failing regression test**

In `packages/tools/src/commit-confirmed-edit.test.ts`, add this test inside the existing `describe('commitConfirmedEditTool …')` block (after the dry-run test). It builds a proposal **with** a locator and confirms it commits in dry-run — which can only happen if the schema preserves `locator` so the re-hash matches the token:

```typescript
  test('a proposal carrying an ext-ref locator: the locator survives schema parsing so the token still binds (dry-run)', async () => {
    const base: Omit<ProposedEdit, 'hash'> = {
      id: 'p1',
      sink: 'wikipedia',
      target: { title: 'Test', wiki: 'test', baseRevision: 10 },
      before: '<ref>https://example.org/x</ref>',
      after: '<ref>{{cite web |url=https://example.org/x |access-date=2026-06-02}}</ref>',
      summary: 'Fill bare citation (AI-assisted)',
      evidence: [],
      provenance: { sources: [], createdAt: 0, toolName: 'repair_citation' },
      locator: { kind: 'ext-ref', ordinal: 1 },
      rationale: 'r',
    };
    const p: ProposedEdit = { ...base, hash: hashProposedEdit(base) };
    const wiki = wikiWithArticle(p);
    const result = await commitConfirmedEditTool.execute(
      { proposal: p, confirmationToken: p.hash },
      makeTestContext({ wiki }),
    );
    expect(result.data.committed).toBe(true);
    expect(result.data.dryRun).toBe(true);
  });
```

**Step 2: Run it — watch it FAIL**

Run: `npx vitest run packages/tools/src/commit-confirmed-edit.test.ts -t "locator survives schema parsing"`
Expected: FAIL — `committed` is `false` (the unmodified schema strips `locator`, so the re-hash differs from the token). This also won't typecheck until Step 3a adds `locator` to `ProposedEdit`. (If TS blocks the run, do Step 3a first, then re-run to see the runtime failure.)

**Step 3a: Add the `locator` field to `ProposedEdit`**

In `packages/core/src/types.ts`, insert the field immediately after the `after: string;` line (line 142), before `summary`:

```typescript
  /** The proposed replacement wikitext fragment. */
  after: string;
  /**
   * Optional node locator (#9, `adr/0009`): identifies the exact AST node to replace, so
   * apply-time is node-anchored (not a first-match string `.replace`) and duplicate identical
   * fragments are disambiguated. Hash-bound (the confirmation token covers it). Extensible to
   * other node kinds later (e.g. `{ kind: 'template'; ordinal }`).
   */
  locator?: { kind: 'ext-ref'; ordinal: number };
  /** Honest edit summary, disclosing AI assistance (`02`). */
  summary: string;
```

**Step 3b: Preserve `locator` through the commit schema**

In `packages/tools/src/commit-confirmed-edit.ts`, the `proposedEditSchema` (lines 15–30) currently lists fields and ends `provenance`, `rationale`, `hash`. Add a `locator` field so zod keeps it on parse. Insert after the `provenance: z.unknown(),` line:

```typescript
  evidence: z.array(z.unknown()),
  provenance: z.unknown(),
  locator: z
    .object({ kind: z.literal('ext-ref'), ordinal: z.number().int().nonnegative() })
    .optional(),
  rationale: z.string(),
  hash: z.string(),
```

**Step 4: Run the test — watch it PASS**

Run: `npx vitest run packages/tools/src/commit-confirmed-edit.test.ts`
Expected: all tests PASS (the existing ones unchanged; the new one now green).

**Step 5: Typecheck + commit**

Run: `npm run typecheck`
Expected: clean.

```bash
git add packages/core/src/types.ts packages/tools/src/commit-confirmed-edit.ts packages/tools/src/commit-confirmed-edit.test.ts
git commit -m "feat(core): add ProposedEdit.locator; preserve it through the commit schema (#9)"
```

---

## Task 2: Migrate detection onto the AST (core classifier + the tool) and emit the locator

The core symbol rename (`findBareUrlRefs`/`BareUrlRef` → `classifyBareUrlInner`) and the tool's switch to `ctx.wikitextParser.findRefs` are **coupled** — `repair-citation.ts` imports the old symbols, so they change together in one task. Within the task we still go test-first.

**Files:**
- Modify: `packages/core/src/citation/repair.ts` (replace raw-wikitext scan with an inner-body classifier; change `wrapBareUrl` signature)
- Modify: `packages/core/src/citation/repair.test.ts` (rewrite for the new pure API)
- Modify: `packages/core/src/property.test.ts` (port the property tests to `classifyBareUrlInner`)
- Modify: `packages/tools/src/citation/repair-citation.ts` (use the edge + classifier; emit the locator)
- Modify: `packages/tools/src/citation/repair-citation.test.ts` (preserve behavior; assert the locator + full-list ordinal)

**Step 1: Rewrite the core unit test (`repair.test.ts`) for the new API**

Replace the entire contents of `packages/core/src/citation/repair.test.ts` with:

```typescript
import { describe, expect, test } from 'vitest';
import { classifyBareUrlInner, wrapBareUrl } from './repair.ts';

describe('classifyBareUrlInner (is a <ref> body a bare URL?)', () => {
  test('a plain bare URL classifies and returns the url', () => {
    expect(classifyBareUrlInner('https://example.org/a')).toBe('https://example.org/a');
  });

  test('a bracketed bare URL [http://x] with no title classifies (bracket stripped)', () => {
    expect(classifyBareUrlInner('[https://example.org/a]')).toBe('https://example.org/a');
  });

  test('a bracketed external link WITH a title does not classify', () => {
    expect(classifyBareUrlInner('[https://example.org/a A descriptive title]')).toBeNull();
  });

  test('a body that already uses a citation template does not classify', () => {
    expect(classifyBareUrlInner('{{cite web|url=https://example.org}}')).toBeNull();
  });

  test('a body with surrounding prose does not classify', () => {
    expect(classifyBareUrlInner('See https://example.org for details')).toBeNull();
  });

  test('tolerates surrounding whitespace around an otherwise-bare URL', () => {
    expect(classifyBareUrlInner('  https://example.org/a  ')).toBe('https://example.org/a');
  });

  test('an empty body does not classify', () => {
    expect(classifyBareUrlInner('')).toBeNull();
  });
});

describe('wrapBareUrl (build a {{cite web}} replacement node, preserving attrs)', () => {
  test('wraps url + access-date with no attrs', () => {
    expect(wrapBareUrl('', 'https://example.org/a', '2026-06-02')).toBe(
      '<ref>{{cite web |url=https://example.org/a |access-date=2026-06-02}}</ref>',
    );
  });

  test('preserves a name= attr (leading space included by the edge contract)', () => {
    expect(wrapBareUrl(' name="s"', 'https://example.org/a', '2026-06-02')).toBe(
      '<ref name="s">{{cite web |url=https://example.org/a |access-date=2026-06-02}}</ref>',
    );
  });
});
```

**Step 2: Run it — watch it FAIL**

Run: `npx vitest run packages/core/src/citation/repair.test.ts`
Expected: FAIL — `classifyBareUrlInner` is not exported yet.

**Step 3: Rewrite `repair.ts`**

Replace the entire contents of `packages/core/src/citation/repair.ts` with:

```typescript
/**
 * Detect bare-URL `<ref>` bodies and build a `{{cite web}}` replacement — the
 * deterministic, source-free citation improvement MVP-1 can make (port spirit of
 * citation-checker's enrich path). Pure: operates on a ref's INNER body text; the
 * wikitext-parser edge (`adr/0009`) supplies ref boundaries + ordinals, and the tool
 * layer turns a positive classification into a confirmation-gated, node-anchored
 * ProposedEdit. (Filling title/author via Citoid is a later enrichment; a url +
 * access-date cite is a valid first step the human can edit.)
 */

// A bare-URL body: a single (optionally bracketed) http(s) URL and NOTHING else — no
// template, no link title, no surrounding prose. [^\s<|{}\]] keeps it to one bare URL.
// Anchored (^…$) + bounded ({1,2000}) so the match is linear — no super-linear
// backtracking on adversarial input (the URL class and \s are disjoint).
const BARE_URL_BODY = /^\s*\[?(https?:\/\/[^\s<|{}\]]{1,2000})\]?\s*$/i;

/** If a `<ref>` body is ONLY a (possibly bracketed) bare URL, return that URL; otherwise null. */
export function classifyBareUrlInner(inner: string): string | null {
  return BARE_URL_BODY.exec(inner)?.[1] ?? null;
}

/** Build the `{{cite web}}` replacement node, preserving the ref's opening-tag attrs. */
export function wrapBareUrl(attrs: string, url: string, accessDate: string): string {
  return `<ref${attrs}>{{cite web |url=${url} |access-date=${accessDate}}}</ref>`;
}
```

**Step 4: Run the core test — watch it PASS**

Run: `npx vitest run packages/core/src/citation/repair.test.ts`
Expected: PASS. (`tsc`/`tools` are now broken because `repair-citation.ts` still imports the old symbols — fixed in Step 6.)

**Step 5: Port the property tests**

In `packages/core/src/property.test.ts`:

(a) Change the import on line 12 from:

```typescript
import { findBareUrlRefs } from './citation/repair.ts';
```

to:

```typescript
import { classifyBareUrlInner } from './citation/repair.ts';
```

(b) Replace the entire `describe('property: findBareUrlRefs …')` block (lines 88–105) with:

```typescript
describe('property: classifyBareUrlInner (the bare-URL body classifier, hardened)', () => {
  test('never throws, and never classifies a {{template}} body as a bare URL', () => {
    fc.assert(
      fc.property(fc.webUrl(), (url) => {
        expect(classifyBareUrlInner(`{{cite web |url=${url}}}`)).toBeNull();
        const r = classifyBareUrlInner(url);
        return r === null || typeof r === 'string';
      }),
    );
  });
  test('is ReDoS-safe: completes on a long adversarial body', () => {
    fc.assert(
      fc.property(fc.integer({ min: 0, max: 5000 }), (n) => {
        classifyBareUrlInner('a'.repeat(n) + '<'.repeat(n));
        return true;
      }),
    );
  });
});
```

**Step 6: Migrate the tool (`repair-citation.ts`)**

Replace the import block and the `execute` body of `packages/tools/src/citation/repair-citation.ts`. The new file:

```typescript
import { z } from 'zod';
import {
  classifyBareUrlInner,
  diffGroundingHash,
  hashProposedEdit,
  wrapBareUrl,
  type Evidence,
  type ProposedEdit,
  type ProvenanceRecord,
  type Tool,
  type ToolContext,
  type ToolResult,
} from '@wikiharness/core';

const params = z.object({
  title: z.string().min(1),
  wiki: z.string().min(1),
  wikitext: z.string(),
  revision: z.number().int(),
});
export type RepairCitationParams = z.infer<typeof params>;

export interface RepairCitationResult {
  count: number;
}

function isoDate(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

/**
 * `repair_citation` (`07`) — propose converting bare-URL `<ref>`s into `{{cite web}}`
 * templates. Detection is node-anchored: the wikitext-parser edge (`adr/0009`) enumerates
 * `<ref>` nodes in document order; pure `core` classifies each node's inner body. Each
 * proposal carries the ref's document-order `ordinal` so apply-time replaces the EXACT node
 * (disambiguating duplicate identical refs). Emits diff-grounded ProposedEdits ONLY (never
 * writes; the human confirms each). The proposal is intentionally minimal (url + access date)
 * so the editor can enrich it (title/author) via the edit-then-approve path.
 */
export const repairCitationTool: Tool<RepairCitationParams, RepairCitationResult> = {
  name: 'repair_citation',
  access: 'read+write',
  params,
  async execute(
    p: RepairCitationParams,
    ctx: ToolContext,
  ): Promise<ToolResult<RepairCitationResult>> {
    const now = ctx.clock.now();
    const accessDate = isoDate(now);
    const provenance: ProvenanceRecord = {
      sources: [],
      createdAt: now,
      toolName: 'repair_citation',
    };

    const proposedEdits: ProposedEdit[] = [];
    for (const ref of ctx.wikitextParser.findRefs(p.wikitext, p.wiki)) {
      const url = classifyBareUrlInner(ref.inner);
      if (url === null) continue; // not a bare URL — leave it alone
      const before = ref.text;
      const after = wrapBareUrl(ref.attrs, url, accessDate);
      const evidence: Evidence = {
        kind: 'diff',
        primary: { summary: `Convert a bare URL (${url}) into a {{cite web}} template.` },
        provenance,
        grounding: { kind: 'diff', expectedDiffHash: diffGroundingHash(before, after) },
      };
      const base: Omit<ProposedEdit, 'hash'> = {
        id: `bare-url-${ref.ordinal}`,
        sink: 'wikipedia',
        target: { title: p.title, wiki: p.wiki, baseRevision: p.revision },
        before,
        after,
        // The document-order ordinal among ALL <ref> nodes — apply-time re-parses and
        // indexes this exact node (not the first string match).
        locator: { kind: 'ext-ref', ordinal: ref.ordinal },
        summary: 'Convert bare URL to {{cite web}} (AI-assisted)',
        evidence: [evidence],
        provenance,
        rationale:
          'This citation is a bare URL; wrapping it in {{cite web}} with an access-date improves citation quality. Edit to add a title/author before approving.',
      };
      proposedEdits.push({ ...base, hash: hashProposedEdit(base) });
    }

    return {
      data: { count: proposedEdits.length },
      provenance,
      proposedEdits,
      rationale: `Found ${proposedEdits.length} bare-URL citation(s) to improve.`,
    };
  },
};
```

**Step 7: Update the tool test (`repair-citation.test.ts`)**

The existing tests use `makeTestContext` (which now carries the real parser) — they should pass unchanged because the parser serializes `<ref>` nodes losslessly. Add locator assertions, including the **full-list-ordinal** proof. Append these tests inside the existing `describe` block, and run to confirm the existing three still pass:

```typescript
  test('each proposal carries an ext-ref locator with the ref document-order ordinal', async () => {
    const result = await repairCitationTool.execute(
      { title: 'T', wiki: 'en', wikitext, revision: 42 },
      ctx,
    );
    expect(result.proposedEdits![0]!.locator).toEqual({ kind: 'ext-ref', ordinal: 0 });
    expect(result.proposedEdits![1]!.locator).toEqual({ kind: 'ext-ref', ordinal: 1 });
  });

  test('locator ordinal is the FULL-list document index (a templated ref shifts later ordinals)', async () => {
    // First ref is a {{template}} (not bare → no proposal); the bare ref is ordinal 1 overall.
    const wt = 'X<ref>{{cite web |url=https://t.org}}</ref> Y<ref>https://bare.org/z</ref>';
    const result = await repairCitationTool.execute(
      { title: 'T', wiki: 'en', wikitext: wt, revision: 1 },
      ctx,
    );
    expect(result.proposedEdits).toHaveLength(1);
    expect(result.proposedEdits![0]!.locator).toEqual({ kind: 'ext-ref', ordinal: 1 });
  });
```

**Step 8: Run the tool tests — watch them PASS**

Run: `npx vitest run packages/tools/src/citation/repair-citation.test.ts`
Expected: PASS (the original three behavior tests + the two new locator tests).

> If an original assertion fails on `before`/`after` exact text, it means the parser's lossless serialization differs from the raw input in some whitespace/attr detail. Reconcile via the Phase 1 `attrs`/round-trip guarantees (the edge is the source of truth for `before`); update the expected string only if the difference is a genuinely-equivalent serialization.

**Step 9: Full suite + CI**

Run: `npm test`
Expected: green across all packages — crucially `packages/agent/src/hitl.ts` tests (improve→approve→commit) and `improve-pipeline.integration.test.ts` still pass: bare-url proposals now carry a locator, and Task 1's schema change preserves it so the confirmation token still binds.

Run: `npm run ci`
Expected: green (incl. coverage ≥90% on `core`/`tools` — the new `classifyBareUrlInner` and the tool branches are covered by the tests above).

**Step 10: Commit**

```bash
git add packages/core/src/citation/repair.ts packages/core/src/citation/repair.test.ts packages/core/src/property.test.ts packages/tools/src/citation/repair-citation.ts packages/tools/src/citation/repair-citation.test.ts
git commit -m "feat(tools): repair_citation detects via the AST edge + emits an ext-ref locator (#9)"
```

---

## Phase 3 Done When

- `npx vitest run packages/core/src/citation/repair.test.ts` and `…/repair-citation.test.ts` pass.
- Proposals carry `locator: { kind: 'ext-ref', ordinal }`, where `ordinal` is the full-list document index (proven by the templated-ref-shift test).
- Diff grounding is unchanged (`evidence[0].grounding` still `{ kind: 'diff', expectedDiffHash }`).
- `npm test` green (including the `agent` improve→commit tests — the locator binds through the commit schema).
- `npm run ci` green.
