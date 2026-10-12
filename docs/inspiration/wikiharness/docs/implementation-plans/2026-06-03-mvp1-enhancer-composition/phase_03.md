# Phase 3: Enhancer Registry + `bare-url-repair` + `improve()` Rewrite — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `ed3d-plan-and-execute:executing-an-implementation-plan`.

**Goal:** Turn `improve()` from a single hard-wired `repair_citation` call into a **capability-gated
registry loop** over `WikipediaArticleEnhancer`s, routing every output through the Phase-1
enforcement pipeline and persisting edits → `ProposalStore`, findings → `FindingStore`. Ship the
first real enhancer, `bare-url-repair` (wraps the existing `repair_citation` tool).

**Scope note (decided 2026-06-03):** `wayback-archive` is **deferred to #9** — attaching an
`archive-url=` to an arbitrary citation needs the wikitext-AST that #9 builds. This work ships
**two** enhancers: `bare-url-repair` (here) and `citation-verify` (Phase 5). File a chainlink note
for wayback-archive post-#9 (see Task 4).

**Architecture:** `packages/enhancers` gains the contract (`WikipediaArticleEnhancer`,
`WikipediaArticleSubject`, `EnhancerResult`, `Capability`) + the `ENHANCERS` registry +
`bare-url-repair`. `improve()` (in `agent`) imports `ENHANCERS`; a new `agent → enhancers` import
boundary is added. Outputs route through the shared pipeline via `surfaceProposal`/`surfaceFinding`.

**Tech Stack:** TypeScript (NodeNext ESM), Vitest 3, Zod.

**Scope:** Phase 3 of 6. **Depends on Phase 1** (pipeline) **and Phase 2** (`FindingStore`,
`SessionResult.findings`).

**Codebase verified:** 2026-06-03 (`packages/enhancers/src/index.ts` is `export const ENHANCERS = []`;
`repairCitationTool` at `packages/tools/src/citation/repair-citation.ts:37`).

---

## Task 1: Enhancer contract + `bare-url-repair` + registry (`packages/enhancers`)

**Files:**
- Create: `packages/enhancers/src/types.ts`
- Create: `packages/enhancers/src/bare-url-repair.ts`
- Create: `packages/enhancers/src/bare-url-repair.test.ts`
- Modify: `packages/enhancers/src/index.ts`

**Step 0: Define `Capability` in `core`** (so the server composition root can build the capability
set without a `server → enhancers` boundary — capabilities describe the injected runtime `core`
already defines).

- Create: `packages/core/src/capabilities.ts`
- Modify: `packages/core/src/index.ts` — add `export * from './capabilities.ts';` immediately after
  the existing `export * from './types.ts';` line (currently `:5`); ordering among `export *` lines
  is not significant, but keep it next to `types.ts` for readability.

```ts
/**
 * Runtime capabilities the injected world (ctx/edges) provides, used to gate enhancers (`adr/0005`):
 * 'model' = a usable ModelClient is wired (not the no-op NoModelClient); 'htmlExtractor' = a
 * main-content extractor (ctx.htmlExtractor) is present. The composition root advertises the set;
 * improve() skips an enhancer whose `requires` are not all satisfied.
 */
export type Capability = 'model' | 'htmlExtractor';
```

**Step 1: The contract** (`types.ts`)

```ts
/**
 * The enhancer contract (`adr/0005`, `06`/`07`): a registrable plugin that inspects ONE Wikipedia
 * article and returns zero or more ProposedEdits and/or Findings. Intentionally CONCRETE-Wikipedia
 * (no `Enhancer<TSubject>` generic) — strict YAGNI; generalizing to Wikidata (`09` Phase 3) is a
 * known, bounded future refactor of this interface + the registry.
 */
import type { Capability, Finding, ProposedEdit, ToolContext } from '@wikiharness/core';

/** What improve() builds once per session and hands to every enhancer. */
export interface WikipediaArticleSubject {
  title: string;
  wiki: string;
  wikitext: string;
  revision: number;
  // Phase 5 extends this with `article: ParsedArticle` (the parsed citations citation-verify needs).
}

export interface EnhancerResult {
  proposedEdits?: readonly ProposedEdit[];
  findings?: readonly Finding[];
}

export interface WikipediaArticleEnhancer {
  /** Stable id for logging/audit, e.g. 'bare-url-repair'. */
  name: string;
  /** Capabilities the runtime must provide; improve() skips (and audits) the enhancer otherwise. */
  requires: readonly Capability[];
  analyze(subject: WikipediaArticleSubject, ctx: ToolContext): Promise<EnhancerResult>;
}
```

