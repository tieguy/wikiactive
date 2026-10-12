# Wikitext-AST Edit Layer — Phase 1: The `WikitextParser` edge

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** A working, unit-tested wikitext-parser edge over `wikiparser-node`, standalone — the `core` interface plus its `edges` implementation. Nothing consumes it yet.

**Architecture:** Mirror the existing `HtmlExtractor` edge: a pure interface in `packages/core/src/edges.ts` (`WikitextParser` + `WikitextRef`), implemented by a class in `packages/edges/src/`. The third-party `wikiparser-node` library carries **global parser state** (`Parser.config`) and lazily loads its site config from the **filesystem** — both forbidden in pure `core`. That impurity is isolated inside the edge: each method sets `Parser.config` then parses **synchronously** (no `await` in between), so concurrent async callers cannot interleave at the config boundary.

**Tech Stack:** TypeScript (ESM, `moduleResolution: Bundler`, `.ts` import extensions, `strict`, `noUncheckedIndexedAccess`), Vitest, npm workspaces, `wikiparser-node` v1.43.0 (GPL-3.0-or-later).

**Scope:** Phase 1 of 5 from `docs/design-plans/2026-06-03-wikitext-ast-edit-layer.md`.

**Codebase verified:** 2026-06-03 (codebase-investigator + remote-code-researcher on `wikiparser-node`).

---

## Library facts (verified against `wikiparser-node` v1.43.0 source — use these, don't re-derive)

- **Import:** `import Parser from 'wikiparser-node';` (default export). The package ships compiled `dist/` with types; bare-name import works under `moduleResolution: Bundler` exactly like `defuddle`/`linkedom`/`mwn`.
- **Config:** `Parser.config` is a **global string property**. Set it to a site id (`Parser.config = 'enwiki'`) before parsing. Setting it triggers an internal **synchronous** filesystem load of the bundled config. `enwiki` is a bundled config; `default` exists as a fallback.
- **Parse:** `Parser.parse(wikitext)` is **synchronous** and returns the root AST node.
- **Query refs:** `root.querySelectorAll('ext#ref')` returns the `<ref>…</ref>` ext-tag nodes in **document order** (depth-first). `ordinal === array index`.
- **Read a ref node:** `String(node)` (or `node.toString()`) → the node's **full** wikitext (lossless). `node.firstChild` is the attributes node (`String(node.firstChild)` → e.g. `' name="n"'` or `''`). `node.lastChild` is the inner-body node (`String(node.lastChild)` → e.g. `'https://a/x'`).
- **Replace one node losslessly:** parse the replacement text, grab its `ext#ref` node, then `target.safeReplaceWith(replacement)`. `safeReplaceWith` requires the two nodes be the same token type (both `<ref>` ext-tags — satisfied here). Re-serialize with `String(root)`; **unedited nodes are byte-preserved**.
- **Engines:** `wikiparser-node` requires Node `^20.19.0 || ^22.13.0 || >=24.11.0`. License: `GPL-3.0-or-later`.

> **Typecheck note for the executor:** `wikiparser-node` ships its own `.d.ts`. The exact generic form of `querySelectorAll`/`querySelector` and the static types of `firstChild`/`lastChild`/`safeReplaceWith` are the library's, not ours. Write the runtime logic exactly as below (it is spike-confirmed), then run `npm run typecheck` and add the minimal annotations/casts the shipped types require (e.g. `querySelectorAll<…>('ext#ref')` or a local `as` cast). Do **not** change the runtime behavior to satisfy types.

---

## Task 1: Add the `WikitextParser` + `WikitextRef` interface to `core`

**Files:**
- Modify: `packages/core/src/edges.ts` (insert after the `HtmlExtractor` block that ends at line 180, before the `Clock` block at line 182)

This is a types-only change (no runtime), so there is no test for it directly — Task 2's edge implementation `implements WikitextParser`, which is the compile-time proof the interface is right.

**Step 1: Insert the interface block**

Insert the following immediately after the `HtmlExtractor` interface (after line 180) and before the `// ── Clock ──` comment:

