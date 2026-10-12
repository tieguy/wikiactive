# Phase 6: End-to-End Integration + CI Gates — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `ed3d-plan-and-execute:executing-an-implementation-plan`.

**Goal:** Prove the whole path hermetically: `improve()` over the REAL recorded article, with the
full registry, **populates both output channels** (`ProposedEdit`s from `bare-url-repair`,
`Finding`s from `citation-verify`) and **writes nothing** (`appliedEdits === 0`); a hallucinated
finding is **suppressed** end-to-end; and the 0-hallucination eval gate's semantics cover findings.
`npm run ci` green.

**Architecture:** A new sibling integration test drives `SessionService.improve()` with
`ScriptedModelClient`/a constant-verdict model + the recorded REST cassette + a stubbed Action API +
canned sources. The import boundaries (incl. the `agent → enhancers` edge added in Phase 3) stay
green — the design's "add `enhancers → [core, tools]`" was already satisfied (it is the broader
`enhancers → [core, tools, outputs, policy, hooks]`).

**Tech Stack:** TypeScript (NodeNext ESM), Vitest 3.

**Scope:** Phase 6 of 6. **Depends on Phases 1–5.**

**Codebase verified:** 2026-06-03 (`mvp1-pipeline.integration.test.ts` uses the cassette +
`InMemoryWikiClient.appliedEdits`; `antiHallucinationGate` scores `CaseOutcome.hallucinated`;
`LAYER_RULE` already has `enhancers` + `agent → enhancers` from Phase 3).

---

## Task 1: End-to-end `improve()` integration test (both channels + zero writes + suppression)

**Files:**
- Create: `packages/agent/src/improve-pipeline.integration.test.ts`

**Step 1: Write the test**

