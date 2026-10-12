# Phase 5: `citation-verify` Enhancer — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `ed3d-plan-and-execute:executing-an-implementation-plan`.

**Goal:** The model-dependent enhancer — walk every prose citation through `resolve_citation_url →
fetch_source → verify_citation`, emit **one graded `Finding` per citation**, with the
anti-hallucination invariant enforced by the **gate** (not the enhancer): a located quote grounds on
`located-quote`; NOT_SUPPORTED/SOURCE_UNAVAILABLE ground on `source-fetched`; a *claimed but
unlocatable* quote is suppressed by `groundingVerifier`.

**Architecture:** A pure `mapWithConcurrency` bounded-concurrency primitive (≤3 per citation) is
extracted to `core` and adopted by **both** the enhancer and the benchmark runner (one pool, no
duplicate). `WikipediaArticleSubject` gains `article: ParsedArticle`; `improve()` loads it
tolerantly. `citation-verify` reuses the existing verify spine + `classifyBodyUsability`.

**Tech Stack:** TypeScript (NodeNext ESM), Vitest 3.

**Scope:** Phase 5 of 6. **Depends on** Phase 1 (grounding profiles), Phase 3 (registry + subject),
Phase 4 (`model` capability).

**Codebase verified:** 2026-06-03 (`verifyCitationTool` emits `located-quote` evidence iff
`quoteLocated`; `fetchSourceTool` content-addresses the body; the worker-pool at
`packages/evals/src/benchmark.ts:197-225`).

---

## Task 1: Extract `mapWithConcurrency` to `core`; adopt it in the benchmark (no duplicate pool)

**Files:**
- Create: `packages/core/src/concurrency.ts`
- Create: `packages/core/src/concurrency.test.ts`
- Modify: `packages/core/src/index.ts` (add `export * from './concurrency.ts';`)
- Modify: `packages/evals/src/benchmark.ts` (`runBenchmark` uses `mapWithConcurrency`)

**Step 1: Write the failing test** (`concurrency.test.ts`)

```ts
import { describe, expect, test } from 'vitest';
import { mapWithConcurrency } from './concurrency.ts';

describe('mapWithConcurrency (the one bounded-concurrency primitive)', () => {
  test('returns results in INPUT order regardless of completion order', async () => {
    const out = await mapWithConcurrency([30, 5, 15], 3, async (ms) => {
      await new Promise((r) => setTimeout(r, ms));
      return ms;
    });
    expect(out).toEqual([30, 5, 15]);
  });

  test('never exceeds the concurrency limit', async () => {
    let active = 0;
    let peak = 0;
    await mapWithConcurrency([1, 2, 3, 4, 5, 6], 2, async () => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((r) => setTimeout(r, 5));
      active -= 1;
    });
    expect(peak).toBeLessThanOrEqual(2);
  });

  test('empty input → empty output; fn never called', async () => {
    let called = 0;
    const out = await mapWithConcurrency([], 3, async () => {
      called += 1;
      return 1;
    });
    expect(out).toEqual([]);
    expect(called).toBe(0);
  });

  test('a non-finite limit falls back to 1 (never spawns zero workers)', async () => {
    expect(await mapWithConcurrency([1, 2], Number.POSITIVE_INFINITY, async (x) => x)).toEqual([1, 2]);
  });
});
```

**Step 2: Run — expect failure**

```
npx vitest run packages/core/src/concurrency.test.ts
```

**Step 3: Implement** (`concurrency.ts`)

```ts
/**
 * Run `fn` over `items` with at most `limit` concurrent calls, returning results in INPUT order.
 * A pure, dependency-free bounded-concurrency map — the ONE worker-pool primitive in the repo (no
 * pLimit dep, no duplicated pools). Used by the citation-verify enhancer (≤3 per citation) and the
 * benchmark runner. `limit` is normalized: a non-finite value falls back to 1, so a bad value can
 * never silently spawn zero workers and return empty results.
 */
export async function mapWithConcurrency<T, R>(
  items: readonly T[],
  limit: number,
  fn: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const n = items.length;
  const results = new Array<R>(n);
  const effectiveLimit = Number.isFinite(limit) ? Math.max(1, Math.floor(limit)) : 1;
  let cursor = 0;
  async function worker(): Promise<void> {
    while (cursor < n) {
      const i = cursor++;
      results[i] = await fn(items[i]!, i);
    }
  }
  await Promise.all(Array.from({ length: Math.min(effectiveLimit, n) }, () => worker()));
  return results;
}
```

