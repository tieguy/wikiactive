# Wikitext-AST Edit Layer — Phase 5: Integration, CI, and ADR reconciliation

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Prove the whole path end-to-end hermetically (`improve` → bare-URL `ProposedEdit` with locator → `commit_confirmed_edit` applies node-anchored through the gated write path), confirm the CI Node engine satisfies `wikiparser-node`, and reconcile `adr/0009` to **Built**.

**Architecture:** A sibling integration test in `packages/agent/src/` mirrors `improve-pipeline.integration.test.ts`: seed the in-memory fake wiki, drive `SessionService.improve(...)` over a `StubHttpClient` (no network), approve a proposal via `decide(...)`, and assert the node-anchored apply landed precisely on the in-memory fake (the only writer ever wired in tests). The article carries **two identical bare-URL `<ref>`s** so the test exercises the headline success criterion through the full stack.

**Tech Stack:** TypeScript (strict), Vitest, `InMemoryWikiClient` + `StubHttpClient` fakes; Markdown docs for the ADR.

**Scope:** Phase 5 of 5. Depends on Phases 1–4.

**Codebase verified:** 2026-06-03. `improve()` loads wikitext via the Action API (`getWikitextTool`, over HTTP) and the parsed article via REST (`getArticleTool`) **tolerantly** (a REST failure audits `article_load_failed` and degrades to zero citations — a bare-URL proposal needs only wikitext). `decide(sessionId, proposalId, { decision:'approve', confirmationToken })` runs the confirm→commit step using the session ctx (so `dryRun:false` + the write allowlist apply). `adr/0009` is `docs/adr/0009-wikitext-manipulation-via-ast.md`.

---

## Task 1: Hermetic end-to-end integration test

**Files:**
- Create: `packages/agent/src/wikitext-ast.integration.test.ts`

**Step 1: Write the test**

Create `packages/agent/src/wikitext-ast.integration.test.ts`:

```typescript
import { describe, expect, test } from 'vitest';
import {
  InMemoryStorage,
  InMemoryWikiClient,
  StubHttpClient,
  makeTestContext,
} from '@wikiharness/edges';
import { bareUrlRepairEnhancer } from '@wikiharness/enhancers';
import type { Policy } from '@wikiharness/policy';
import { SessionService } from './hitl.ts';

const POLICY: Policy = {
  id: 'citation-verification',
  tier: 'suggested',
  sinks: ['wikipedia'],
  requireDisclosure: true,
  requireGroundedEvidence: true,
  maxChangedChars: 2000,
};

// Two IDENTICAL bare-URL <ref>s — the case that fails closed without the locator.
const BARE = '<ref>https://example.org/x</ref>';
const WIKITEXT = `Intro.${BARE} Middle.${BARE} End.`;

function actionApiBody(): string {
  return JSON.stringify({
    query: {
      pages: [
        { title: 'Demo', revisions: [{ revid: 100, slots: { main: { content: WIKITEXT } } }] },
      ],
    },
  });
}

// One HTTP double: the Action-API wikitext read. REST (parsed article) 404s → improve()
// degrades tolerantly (bare-URL repair needs only wikitext). No network is ever touched.
function http(): StubHttpClient {
  return new StubHttpClient((req) => {
    if (req.url.includes('action=query'))
      return { status: 200, headers: {}, body: actionApiBody() };
    return { status: 404, headers: {}, body: '' };
  });
}

describe('wikitext-AST end-to-end: node-anchored bare-URL repair (#9)', () => {
  test('improve → locator proposal → decide(approve) applies the EXACT ref via the gated path', async () => {
    const wiki = new InMemoryWikiClient([
      { title: 'Demo', wiki: 'test', revision: 100, wikitext: WIKITEXT },
    ]);
    // dryRun off + test-wiki allowlist → the confirm step actually applies to the fake wiki.
    const ctx = makeTestContext({
      http: http(),
      wiki,
      storage: new InMemoryStorage(),
      dryRun: false,
      writePolicy: { allowedWikis: ['test'] },
    });
    const svc = new SessionService(ctx, POLICY, { enhancers: [bareUrlRepairEnhancer] });

    const session = await svc.improve({ title: 'Demo', wiki: 'test' });

    // Both bare refs surfaced, each with its document-order ext-ref locator.
    expect(session.proposals).toHaveLength(2);
    expect(session.proposals.map((r) => r.proposal.locator)).toEqual(
      expect.arrayContaining([
        { kind: 'ext-ref', ordinal: 0 },
        { kind: 'ext-ref', ordinal: 1 },
      ]),
    );

    // Approve the SECOND ref (ordinal 1).
    const second = session.proposals.find((r) => r.proposal.locator?.ordinal === 1)!;
    const result = await svc.decide(session.sessionId, second.proposal.id, {
      decision: 'approve',
      confirmationToken: second.proposal.hash,
    });

    expect(result.committed).toBe(true);
    expect(wiki.appliedEdits).toHaveLength(1);
    // Node-anchored precision through the full stack: ONLY the second twin was converted.
    expect(wiki.appliedEdits[0]!.newWikitext).toBe(
      `Intro.${BARE} Middle.${second.proposal.after} End.`,
    );
  });

  test('the absolute invariant holds under dry-run: improve proposes, nothing is written', async () => {
    const wiki = new InMemoryWikiClient([
      { title: 'Demo', wiki: 'test', revision: 100, wikitext: WIKITEXT },
    ]);
    const ctx = makeTestContext({ http: http(), wiki, storage: new InMemoryStorage() }); // dryRun defaults true
    const svc = new SessionService(ctx, POLICY, { enhancers: [bareUrlRepairEnhancer] });

    const session = await svc.improve({ title: 'Demo', wiki: 'test' });
    expect(session.proposals.length).toBeGreaterThan(0);
    expect(wiki.appliedEdits).toHaveLength(0);
  });
});
```