```ts
import { describe, expect, test } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  InMemoryStorage,
  InMemoryWikiClient,
  StubHttpClient,
  makeTestContext,
  type Cassette,
} from '@wikiharness/edges';
import { bareUrlRepairEnhancer, citationVerifyEnhancer } from '@wikiharness/enhancers';
import type { ModelClient, ModelResponse } from '@wikiharness/core';
import type { Policy } from '@wikiharness/policy';
import { SessionService } from './hitl.ts';

const cassette = JSON.parse(
  readFileSync(join(import.meta.dirname, '../../../fixtures/en-Markdown.rest-html.cassette.json'), 'utf8'),
) as Cassette;

const POLICY: Policy = {
  id: 'citation-verification',
  tier: 'suggested',
  sinks: ['wikipedia'],
  requireDisclosure: true,
  requireGroundedEvidence: true,
  maxChangedChars: 2000,
};

// Wikitext with a bare URL → bare-url-repair surfaces a ProposedEdit.
const WIKITEXT = 'Markdown is a lightweight markup language.<ref>https://example.org/bare-cite</ref>';

// A usable (non-short) source so the body-usability gate passes and the model is actually called.
const SOURCE_HTML =
  '<html><body><p>Markdown is a lightweight markup language for creating formatted text using a ' +
  'plain-text editor. John Gruber and Aaron Swartz created Markdown in 2004 as a markup language ' +
  'that is appealing to human readers in its source code form. It is widely used for blogging, ' +
  'instant messaging, online forums, documentation pages, and readme files.</p></body></html>';

function actionApiBody(): string {
  return JSON.stringify({
    query: { pages: [{ title: 'Markdown', revisions: [{ revid: 100, slots: { main: { content: WIKITEXT } } }] }] },
  });
}

// One HTTP double for all three reads: REST article (cassette), Action-API wikitext, canned sources.
function http(): StubHttpClient {
  return new StubHttpClient((req) => {
    if (req.url.includes('/api/rest_v1/page/html/')) return cassette.entries[0]!.response;
    if (req.url.includes('action=query')) return { status: 200, headers: {}, body: actionApiBody() };
    return { status: 200, headers: { 'content-type': 'text/html' }, body: SOURCE_HTML };
  });
}

/**
 * A constant-verdict model: the real Markdown article has dozens of citations (one model call each),
 * so a fixed-length ScriptedModelClient script would be brittle. This returns the same verdict for
 * every call — exactly what an end-to-end "both channels populate" / "all hallucinations suppressed"
 * check needs.
 */
class ConstantModelClient implements ModelClient {
  constructor(private readonly response: ModelResponse) {}
  async complete(): Promise<ModelResponse> {
    return this.response;
  }
}
const verdict = (body: object): ModelResponse => ({ text: JSON.stringify(body), toolCalls: [], finishReason: 'stop' });

const CAPS = new Set<'model' | 'htmlExtractor'>(['model', 'htmlExtractor']);

describe('improve() end-to-end over the recorded article (#7/#13)', () => {
  test('BOTH channels populate (bare-url proposals + citation-verify findings) and NOTHING is written', async () => {
    const wiki = new InMemoryWikiClient();
    const ctx = makeTestContext({
      http: http(),
      wiki,
      storage: new InMemoryStorage(),
      // NOT_SUPPORTED grounds on source-fetched → every citation-verify finding surfaces.
      model: new ConstantModelClient(verdict({ verdict: 'NOT_SUPPORTED' })),
    });
    const svc = new SessionService(ctx, POLICY, {
      capabilities: CAPS,
      enhancers: [bareUrlRepairEnhancer, citationVerifyEnhancer],
    });

    const session = await svc.improve({ title: 'Markdown', wiki: 'en' });

    expect(session.proposals.length).toBeGreaterThan(0); // bare-url-repair (edit channel)
    expect(session.findings.length).toBeGreaterThan(0); // citation-verify (finding channel)
    expect(session.findings.every((r) => r.finding.kind === 'citation-verdict')).toBe(true);

    // THE ABSOLUTE INVARIANT, end to end: nothing reached a wiki.
    expect(wiki.appliedEdits).toHaveLength(0);
  });

  test('hallucinated findings are SUPPRESSED end-to-end (the gate, not the enhancer)', async () => {
    const wiki = new InMemoryWikiClient();
    const ctx = makeTestContext({
      http: http(),
      wiki,
      storage: new InMemoryStorage(),
      // SUPPORTED with a quote absent from the source → groundingVerifier rejects → suppressed.
      model: new ConstantModelClient(verdict({ verdict: 'SUPPORTED', quote: 'a fabricated span absent from every source' })),
    });
    const svc = new SessionService(ctx, POLICY, {
      capabilities: CAPS,
      enhancers: [bareUrlRepairEnhancer, citationVerifyEnhancer],
    });

    const session = await svc.improve({ title: 'Markdown', wiki: 'en' });

    expect(session.findings).toHaveLength(0); // every hallucinated verdict was suppressed pre-human
    expect(session.events.some((e) => e.type === 'finding_suppressed')).toBe(true);
    const audit = (await svc.audit(session.sessionId)) as Array<{ event?: string }>;
    expect(audit.some((a) => a.event === 'finding_suppressed')).toBe(true);

    // bare-url proposals are unaffected; still nothing written.
    expect(session.proposals.length).toBeGreaterThan(0);
    expect(wiki.appliedEdits).toHaveLength(0);
  });
});
```

**Step 2: Run it**

```
npx vitest run packages/agent/src/improve-pipeline.integration.test.ts
```
Expected: both tests pass. If `session.findings.length` is `0` in the first test, confirm the
recorded article has prose citations with source URLs (it has 40+) and that `SOURCE_HTML` is long
enough to pass `classifyBodyUsability` (it is). If the body is judged unusable, citation-verify
emits `SOURCE_UNAVAILABLE` findings (also `source-fetched`-grounded) — still a populated channel.

**Step 3: Commit**

```bash
git add packages/agent/src/improve-pipeline.integration.test.ts
git commit -m "test(agent): end-to-end improve() — both channels populate, zero writes, hallucinations suppressed (#7/#13)"
```

---

## Task 2: The 0-hallucination eval gate covers findings

The boundary keeps `evals → [core, tools, policy]` (no `enhancers`), so the gate cannot run
`citation-verify` directly. This test proves the gate's **unit of violation** (`hallucinated`)
applies equally to finding-derived outcomes — the end-to-end finding-suppression proof lives in
Task 1 and Phase 5.

**Files:**
- Create: `packages/evals/src/findings-gate.test.ts`

**Step 1: Write the test**