**Step 2: `bare-url-repair`** (`bare-url-repair.ts`) — wraps `repairCitationTool` (do NOT
re-implement bare-URL detection/templating)

```ts
/**
 * `bare-url-repair` — wrap each bare-URL `<ref>` into a {{cite web}} template. A thin adapter over
 * the existing pure `repair_citation` tool (the bare-URL logic + diff grounding live there);
 * deterministic, so it requires no capabilities.
 */
import { repairCitationTool } from '@wikiharness/tools';
import type { WikipediaArticleEnhancer } from './types.ts';

export const bareUrlRepairEnhancer: WikipediaArticleEnhancer = {
  name: 'bare-url-repair',
  requires: [],
  async analyze(subject, ctx) {
    const res = await repairCitationTool.execute(
      {
        title: subject.title,
        wiki: subject.wiki,
        wikitext: subject.wikitext,
        revision: subject.revision,
      },
      ctx,
    );
    return { proposedEdits: res.proposedEdits ?? [] };
  },
};
```

**Step 3: Registry** (`index.ts`) — replace the placeholder `export const ENHANCERS = [] as const;`

```ts
// Enhancers (registrable plugins composing tools, per `06`/`adr/0005`).
import type { WikipediaArticleEnhancer } from './types.ts';
import { bareUrlRepairEnhancer } from './bare-url-repair.ts';

export * from './types.ts';
export * from './bare-url-repair.ts';

/**
 * The ordered registry improve() iterates. Sequential for deterministic proposal ordering
 * (`08`); per-citation concurrency lives INSIDE an enhancer (Phase 5). `citation-verify` is
 * appended in Phase 5; `wayback-archive` is deferred to #9 (needs the wikitext-AST).
 */
export const ENHANCERS: readonly WikipediaArticleEnhancer[] = [bareUrlRepairEnhancer];
```

**Step 4: Test** (`bare-url-repair.test.ts`)

```ts
import { describe, expect, test } from 'vitest';
import { makeTestContext } from '@wikiharness/edges';
import { bareUrlRepairEnhancer } from './bare-url-repair.ts';

describe('bare-url-repair enhancer (delegates to repair_citation)', () => {
  test('requires no capabilities (deterministic)', () => {
    expect(bareUrlRepairEnhancer.requires).toEqual([]);
  });

  test('wraps each bare-URL ref into a {{cite web}} ProposedEdit', async () => {
    const result = await bareUrlRepairEnhancer.analyze(
      {
        title: 'Demo',
        wiki: 'en',
        wikitext: 'A.<ref>https://a.org/x</ref> B.<ref name="b">https://b.org/y</ref>',
        revision: 1,
      },
      makeTestContext(),
    );
    expect(result.proposedEdits).toHaveLength(2);
    expect(result.proposedEdits!.every((e) => e.after.includes('{{cite web'))).toBe(true);
    expect(result.findings ?? []).toHaveLength(0);
  });

  test('produces no edits for an article with no bare URLs', async () => {
    const result = await bareUrlRepairEnhancer.analyze(
      { title: 'Demo', wiki: 'en', wikitext: 'No refs here.', revision: 1 },
      makeTestContext(),
    );
    expect(result.proposedEdits ?? []).toHaveLength(0);
  });
});
```

**Step 5: Verify**

```
npx vitest run packages/enhancers
npm run typecheck
```
Expected: green.

**Step 6: Commit**

```bash
git add packages/enhancers/src
git commit -m "feat(enhancers): contract + ENHANCERS registry + bare-url-repair (#13)"
```

---

## Task 2: Add the `agent → enhancers` import boundary

`improve()` (in `agent`) will import `ENHANCERS` (in `enhancers`). The `enhancers` package is
**already** in the layer rule as a key (`boundaries.ts:16`: `enhancers: ['core', 'tools', 'outputs',
'policy', 'hooks']`) and already declares those deps in its `package.json` — that part needs **no**
change. What is missing is the `agent → enhancers` *edge*: add `enhancers` to the **`agent`** rule's
allowed list (no cycle — `enhancers` never imports `agent`).