**Step 4: Adopt it in `runBenchmark`** (`evals/benchmark.ts:197-225`) — replace the hand-rolled
worker/cursor block with the shared primitive (preserving the `onProgress` completion-count index):

Add to the `@wikiharness/core` import in `benchmark.ts`:

```ts
  mapWithConcurrency,
```

Replace the body of `runBenchmark` (`:200-225`) with:

```ts
  const total = dataset.length;
  let completed = 0; // completion-order progress index (preserved)
  // mapWithConcurrency normalizes a non-finite/zero concurrency to 1 — no separate guard needed.
  const perCase = await mapWithConcurrency(dataset, options.concurrency ?? 1, async (c) => {
    const caseResults = await runCase(c, options);
    options.onProgress?.({ index: completed, total, caseId: c.id, caseResults });
    completed += 1;
    return caseResults;
  });
  return perCase.flat();
```

**Step 5: Run benchmark tests — must stay green**

```
npx vitest run packages/core/src/concurrency.test.ts packages/evals/src/benchmark.test.ts
```
Expected: green. Note: `runBenchmark`'s returned array is now in **input order** (deterministic)
rather than completion order. If a `benchmark.test.ts` assertion happened to depend on completion
order, update it to the (now-deterministic) input order — that is a strict improvement, not a
regression. `onProgress` indices are unchanged (still completion order).

**Step 6: Commit**

```bash
git add packages/core/src/concurrency.ts packages/core/src/concurrency.test.ts packages/core/src/index.ts packages/evals/src/benchmark.ts
git commit -m "feat(core): mapWithConcurrency primitive; benchmark reuses it (no duplicate pool) (#13)"
```

---

## Task 2: Extend `WikipediaArticleSubject` with `article`; load it tolerantly in `improve()`

**Files:**
- Modify: `packages/enhancers/src/types.ts` (`WikipediaArticleSubject` gains `article`)
- Modify: `packages/agent/src/hitl.ts` (`improve()` loads the article; imports)

**Step 1: Extend the subject** (`types.ts`)

```ts
import type { Capability, Finding, ParsedArticle, ProposedEdit, ToolContext } from '@wikiharness/core';

/** What improve() builds once per session and hands to every enhancer. */
export interface WikipediaArticleSubject {
  title: string;
  wiki: string;
  wikitext: string;
  revision: number;
  /** The parsed article (citations + claims + source URLs) — what citation-verify walks. */
  article: ParsedArticle;
}
```

**Step 2: Load the article in `improve()`** (`hitl.ts`)

- Add `getArticleTool` to the `@wikiharness/tools` import; add `type ParsedArticle` to the
  `@wikiharness/core` import.

```ts
import { commitConfirmedEditTool, getArticleTool, getWikitextTool } from '@wikiharness/tools';
```

- In `improve()`, after `const { wikitext, revision } = wt.data;` and the `session:` storage put,
  build the article **tolerantly** (a REST failure must not kill the session — bare-url-repair still
  works off wikitext; citation-verify just gets no citations), then the subject:

```ts
    // The parsed article is a SECOND read (REST HTML), independent of the wikitext (Action API).
    // If it fails, degrade gracefully: enhancers that need citations get none; the failure is audited.
    let article: ParsedArticle = { title: params.title, wiki: params.wiki, revision, citations: [] };
    try {
      article = (await getArticleTool.execute({ title: params.title, wiki: params.wiki }, this.ctx)).data;
    } catch (err) {
      await this.store.appendAudit(sessionId, {
        event: 'article_load_failed',
        reason: err instanceof Error ? err.message : String(err),
        at: this.ctx.clock.now(),
      });
    }

    const subject: WikipediaArticleSubject = {
      title: params.title,
      wiki: params.wiki,
      wikitext,
      revision,
      article,
    };
```

