# Phase 2: Finding Plumbing End-to-End — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `ed3d-plan-and-execute:executing-an-implementation-plan`.

**Goal:** A `Finding` can be **persisted** (immutable), **served** over the API, and **rendered** in
the CLI — independent of any enhancer. This is the second output channel alongside `ProposedEdit`s;
findings **never** enter the confirmation/commit path.

**Architecture:** `FindingStore` mirrors `ProposalStore` (minus the status machine) over the same
`Storage` edge, sharing key helpers extracted to `keys.ts` (anti-duplication). `SessionResult`
gains `findings`; `SessionService` gains a `findings(sessionId)` reader; the server gains
`GET /api/sessions/{id}/findings`; the CLI gains `printFinding` (read-only, no decide affordance).

**Tech Stack:** TypeScript (NodeNext ESM), Vitest 3.

**Scope:** Phase 2 of 6. **Depends on Phase 1** (`Finding.evidence`).

**Codebase verified:** 2026-06-03.

---

## Task 1: `FindingRecord` core type + shared key helpers + `FindingStore`

**Files:**
- Modify: `packages/core/src/types.ts` — add the `FindingRecord` interface immediately **after** the
  `ProposalRecord` interface (the last interface in the file; it ends around `:178`, though Phase 1
  grew the file slightly — anchor on "after `ProposalRecord`", not the exact line)
- Create: `packages/outputs/src/keys.ts`
- Modify: `packages/outputs/src/proposal-store.ts` (use the shared keys — DRY)
- Create: `packages/outputs/src/finding-store.ts`
- Create: `packages/outputs/src/finding-store.test.ts`
- Modify: `packages/outputs/src/index.ts`

**Step 1: Add `FindingRecord`** (`types.ts`, after the `ProposalRecord` interface)

```ts
/**
 * A persisted finding row — IMMUTABLE (no status machine, no confirmation token). A finding is a
 * read-only verdict; it is surfaced and read, never decided or written to a wiki.
 */
export interface FindingRecord {
  finding: Finding;
  sessionId: string;
  /** ms epoch when the finding was first surfaced. */
  surfacedAt: number;
}
```

**Step 2: Extract shared key helpers** (`keys.ts`)

```ts
/**
 * Session-scoped storage keys, shared by ProposalStore and FindingStore so the two stores can
 * never drift on key format. `audit:{sessionId}` is the single per-session ledger both write to.
 */
export function proposalKey(sessionId: string, proposalId: string): string {
  return `proposal:${sessionId}:${proposalId}`;
}
export function proposalPrefix(sessionId: string): string {
  return `proposal:${sessionId}:`;
}
export function findingKey(sessionId: string, findingId: string): string {
  return `finding:${sessionId}:${findingId}`;
}
export function findingPrefix(sessionId: string): string {
  return `finding:${sessionId}:`;
}
export function auditStream(sessionId: string): string {
  return `audit:${sessionId}`;
}
```

**Step 3: Refactor `proposal-store.ts` to use the shared keys** (behavior-preserving)

Delete the three local functions (`proposalKey`, `proposalPrefix`, `auditStream`, `:10-18`) and add
an import at the top:

```ts
import type { ProposalRecord, Storage } from '@wikiharness/core';
import { auditStream, proposalKey, proposalPrefix } from './keys.ts';
```

(The rest of `ProposalStore` is unchanged — it already calls `proposalKey`/`proposalPrefix`/
`auditStream`.)

**Step 4: Create `FindingStore`** (`finding-store.ts`)

```ts
/**
 * Immutable finding persistence (`adr/0005`) — the second half of the outputs store. Mirrors
 * {@link ProposalStore} over the same Storage edge and key scheme, MINUS the status machine: a
 * finding is written once and only ever read (it is a read-only verdict, never decided/committed).
 * Finding lifecycle audit entries (`finding_surfaced`/`finding_suppressed`) go on the shared
 * `audit:{sessionId}` ledger via the existing ProposalStore.appendAudit — not duplicated here.
 */
import type { FindingRecord, Storage } from '@wikiharness/core';
import { findingKey, findingPrefix } from './keys.ts';

export class FindingStore {
  constructor(private readonly storage: Storage) {}

  async save(record: FindingRecord): Promise<void> {
    await this.storage.put(findingKey(record.sessionId, record.finding.id), record);
  }

  async get(sessionId: string, findingId: string): Promise<FindingRecord | undefined> {
    return this.storage.get<FindingRecord>(findingKey(sessionId, findingId));
  }

  async list(sessionId: string): Promise<FindingRecord[]> {
    const keys = await this.storage.list(findingPrefix(sessionId));
    const records: FindingRecord[] = [];
    for (const key of keys) {
      const record = await this.storage.get<FindingRecord>(key);
      if (record) records.push(record);
    }
    return records.sort((a, b) =>
      a.finding.id.localeCompare(b.finding.id, undefined, { numeric: true }),
    );
  }
}
```

