# 02 — Constraints, Policy & Governance

> **Staging — WHAT (converging).** Vision / scope / posture / findings: close to settled, changes deliberately. Architecture & mechanics (the *how*) live in `06`–`10` and stay tentative until base-framework sign-off — see [Staging](../README.md#staging).

These are the **current operating rules** the harness must *encode and enforce* —
not merely document. Where a rule can be expressed as code (a guard, a schema, an
eval gate), it must be. They bind our *behavior today*.

> **Constraints vs. ceiling.** Treat everything here as the rules we operate under
> *now* and engineer to *exceed* — not as a permanent ceiling. The project's
> mission is to **move** several of these boundaries by demonstrating quality with
> firm, reproducible evidence (`11-evidence-and-policy-strategy.md`,
> `docs/adr/0003`). Changing a rule happens only through Wikipedia's own
> processes (BRFA — Wikipedia's *Bot requests for approval* — and RfC), with data
> as the argument, and never by quietly
> exceeding the rule in production. Until a boundary is moved by consensus, the
> rule as written here is enforced.

> Note on sourcing: policy summaries below were gathered via web research in
> mid-2026 and reflect the state at that time. Wikipedia policy evolves by
> community consensus; the harness must treat policy thresholds as **configurable
> data** (see "Policy-as-data") and must re-verify current policy, not hard-code
> 2026 assumptions, before any write path is enabled.

## 1. The LLM-text policy is the defining constraint

English Wikipedia's large-language-model guideline (RfC closed **20 March 2026**)
is, in effect, a **near-blanket prohibition on using LLMs to generate or rewrite
article *text***, with narrow exceptions (basic copyedits to one's own writing;
first-pass translation under `WP:LLM-assisted translation`). The community's
stated concerns are precisely the ones we must engineer against:

- **Hallucinated statements** — fabricated facts presented as sourced.
- **Fabricated citations** — references that look real but do not exist or do not
  support the claim.
- **Unverifiable content** and **algorithmic bias**.
- The difficulty of *rigorously scrutinizing* high-speed AI edits for compliance.

Sources:
[WP:Large language models](https://en.wikipedia.org/wiki/Wikipedia:Large_language_models),
[WP:Writing articles with large language models](https://en.wikipedia.org/wiki/Wikipedia:Writing_articles_with_large_language_models).

### Engineering consequences (these drive the whole design)

1. **Metadata/non-text first.** Our roadmap leads with citations, archiving,
   Wikidata, and media — additive, *verifiable*, low-controversy edits — and
   defers prose. This is not just product sequencing; it is the path of least
   policy resistance.
2. **No unsourced assertions, ever.** The agent must never *introduce* a factual
   claim. It may only (a) verify existing claims against sources, (b) attach or
   repair sources for existing claims, (c) add structured data that is itself
   directly cited, or (d) propose media. A "proposed edit that adds a sentence
   without a citation" is rejected by a guard before it can reach a human.
3. **Anti-hallucination is a first-class subsystem.** Every citation the agent
   proposes must be *grounded in a fetched, real document* — the URL/DOI/ISBN
   must resolve, the page must be retrievable, and the supporting quote must be
   found in the fetched text (string-locatable), not paraphrased from model
   memory. This is enforced by tooling and eval gates, not prompt etiquette.
4. **Text edits, when eventually built, are gated hardest** — copyedit-only,
   diff-bounded, and explicitly flagged, never free generation.
5. **Algorithmic bias is tested with adversarial datasets, not assumed away.**
   Because we deliberately run *multiple* open-weight backends — including models
   trained under different national and political regimes — bias profiles differ by
   model (§8). We will need **curated evaluation sets of politically contested or
   censorship-sensitive articles** — e.g. Tiananmen Square / Tank Man for
   Chinese-origin models; January 6 and climate change for US-origin models — to
   catch where a given backend silently omits, softens, or skews *well-sourced,
   verifiable* content. These sets feed the eval gates (`05`) and the published
   evidence (`11`); a model that fails them on a topic is not trusted to verify or
   match on that topic.

## 2. Human-in-the-loop is mandatory and architectural

The chosen edit-delivery model is **assisted editing — the human confirms every
edit** (see decision in `01-vision-scope.md` and `docs/adr/0001`). Therefore:

- The agent loop must be able to **pause before any write**, surface the exact
  proposed change (rendered diff + rationale + provenance), and **resume only on
  explicit human approval** (approve / edit-then-approve / reject).
- This pause/resume must work **across a network boundary** (web client) and be
  **durable** (survive a page reload / session resume). This is a primary
  selection criterion for the base framework (`docs/adr/0002`).
- "Human-supremacy" is a constitutional rule: there is **no code path** that
  writes to a public project without a recorded human confirmation token for that
  specific edit.
- **Both targets are governed identically.** Whether the core is improving an
  article or checking someone else's incoming edit, it surfaces a proposal or a
  **verdict for human action and never auto-reverts** — checking a diff is the same
  verification core pointed at a diff, not a separate "patrol" mode (`01`, `adr/0005`).
  Autonomy (reduced or removed per-edit confirmation) is a future, **evidence-gated**
  step reached only via BRFA — not a default and not categorically barred (see `01`
  North star, `docs/adr/0003`).

## 3. Bot & assisted-editing policy

Even human-confirmed semi-automated editing is governed. The harness must comply
with, and where possible *enforce*, the following
([WP:Bot policy](https://en.wikipedia.org/wiki/Wikipedia:Bot_policy),
[API:Edit](https://www.mediawiki.org/wiki/API:Edit),
[Wikimedia API rate limits](https://www.mediawiki.org/wiki/Wikimedia_APIs/Rate_limits)):

- **Authentication:** OAuth 2.0 for acting on behalf of a user (preferred);
  tokens held **server-side only**, never exposed to a browser client (SP42's
  rule — see `docs/adr/0001`).
- **Identity & contact:** a descriptive `User-Agent` with project URL and contact
  email on every request (Wikimedia User-Agent policy).
- **Concurrency & rate:** ≤ 3 concurrent requests; respect `maxlag`; back off on
  rate-limit responses. These limits are enforced centrally in the HTTP layer,
  not left to individual tools.
- **Edit cadence:** assisted edits must not exceed a rate a human can genuinely
  review; the confirm-each-edit UX naturally enforces this, but we also cap it.
- **Approval path:** anything that crosses from "human confirms each" toward
  higher automation requires a **BRFA** and a separate
  bot account. We design for this path but do **not** enable autonomous editing in
  any MVP.
- **Edit summaries** must be honest and informative, and **disclose AI
  assistance** (e.g. a standard suffix and a link to the tool/he session),
  consistent with assisted-editing norms and the spirit of the LLM guideline.

## 4. Provenance, attribution & audit (non-negotiable)

Every proposed edit carries a structured **provenance record**:

- The claim/target it addresses (article, revision id, location).
- Every source consulted (URL/DOI/ISBN/QID), with fetch timestamps and content
  hashes, distinguishing *evidence* from *non-evidence* (Wikipedia/Wikidata URLs
  are circular and must be marked non-evidence — a lesson lifted directly from the
  SIFT prompt).
- The model(s) used, prompt version, and tool-call trace.
- The supporting quote, **string-located** in the fetched source.
- The human confirmation (who, when, approve/edit/reject, final text).

This record is (a) attached to the edit summary / talk-page note where
appropriate, (b) persisted in an append-only **audit ledger**, and (c) the basis
of reproducibility and dispute resolution. (Pattern: SP42's
`PublicAuditLedgerEntry` + explainability-first scoring.)

## 5. Licensing & copyright

- Article contributions are CC BY-SA (and GFDL) per Wikipedia's terms; Wikidata
  is CC0; Commons requires a free license / public domain.
- **Source text is used as evidence, not copied.** Supporting quotes must be
  short, attributed, and within fair-use/quotation norms; the agent must not
  paste source prose into articles.
- For media/data from third-party GLAM sources, the harness must **check and
  record the license** before proposing an upload to Commons, and only propose
  CC0 / public-domain / Commons-compatible items. License determination is a
  tool with its own tests, not an afterthought.

## 6. Policy-as-data (governance lifecycle)

Borrowing SP42's "no hidden tuning in code" model: editorial thresholds,
disclosure templates, rate limits, allowed/blocked source domains, ensemble-agreement
(auto-accept) thresholds, and enablement of each write path live in **versioned YAML
policy**, not in source. Each policy moves through a lifecycle:

```
suggested  →  candidate  →  active
```

A policy cannot be promoted to `active` (and thus actually govern live behavior)
until it **passes its declared evaluation gates** (`05-evaluation-and-benchmarking.md`).
This makes the agent's editing behavior auditable, reviewable, and reversible —
appropriate for a tool that edits a public encyclopedia.

## 7. Fairness: assess the edit, not the editor

Carried from Principle 5 (`01`; the ORES / Sarabadani-2017 baseline it critiques).
The harness evaluates **the edit and its sources on their merits** and must **not**
consume **editor-identity signals** — anonymous vs. registered, account age, edit
count, curator-group membership, IP/geolocation — as inputs to any verdict, score,
or proposal ranking.

- These signals are **never placed in model context** and **never used as features**
  in any scoring/ranking. Enforced structurally (the verdict-input type carries no
  such field) and by an *invariance* eval gate that checks verdicts don't change
  when identity metadata is injected (`06`, `05`).
- Rationale: the baseline systems weight exactly these features, which their own
  authors call *"not fair to anonymous and new editors."* "Assess the edit, not the
  editor" is the fairness pivot of this approach — the same discipline a careful
  human reviewer applies.

## 8. Open-model posture (credibility constraint)

From Principle 6 (`01`) and `docs/adr/0006`: the credibility tooling must not
depend on a frontier vendor.

- **Open-capable, open-default, open-tested.** The system always works on cheap
  open-weight models; open is the default; CI eval gates (`05`) and the published
  evidence (`11`) run on a **pinned open model**, so "runnable on community
  infrastructure" is *demonstrated, not claimed*.
- **Frontier optional, never required.** Frontier models are a pluggable backend; no
  verdict or credibility claim depends on one.
- **Open-vs-frontier benchmark (the "carrot").** The evidence harness publishes the
  gap between open and frontier models on WikiHarness's own tasks, as a public lever
  to recruit open-model investment (`11`).

## 9. Safety defaults

- **Dry-run by default.** Every write tool defaults to producing a *proposed* edit
  object; actually committing requires both an explicit `--commit`/confirm and a
  human confirmation token.
- **Test wikis for development.** Writes during development target
  `test.wikipedia.org` / `test.wikidata.org` (as SIFT already does), never
  production, until a path is explicitly graduated.
  - **Current write authorization (set 2026-06-02; the binding rule).** Real writes
    to **`test.wikipedia.org`** are authorized **for testing**. **Every production
    wiki** (`en.wikipedia.org`, Wikidata, Commons, and all others) remains
    **off-limits** until the owner authorizes that specific target explicitly. This
    is authorization *in principle*, not a standing instruction to write: a live
    write still requires the real `WikiClient` adapter, the `baseRevision`
    edit-conflict guard, owner-supplied OAuth credentials, and the kill switch left
    closed by default. Permitted because wikiharness's propose→confirm→commit design
    qualifies as **semi-automated** under [en:WP:AI agents and the bot
    policy](https://en.wikipedia.org/wiki/Wikipedia:AI_agents_and_the_bot_policy)
    (a full draft is shown, the human exercises editorial judgment, the edit is
    marked AI-assisted, and one approval is never reused for another action — the
    confirmation token binds to the exact `ProposedEdit` hash), so no BRFA is needed
    to test on `test.wikipedia.org`. The full graduating-to-writes design is to be
    recorded as `adr/0008`.
- **Kill switch.** A single config/flag disables all write paths globally.
- **Scope guard.** The agent only touches the article/entity it was invoked on and
  the specific claims under review; no opportunistic edits.
- **Blocking hooks (`docs/adr/0007`, `06`).** The guards above are enforced by
  pre-/post-action **hooks** that `DENY`/`ASK`/`ALLOW` structurally, not by relying
  on the agent to comply — e.g. PreToolUse denies a write lacking a confirmation
  token, failing policy, or carrying identity signals; PostToolUse runs the
  anti-hallucination quote-locator, provenance check, and stub scan. Higher-risk
  classes (content edits) additionally require **N-of-M multi-model agreement**.
