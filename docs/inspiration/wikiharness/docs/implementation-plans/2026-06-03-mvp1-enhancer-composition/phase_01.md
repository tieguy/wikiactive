# Phase 1: Unified Enforcement Pipeline (closes #7) — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `ed3d-plan-and-execute:executing-an-implementation-plan`
> to implement this plan task-by-task.

**Goal:** Collapse the two enforcement sites (the agent loop's post-hooks and `improve()`'s inline
policy+grounding block) onto **one** canonical hook chain — `[groundingVerifier, policyHook]` built
by a single `enforcementPipeline(policy)` factory and run by a shared `runEnforcement` — so a hook
added once is enforced in both. Extend `groundingVerifier` (and the core types) so the same gate
covers `Finding`s and no-quote (`source-fetched`) verdicts.

**Architecture:** `policyHook` (a factory wrapping the pure `evaluatePolicy`) + `groundingVerifier`
(extended) compose into `enforcementPipeline(policy)`. `runEnforcement` runs the chain and
aggregates reasons. `improve()` routes each proposal through it (behavior-preserving). The chain
lives in `packages/agent/src/enforcement.ts` because `policyHook` needs `policy` and the import
boundary keeps `hooks` policy-free; both consumers are in `agent`.

**Tech Stack:** TypeScript (NodeNext ESM, `.ts` specifiers), Vitest 3, Zod.