**Step 2: Run it**

Run: `npx vitest run packages/agent/src/wikitext-ast.integration.test.ts`
Expected: PASS — both refs proposed with locators, the second twin converted precisely (the first stays bare), one applied edit on the fake.

> If `decide`'s result shape differs (e.g. the committed flag is nested), read `packages/agent/src/hitl.ts`'s `decide` return type and adjust the assertion (`result.committed`) to match — the existing `hitl.test.ts` uses `result.committed`, so this should line up. If `improve` surfaces fewer than 2 proposals, confirm the `actionApiBody` content reached `getWikitextTool` (the URL must contain `action=query`).

**Step 3: Commit**

```bash
git add packages/agent/src/wikitext-ast.integration.test.ts
git commit -m "test(agent): hermetic end-to-end node-anchored bare-URL repair (#9)"
```

---

## Task 2: Confirm the CI Node engine

The Node-engine tightening already happened in **Phase 1, Task 2** (root `package.json` → `"^20.19.0 || ^22.13.0 || >=24.11.0"`), because that is where the `wikiparser-node` dependency was introduced. This task confirms it and records that there is no separate CI workflow to bump.

**Files:** none (verification only).

**Step 1: Confirm the engine field is tightened**

Run: `node -e "console.log(require('./package.json').engines.node)"`
Expected: `^20.19.0 || ^22.13.0 || >=24.11.0`

**Step 2: Confirm the running Node satisfies it**

Run: `node -p "process.version"`
Expected: a version satisfying the range (local dev is on Node 26 → `>=24.11.0` ✓).

**Step 3: Confirm there is no CI workflow Node pin to update**

Run: `ls .github/workflows 2>/dev/null || echo "no workflows dir"`
Expected: `no workflows dir` (CI is `npm run ci` run locally/pre-merge; there is no GitHub Actions matrix to bump). Note this in the ADR update (Task 3). If a workflows dir DOES exist by execution time, set its `node-version` to `24` (or a value satisfying the range) and commit.

(No commit unless a workflow file needed editing.)

---

## Task 3: Reconcile `adr/0009` to Built

**Files:**
- Modify: `docs/adr/0009-wikitext-manipulation-via-ast.md`

**Step 1: Update the Status line**

Replace the Status block (lines 3–5):

```
Status: **Accepted** — owner-prompted 2026-06-02. The audit's "evaluate-spike" on
`wikiparser-node` is resolved: adopt a wikitext-AST layer as the foundation for wikitext
manipulation, but **defer the build to its first real consumer** (YAGNI).
```

with:

```
Status: **Accepted + Built** — accepted (owner-prompted) 2026-06-02; **built 2026-06-03 (#9)**.
The audit's "evaluate-spike" on `wikiparser-node` is resolved: a wikitext-AST layer is the
foundation for wikitext manipulation. The first real consumer arrived — making `repair_citation`
detection and `commit_confirmed_edit` application node-anchored — so the deferred build (YAGNI)
was done as #9, ahead of the M6 prose enhancer.
```

**Step 2: Correct the license note**

Replace the License note (lines 52–53):

```
- License note: `wikiparser-node` is GPL-family/permissive (confirm at adoption); the owner
  is comfortable with copyleft (cf. `adr/0008` on mwn's LGPL).
```

with:

```
- License note: `wikiparser-node` is **GPL-3.0-or-later** (strong copyleft) — confirmed at
  adoption (v1.43.0, 2026-06-03). The owner is comfortable with copyleft (cf. `adr/0008` on
  mwn's LGPL).
```

**Step 3: Append a "Built" section**

Add the following section at the end of the file (after the Consequences list):

```markdown
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
```

**Step 4: Verify cross-references and formatting**

Run: `npm run format`
Expected: clean (or run `npm run format:write` and re-stage if prettier reflows the Markdown).

**Step 5: Commit**

```bash
git add docs/adr/0009-wikitext-manipulation-via-ast.md
git commit -m "docs(adr): reconcile adr/0009 to Built — GPL-3.0-or-later + node-anchored edit (#9)"
```

---

## Task 4: Final CI gate

**Files:** none (verification only).

**Step 1: Run the full gate**

Run: `npm run ci`
Expected: green — format, typecheck, lint, boundaries, stubscan, coverage (≥90% on `core`/`tools`), eval. This is the slice's exit gate: hermetic (no test touches the network; the only writer wired in tests is the in-memory fake).

**Step 2: Closeout note (owner's call)**

This project applies an ADR-0007 stage-end adversarial bug-hunt before declaring a milestone done. The per-task `code-reviewer` runs (via the execute-implementation-plan skill) cover incremental review; whether to run a broader stage-end bug-hunt on this slice is the owner's call. If run and it surfaces Critical/Important findings, fix them and re-run `npm run ci` before closing #9.

(No commit — verification only.)

---

## Phase 5 Done When

- `npx vitest run packages/agent/src/wikitext-ast.integration.test.ts` passes — the end-to-end node-anchored apply lands precisely on the second of two identical refs; dry-run writes nothing.
- The root Node engine satisfies `wikiparser-node` (confirmed); no CI workflow needed bumping.
- `adr/0009` reflects the **Built** state with the confirmed **GPL-3.0-or-later** license and the spike evidence.
- `npm run ci` is green.
- Nothing has written to a real wiki (the in-memory fake remains the only writer).