(Replace the Phase-3 `subject` literal — which had no `article` — with this.)

**Step 3: Verify Phases 1–3 tests still pass** (their HTTP stubs only cover the wikitext URL; the
article fetch fails → tolerant fallback → bare-url-repair still surfaces proposals)

```
npx vitest run packages/agent
npm run typecheck
```
Expected: green (the `article_load_failed` audit appears in those sessions, harmlessly).

**Step 4: Commit**

```bash
git add packages/enhancers/src/types.ts packages/agent/src/hitl.ts
git commit -m "feat: WikipediaArticleSubject.article; improve() loads it tolerantly (#13)"
```

---

## Task 3: The `citation-verify` enhancer

**Files:**
- Create: `packages/enhancers/src/citation-verify.ts`
- Create: `packages/enhancers/src/citation-verify.test.ts`
- Modify: `packages/enhancers/src/index.ts` (register in `ENHANCERS`)

**Step 1: Implement** (`citation-verify.ts`)

```ts
/**
 * `citation-verify` — the model spine as an enhancer. For each PROSE citation with a claim and a
 * source URL: resolve → fetch → (body-usability gate) → verify, emitting ONE graded Finding. The
 * anti-hallucination invariant is enforced by the GATE (groundingVerifier), not here:
 *   - SUPPORTED/PARTIAL with a located quote  → reuse verify_citation's located-quote evidence.
 *   - SUPPORTED/PARTIAL whose quote did NOT locate (or is absent) → attach located-quote evidence
 *     from the model's CLAIMED quote, so the gate re-checks it and SUPPRESSES the finding.
 *   - NOT_SUPPORTED / SOURCE_UNAVAILABLE → source-fetched evidence (no quote needed).
 * Reuses the verify spine + classifyBodyUsability + mapWithConcurrency — nothing re-implemented.
 */
import {
  classifyBodyUsability,
  mapWithConcurrency,
  type Evidence,
  type Finding,
  type ParsedCitation,
  type ProvenanceRecord,
  type ToolContext,
  type Verdict,
} from '@wikiharness/core';
import { fetchSourceTool, resolveCitationUrlTool, verifyCitationTool } from '@wikiharness/tools';
import type { EnhancerResult, WikipediaArticleEnhancer, WikipediaArticleSubject } from './types.ts';

const SUPPORTING = new Set<Verdict>(['SUPPORTED', 'PARTIAL']);
const MAX_CITATION_CONCURRENCY = 3;

function sourceFetchedEvidence(summary: string, sourceHash: string, provenance: ProvenanceRecord): Evidence {
  return { kind: 'source-fetched', primary: { summary }, provenance, grounding: { kind: 'source-fetched', sourceHash } };
}

function finding(verdict: Verdict, c: ParsedCitation, url: string, evidence: Evidence[], provenance: ProvenanceRecord): Finding {
  return {
    id: `cv-${c.index}`,
    kind: 'citation-verdict',
    summary: `${verdict} — ${c.claim ?? ''}`.slice(0, 200),
    detail: { verdict, refId: c.refId, sourceUrl: url },
    provenance,
    evidence,
  };
}

async function verifyOne(c: ParsedCitation, ctx: ToolContext): Promise<Finding | null> {
  const resolved = await resolveCitationUrlTool.execute({ sourceUrls: c.sourceUrls }, ctx);
  if (resolved.data === null) return null;
  const url = resolved.data.url;

  const fetched = await fetchSourceTool.execute({ url }, ctx);
  // Body-usability GIGO gate: an unusable body is SOURCE_UNAVAILABLE WITHOUT a model call.
  if (!fetched.data.ok || !classifyBodyUsability(fetched.data.text).usable) {
    const provenance: ProvenanceRecord = {
      sources: [{ url, fetchedAt: ctx.clock.now(), contentHash: fetched.data.contentHash }],
      createdAt: ctx.clock.now(),
      toolName: 'citation_verify',
    };
    const ev = sourceFetchedEvidence('The cited source could not be used (unavailable or unusable body).', fetched.data.contentHash, provenance);
    return finding('SOURCE_UNAVAILABLE', c, url, [ev], provenance);
  }

  const verify = await verifyCitationTool.execute(
    { claim: c.claim ?? '', sourceText: fetched.data.text, sourceUrl: url },
    ctx,
  );
  const { verdict, quote, quoteLocated, sourceHash } = verify.data;
  const provenance = verify.provenance;

  let evidence: Evidence[];
  if (SUPPORTING.has(verdict)) {
    if (quoteLocated && (verify.evidence?.length ?? 0) > 0) {
      // Grounded support: reuse the tool's located-quote evidence (correct offset + highlight).
      evidence = [...verify.evidence!];
    } else {
      // Unverified support: attach the model's CLAIMED quote so the gate re-checks and suppresses.
      // (An empty/absent quote → empty string, which groundingVerifier's empty-quote guard rejects.)
      evidence = [
        {
          kind: 'located-quote',
          primary: { summary: 'Claimed support pending grounding re-check.' },
          provenance,
          grounding: { kind: 'located-quote', quote: quote ?? '', sourceHash, offset: 0 },
        },
      ];
    }
  } else {
    evidence = [sourceFetchedEvidence('The fetched source does not support the claim.', sourceHash, provenance)];
  }
  return finding(verdict, c, url, evidence, provenance);
}

export const citationVerifyEnhancer: WikipediaArticleEnhancer = {
  name: 'citation-verify',
  requires: ['model', 'htmlExtractor'],
  async analyze(subject: WikipediaArticleSubject, ctx: ToolContext): Promise<EnhancerResult> {
    const candidates = subject.article.citations.filter(
      (c) => c.inProse && c.claim !== null && c.claim !== '' && c.sourceUrls.length > 0,
    );
    const findings = (await mapWithConcurrency(candidates, MAX_CITATION_CONCURRENCY, (c) => verifyOne(c, ctx))).filter(
      (f): f is Finding => f !== null,
    );
    return { findings };
  },
};
```