**Step 5: Export it** (`index.ts`)

```ts
// @wikiharness/outputs — consumer-agnostic findings/proposals store.

export * from './proposal-store.ts';
export * from './finding-store.ts';
```

**Step 6: Write `finding-store.test.ts`**

```ts
import { describe, expect, test } from 'vitest';
import { InMemoryStorage } from '@wikiharness/edges';
import type { FindingRecord } from '@wikiharness/core';
import { FindingStore } from './finding-store.ts';

function record(id: string, sessionId = 'sess-1'): FindingRecord {
  return {
    finding: {
      id,
      kind: 'citation-verdict',
      summary: `verdict for ${id}`,
      provenance: { sources: [], createdAt: 0, toolName: 'citation_verify' },
      evidence: [],
    },
    sessionId,
    surfacedAt: 1,
  };
}

describe('FindingStore (immutable, mirrors ProposalStore minus the status machine)', () => {
  test('save → get round-trips a finding', async () => {
    const store = new FindingStore(new InMemoryStorage());
    await store.save(record('cv-0'));
    expect((await store.get('sess-1', 'cv-0'))?.finding.id).toBe('cv-0');
    expect(await store.get('sess-1', 'nope')).toBeUndefined();
  });

  test('list returns a session’s findings in numeric-aware id order, scoped by session', async () => {
    const store = new FindingStore(new InMemoryStorage());
    await store.save(record('cv-10'));
    await store.save(record('cv-2'));
    await store.save(record('cv-0', 'other-session'));
    const got = await store.list('sess-1');
    expect(got.map((r) => r.finding.id)).toEqual(['cv-2', 'cv-10']); // numeric sort, other-session excluded
  });
});
```

**Step 7: Verify** (the refactor keeps proposal-store green; finding-store passes)

```
npx vitest run packages/outputs
npm run typecheck
```
Expected: all green (incl. existing `proposal-store.test.ts`).

**Step 8: Commit**

```bash
git add packages/core/src/types.ts packages/outputs/src
git commit -m "feat(outputs): FindingStore + FindingRecord + shared key helpers (#13)"
```

---

## Task 2: `SessionResult.findings` + `SessionService.findings()`

**Files:**
- Modify: `packages/agent/src/hitl.ts` (imports, `SessionResult`, constructor, `improve()` return, new `findings()` method)
- Create: `packages/agent/src/hitl-findings.test.ts`

**Step 1: Imports** — extend the `@wikiharness/core` type import with `FindingRecord`, and the
`@wikiharness/outputs` import with `FindingStore`:

```ts
import {
  hashProposedEdit,
  sha256Hex,
  type FindingRecord,
  type PostToolUseHook,
  type ProposalRecord,
  type ProposalStatus,
  type ProposedEdit,
  type ToolCall,
  type ToolContext,
} from '@wikiharness/core';
import { FindingStore, ProposalStore } from '@wikiharness/outputs';
```

**Step 2: Add `findings` to `SessionResult`** (`hitl.ts:53-56`)

```ts
export interface SessionResult extends SessionInfo {
  proposals: ProposalRecord[];
  findings: FindingRecord[];
  events: SessionEvent[];
}
```

**Step 3: Construct the `FindingStore`** (in the constructor body, beside `this.store`)

```ts
  private readonly findingStore: FindingStore;
  // …in constructor body:
  this.store = new ProposalStore(ctx.storage);
  this.findingStore = new FindingStore(ctx.storage);
```

**Step 4: Add the reader + populate the result**

Add a method beside `proposals()`:

```ts
  async findings(sessionId: string): Promise<FindingRecord[]> {
    return this.findingStore.list(sessionId);
  }
```

In `improve()`'s `return { … }`, add `findings`:

```ts
    return {
      sessionId,
      title: params.title,
      wiki: params.wiki,
      revision,
      proposals: await this.proposals(sessionId),
      findings: await this.findings(sessionId),
      events,
    };
```