**Scope:** Phase 1 of 6 (the design's Phase 1).

**Codebase verified:** 2026-06-03 (8 codebase-investigators + direct reads of every file below).

---

## Invariants this phase must NOT break (verified current behavior)

- `improve()` surfaces only proposals that pass **both** policy and grounding; failures are
  **suppressed** with a `proposal_suppressed` event (`reasons: string[]`) + a `suppressed` audit
  entry; nothing reaches a wiki (`appliedEdits` stays `[]`). (`hitl.ts:95-129`.)
- The suppress/surface **decision** (suppress on policy OR grounding failure) and the audit trail
  are preserved. The cosmetic **order** of the `reasons` array is intentionally NOT preserved: this
  phase adopts the design doc's grounding-first chain `[groundingVerifier, policyHook]`, so
  `reasons` reads `[groundingReason, ...policyReasons]`. (Order is set in one place — the
  `enforcementPipeline` factory — and is cosmetic/untested today; pre-MVP we don't preserve legacy
  behavior for its own sake.)
- The commit path (`decide`/`commitDecision`) — token binding, the edit-path policy re-gate
  (`hitl.ts:252-267`), `AWAITING_CONFIRMATION`/`COMMITTED`, audit events — is **untouched**.
- `groundingVerifier`'s existing `located-quote` and `diff` behavior is unchanged
  (`grounding-verifier.test.ts` stays green).

---

## Task 1: Characterization test — pin `improve()` suppression before refactoring

This pins the *current* suppression behavior so the refactor in Task 6 can be proven
behavior-preserving. A characterization test passes immediately on current code (it documents what
is) — its value is that it must stay green through Task 6.

**Files:**
- Create: `packages/agent/src/hitl-enforcement.test.ts`

**Step 1: Write the characterization test**

```ts
import { describe, expect, test } from 'vitest';
import {
  InMemoryStorage,
  InMemoryWikiClient,
  StubHttpClient,
  makeTestContext,
} from '@wikiharness/edges';
import { buildWikitextQueryUrl, type ToolContext } from '@wikiharness/core';
import type { Policy } from '@wikiharness/policy';
import { SessionService } from './hitl.ts';

const WIKITEXT =
  'Lead claim.<ref>https://a.org/x</ref> Second claim.<ref name="b">https://b.org/y</ref> End.';

// A policy that FORBIDS the wikipedia sink → every bare-URL repair proposal (sink 'wikipedia')
// fails policy and must be suppressed before surfacing. This drives improve()'s suppression path.
const FORBIDS_SINK: Policy = {
  id: 'citation-verification',
  tier: 'suggested',
  sinks: ['wikidata'],
  requireDisclosure: true,
  requireGroundedEvidence: true,
  maxChangedChars: 2000,
};

function actionApiBody(): string {
  return JSON.stringify({
    query: {
      pages: [
        { title: 'Demo', revisions: [{ revid: 100, slots: { main: { content: WIKITEXT } } }] },
      ],
    },
  });
}

function buildCtx(): { ctx: ToolContext; wiki: InMemoryWikiClient } {
  const url = buildWikitextQueryUrl('en', 'Demo');
  const wiki = new InMemoryWikiClient();
  const http = new StubHttpClient({
    [`GET ${url}`]: { status: 200, headers: {}, body: actionApiBody() },
  });
  return { ctx: makeTestContext({ http, wiki, storage: new InMemoryStorage() }), wiki };
}

describe('improve() enforcement (characterization — pins suppression before the #7 refactor)', () => {
  test('a proposal that fails policy is SUPPRESSED: 0 surfaced, a proposal_suppressed event + suppressed audit per bare URL, no write', async () => {
    const { ctx, wiki } = buildCtx();
    const svc = new SessionService(ctx, FORBIDS_SINK);
    const session = await svc.improve({ title: 'Demo', wiki: 'en' });

    // Both bare-URL proposals fail the sink check → nothing surfaces.
    expect(session.proposals).toHaveLength(0);

    // One proposal_suppressed event per bare URL, each carrying the policy reason(s).
    const suppressed = session.events.filter((e) => e.type === 'proposal_suppressed');
    expect(suppressed).toHaveLength(2);
    for (const e of suppressed) {
      if (e.type === 'proposal_suppressed') {
        expect(e.reasons.join(' ')).toMatch(/sink .* is not permitted/i);
      }
    }

    // The suppression is audited on the session ledger.
    const audit = (await svc.audit(session.sessionId)) as Array<{ event?: string }>;
    expect(audit.filter((a) => a.event === 'suppressed')).toHaveLength(2);

    // The absolute invariant: nothing reached a wiki.
    expect(wiki.appliedEdits).toHaveLength(0);
  });

  test('with a passing policy the same proposals SURFACE (suppression is not unconditional)', async () => {
    const { ctx } = buildCtx();
    const svc = new SessionService(ctx, { ...FORBIDS_SINK, sinks: ['wikipedia'] });
    const session = await svc.improve({ title: 'Demo', wiki: 'en' });
    expect(session.proposals).toHaveLength(2);
    expect(session.proposals.every((r) => r.status === 'AWAITING_CONFIRMATION')).toBe(true);
    expect(session.events.some((e) => e.type === 'proposal_suppressed')).toBe(false);
  });
});
```

**Step 2: Run it — it must PASS against current code**

```
npx vitest run packages/agent/src/hitl-enforcement.test.ts
```
Expected: 2 passed. (It characterizes existing behavior; if it fails, the assumption about current
behavior is wrong — investigate before continuing.)

**Step 3: Commit**

```bash
git add packages/agent/src/hitl-enforcement.test.ts
git commit -m "test(agent): characterize improve() policy-suppression before #7 refactor"
```

---

## Task 2: Core type changes — `Finding.evidence`, `source-fetched` grounding, `PostVerification.reasons`

Type-only changes (no runtime branches → no coverage impact). Everything must still typecheck and
all tests stay green.

**Files:**
- Modify: `packages/core/src/types.ts:148-155` (Finding), `:80-102` (GroundingAssertion)
- Modify: `packages/core/src/hooks.ts:28-32` (PostVerification)

**Step 1: Replace the `Finding` interface to add `evidence`** (`types.ts:148-155`)

This is a **Modify** — replace the *entire current* `Finding` interface (id/kind/summary/detail/
provenance, no `evidence`) with the version below. No existing code constructs a `Finding` literal
(grep confirms `ToolResult.findings` is declared but unpopulated), so adding a required `evidence`
field is non-breaking — `npm run typecheck` in Step 4 confirms.

```ts
/**
 * A non-edit output (e.g. MVP-4 discovery, or a citation-verdict Finding): a suggestion/verdict
 * for humans. Never a wiki write, not confirmation-gated, because it changes nothing on-wiki.
 */
export interface Finding {
  id: string;
  /** 'candidate-rule' | 'smell' | 'citation-verdict' | … */
  kind: string;
  summary: string;
  detail?: unknown;
  provenance: ProvenanceRecord;
  /**
   * Evidence-before-verdict envelope (may be empty). Each item's `grounding` is independently
   * re-checked by `groundingVerifier` — the anti-hallucination gate applies to findings too.
   */
  evidence: readonly Evidence[];
}
```

**Step 2: Add the `source-fetched` variant to `GroundingAssertion`** (`types.ts:80-102`)

This is a **Modify** — replace the *entire current* `GroundingAssertion` union (the three existing
variants `located-quote` / `in-article-span` / `diff`) with the four-variant version below. The
existing three are byte-for-byte unchanged; only the `source-fetched` variant is appended.

```ts
export type GroundingAssertion =
  | {
      kind: 'located-quote';
      /** The quote string-located in a source fetched THIS session. */
      quote: string;
      /** ObjectStore address of the source the quote was located in. */
      sourceHash: string;
      /** Offset returned by locateQuoteInSource. */
      offset: number;
    }
  | {
      // MOS:PEACOCK and other codified rules: no external source at all.
      kind: 'in-article-span';
      revision: number;
      spanStart: number;
      spanEnd: number;
      ruleId: string;
    }
  | {
      kind: 'diff';
      /** Hash of the diff actually computed; the shown diff must equal it. */
      expectedDiffHash: string;
    }
  | {
      /**
       * A verdict with NO supporting quote (NOT_SUPPORTED / SOURCE_UNAVAILABLE) grounds on
       * source-provenance: the source was actually fetched this session (its bytes are in the
       * ObjectStore at `sourceHash`). Generalizes the anti-hallucination gate to no-quote verdicts
       * — a fabricated "I read the source and it doesn't support this" is still caught.
       */
      kind: 'source-fetched';
      sourceHash: string;
    };
```

**Step 3: Add `reasons?` to `PostVerification`** (`hooks.ts`)

```ts
export interface PostVerification {
  ok: boolean;
  /** Required when `ok` is false. */
  reason?: string;
  /**
   * Optional itemized reasons (e.g. a policy's several failures), preserved as separate elements
   * when a hook fails for more than one reason. `reason` stays the single-string summary the loop
   * logs; `runEnforcement` prefers `reasons` when present.
   */
  reasons?: readonly string[];
}
```

**Step 4: Verify typecheck + existing tests**

```
npm run typecheck
npx vitest run packages/core packages/hooks
```
Expected: typecheck clean; all green. (No existing `Finding` object literal exists, so making
`evidence` required does not break compilation — confirm via typecheck.)

**Step 5: Commit**

```bash
git add packages/core/src/types.ts packages/core/src/hooks.ts
git commit -m "feat(core): Finding.evidence, source-fetched grounding, PostVerification.reasons (#7)"
```

---

## Task 3: Extend `groundingVerifier` — findings, `source-fetched`, empty-quote guard

**Files:**
- Modify: `packages/hooks/src/grounding-verifier.ts`
- Modify: `packages/hooks/src/grounding-verifier.test.ts`

**Step 1: Write the failing tests** (append to `grounding-verifier.test.ts`)

```ts
import { type Finding } from '@wikiharness/core';

function resultWithFinding(finding: Finding): ToolResult<unknown> {
  return {
    data: {},
    provenance: { sources: [], createdAt: 0, toolName: 'citation_verify' },
    rationale: 'r',
    findings: [finding],
  };
}

function findingWith(evidence: Evidence[]): Finding {
  return {
    id: 'f1',
    kind: 'citation-verdict',
    summary: 'verdict',
    provenance: { sources: [], createdAt: 0, toolName: 'citation_verify' },
    evidence,
  };
}

function sourceFetchedEvidence(hash: string): Evidence {
  return {
    kind: 'source-fetched',
    primary: { summary: 'The fetched source does not support the claim.' },
    provenance: { sources: [], createdAt: 0, toolName: 'citation_verify' },
    grounding: { kind: 'source-fetched', sourceHash: hash },
  };
}

describe('groundingVerifier — findings + source-fetched grounding (#7)', () => {
  test('source-fetched passes when the source IS in the object store (it was really fetched)', async () => {
    const { ctx, hash } = await ctxWithSource();
    const v = await groundingVerifier.verify(CALL, resultWithFinding(findingWith([sourceFetchedEvidence(hash)])), ctx);
    expect(v.ok).toBe(true);
  });

  test('source-fetched REJECTS when the source was never fetched this session', async () => {
    const { ctx } = await ctxWithSource();
    const v = await groundingVerifier.verify(CALL, resultWithFinding(findingWith([sourceFetchedEvidence('0'.repeat(64))])), ctx);
    expect(v.ok).toBe(false);
    expect(v.reason).toMatch(/source|fetched|object store/i);
  });

  test("a finding's located-quote evidence is re-verified (a hallucinated quote is rejected)", async () => {
    const { ctx, hash } = await ctxWithSource();
    const badQuote = locatedQuoteEvidence(hash, 'a quote that is not in the source', 0);
    const v = await groundingVerifier.verify(CALL, resultWithFinding(findingWith([badQuote])), ctx);
    expect(v.ok).toBe(false);
  });

  test('an empty located-quote is rejected (an empty string locates everywhere — never grounded)', async () => {
    const { ctx, hash } = await ctxWithSource();
    const empty = locatedQuoteEvidence(hash, '', 0);
    const v = await groundingVerifier.verify(CALL, resultWithEvidence([empty]), ctx);
    expect(v.ok).toBe(false);
    expect(v.reason).toMatch(/empty/i);
  });

  test('a finding with empty evidence has nothing to ground (passes)', async () => {
    const { ctx } = await ctxWithSource();
    const v = await groundingVerifier.verify(CALL, resultWithFinding(findingWith([])), ctx);
    expect(v.ok).toBe(true);
  });
});
```

**Step 2: Run — expect failures** (source-fetched silently passes today; empty quote passes today)

```
npx vitest run packages/hooks/src/grounding-verifier.test.ts
```
Expected: the 4 new behavioral tests FAIL (source-fetched not handled → the no-store case passes
wrongly; empty quote locates at 0 → passes wrongly; findings not iterated).

**Step 3: Implement** (`grounding-verifier.ts`)

In `verifyEvidence`, add the empty-quote guard at the top of the `located-quote` branch (before the
`ctx.objects.get` call):

```ts
  if (grounding.kind === 'located-quote') {
    if (grounding.quote.length === 0) {
      return {
        ok: false,
        reason: 'located-quote grounding has an empty quote, which is not grounded evidence',
      };
    }
    const source = await ctx.objects.get(grounding.sourceHash);
    // …unchanged…
  }
```

Add a `source-fetched` branch (before the final `// 'in-article-span' …` fallthrough):

```ts
  if (grounding.kind === 'source-fetched') {
    // No quote to locate; the honest claim is "this source was really fetched this session".
    if (!(await ctx.objects.has(grounding.sourceHash))) {
      return {
        ok: false,
        reason: `source-fetched grounding references source ${grounding.sourceHash}, which was not fetched this session (not in the object store)`,
      };
    }
    return { ok: true };
  }
```

In `groundingVerifier.verify`, add a findings loop after the `proposedEdits` loop (before
`return { ok: true }`):

```ts
    // Findings carry their own evidence (e.g. citation verdicts). The same gate applies — a
    // finding's verdict is only surfaced if its grounding re-verifies.
    for (const finding of result.findings ?? []) {
      for (const ev of finding.evidence) {
        const verification = await verifyEvidence(ev, ctx);
        if (!verification.ok) return verification;
      }
    }
```

**Step 4: Run — expect green**

```
npx vitest run packages/hooks/src/grounding-verifier.test.ts
```
Expected: all pass (new + existing `located-quote`/`diff` tests).

**Step 5: Commit**

```bash
git add packages/hooks/src/grounding-verifier.ts packages/hooks/src/grounding-verifier.test.ts
git commit -m "feat(hooks): groundingVerifier verifies findings + source-fetched, rejects empty quote (#7)"
```

---

## Task 4: `makePolicyHook` — adapt `evaluatePolicy` to a PostToolUse hook

**Files:**
- Create: `packages/agent/src/enforcement.ts`
- Create: `packages/agent/src/enforcement.test.ts`

> The chain lives in `agent` (not `hooks`) because `policyHook` imports `evaluatePolicy`, and the
> import boundary forbids `hooks → policy` (`boundaries.ts:13`) but allows `agent → policy`
> (`:17`). Both consumers (loop + `improve()`) are in `agent`.

**Step 1: Write `makePolicyHook` + `enforcementPipeline` + `runEnforcement`** (`enforcement.ts`)

(Task 4 and Task 5 build this one file; it is written in full here and tested in Task 5.)

```ts
/**
 * The ONE enforcement pipeline both the agent loop and improve() consume (#7). Before #7 these two
 * sites checked policy + grounding independently, so a hook added to one could silently miss the
 * other. Now: `enforcementPipeline(policy)` is the single source of the hook chain, and
 * `runEnforcement` runs it; the loop feeds the same array as its `postHooks`. Adding a hook here
 * reaches both paths.
 *
 * `policyHook` is a factory (closes over the session Policy) because PostToolUseHook.verify has no
 * policy slot. It wraps the pure `evaluatePolicy` — never reimplementing policy logic — and is a
 * NO-OP for findings (a result with no proposedEdits), which never write.
 *
 * Chain ORDER is decided in ONE place: this factory. It is `[groundingVerifier, policyHook]`
 * (grounding-first) — the anti-hallucination gate is the most fundamental invariant, so it leads
 * the aggregated reasons (`[groundingReason, ...policyReasons]`). Order is cosmetic today (nothing
 * depends on it); centralizing it here means it changes in one line if it ever becomes load-bearing.
 */
import {
  type PostToolUseHook,
  type PostVerification,
  type ToolCall,
  type ToolContext,
  type ToolResult,
} from '@wikiharness/core';
import { evaluatePolicy, type Policy } from '@wikiharness/policy';
import { groundingVerifier } from '@wikiharness/hooks';

export function makePolicyHook(policy: Policy): PostToolUseHook {
  return {
    name: 'policy',
    verify(_call: ToolCall, result: ToolResult<unknown>, _ctx: ToolContext): PostVerification {
      for (const edit of result.proposedEdits ?? []) {
        const decision = evaluatePolicy(policy, edit);
        if (!decision.allowed) {
          return { ok: false, reason: decision.reasons.join('; '), reasons: decision.reasons };
        }
      }
      return { ok: true };
    },
  };
}

/**
 * The canonical post-hook chain for a session — the SINGLE source of truth for both membership and
 * order. Both improve() and the loop obtain their hooks from here, so adding/reordering a hook
 * reaches both paths. Grounding-first (see the file header for the rationale + the recorded
 * fail-fast tradeoff).
 */
export function enforcementPipeline(policy: Policy): readonly PostToolUseHook[] {
  return [groundingVerifier, makePolicyHook(policy)];
}

export interface EnforcementOutcome {
  ok: boolean;
  /** Itemized failure reasons in chain order; empty when ok. */
  reasons: string[];
}

/**
 * Run a hook chain over one output and aggregate reasons (NO short-circuit — improve() reports
 * EVERY failing gate at once, so the human sees all problems to fix, not just the first). The loop
 * keeps its own short-circuiting `runPostHooks` (which also emits events + feeds the model); both
 * run the SAME `hooks` array, so the enforced SET never drifts (that is the #7 fix; the differing
 * aggregation semantics are a per-site presentation choice, not part of the invariant).
 */
export async function runEnforcement(
  call: ToolCall,
  result: ToolResult<unknown>,
  ctx: ToolContext,
  hooks: readonly PostToolUseHook[],
): Promise<EnforcementOutcome> {
  let ok = true;
  const reasons: string[] = [];
  for (const hook of hooks) {
    const v = await hook.verify(call, result, ctx);
    if (!v.ok) {
      ok = false;
      const items = v.reasons ?? (v.reason !== undefined ? [v.reason] : []);
      reasons.push(...items);
    }
  }
  return { ok, reasons };
}
```

**Step 2: Verify typecheck**

```
npm run typecheck
```
Expected: clean (boundary OK: `agent` may import `policy`, `hooks`, `core`).

**Step 3: Commit**

```bash
git add packages/agent/src/enforcement.ts
git commit -m "feat(agent): enforcement pipeline — policyHook + enforcementPipeline + runEnforcement (#7)"
```

---

## Task 5: Test the pipeline + prove BOTH the loop and improve() honor the same chain

**Files:**
- Create: `packages/agent/src/enforcement.test.ts`

**Step 1: Write the tests**

```ts
import { describe, expect, test } from 'vitest';
import {
  InMemoryObjectStore,
  InMemoryStorage,
  InMemoryWikiClient,
  ScriptedModelClient,
  StubHttpClient,
  makeTestContext,
  modelToolCalls,
  toolCall,
} from '@wikiharness/edges';
import { repairCitationTool } from '@wikiharness/tools';
import { runAgentLoop } from './loop.ts';
import { SessionService } from './hitl.ts';
import {
  enforcementPipeline,
  makePolicyHook,
  runEnforcement,
} from './enforcement.ts';
import {
  buildWikitextQueryUrl,
  type Evidence,
  type PostToolUseHook,
  type ProposedEdit,
  type ToolCall,
  type ToolResult,
} from '@wikiharness/core';
import type { Policy } from '@wikiharness/policy';

const POLICY: Policy = {
  id: 'citation-verification',
  tier: 'suggested',
  sinks: ['wikipedia'],
  requireDisclosure: true,
  requireGroundedEvidence: true,
  maxChangedChars: 2000,
};
const WIKITEXT = 'Claim.<ref>https://a.org/x</ref> End.';
const CALL: ToolCall = { id: 'c', name: 'session', arguments: {} };

// A proposal that fails policy two ways (forbidden sink + no disclosure) AND has no evidence.
function badEdit(): ProposedEdit {
  return {
    id: 'p1',
    sink: 'commons', // not in POLICY.sinks
    target: { title: 'T', wiki: 'en', baseRevision: 1 },
    before: 'a',
    after: 'b',
    summary: 'no disclosure here',
    evidence: [], // requireGroundedEvidence → another reason
    provenance: { sources: [], createdAt: 0, toolName: 'x' },
    rationale: 'r',
    hash: 'h',
  };
}
function resultWith(edit: ProposedEdit, evidence?: Evidence[]): ToolResult<unknown> {
  return {
    data: {},
    provenance: { sources: [], createdAt: 0, toolName: 'x' },
    rationale: 'r',
    proposedEdits: [edit],
    ...(evidence ? { evidence } : {}),
  };
}

describe('runEnforcement + enforcementPipeline (#7)', () => {
  test('policyHook is a NO-OP for findings (a result with no proposedEdits passes policy)', async () => {
    const hook = makePolicyHook(POLICY);
    const v = await hook.verify(CALL, { data: {}, provenance: { sources: [], createdAt: 0, toolName: 'x' }, rationale: 'r', findings: [] }, makeTestContext());
    expect(v.ok).toBe(true);
  });

  test('reasons aggregate in chain order — grounding-first (the order is owned by enforcementPipeline)', async () => {
    const objects = new InMemoryObjectStore();
    const hash = await objects.put('real source text');
    const ctx = makeTestContext({ objects });
    // A proposal failing policy (sink/disclosure) AND carrying a hallucinated located-quote.
    const edit = badEdit();
    const hallucinated: Evidence = {
      kind: 'located-quote',
      primary: { summary: 's' },
      provenance: { sources: [], createdAt: 0, toolName: 'x' },
      grounding: { kind: 'located-quote', quote: 'not in the source', sourceHash: hash, offset: 0 },
    };
    const withEv: ProposedEdit = { ...edit, evidence: [hallucinated] };
    const outcome = await runEnforcement(CALL, resultWith(withEv), ctx, enforcementPipeline(POLICY));
    expect(outcome.ok).toBe(false);
    // Grounding-first: the anti-hallucination reason leads; policy reasons follow. This pins the
    // factory's declared order — flip here AND in enforcementPipeline together if it ever changes.
    expect(outcome.reasons[0]).toMatch(/does not appear|hallucinat/i);
    expect(outcome.reasons.some((r) => /sink .* not permitted/i.test(r))).toBe(true);
  });

  test('a clean proposal passes the whole chain', async () => {
    const ctx = makeTestContext();
    const repaired = await repairCitationTool.execute(
      { title: 'Demo', wiki: 'en', wikitext: WIKITEXT, revision: 1 },
      ctx,
    );
    const ok = await runEnforcement(CALL, repaired, ctx, enforcementPipeline(POLICY));
    expect(ok.ok).toBe(true);
  });
});

describe('the SAME pipeline is honored by BOTH the loop and improve() (closes the #7 drift risk)', () => {
  const alwaysFail: PostToolUseHook = {
    name: 'always-fail',
    verify: () => ({ ok: false, reason: 'belt-and-suspenders' }),
  };

  function ctxFor(): { ctx: ReturnType<typeof makeTestContext>; wiki: InMemoryWikiClient } {
    const url = buildWikitextQueryUrl('en', 'Demo');
    const wiki = new InMemoryWikiClient();
    const http = new StubHttpClient({
      [`GET ${url}`]: {
        status: 200,
        headers: {},
        body: JSON.stringify({
          query: { pages: [{ title: 'Demo', revisions: [{ revid: 1, slots: { main: { content: WIKITEXT } } }] }] },
        }),
      },
    });
    return { ctx: makeTestContext({ http, wiki, storage: new InMemoryStorage() }), wiki };
  }

  test('the loop rejects a proposing tool when a hook in the chain fails', async () => {
    const { ctx } = ctxFor();
    const pipeline = [...enforcementPipeline(POLICY), alwaysFail];
    const run = await runAgentLoop({
      messages: [{ role: 'user', content: 'go' }],
      tools: [repairCitationTool],
      ctx: { ...ctx, model: new ScriptedModelClient([modelToolCalls(toolCall('repair_citation', { title: 'Demo', wiki: 'en', wikitext: WIKITEXT, revision: 1 }))]) },
      postHooks: pipeline,
      maxSteps: 2,
    });
    expect(run.proposals).toHaveLength(0); // never collected — the chain rejected it
    expect(run.events.some((e) => e.type === 'tool_rejected')).toBe(true);
  });

  test('improve() suppresses the same proposal when the same pipeline is injected', async () => {
    const { ctx, wiki } = ctxFor();
    const pipeline = [...enforcementPipeline(POLICY), alwaysFail];
    const svc = new SessionService(ctx, POLICY, { enforcement: pipeline });
    const session = await svc.improve({ title: 'Demo', wiki: 'en' });
    expect(session.proposals).toHaveLength(0);
    expect(session.events.some((e) => e.type === 'proposal_suppressed')).toBe(true);
    expect(wiki.appliedEdits).toHaveLength(0);
  });
});
```

**Step 2: Run — the first describe passes now; the second `describe` needs the Task 6 constructor seam**

```
npx vitest run packages/agent/src/enforcement.test.ts
```
Expected: the `runEnforcement`/`enforcementPipeline` tests pass; the "improve() suppresses … same
pipeline injected" test FAILS until Task 6 adds the optional `enforcement` constructor param. (The
loop test passes now — `runAgentLoop` already accepts `postHooks`.)

