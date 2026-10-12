# Phase 4: Env-Gated Model Wiring — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `ed3d-plan-and-execute:executing-an-implementation-plan`.

**Goal:** Verification turns on by **configuration**, default-closed — the server runs fine with
**no** API key (model-dependent enhancers are skipped by capability gating); with a key, a real
`VercelModelClient` over an open model is wired and the `model` capability is advertised.

**Architecture:** `resolveModelConfig(env)` mirrors `resolveWriteConfig` (pure, default-closed,
unit-tested). `NoModelClient` is the safety net (throws if ever called). The composition root
(`server/main.ts`) builds the real model when enabled (via `@ai-sdk/openai-compatible`, exactly as
`scripts/benchmark.ts` does), builds the `Capability` set, and passes it to `SessionService`. A
startup banner line reports `verification: on/off`.

**Tech Stack:** TypeScript (NodeNext ESM), Vitest 3, `@ai-sdk/openai-compatible` (already a root
devDep; promoted to a `server` dependency here).

**Scope:** Phase 4 of 6. **Depends on Phase 3** (`improve()` capability gating + `opts.capabilities`).

**Codebase verified:** 2026-06-03 (`resolveWriteConfig` + its test; `VercelModelClient`;
`scripts/benchmark.ts` `createOpenAICompatible` wiring; `Capability` is defined in `core` in Phase 3).

---

## Task 1: `resolveModelConfig(env)` — the default-closed model gate

**Files:**
- Create: `packages/server/src/model-config.ts`
- Create: `packages/server/src/model-config.test.ts`

**Step 1: Write the failing test** (`model-config.test.ts`)

```ts
import { describe, expect, test } from 'vitest';
import { resolveModelConfig } from './model-config.ts';

describe('resolveModelConfig (the model gate — default-closed, mirrors resolveWriteConfig)', () => {
  test('DEFAULT-CLOSED: no API key → model disabled, no apiKey/modelInfo', () => {
    const cfg = resolveModelConfig({});
    expect(cfg.modelEnabled).toBe(false);
    expect(cfg.apiKey).toBeUndefined();
    expect(cfg.modelInfo).toBeUndefined();
  });

  test('an empty API key does not enable the model', () => {
    expect(resolveModelConfig({ OPENROUTER_API_KEY: '' }).modelEnabled).toBe(false);
  });

  test('a non-empty API key enables the model and records honest model identity', () => {
    const cfg = resolveModelConfig({ OPENROUTER_API_KEY: 'sk-x' });
    expect(cfg.modelEnabled).toBe(true);
    expect(cfg.apiKey).toBe('sk-x');
    expect(cfg.modelInfo).toEqual({ provider: 'openrouter', model: cfg.model });
  });

  test('model id and base URL are env-overridable (with sane open defaults)', () => {
    const def = resolveModelConfig({ OPENROUTER_API_KEY: 'k' });
    expect(def.baseURL).toContain('openrouter.ai');
    expect(def.model.length).toBeGreaterThan(0);
    const over = resolveModelConfig({
      OPENROUTER_API_KEY: 'k',
      WIKIHARNESS_MODEL: 'org/custom',
      WIKIHARNESS_MODEL_BASE_URL: 'https://example/v1',
    });
    expect(over.model).toBe('org/custom');
    expect(over.baseURL).toBe('https://example/v1');
    expect(over.modelInfo).toEqual({ provider: 'openrouter', model: 'org/custom' });
  });
});
```

**Step 2: Run — expect failure** (`resolveModelConfig` not defined)

```
npx vitest run packages/server/src/model-config.test.ts
```

**Step 3: Implement** (`model-config.ts`)