(In Phase 2 no enhancer writes findings yet, so `findings` is `[]` here — the plumbing is proven
with a directly-seeded fixture. Phase 5's `citation-verify` populates it.)

**Step 5: Write `hitl-findings.test.ts`**

```ts
import { describe, expect, test } from 'vitest';
import { InMemoryStorage, makeTestContext } from '@wikiharness/edges';
import { FindingStore } from '@wikiharness/outputs';
import type { FindingRecord } from '@wikiharness/core';
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

function fixture(): FindingRecord {
  return {
    finding: {
      id: 'cv-0',
      kind: 'citation-verdict',
      summary: 'NOT_SUPPORTED — The sky is green.',
      provenance: {
        sources: [{ url: 'https://example.org/a', fetchedAt: 0, contentHash: 'h' }],
        createdAt: 0,
        toolName: 'citation_verify',
      },
      evidence: [],
    },
    sessionId: 'sess-x',
    surfacedAt: 1,
  };
}

describe('SessionService findings channel (read-only)', () => {
  test('findings() returns persisted findings for a session', async () => {
    const storage = new InMemoryStorage();
    const svc = new SessionService(makeTestContext({ storage }), POLICY);
    await new FindingStore(storage).save(fixture());
    const got = await svc.findings('sess-x');
    expect(got).toHaveLength(1);
    expect(got[0]!.finding.summary).toContain('NOT_SUPPORTED');
  });
});
```

**Step 6: Verify**

```
npx vitest run packages/agent
npm run typecheck
```
Expected: green — including the existing `hitl.test.ts` (its happy-path assertions don't touch
`findings`, and `SessionResult.findings` is `[]` for those sessions).

**Step 7: Commit**

```bash
git add packages/agent/src/hitl.ts packages/agent/src/hitl-findings.test.ts
git commit -m "feat(agent): SessionResult.findings + SessionService.findings() (#13)"
```

---

## Task 3: `GET /api/sessions/{id}/findings` route

**Files:**
- Modify: `packages/server/src/app.ts` (`apiRoutes`, add a route beside the `proposals` GET)
- Modify: `packages/server/src/app.test.ts` (add a test)

**Step 1: Add the route** — in `apiRoutes`, after the `GET …/proposals` route (`app.ts:104-108`):

```ts
    {
      method: 'GET',
      pattern: /^\/api\/sessions\/([^/]+)\/findings$/,
      handle: async (m, _req, res) => sendJson(res, 200, await service.findings(sid(m, 1))),
    },
```

**Step 2: Write the test** (append to `app.test.ts`)

```ts
import { FindingStore } from '@wikiharness/outputs';
import type { AddressInfo } from 'node:net';

test('GET /api/sessions/:id/findings returns persisted findings (read-only channel)', async () => {
  const storage = new InMemoryStorage();
  const ctx = makeTestContext({ storage });
  server = await startServer(new SessionService(ctx, POLICY), 0);
  const { port } = server.address() as AddressInfo;
  const base = `http://127.0.0.1:${port}`;

  await new FindingStore(storage).save({
    finding: {
      id: 'cv-0',
      kind: 'citation-verdict',
      summary: 'NOT_SUPPORTED — x',
      provenance: { sources: [{ url: 'https://s', fetchedAt: 0, contentHash: 'h' }], createdAt: 0, toolName: 'citation_verify' },
      evidence: [],
    },
    sessionId: 'sess-x',
    surfacedAt: 1,
  });

  const findings = (await (await fetch(`${base}/api/sessions/sess-x/findings`)).json()) as Array<{ finding: { id: string } }>;
  expect(findings).toHaveLength(1);
  expect(findings[0]!.finding.id).toBe('cv-0');
});
```

(`AddressInfo` is already imported at the top of `app.test.ts`; the duplicate `import type` line
above is illustrative — fold it into the existing import. `InMemoryStorage`/`makeTestContext`/
`SessionService` are already imported.)

**Step 3: Verify**

```
npx vitest run packages/server/src/app.test.ts
```
Expected: green (existing routes + the new findings route).

**Step 4: Commit**

```bash
git add packages/server/src/app.ts packages/server/src/app.test.ts
git commit -m "feat(server): GET /api/sessions/:id/findings route (#13)"
```

---

## Task 4: CLI `printFinding` + render findings in `improve` output

**Files:**
- Modify: `packages/cli/src/main.ts` (import `FindingRecord`, add `printFinding`, render findings)
- Modify: `packages/cli/src/main.test.ts` (add a test)

**Step 1: Import `FindingRecord`** (`main.ts:14`)

```ts
import { hashProposedEdit, renderDiff, type FindingRecord, type ProposalRecord } from '@wikiharness/core';
```

**Step 2: Add `printFinding`** (beside `printProposal`)

```ts
/**
 * Render a read-only Finding (a citation verdict). NO confirmation token / decide affordance —
 * findings change nothing on-wiki. The verdict rides in `summary`; the located quote (if any) and
 * the source URL come from the evidence/provenance.
 */