**Step 3: Commit (after Task 6 makes it green — see Task 6 Step 4)**

---

## Task 6: Refactor `improve()` onto the pipeline (behavior-preserving)

**Files:**
- Modify: `packages/agent/src/hitl.ts` (imports, constructor, the `improve()` suppression block)

**Step 1: Update imports** (`hitl.ts:9-21`)

- Remove the `groundingVerifier` import from `@wikiharness/hooks` (now reached via the pipeline).
- Keep `evaluatePolicy` + `Policy` from `@wikiharness/policy` (still used by `commitDecision`'s
  edit re-gate — that is a separate gate, intentionally unchanged).
- Add `PostToolUseHook` to the `@wikiharness/core` type import.
- Add `import { enforcementPipeline, runEnforcement } from './enforcement.ts';`

Resulting import block:

```ts
import {
  hashProposedEdit,
  sha256Hex,
  type PostToolUseHook,
  type ProposalRecord,
  type ProposalStatus,
  type ProposedEdit,
  type ToolCall,
  type ToolContext,
  type ToolResult,
} from '@wikiharness/core';
import { evaluatePolicy, type Policy } from '@wikiharness/policy';
import { ProposalStore } from '@wikiharness/outputs';
import { commitConfirmedEditTool, getWikitextTool, repairCitationTool } from '@wikiharness/tools';
import { enforcementPipeline, runEnforcement } from './enforcement.ts';
```