**Step 2: Register it** (`index.ts`) — append to `ENHANCERS` and export:

```ts
import type { WikipediaArticleEnhancer } from './types.ts';
import { bareUrlRepairEnhancer } from './bare-url-repair.ts';
import { citationVerifyEnhancer } from './citation-verify.ts';

export * from './types.ts';
export * from './bare-url-repair.ts';
export * from './citation-verify.ts';

export const ENHANCERS: readonly WikipediaArticleEnhancer[] = [
  bareUrlRepairEnhancer,
  citationVerifyEnhancer,
];
```

**Step 3: Write the hermetic tests** (`citation-verify.test.ts`)

```ts
import { describe, expect, test } from 'vitest';
import { ScriptedModelClient, StubHttpClient, makeTestContext, modelText } from '@wikiharness/edges';
import { groundingVerifier } from '@wikiharness/hooks';
import type { ParsedArticle, ToolCall, ToolContext } from '@wikiharness/core';
import { citationVerifyEnhancer } from './citation-verify.ts';
import type { WikipediaArticleSubject } from './types.ts';

const SOURCE_HTML =
  '<html><body><p>The sky is blue on a clear day because of Rayleigh scattering.</p></body></html>';
const CALL: ToolCall = { id: 'c', name: 'citation_verify', arguments: {} };

function subjectWithOneCitation(): WikipediaArticleSubject {
  const article: ParsedArticle = {
    title: 'Demo',
    wiki: 'en',
    revision: 1,
    citations: [
      { index: 0, refId: 'r0', noteId: null, inProse: true, claim: 'The sky is blue.', sourceUrls: ['https://src'] },
    ],
  };
  return { title: 'Demo', wiki: 'en', wikitext: '', revision: 1, article };
}

function ctxWith(verdictJson: string): ToolContext {
  const http = new StubHttpClient((req) =>
    req.url === 'https://src'
      ? { status: 200, headers: { 'content-type': 'text/html' }, body: SOURCE_HTML }
      : { status: 404, headers: {}, body: '' },
  );
  return makeTestContext({ http, model: new ScriptedModelClient([modelText(verdictJson)]) });
}

async function grounds(ctx: ToolContext, finding: unknown): Promise<boolean> {
  const v = await groundingVerifier.verify(
    CALL,
    { data: {}, provenance: (finding as { provenance: ProvenanceRecord }).provenance, rationale: '', findings: [finding as never] },
    ctx,
  );
  return v.ok;
}

describe('citation-verify enhancer (anti-hallucination enforced by the GATE)', () => {
  test('requires model + htmlExtractor', () => {
    expect(citationVerifyEnhancer.requires).toEqual(['model', 'htmlExtractor']);
  });

  test('SUPPORTED with a locatable quote → a located-quote finding that GROUNDS (surfaces)', async () => {
    const ctx = ctxWith(JSON.stringify({ verdict: 'SUPPORTED', quote: 'The sky is blue' }));
    const result = await citationVerifyEnhancer.analyze(subjectWithOneCitation(), ctx);
    expect(result.findings).toHaveLength(1);
    const f = result.findings![0]!;
    expect(f.evidence[0]!.grounding.kind).toBe('located-quote');
    expect(await grounds(ctx, f)).toBe(true);
  });

  test('NOT_SUPPORTED → a source-fetched finding that GROUNDS (surfaces, not suppressed)', async () => {
    const ctx = ctxWith(JSON.stringify({ verdict: 'NOT_SUPPORTED' }));
    const result = await citationVerifyEnhancer.analyze(subjectWithOneCitation(), ctx);
    const f = result.findings![0]!;
    expect(f.evidence[0]!.grounding.kind).toBe('source-fetched');
    expect(await grounds(ctx, f)).toBe(true);
  });

  test('SUPPORTED with a HALLUCINATED quote (not in source) → the gate SUPPRESSES it', async () => {
    const ctx = ctxWith(JSON.stringify({ verdict: 'SUPPORTED', quote: 'a fabricated span absent from the source' }));
    const result = await citationVerifyEnhancer.analyze(subjectWithOneCitation(), ctx);
    const f = result.findings![0]!;
    expect(f.evidence[0]!.grounding.kind).toBe('located-quote'); // synthesized from the claimed quote
    expect(await grounds(ctx, f)).toBe(false); // groundingVerifier rejects → improve() will suppress
  });

  test('only prose citations with a claim and a source are walked', async () => {
    const ctx = ctxWith(JSON.stringify({ verdict: 'NOT_SUPPORTED' }));
    const subject = subjectWithOneCitation();
    // A non-prose / claim-less / source-less citation is skipped.
    subject.article.citations.push({ index: 1, refId: 'r1', noteId: null, inProse: false, claim: null, sourceUrls: [] });
    const result = await citationVerifyEnhancer.analyze(subject, ctx);
    expect(result.findings).toHaveLength(1); // only the prose+claim+source one
  });
});
```