```ts
import { describe, expect, test } from 'vitest';
import { antiHallucinationGate } from './gates.ts';
import type { CaseOutcome } from './types.ts';

/**
 * A citation-verify Finding reduces to the SAME CaseOutcome the gate scores: a SUPPORTED verdict
 * whose quote did not locate is `hallucinated:true` — the gate's unit of violation. So the
 * 0-hallucination gate covers findings, not only citation edits.
 */
function findingOutcome(id: string, hallucinated: boolean): CaseOutcome {
  return { id, expected: 'SUPPORTED', actual: 'SUPPORTED', quoteLocated: !hallucinated, hallucinated, cohorts: [] };
}

describe('anti-hallucination gate covers findings (#13)', () => {
  test('a hallucinated finding is a gate violation (the 0-violation gate fails CLOSED)', () => {
    const r = antiHallucinationGate([findingOutcome('cv-0', false), findingOutcome('cv-1', true)]);
    expect(r.passed).toBe(false);
    expect(r.violations).toContain('cv-1');
  });

  test('all-grounded findings pass the gate', () => {
    const r = antiHallucinationGate([findingOutcome('cv-0', false), findingOutcome('cv-1', false)]);
    expect(r.passed).toBe(true);
  });
});
```

**Step 2: Run + commit**

```
npx vitest run packages/evals/src/findings-gate.test.ts
```
```bash
git add packages/evals/src/findings-gate.test.ts
git commit -m "test(evals): 0-hallucination gate semantics cover findings (#13)"
```

---

## Task 3: Full CI gate (the definition of done)

**Step 1: Confirm import boundaries** (no change needed — verify the `agent → enhancers` edge from
Phase 3 holds and nothing leaked)

```
npm run boundaries
```
Expected: 0 violations. (The design's "add `enhancers → [core, tools]`" was already satisfied by the
pre-existing broader `enhancers` rule; Phase 3 added `agent → enhancers`.)

**Step 2: Full CI**

```
npm run ci
```
Expected: green end-to-end — `format → typecheck → lint → boundaries → stubscan → coverage (≥90%
lines/fns/stmts, ≥85% branches on core/tools) → eval`.

Coverage watch-points (core/tools are the only gated packages):
- `packages/core/src/concurrency.ts` (`mapWithConcurrency`) — covered by `concurrency.test.ts`.
- `packages/core/src/capabilities.ts` — type-only; no runtime to cover.
- The `source-fetched` / empty-quote branches live in `packages/hooks` (not coverage-gated) but are
  covered by `grounding-verifier.test.ts`.
- No new uncovered branches were added to `core`/`tools` beyond `concurrency.ts`.

If `coverage` fails on a new `core`/`tools` line, add the missing unit test (do **not** lower the
threshold). If `format` fails, run `npm run format` and re-commit.

**Step 3: Final commit (if `npm run format` touched anything)**

```bash
git add -A
git commit -m "chore: npm run ci green — MVP-1 enhancer composition complete (#7/#13)"
```

---

## Phase 6 Done When

- `improve()` over the recorded article surfaces **both** a `ProposedEdit` (bare-url) and
  `Finding`s (citation-verify), and `wiki.appliedEdits === 0`.
- A hallucinated finding is suppressed end-to-end (`finding_suppressed` event + audit; not surfaced).
- The anti-hallucination gate's semantics are shown to cover finding-derived outcomes.
- `npm run boundaries` and `npm run ci` are green end-to-end.

---

## After Phase 6 — closing out

- **#7** was closed at the end of Phase 1; **#13** is closed by this phase. Run `chainlink close 13`
  after the work merges and `npm run ci` is green (and confirm `#7` is closed). Do **not** close
  before CI is green.
- Update `CLAUDE.md`'s status section (M0–M5 → "MVP-1 enhancer set built"): `improve()` is now a
  capability-gated enhancer registry over two channels; `bare-url-repair` + `citation-verify` ship;
  `wayback-archive` is deferred to #9; the enforcement pipeline is unified (`#7` closed). Use the
  `ed3d-extending-claude:maintaining-project-context` skill / the project's librarian flow.
- **Deferred, recorded:** `wayback-archive` (→ #9, needs the wikitext-AST); relevance-aware
  chunking; PDF page extraction; multi-model voting in the enhancer (the `voting.ts` primitives
  exist but are unused by `citation-verify` — single-model verdict for MVP-1).