**Step 2: Add an `opts` constructor object + the `enforcement` field** (`hitl.ts:60-68`)

Use a single trailing `opts` object (NOT a positional param) so later phases extend it without
churn: Phase 2 adds `findingStore` to the body; Phase 3 adds `opts.capabilities`/`opts.enhancers`.
Existing `new SessionService(ctx, policy)` callers are unaffected (opts defaults to `{}`).

```ts
export class SessionService {
  private readonly store: ProposalStore;
  private readonly enforcement: readonly PostToolUseHook[];

  constructor(
    private readonly ctx: ToolContext,
    private readonly policy: Policy,
    opts: { enforcement?: readonly PostToolUseHook[] } = {},
  ) {
    this.store = new ProposalStore(ctx.storage);
    this.enforcement = opts.enforcement ?? enforcementPipeline(policy);
  }
```

**Step 3: Replace the inline suppression block** (`hitl.ts:95-116`)

Replace the body of the `for (const proposal of repair.proposedEdits ?? [])` loop down through the
`continue;` with:

```ts
    for (const proposal of repair.proposedEdits ?? []) {
      // ONE enforcement pipeline (policy + grounding) — the same chain the agent loop runs (#7).
      const enforced = await runEnforcement(
        SESSION_CALL,
        { data: {}, provenance: proposal.provenance, rationale: '', proposedEdits: [proposal] },
        this.ctx,
        this.enforcement,
      );
      if (!enforced.ok) {
        events.push({ type: 'proposal_suppressed', proposalId: proposal.id, reasons: enforced.reasons });
        await this.store.appendAudit(sessionId, {
          event: 'suppressed',
          proposalId: proposal.id,
          reasons: enforced.reasons,
          at: this.ctx.clock.now(),
        });
        continue;
      }
      await this.store.save({
        proposal,
        status: 'AWAITING_CONFIRMATION',
        sessionId,
        updatedAt: this.ctx.clock.now(),
      });
      await this.store.appendAudit(sessionId, {
        event: 'proposed',
        proposalId: proposal.id,
        at: this.ctx.clock.now(),
      });
      events.push({ type: 'proposal_pending', proposalId: proposal.id });
    }
```