```typescript
// ── WikitextParser ───────────────────────────────────────────────────────────
// Structured wikitext editing (#9, `adr/0009`): enumerate <ref> nodes and replace
// one by ordinal, node-anchored (not string `.replace`). An edge, not pure — the real
// impl (`wikiparser-node`) carries global parser state + fs-config loading, both
// forbidden in core. The wrapper in `edges` isolates that impurity; core sees only
// these signatures.

/** One `<ref>…</ref>` node, located by its document-order ordinal at a fixed revision. */
export interface WikitextRef {
  /** Index among <ref> nodes in document order — the stable locator at a fixed revision. */
  ordinal: number;
  /** The node's full wikitext, e.g. '<ref name="n">https://a/x</ref>'. */
  text: string;
  /** The inner body, e.g. 'https://a/x' — the input to pure bare-URL classification. */
  inner: string;
  /** The opening-tag attributes, e.g. ' name="n"' (empty string if none). */
  attrs: string;
}

export interface WikitextParser {
  /** Enumerate <ref> nodes in document order; `ordinal` equals the array index. */
  findRefs(wikitext: string, wiki: string): WikitextRef[];
  /**
   * Replace the `ordinal`-th <ref> with `newNodeText`, but ONLY if that node's current
   * wikitext === `expectedText` (anti-drift re-grounding at apply time). Returns the new
   * full wikitext, or `null` if the ordinal is out of range or the node text no longer matches.
   */
  replaceRefNode(
    wikitext: string,
    wiki: string,
    ordinal: number,
    expectedText: string,
    newNodeText: string,
  ): string | null;
}
```

**Step 2: Verify it typechecks and exports**

`packages/core/src/index.ts` already does `export * from './edges.ts'`, so the new interfaces are part of `@wikiharness/core`'s surface automatically — no index edit needed.

Run: `npm run typecheck`
Expected: clean (no errors).

**Step 3: Commit**

```bash
git add packages/core/src/edges.ts
git commit -m "feat(core): add WikitextParser + WikitextRef edge interface (#9)"
```

---

## Task 2: Add `wikiparser-node` as a dependency and bump the Node engine

**Files:**
- Modify: `packages/edges/package.json` (add to `dependencies`)
- Modify: `package.json` (root — tighten `engines.node`)

> **Note on phase placement:** the design assigns the engine bump to Phase 5. We do it here instead, in the same task that introduces the dependency, because `wikiparser-node`'s own `engines` (`^20.19 || ^22.13 || >=24.11`) is stricter than the repo's current `>=22`, and `npm install` would emit `EBADENGINE` warnings otherwise. Phase 5 still confirms the engine and records it in `adr/0009`.

**Step 1: Add the dependency**

In `packages/edges/package.json`, add `wikiparser-node` to the `dependencies` object (keep alphabetical-ish ordering consistent with the file). The block currently reads:

```json
  "dependencies": {
    "@wikiharness/core": "*",
    "ai": "^5.0.0",
    "defuddle": "^0.18.1",
    "linkedom": "^0.18.12"
  },
```

Change it to:

```json
  "dependencies": {
    "@wikiharness/core": "*",
    "ai": "^5.0.0",
    "defuddle": "^0.18.1",
    "linkedom": "^0.18.12",
    "wikiparser-node": "^1.43.0"
  },
```

**Step 2: Tighten the root Node engine**

In the root `package.json`, the `engines` block currently reads:

```json
  "engines": {
    "node": ">=22"
  },
```

Change it to:

```json
  "engines": {
    "node": "^20.19.0 || ^22.13.0 || >=24.11.0"
  },
```

**Step 3: Install**

Run: `npm install`
Expected: installs `wikiparser-node` without errors and without `EBADENGINE` warnings (local dev Node is 26, which satisfies the range).

**Step 4: Commit**

```bash
git add package.json package-lock.json packages/edges/package.json
git commit -m "build(edges): add wikiparser-node dep; align Node engine to its range (#9)"
```

---

## Task 3: Implement the `WikiparserNodeParser` wrapper

**Files:**
- Create: `packages/edges/src/wikitext-parser.ts`
- Modify: `packages/edges/src/index.ts` (add the export)

**Step 1: Create the wrapper**

Create `packages/edges/src/wikitext-parser.ts`:

```typescript
/**
 * The real {@link WikitextParser} edge (#9, `adr/0009`): a thin wrapper over
 * **wikiparser-node** (GPL-3.0-or-later) that parses wikitext into an AST, enumerates
 * `<ref>` extension tags in document order, and replaces exactly one of them losslessly.
 *
 * wikiparser-node carries GLOBAL parser state (`Parser.config`) and lazily loads its
 * site config from the filesystem — both forbidden in pure `core`. That impurity is
 * isolated HERE: each method sets `Parser.config` and parses SYNCHRONOUSLY (no `await`
 * in between), so concurrent async callers cannot interleave at the config boundary.
 */
import Parser from 'wikiparser-node';
import type { WikitextParser, WikitextRef } from '@wikiharness/core';

/** Map a wikiharness wiki code to a wikiparser-node site config; default to the English config. */
const WIKI_CONFIG: Record<string, string> = { en: 'enwiki' };

function configFor(wiki: string): string {
  return WIKI_CONFIG[wiki] ?? 'enwiki';
}

export class WikiparserNodeParser implements WikitextParser {
  findRefs(wikitext: string, wiki: string): WikitextRef[] {
    Parser.config = configFor(wiki);
    const root = Parser.parse(wikitext);
    return root.querySelectorAll('ext#ref').map((node, ordinal) => ({
      ordinal,
      text: String(node),
      inner: String(node.lastChild),
      attrs: String(node.firstChild),
    }));
  }

  replaceRefNode(
    wikitext: string,
    wiki: string,
    ordinal: number,
    expectedText: string,
    newNodeText: string,
  ): string | null {
    Parser.config = configFor(wiki);
    const root = Parser.parse(wikitext);
    const target = root.querySelectorAll('ext#ref')[ordinal];
    if (!target) return null; // ordinal out of range
    if (String(target) !== expectedText) return null; // anti-drift: the node moved/changed
    try {
      const replacement = Parser.parse(newNodeText).querySelector('ext#ref');
      if (!replacement) return null; // newNodeText is not a single <ref> node
      target.safeReplaceWith(replacement);
    } catch {
      return null; // a malformed replacement / type mismatch is a refusal, never a throw
    }
    return String(root);
  }
}
```

**Step 2: Export it**

In `packages/edges/src/index.ts`, add the export alongside `html-extractor.ts` (place it after the `./html-extractor.ts` line):

```typescript
export * from './wikitext-parser.ts';
```

**Step 3: Typecheck**

Run: `npm run typecheck`
Expected: clean. If the shipped `wikiparser-node` types reject `querySelectorAll('ext#ref')` without a generic, or `firstChild`/`lastChild`/`safeReplaceWith` need a narrower type, add the **minimal** annotation/cast the compiler asks for — do not change runtime behavior. (See the "Typecheck note" at the top.)

**Step 4: Commit**

```bash
git add packages/edges/src/wikitext-parser.ts packages/edges/src/index.ts
git commit -m "feat(edges): WikiparserNodeParser wrapper over wikiparser-node (#9)"
```

---

## Task 4: Unit-test the edge

**Files:**
- Create: `packages/edges/src/wikitext-parser.test.ts`

This is a functionality task: the test is the deliverable that proves the wrapper. The edge is offline/deterministic and uses the **real** library (hermetic — no network), matching how `html-extractor.test.ts`'s "REAL defuddle+linkedom" test runs the real deps.

**Step 1: Write the test**

Create `packages/edges/src/wikitext-parser.test.ts`:

```typescript
import { describe, expect, test } from 'vitest';
import { WikiparserNodeParser } from './wikitext-parser.ts';

const parser = new WikiparserNodeParser();

// Two refs: one bare, one named — exercises ordinals, inner, and attrs in one document.
const WT = 'A.<ref>https://a.org/x</ref> B.<ref name="b">https://b.org/y</ref> C.';

describe('WikiparserNodeParser.findRefs', () => {
  test('enumerates <ref> nodes in document order with stable ordinals', () => {
    const refs = parser.findRefs(WT, 'en');
    expect(refs.map((r) => r.ordinal)).toEqual([0, 1]);
    expect(refs[0]!.text).toBe('<ref>https://a.org/x</ref>');
    expect(refs[1]!.text).toBe('<ref name="b">https://b.org/y</ref>');
  });

  test('exposes the inner body and the opening-tag attrs', () => {
    const refs = parser.findRefs(WT, 'en');
    expect(refs[0]!.inner).toBe('https://a.org/x');
    expect(refs[0]!.attrs).toBe(''); // no attributes
    expect(refs[1]!.inner).toBe('https://b.org/y');
    expect(refs[1]!.attrs).toBe(' name="b"'); // leading space, matches the BareUrlRef.attrs contract
  });

  test('a ref whose body is a {{template}} is still enumerated (classification is core, not the edge)', () => {
    const refs = parser.findRefs('x<ref>{{cite web |url=https://z.org}}</ref>', 'en');
    expect(refs).toHaveLength(1);
    expect(refs[0]!.inner).toBe('{{cite web |url=https://z.org}}');
  });
});

describe('WikiparserNodeParser.replaceRefNode', () => {
  test('replacing a node with its own text is a lossless no-op (round-trip)', () => {
    const ref0 = parser.findRefs(WT, 'en')[0]!;
    expect(parser.replaceRefNode(WT, 'en', 0, ref0.text, ref0.text)).toBe(WT);
  });

  test('replaces exactly the ordinal-th node and serializes losslessly', () => {
    const out = parser.replaceRefNode(
      WT,
      'en',
      0,
      '<ref>https://a.org/x</ref>',
      '<ref>{{cite web |url=https://a.org/x}}</ref>',
    );
    expect(out).toBe(
      'A.<ref>{{cite web |url=https://a.org/x}}</ref> B.<ref name="b">https://b.org/y</ref> C.',
    );
  });

  test('returns null when expectedText no longer matches the node (anti-drift)', () => {
    expect(
      parser.replaceRefNode(WT, 'en', 0, '<ref>https://DRIFTED.org</ref>', '<ref>x</ref>'),
    ).toBeNull();
  });

  test('returns null when the ordinal is out of range', () => {
    expect(parser.replaceRefNode(WT, 'en', 9, '<ref>x</ref>', '<ref>y</ref>')).toBeNull();
  });

  test('TWO IDENTICAL <ref>s: replaces precisely the ordinal-th, leaving its twin untouched', () => {
    const dup = 'One <ref>https://x/y</ref> two <ref>https://x/y</ref> three.';
    const before = '<ref>https://x/y</ref>';
    const after = '<ref>{{cite web |url=https://x/y}}</ref>';

    // Replace the SECOND (ordinal 1) → first twin stays bare.
    expect(parser.replaceRefNode(dup, 'en', 1, before, after)).toBe(
      'One <ref>https://x/y</ref> two <ref>{{cite web |url=https://x/y}}</ref> three.',
    );
    // Replace the FIRST (ordinal 0) → second twin stays bare.
    expect(parser.replaceRefNode(dup, 'en', 0, before, after)).toBe(
      'One <ref>{{cite web |url=https://x/y}}</ref> two <ref>https://x/y</ref> three.',
    );
  });
});
```

**Step 2: Run the test and verify it passes**

Run: `npx vitest run packages/edges/src/wikitext-parser.test.ts`
Expected: all tests PASS.

> **If `attrs` / serialization assertions fail:** the library's exact serialization of the attributes node or whitespace differs from the contract. Two allowed fixes, in order of preference:
> 1. Adjust `WikiparserNodeParser` so `attrs` satisfies the contract `<ref${attrs}>` reconstructs the original opening tag (e.g. normalize to a leading single space when non-empty). The `attrs` form **must** match the current `BareUrlRef.attrs` convention (`' name="src1"'` with a leading space, `''` when none) because Phase 3's `wrapBareUrl` depends on it.
> 2. Only if the library genuinely emits a different-but-equivalent form, update the expected string in the test — but keep the round-trip and disambiguation assertions exact.

**Step 3: Verify boundaries + full CI**

Run: `npm run boundaries`
Expected: green — `edges` imports only `core` (+ the third-party `wikiparser-node`).

Run: `npm run ci`
Expected: green (format, typecheck, lint, boundaries, stubscan, coverage, eval). The wrapper lives in `edges`, which is **not** under the ≥90% coverage threshold (that applies to `core`/`tools` only), but it is fully covered by Task 4 regardless.

> **stub-scan note:** the stub scanner flags the exact tokens `TODO`, `FIXME`, `XXX`, `HACK` (word-boundary, case-sensitive) and `not implemented`/`unimplemented` (case-insensitive). None appear in the wrapper or tests above — keep it that way.

**Step 4: Commit**

```bash
git add packages/edges/src/wikitext-parser.test.ts
git commit -m "test(edges): WikiparserNodeParser — order, inner/attrs, node-anchored replace, dup disambiguation (#9)"
```

---

## Phase 1 Done When

- `npm install` succeeds (Task 2).
- `npx vitest run packages/edges/src/wikitext-parser.test.ts` — all pass, including the two-identical-`<ref>`s disambiguation and the lossless round-trip.
- `npm run boundaries` green (`edges → core`).
- `npm run ci` green.
- The `WikitextParser`/`WikitextRef` interface is exported from `@wikiharness/core`; nothing consumes it yet (Phase 2 wires it in).