```ts
/**
 * The model gate, as a pure function of the environment — mirrors resolveWriteConfig (`02 §8`,
 * `adr/0006` open-model posture). Verification is OFF unless an API key is present; default-closed
 * so the server runs key-free (model-dependent enhancers are then skipped by capability gating).
 * The model id + base URL are env-overridable but default to a pinned OPEN model via OpenRouter.
 */
export interface ModelConfig {
  /** True iff a usable model is wired (an API key is present). */
  modelEnabled: boolean;
  /** Provider API key — present iff `modelEnabled`. */
  apiKey?: string;
  /** OpenAI-compatible base URL (default: OpenRouter). */
  baseURL: string;
  /** The model id to call (a pinned open model by default). */
  model: string;
  /** Honest model identity for provenance — present iff `modelEnabled`. Never a secret. */
  modelInfo?: { provider: string; model: string };
}

const DEFAULT_BASE_URL = 'https://openrouter.ai/api/v1';
// A pinned OPEN model (`adr/0006`), drawn from the benchmark panel (scripts/benchmark.ts
// DEFAULT_PANEL). Overridable via WIKIHARNESS_MODEL.
const DEFAULT_MODEL = 'mistralai/mistral-small-3.2-24b-instruct';

export function resolveModelConfig(env: Readonly<Record<string, string | undefined>>): ModelConfig {
  const apiKey = env.OPENROUTER_API_KEY;
  const baseURL = env.WIKIHARNESS_MODEL_BASE_URL ?? DEFAULT_BASE_URL;
  const model = env.WIKIHARNESS_MODEL ?? DEFAULT_MODEL;
  const modelEnabled = typeof apiKey === 'string' && apiKey.length > 0;
  if (!modelEnabled) return { modelEnabled: false, baseURL, model };
  return { modelEnabled: true, apiKey, baseURL, model, modelInfo: { provider: 'openrouter', model } };
}
```

**Step 4: Run — green; commit**

```
npx vitest run packages/server/src/model-config.test.ts
```
```bash
git add packages/server/src/model-config.ts packages/server/src/model-config.test.ts
git commit -m "feat(server): resolveModelConfig — default-closed model gate (#13)"
```

---

## Task 2: `NoModelClient` safety net

**Files:**
- Create: `packages/edges/src/model/no-model.ts`
- Create: `packages/edges/src/model/no-model.test.ts`
- Modify: `packages/edges/src/index.ts`

**Step 1: Write the failing test** (`no-model.test.ts`)

```ts
import { describe, expect, test } from 'vitest';
import { NoModelClient } from './no-model.ts';

describe('NoModelClient (default-closed safety net)', () => {
  test('throws if complete() is ever called', async () => {
    await expect(new NoModelClient().complete({ messages: [] })).rejects.toThrow(/not enabled|no api key/i);
  });
});
```

**Step 2: Implement** (`no-model.ts`)

```ts
import type { ModelClient, ModelRequest, ModelResponse } from '@wikiharness/core';

/**
 * The default-closed model safety net (`adr/0006`). Wired when no API key is configured: the server
 * runs key-free and model-dependent enhancers are SKIPPED by capability gating, so this is never
 * reached on the happy path. If something does call it, it throws loudly rather than silently
 * producing an empty/garbage verdict (which would corrupt the anti-hallucination accounting).
 */
export class NoModelClient implements ModelClient {
  async complete(_req: ModelRequest): Promise<ModelResponse> {
    throw new Error(
      'NoModelClient: model verification is not enabled (no API key). Set OPENROUTER_API_KEY to ' +
        'enable, or rely on capability gating to skip model-dependent enhancers.',
    );
  }
}
```

**Step 3: Export it** (`edges/src/index.ts`) — add beside the other model exports:

```ts
export * from './model/scripted.ts';
export * from './model/vercel.ts';
export * from './model/no-model.ts';
```

**Step 4: Run — green; commit**

```
npx vitest run packages/edges/src/model/no-model.test.ts
```
```bash
git add packages/edges/src/model/no-model.ts packages/edges/src/model/no-model.test.ts packages/edges/src/index.ts
git commit -m "feat(edges): NoModelClient safety net (#13)"
```

---

## Task 3: Wire the model + capabilities into the server composition root

**Files:**
- Modify: `packages/server/package.json` (add the `@ai-sdk/openai-compatible` dependency)
- Modify: `packages/server/src/main.ts`

**Step 1: Add the dependency** — in `packages/server/package.json` `"dependencies"`, add (match the
root pin; confirm the exact version with `grep '@ai-sdk/openai-compatible' package.json`):

```json
    "@ai-sdk/openai-compatible": "^1.0.39",
```

**Step 2: Wire it in `main.ts`**