function printFinding(deps: CliDeps, record: FindingRecord): void {
  const f = record.finding;
  deps.out(`\n◆ ${f.id}  [${f.kind}]`);
  deps.out(`  ${f.summary}`);
  const quote = f.evidence.map((e) => e.primary.highlight?.quote).find((q): q is string => Boolean(q));
  deps.out(quote ? `    quote: “${quote}”` : '    (no supporting quote)');
  for (const s of f.provenance.sources) deps.out(`    source: ${s.url}`);
}
```

**Step 3: Render findings in the `improve` handler** — replace the body of `improve()`
(`main.ts:59-77`) with:

```ts
async function improve(deps: CliDeps, title: string, wiki: string): Promise<void> {
  const session = (await api(deps, '/api/sessions', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ title, wiki }),
  })) as {
    sessionId: string;
    revision: number;
    proposals: ProposalRecord[];
    findings?: FindingRecord[];
  };

  deps.out(`session ${session.sessionId} — "${title}" (rev ${session.revision})`);
  if (session.proposals.length === 0) {
    deps.out('No bare-URL citations found. Try another article.');
  } else {
    deps.out(`${session.proposals.length} proposal(s):`);
    for (const record of session.proposals) printProposal(deps, record);
    deps.out(
      `\nTo approve: decide ${session.sessionId} <proposalId> approve <confirmationToken>` +
        `\nTo edit:    edit ${session.sessionId} <proposalId> <confirmationToken> <new wikitext…>`,
    );
  }

  const findings = session.findings ?? [];
  if (findings.length > 0) {
    deps.out(`\n${findings.length} finding(s) (read-only — no action needed):`);
    for (const record of findings) printFinding(deps, record);
  }
}
```

**Step 4: Write the test** (append to `main.test.ts`, in the `CLI improve / decide / audit verbs` describe)

```ts
test('improve renders read-only findings (verdict, quote/absence, source) with no decide affordance', async () => {
  const { impl } = fakeFetch({
    sessionId: 'sess-9',
    revision: 5,
    proposals: [],
    findings: [
      {
        finding: {
          id: 'cv-0',
          kind: 'citation-verdict',
          summary: 'NOT_SUPPORTED — The sky is green.',
          provenance: { sources: [{ url: 'https://example.org/a', fetchedAt: 0, contentHash: 'h' }], createdAt: 0, toolName: 'citation_verify' },
          evidence: [],
        },
        sessionId: 'sess-9',
        surfacedAt: 1,
      },
    ],
  });
  const { d, lines } = deps(impl);
  await runCli(['improve', 'Some Article', 'en'], d);
  const out = lines.join('\n');
  expect(out).toContain('cv-0');
  expect(out).toContain('NOT_SUPPORTED');
  expect(out).toContain('https://example.org/a');
  expect(out).toContain('(no supporting quote)');
  expect(out).not.toMatch(/confirmationToken|approve <conf/i); // findings never offer a write action
});
```

**Step 5: Verify** (existing CLI tests stay green — the no-bare-URL message still prints)

```
npx vitest run packages/cli/src/main.test.ts
```
Expected: green.

**Step 6: Full CI + commit**

```
npm run ci
```
Expected: green.

```bash
git add packages/cli/src/main.ts packages/cli/src/main.test.ts
git commit -m "feat(cli): printFinding — render read-only findings in improve output (#13)"
```

---

## Phase 2 Done When

- `FindingStore` persists/reads findings immutably over `Storage`, sharing key helpers with
  `ProposalStore` (no duplicated key logic).
- `SessionResult.findings` + `SessionService.findings()` exist; `improve()` returns the findings
  channel (empty until Phase 5).
- `GET /api/sessions/{id}/findings` returns persisted findings; the existing routes are unchanged.
- The CLI renders findings read-only (verdict summary, quote-or-absence, source URL) with **no**
  confirmation/decide affordance.
- A fixture finding round-trips store → API → CLI in tests; `npm run ci` is green.