**Files:**
- Modify: `scripts/lib/boundaries.ts:17` (the `agent` rule — NOT the `enhancers` rule at `:16`)
- Modify: `packages/agent/package.json` (declare the dependency)

**Step 1: Add `enhancers` to the `agent` rule** (`boundaries.ts:17`)

```ts
  agent: ['core', 'tools', 'outputs', 'hooks', 'policy', 'enhancers'],
```

**Step 2: Declare the dependency** — add to `packages/agent/package.json` `"dependencies"`:

```json
    "@wikiharness/enhancers": "*",
```

**Step 3: Verify the boundary checker still passes** (and its own tests)

```
npm run boundaries
npx vitest run scripts/lib/boundaries.test.ts
```
Expected: boundaries report 0 violations; `boundaries.test.ts` green (it tests the checker logic,
not the exact rule contents).

**Step 4: Commit**

```bash
git add scripts/lib/boundaries.ts packages/agent/package.json
git commit -m "chore(boundaries): allow agent -> enhancers (improve composes the registry) (#13)"
```

---

## Task 3: Rewrite `improve()` as the registry loop

**Files:**
- Modify: `packages/agent/src/hitl.ts` (imports, `SessionEvent`, constructor `opts`, `improve()`, two new private helpers)
- Create: `packages/agent/src/hitl-registry.test.ts`

**Step 1: Imports** — final `hitl.ts` import block:

```ts
import {
  hashProposedEdit,
  sha256Hex,
  type Capability,
  type Finding,
  type FindingRecord,
  type PostToolUseHook,
  type ProposalRecord,
  type ProposalStatus,
  type ProposedEdit,
  type ToolCall,
  type ToolContext,
} from '@wikiharness/core';
import { evaluatePolicy, type Policy } from '@wikiharness/policy';
import { FindingStore, ProposalStore } from '@wikiharness/outputs';
import { commitConfirmedEditTool, getWikitextTool } from '@wikiharness/tools';
import {
  ENHANCERS,
  type EnhancerResult,
  type WikipediaArticleEnhancer,
  type WikipediaArticleSubject,
} from '@wikiharness/enhancers';
import { enforcementPipeline, runEnforcement } from './enforcement.ts';
```

(`repairCitationTool` is no longer imported here — it is reached via the enhancer.)

**Step 2: Extend `SessionEvent`** (`hitl.ts:48-51`)

```ts
export type SessionEvent =
  | { type: 'state'; state: string }
  | { type: 'proposal_pending'; proposalId: string }
  | { type: 'proposal_suppressed'; proposalId: string; reasons: string[] }
  | { type: 'finding_surfaced'; findingId: string }
  | { type: 'finding_suppressed'; findingId: string; reasons: string[] };
```

**Step 3: Extend the constructor `opts`** (and add the fields)

```ts
export class SessionService {
  private readonly store: ProposalStore;
  private readonly findingStore: FindingStore;
  private readonly enforcement: readonly PostToolUseHook[];
  private readonly capabilities: ReadonlySet<Capability>;
  private readonly enhancers: readonly WikipediaArticleEnhancer[];

  constructor(
    private readonly ctx: ToolContext,
    private readonly policy: Policy,
    opts: {
      enforcement?: readonly PostToolUseHook[];
      capabilities?: ReadonlySet<Capability>;
      enhancers?: readonly WikipediaArticleEnhancer[];
    } = {},
  ) {
    this.store = new ProposalStore(ctx.storage);
    this.findingStore = new FindingStore(ctx.storage);
    this.enforcement = opts.enforcement ?? enforcementPipeline(policy);
    this.capabilities = opts.capabilities ?? new Set();
    this.enhancers = opts.enhancers ?? ENHANCERS;
  }
```

**Step 4: Rewrite `improve()`** (replace the whole method body, `hitl.ts:76-140`)