- Add imports (top of `main.ts`):

```ts
import { createOpenAICompatible } from '@ai-sdk/openai-compatible';
import { resolveModelConfig } from './model-config.ts';
```

- Add `NoModelClient` + `VercelModelClient` to the existing `@wikiharness/edges` import, and **remove**
  `ScriptedModelClient` from it (verified present at `main.ts:18`; it is used only at `main.ts:73`,
  `model: new ScriptedModelClient([])`, which Step 2 below replaces with `model: buildModel()`):

```ts
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
  buildUserAgent,
  type MwnBot,
} from '@wikiharness/edges';
```

- Add `Capability` + `ModelClient` to the `@wikiharness/core` type import:

```ts
import type { Capability, ModelClient, ToolContext, WikiClient } from '@wikiharness/core';
```

- After `const writeConfig = resolveWriteConfig(process.env);`, add the model gate + builder:

```ts
// Verification is OFF by default (default-closed, like the write gate). With OPENROUTER_API_KEY a
// real open-model client is wired (`adr/0006`); without it, NoModelClient + no 'model' capability,
// so model-dependent enhancers (citation-verify) are skipped and NoModelClient is never called.
const modelConfig = resolveModelConfig(process.env);

function buildModel(): ModelClient {
  if (!modelConfig.modelEnabled || modelConfig.apiKey === undefined) {
    return new NoModelClient();
  }
  const provider = createOpenAICompatible({
    name: 'openrouter',
    baseURL: modelConfig.baseURL,
    apiKey: modelConfig.apiKey,
  });
  return new VercelModelClient(provider(modelConfig.model), {
    modelInfo: modelConfig.modelInfo,
    temperature: 0.1,
    maxOutputTokens: 1000,
  });
}
```

- In the `ctx` object, replace `model: new ScriptedModelClient([])` with:

```ts
  model: buildModel(),
```

- After the `ctx` definition, build the capability set and pass it to `SessionService`. Replace
  `const service = new SessionService(ctx, policy);` with:

```ts
// Advertise what the runtime provides so improve() can gate enhancers. The Defuddle extractor is
// always wired; 'model' is advertised only when a key turned verification on.
const capabilities = new Set<Capability>(['htmlExtractor']);
if (modelConfig.modelEnabled) capabilities.add('model');

const service = new SessionService(ctx, policy, { capabilities });
```

- Add a `verification:` line to the startup banner (after the existing `WikiHarness [...] listening`
  line):

```ts
  console.log(`WikiHarness [${mode}] listening on http://localhost:${PORT}`);
  console.log(
    `  verification: ${modelConfig.modelEnabled ? `on (${modelConfig.model})` : 'off (no API key — model-dependent enhancers skipped)'}`,
  );
```

**Step 3: Verify the server still starts both ways**

```
npm run typecheck
# key-free start (Ctrl-C after the banner prints):
node packages/server/src/main.ts
# expected banner includes:  verification: off (no API key — model-dependent enhancers skipped)
```

(Optionally, with a key from `../alex-cite-checker/.env` — owner-authorized — confirm the banner
reads `verification: on (...)`. This is opt-in/network; not part of `npm run ci`.)

**Step 4: Full CI + commit**

```
npm run ci
```
Expected: green (model-config + no-model unit tests; the server/CLI contract tests are unchanged —
they construct `SessionService`/`startServer` directly with `makeTestContext`, never `main.ts`).

```bash
git add packages/server/package.json packages/server/src/main.ts
git commit -m "feat(server): wire env-gated model + capabilities into the composition root (#13)"
```

---

## Phase 4 Done When

- `resolveModelConfig` is unit-tested (no key → disabled; key → enabled with honest `modelInfo`;
  model/baseURL env-overridable), mirroring `resolveWriteConfig`.
- `NoModelClient` throws if called and is exported from `@wikiharness/edges`.
- `main.ts` wires a real `VercelModelClient` (open model via `@ai-sdk/openai-compatible`) when a key
  is present, else `NoModelClient`; builds the `Capability` set (`htmlExtractor` always, `model`
  iff enabled) and passes it to `SessionService`; the banner reports `verification: on/off`.
- The server starts both with and without a key; `npm run ci` is green.
