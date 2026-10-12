# Wikitext-AST Edit Layer — Phase 4: Node-anchored application in `commit_confirmed_edit`

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Apply a confirmed edit via the **exact AST node** when the proposal carries an `ext-ref` locator — retiring the fail-closed ambiguity guard for those proposals and handling duplicate identical refs precisely. Locator-less proposals keep the existing text-anchored path.

**Architecture — dispatch on the locator (a deliberate, safer reading of the design's "retire the ambiguity guard"):** `commit_confirmed_edit` is the **single generic writer**; besides `repair_citation` it also commits `propose_book_citation`'s `{{cite book}}` proposals, which are text-anchored and carry **no** locator. So we do **not** delete the text-replace path — we **dispatch**:
- **`proposal.locator?.kind === 'ext-ref'`** → `ctx.wikitextParser.replaceRefNode(current.wikitext, wiki, ordinal, before, after)`. This re-parses, re-verifies `String(node) === before` (an additional, strictly-stronger anti-drift gate), and replaces exactly that node. `null` → structured edit-conflict refusal. **No `includes`/ambiguity checks** — duplicate identical refs are disambiguated by ordinal.
- **no locator** → the existing path verbatim: `includes(before)` presence check + the `indexOf !== lastIndexOf` ambiguity guard + first-match `wikitext.replace`. Preserved for book citations and any future text-anchored proposal.

This satisfies the success criterion (two identical bare-URL `<ref>`s now apply precisely) without breaking the locator-less writers. The existing duplicate-`<ref>` test stays green because its proposal has no locator (it now characterizes the **fallback** path); a **new** locator-bearing duplicate test proves precise application.

**Invariants preserved verbatim:** confirmation-token binding (`hashProposedEdit`), diff grounding (`groundingVerifier`), `baseRevision`/`baseTimestamp` TOCTOU, dry-run default, production ban + write-allowlist / test-wiki gates. The dispatch sits **after** the baseRevision guard and **before** `applyEdit`.

**Tech Stack:** TypeScript (strict), Vitest, `InMemoryWikiClient` fake.

**Scope:** Phase 4 of 5. Depends on Phase 3 (the locator field + emitter + preserved-through-schema).

**Codebase verified:** 2026-06-03. The block to replace is `commit-confirmed-edit.ts` lines 163–193 (the `includes` check, the `indexOf !== lastIndexOf` ambiguity guard, and `const newWikitext = current.wikitext.replace(...)`). `WikiEditParams.newWikitext` is the field the fake records in `appliedEdits`.

---

## Task 1: Dispatch the apply on the locator

**Files:**
- Modify: `packages/tools/src/commit-confirmed-edit.ts` (replace the apply block; `newWikitext` becomes a `let` assigned in two branches)
- Modify: `packages/tools/src/commit-confirmed-edit.test.ts` (add node-anchored success + anti-drift + out-of-range tests)

**Step 1: Write the failing node-anchored tests**

In `packages/tools/src/commit-confirmed-edit.test.ts`, add a locator-proposal helper near the existing `proposal()` / `wikiWithArticle()` helpers (top of file, after `wikiWithArticle`):

```typescript
function proposalWithLocator(ordinal: number, wiki = 'test'): ProposedEdit {
  const base: Omit<ProposedEdit, 'hash'> = {
    id: `bare-url-${ordinal}`,
    sink: 'wikipedia',
    target: { title: 'Test', wiki, baseRevision: 10 },
    before: '<ref>https://example.org/x</ref>',
    after: '<ref>{{cite web |url=https://example.org/x |access-date=2026-06-02}}</ref>',
    summary: 'Convert bare URL to {{cite web}} (AI-assisted)',
    evidence: [],
    provenance: { sources: [], createdAt: 0, toolName: 'repair_citation' },
    locator: { kind: 'ext-ref', ordinal },
    rationale: 'r',
  };
  return { ...base, hash: hashProposedEdit(base) };
}
```

Then add these tests inside the `describe('commitConfirmedEditTool …')` block:

```typescript
  test('NODE-ANCHORED: two identical bare-URL refs — applies the located ordinal precisely (no ambiguity refusal)', async () => {
    const p = proposalWithLocator(1); // target the SECOND ref
    const wiki = new InMemoryWikiClient([
      {
        title: p.target.title,
        wiki: p.target.wiki,
        revision: p.target.baseRevision,
        wikitext: `One ${p.before} two ${p.before} three.`, // two IDENTICAL bare-URL refs
      },
    ]);
    const ctx = makeTestContext({ wiki, dryRun: false, writePolicy: { allowedWikis: ['test'] } });
    const result = await commitConfirmedEditTool.execute(
      { proposal: p, confirmationToken: p.hash },
      ctx,
    );
    expect(result.data.committed).toBe(true);
    expect(wiki.appliedEdits).toHaveLength(1);
    // The SECOND ref was converted; the first twin stays bare — precise, not first-match.
    expect(wiki.appliedEdits[0]!.newWikitext).toBe(`One ${p.before} two ${p.after} three.`);
  });

  test('NODE-ANCHORED: ordinal 0 of the identical pair converts the FIRST, leaving the second bare', async () => {
    const p = proposalWithLocator(0);
    const wiki = new InMemoryWikiClient([
      {
        title: p.target.title,
        wiki: p.target.wiki,
        revision: p.target.baseRevision,
        wikitext: `One ${p.before} two ${p.before} three.`,
      },
    ]);
    const ctx = makeTestContext({ wiki, dryRun: false, writePolicy: { allowedWikis: ['test'] } });
    const result = await commitConfirmedEditTool.execute(
      { proposal: p, confirmationToken: p.hash },
      ctx,
    );
    expect(result.data.committed).toBe(true);
    expect(wiki.appliedEdits[0]!.newWikitext).toBe(`One ${p.after} two ${p.before} three.`);
  });

  test('NODE-ANCHORED anti-drift: refuses (no write) when the located node no longer matches the reviewed text', async () => {
    const p = proposalWithLocator(0);
    const wiki = new InMemoryWikiClient([
      {
        title: p.target.title,
        wiki: p.target.wiki,
        revision: p.target.baseRevision,
        // ordinal-0 ref is a DIFFERENT url than the proposal's `before`.
        wikitext: `Drifted <ref>https://example.org/CHANGED</ref> outro.`,
      },
    ]);
    const ctx = makeTestContext({ wiki, dryRun: false, writePolicy: { allowedWikis: ['test'] } });
    const result = await commitConfirmedEditTool.execute(
      { proposal: p, confirmationToken: p.hash },
      ctx,
    );
    expect(result.data.committed).toBe(false);
    expect(result.data.refusedReason).toMatch(/conflict|match|drift/i);
    expect(wiki.appliedEdits).toHaveLength(0);
  });

  test('NODE-ANCHORED out-of-range: refuses (no write) when the locator ordinal exceeds the ref count', async () => {
    const p = proposalWithLocator(5); // only one ref present
    const wiki = new InMemoryWikiClient([
      {
        title: p.target.title,
        wiki: p.target.wiki,
        revision: p.target.baseRevision,
        wikitext: `Only one ${p.before} here.`,
      },
    ]);
    const ctx = makeTestContext({ wiki, dryRun: false, writePolicy: { allowedWikis: ['test'] } });
    const result = await commitConfirmedEditTool.execute(
      { proposal: p, confirmationToken: p.hash },
      ctx,
    );
    expect(result.data.committed).toBe(false);
    expect(wiki.appliedEdits).toHaveLength(0);
  });