```ts
  /** Start an "improve" session: load the article, run the enhancer registry, persist outputs. */
  async improve(params: { title: string; wiki: string }): Promise<SessionResult> {
    const sessionId = this.newSessionId(params.title, params.wiki);
    const events: SessionEvent[] = [{ type: 'state', state: 'LOADING' }];

    const wt = await getWikitextTool.execute(params, this.ctx);
    const { wikitext, revision } = wt.data;
    await this.ctx.storage.put<SessionInfo>(`session:${sessionId}`, {
      sessionId,
      title: params.title,
      wiki: params.wiki,
      revision,
    });

    const subject: WikipediaArticleSubject = {
      title: params.title,
      wiki: params.wiki,
      wikitext,
      revision,
    };

    events.push({ type: 'state', state: 'PROPOSING' });
    for (const enhancer of this.enhancers) {
      const missing = enhancer.requires.filter((c) => !this.capabilities.has(c));
      if (missing.length > 0) {
        await this.store.appendAudit(sessionId, {
          event: 'enhancer_skipped',
          enhancer: enhancer.name,
          missing,
          at: this.ctx.clock.now(),
        });
        continue;
      }
      let result: EnhancerResult;
      try {
        result = await enhancer.analyze(subject, this.ctx);
      } catch (err) {
        // A failing enhancer is skipped; partial results from others survive.
        await this.store.appendAudit(sessionId, {
          event: 'enhancer_failed',
          enhancer: enhancer.name,
          reason: err instanceof Error ? err.message : String(err),
          at: this.ctx.clock.now(),
        });
        continue;
      }
      for (const proposal of result.proposedEdits ?? []) {
        await this.surfaceProposal(sessionId, proposal, events);
      }
      for (const finding of result.findings ?? []) {
        await this.surfaceFinding(sessionId, finding, events);
      }
    }

    events.push({ type: 'state', state: 'AWAITING_HUMAN_CONFIRMATION' });
    return {
      sessionId,
      title: params.title,
      wiki: params.wiki,
      revision,
      proposals: await this.proposals(sessionId),
      findings: await this.findings(sessionId),
      events,
    };
  }

  /** Route a proposed edit through the shared enforcement pipeline; persist or suppress + audit. */
  private async surfaceProposal(
    sessionId: string,
    proposal: ProposedEdit,
    events: SessionEvent[],
  ): Promise<void> {
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
      return;
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

  /** Route a finding through the SAME pipeline; persist (immutable) or suppress + audit. */
  private async surfaceFinding(
    sessionId: string,
    finding: Finding,
    events: SessionEvent[],
  ): Promise<void> {
    const enforced = await runEnforcement(
      SESSION_CALL,
      { data: {}, provenance: finding.provenance, rationale: '', findings: [finding] },
      this.ctx,
      this.enforcement,
    );
    if (!enforced.ok) {
      // e.g. a hallucinated supporting quote — suppressed pre-human by the grounding gate.
      events.push({ type: 'finding_suppressed', findingId: finding.id, reasons: enforced.reasons });
      await this.store.appendAudit(sessionId, {
        event: 'finding_suppressed',
        findingId: finding.id,
        reasons: enforced.reasons,
        at: this.ctx.clock.now(),
      });
      return;
    }
    await this.findingStore.save({ finding, sessionId, surfacedAt: this.ctx.clock.now() });
    await this.store.appendAudit(sessionId, {
      event: 'finding_surfaced',
      findingId: finding.id,
      at: this.ctx.clock.now(),
    });
    events.push({ type: 'finding_surfaced', findingId: finding.id });
  }
```

> The Phase-1 `hitl-enforcement.test.ts` characterization (policy-fail → suppressed) and the
> existing `hitl.test.ts` happy path both still pass: `bare-url-repair` produces the same proposals,
> now routed through the same pipeline via `surfaceProposal`.

**Step 5: Write the registry-behavior test** (`hitl-registry.test.ts`)