(The `proposedEdits` synthetic result object is structurally a `ToolResult<unknown>` — same shape
that was passed to `groundingVerifier.verify` before. The `ToolResult`/`ToolCall` type imports
remain used by `SESSION_CALL` and `commitDecision`.)

**Step 4: Run the full Phase-1 surface — everything green**

```
npx vitest run packages/agent packages/hooks packages/core packages/policy
```
Expected: green — including the Task-1 characterization tests (behavior preserved), the Task-5
"improve() suppresses … same pipeline injected" test (now wired via the constructor seam), and the
existing `hitl.test.ts` happy-path + commit tests.

**Step 5: Full CI gate**

```
npm run ci
```
Expected: green (format, typecheck, lint, boundaries, stubscan, coverage ≥90% on core/tools, eval).
If `lint`/`format` complain, run `npm run format` (writes) and re-run.

**Step 6: Commit**

```bash
git add packages/agent/src/hitl.ts packages/agent/src/enforcement.test.ts
git commit -m "refactor(agent): route improve() through the shared enforcement pipeline (closes #7)"
```

---

## Phase 1 Done When

- `improve()` no longer contains an inline policy+grounding block; it calls `runEnforcement` over
  `enforcementPipeline(this.policy)`.
- The characterization tests (Task 1) are still green → behavior preserved (suppress on
  policy/grounding fail, `proposal_suppressed` event, `suppressed` audit, no write).
- A test proves the SAME pipeline array is honored by BOTH `runAgentLoop` (postHooks) and
  `improve()` (constructor seam) → #7 drift risk closed.
- `groundingVerifier` verifies `result.findings[].evidence`, the `source-fetched` profile, and
  rejects empty quotes; its existing `located-quote`/`diff` tests still pass.
- `Finding.evidence`, `GroundingAssertion`'s `source-fetched` variant, and `PostVerification.reasons`
  exist in `core`.
- `npm run ci` is green.

**Chainlink:** this phase closes **#7**. Run `chainlink close 7` after the phase's PR/branch merges
(or per the project's close convention) — do **not** close before `npm run ci` is green.