```

Also update the **comment** on the existing `'AMBIGUOUS TARGET …'` test (line 129) to note it now characterizes the locator-less fallback (the proposal from `proposal()` has no locator):

```typescript
  // Locator-less proposals (e.g. {{cite book}}) keep the text-anchored fallback: a non-unique
  // `before` is ambiguous, so we fail closed. (Locator-bearing refs are disambiguated above.)
  test('AMBIGUOUS TARGET: refuses (no write) when the before-text occurs more than once', async () => {
```

**Step 2: Run the new tests — watch them FAIL**

Run: `npx vitest run packages/tools/src/commit-confirmed-edit.test.ts -t "NODE-ANCHORED"`
Expected: FAIL — `commit_confirmed_edit` still ignores the locator (it runs the text-replace path; the `NODE-ANCHORED` duplicate test trips the ambiguity guard → `committed: false`).

**Step 3: Replace the apply block with the dispatch**

In `packages/tools/src/commit-confirmed-edit.ts`, replace lines 163–193 — i.e. the `if (!current.wikitext.includes(proposal.before)) { … }` presence check, the `// Ambiguity guard:` comment + its `if (current.wikitext.indexOf(...) !== current.wikitext.lastIndexOf(...)) { … }` block, and the `const newWikitext = current.wikitext.replace(proposal.before, proposal.after);` line — with the following:

```typescript
    // Apply the confirmed change. Node-anchored when the proposal carries an ext-ref locator
    // (#9, `adr/0009`): replace the EXACT ordinal-th <ref>, re-verifying its current text ==
    // proposal.before — duplicate identical refs are disambiguated precisely, no ambiguity
    // guard needed. Otherwise (e.g. {{cite book}} proposals) keep the text-anchored fallback.
    let newWikitext: string;
    if (proposal.locator?.kind === 'ext-ref') {
      const replaced = ctx.wikitextParser.replaceRefNode(
        current.wikitext,
        proposal.target.wiki,
        proposal.locator.ordinal,
        proposal.before,
        proposal.after,
      );
      if (replaced === null) {
        return {
          data: {
            committed: false,
            refusedReason:
              'edit conflict: the located <ref> no longer matches the reviewed text (node drift or out of range)',
          },
          provenance: baseProvenance,
          rationale:
            'Refused: the node-anchored target no longer matches the reviewed wikitext (edit conflict); re-review required.',
        };
      }
      newWikitext = replaced;
    } else {
      if (!current.wikitext.includes(proposal.before)) {
        return {
          data: {
            committed: false,
            refusedReason: 'the original text was not found (edit conflict)',
          },
          provenance: baseProvenance,
          rationale:
            'Refused: the "before" text is no longer present in the article (stale proposal).',
        };
      }
      // Ambiguity guard (text-anchored fallback): `.replace` rewrites only the FIRST occurrence,
      // and a locator-less proposal binds to the before/after TEXT, not a position — so if the
      // fragment is non-unique we cannot know which the human meant. Fail closed rather than
      // silently rewrite the wrong one. (Locator-bearing proposals are node-anchored above.)
      if (
        current.wikitext.indexOf(proposal.before) !== current.wikitext.lastIndexOf(proposal.before)
      ) {
        return {
          data: {
            committed: false,
            refusedReason:
              'ambiguous target: the "before" text occurs more than once; cannot determine which instance to edit',
          },
          provenance: baseProvenance,
          rationale:
            'Refused: the before-text is not unique in the article, so the intended occurrence is ambiguous (a position-anchored proposal is required).',
        };
      }
      newWikitext = current.wikitext.replace(proposal.before, proposal.after);
    }
```

The subsequent `const result = await ctx.wiki.applyEdit({ … newWikitext … })` call is unchanged and now consumes the `newWikitext` set by whichever branch ran.

**Step 4: Run the full commit test file — watch everything PASS**

Run: `npx vitest run packages/tools/src/commit-confirmed-edit.test.ts`
Expected: PASS — the four new `NODE-ANCHORED` tests **and** every pre-existing test (dry-run, allowlist apply, kill switch, baseTimestamp, edit conflict, production ban, the **locator-less ambiguity** refusal, token binding, tamper). The locator-less tests exercise the fallback branch unchanged.

**Step 5: Typecheck + full suite + CI**

Run: `npm run typecheck`
Expected: clean (`newWikitext` is definitely assigned on every path that reaches `applyEdit`).

Run: `npm test`
Expected: green across all packages.

Run: `npm run ci`
Expected: green — coverage ≥90% on `tools` holds (both dispatch branches and the refusal paths are covered by Step 1's tests plus the retained fallback tests).

**Step 6: Commit**

```bash
git add packages/tools/src/commit-confirmed-edit.ts packages/tools/src/commit-confirmed-edit.test.ts
git commit -m "feat(tools): node-anchored commit via locator; retire ambiguity guard for located refs (#9)"
```

---

## Phase 4 Done When

- A page with **two identical bare-URL `<ref>`s** + a locator-bearing proposal applies the correct one precisely (the success criterion); ordinal 0 vs 1 select the right twin.
- Node-anchored anti-drift (`String(node) !== before`) and out-of-range ordinals refuse with no write.
- Locator-less proposals keep the text-anchored fallback (presence check + ambiguity guard) — the existing duplicate-`<ref>` test still refuses; `{{cite book}}` commits unaffected.
- All invariant tests (token binding, `baseRevision`/`baseTimestamp`, dry-run, production ban, write-allowlist) green.
- `npm run ci` green.