```ts
import { describe, expect, test } from 'vitest';
import {
  InMemoryStorage,
  InMemoryWikiClient,
  StubHttpClient,
  makeTestContext,
} from '@wikiharness/edges';
import { buildWikitextQueryUrl, type ToolContext } from '@wikiharness/core';
import { bareUrlRepairEnhancer, type WikipediaArticleEnhancer } from '@wikiharness/enhancers';
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
const WIKITEXT = 'A.<ref>https://a.org/x</ref> B.<ref name="b">https://b.org/y</ref> C.';

function ctxFor(): ToolContext {
  const url = buildWikitextQueryUrl('en', 'Demo');
  const http = new StubHttpClient({
    [`GET ${url}`]: {
      status: 200,
      headers: {},
      body: JSON.stringify({
        query: { pages: [{ title: 'Demo', revisions: [{ revid: 100, slots: { main: { content: WIKITEXT } } }] }] },
      }),
    },
  });
  return makeTestContext({ http, wiki: new InMemoryWikiClient(), storage: new InMemoryStorage() });
}

async function auditOf(svc: SessionService, sessionId: string) {
  return (await svc.audit(sessionId)) as Array<{ event?: string; enhancer?: string }>;
}

describe('improve() enhancer registry loop (#13)', () => {
  test('the default registry (bare-url-repair) surfaces one proposal per bare URL', async () => {
    const svc = new SessionService(ctxFor(), POLICY, { enhancers: [bareUrlRepairEnhancer] });
    const session = await svc.improve({ title: 'Demo', wiki: 'en' });
    expect(session.proposals).toHaveLength(2);
    expect(session.findings).toHaveLength(0);
  });

  test('a throwing enhancer is skipped (audited); others still surface their outputs', async () => {
    const boom: WikipediaArticleEnhancer = {
      name: 'boom',
      requires: [],
      analyze: async () => {
        throw new Error('kaboom');
      },
    };
    const svc = new SessionService(ctxFor(), POLICY, { enhancers: [boom, bareUrlRepairEnhancer] });
    const session = await svc.improve({ title: 'Demo', wiki: 'en' });
    expect(session.proposals).toHaveLength(2); // bare-url survived the sibling's failure
    expect((await auditOf(svc, session.sessionId)).some((a) => a.event === 'enhancer_failed' && a.enhancer === 'boom')).toBe(true);
  });

  test('an enhancer whose required capability is absent is skipped (audited), never run', async () => {
    let ran = false;
    const needsModel: WikipediaArticleEnhancer = {
      name: 'needs-model',
      requires: ['model'],
      analyze: async () => {
        ran = true;
        return {};
      },
    };
    const svc = new SessionService(ctxFor(), POLICY, { enhancers: [needsModel], capabilities: new Set() });
    const session = await svc.improve({ title: 'Demo', wiki: 'en' });
    expect(ran).toBe(false);
    expect((await auditOf(svc, session.sessionId)).some((a) => a.event === 'enhancer_skipped' && a.enhancer === 'needs-model')).toBe(true);
  });

  test('a provided capability lets the enhancer run', async () => {
    let ran = false;
    const needsModel: WikipediaArticleEnhancer = {
      name: 'needs-model',
      requires: ['model'],
      analyze: async () => {
        ran = true;
        return {};
      },
    };
    const svc = new SessionService(ctxFor(), POLICY, { enhancers: [needsModel], capabilities: new Set(['model']) });
    await svc.improve({ title: 'Demo', wiki: 'en' });
    expect(ran).toBe(true);
  });
});
```

**Step 6: Verify the agent surface**

```
npx vitest run packages/agent
npm run typecheck
```
Expected: green (registry tests + Phase-1 characterization + Phase-2 findings + existing `hitl.test.ts`).

**Step 7: Commit**

```bash
git add packages/agent/src/hitl.ts packages/agent/src/hitl-registry.test.ts
git commit -m "refactor(agent): improve() iterates the enhancer registry, capability-gated (#13)"
```

---

## Task 4: Full CI + record the wayback-archive deferral

**Step 1: Full gate**

```
npm run ci
```
Expected: green (boundaries now include `agent → enhancers`).

**Step 2: Record the deferral** — append a note to the relevant chainlink issue so wayback-archive
is not lost:

```bash
chainlink comment 13 "wayback-archive enhancer deferred to #9 (wikitext-AST) — attaching archive-url to an existing citation needs the AST. This work ships bare-url-repair + citation-verify only."
```

(If `chainlink comment` is not the exact verb in this repo, run `chainlink --help` and use the
equivalent note/append command; do not invent issue state.)

**Step 3: Commit any remaining staged changes** (none expected beyond Tasks 1–3).

---

## Phase 3 Done When

- `packages/enhancers` exports the contract + a populated `ENHANCERS` registry containing
  `bare-url-repair` (which wraps `repair_citation`, not re-implementing it).
- `improve()` iterates the registry: capability-gated (skips + audits unmet `requires`), try/catch
  (a throwing enhancer is skipped, others survive), routing each output through the Phase-1
  pipeline, persisting edits → `ProposalStore` and findings → `FindingStore`.
- The `agent → enhancers` import boundary exists and `npm run boundaries` is green.
- Phase-1/Phase-2 tests still pass (behavior preserved); `npm run ci` is green.
- The wayback-archive deferral is recorded in chainlink.