(Add `import type { ProvenanceRecord } from '@wikiharness/core';` to the test's core type import.)

**Step 4: Verify**

```
npx vitest run packages/enhancers
npm run typecheck
```
Expected: green.

**Step 5: Full CI + commit**

```
npm run ci
```
Expected: green.

```bash
git add packages/enhancers/src/citation-verify.ts packages/enhancers/src/citation-verify.test.ts packages/enhancers/src/index.ts
git commit -m "feat(enhancers): citation-verify — one graded Finding per citation, gate-enforced (#13)"
```

---

## Phase 5 Done When (hermetic)

- A SUPPORTED verdict whose quote locates surfaces a `located-quote` finding that **grounds**.
- A NOT_SUPPORTED verdict surfaces a `source-fetched` finding that **grounds** (not suppressed for
  lacking a quote).
- A SUPPORTED verdict with a quote **not** in the source produces a finding whose
  `located-quote` grounding **fails** (so `improve()`/the gate suppresses it — proven end-to-end in
  Phase 6).
- `citation-verify` requires `['model', 'htmlExtractor']`; the Phase-3 capability gate skips it when
  absent (so the model is never called key-free).
- Per-citation concurrency is bounded (≤3) via the shared `mapWithConcurrency`; the benchmark uses
  the same primitive (no duplicate pool). `npm run ci` is green.
