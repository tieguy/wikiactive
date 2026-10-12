# 00 — Research Findings (prior art & ecosystem)

> **Staging — WHAT (converging).** Vision / scope / posture / findings: close to settled, changes deliberately. Architecture & mechanics (the *how*) live in `06`–`10` and stay tentative until base-framework sign-off — see [Staging](../README.md#staging).

Condensed record of the research that grounds this plan. Full reasoning is in the
referenced docs/ADRs. Dates reflect mid-2026 research.

## Prior-art repositories

### `alex-o-748/citation-checker-script` (JS) — *reuse heavily for MVP-1*
AI tool that verifies whether a Wikipedia citation actually supports its claim.
Verdicts: **SUPPORTED / PARTIAL / NOT_SUPPORTED / SOURCE_UNAVAILABLE** + confidence
+ supporting quote. *(WikiHarness ports the **graded verdict** but **not** the numeric
confidence — false precision; see `07`/`03`.)* Three consumers (userscript, Node CLI, benchmark) share pure
`core/` ESM modules:
- `core/claim.js` — extract claim text via the "between adjacent citations"
  algorithm (adjacent `.reference`s with no text between cite the same claim).
- `core/urls.js` — citation→source URL (live-over-archive, Harvard/`sfn`
  `#CITEREF` following, page-number extraction).
- `core/prompts.js` — tuned system prompt (9 few-shots, "use only provided source"
  gate, confidence→verdict bands). *(We port the prompt but **drop** the confidence→verdict
  banding — graded categorical verdict only; `07`/`03`.)*
- `core/parsing.js` + `core/verdicts.js` — robust verdict parsing (JSON / fenced /
  markdown-emphasis recovery) + canonicalization.
- `core/providers.js` — multi-provider dispatch (HF/PublicAI/Anthropic/Gemini/
  OpenAI/OpenRouter).
Pure, DOM-injectable, tested with `node --test`; ~189 labeled benchmark entries.
**Liftable:** all of `core/` (→ TS), and the benchmark as an eval seed (`05`).

### `tieguy/open-graph-next/wikidata-SIFT` (Python) — *the user's own; reuse for Wikidata phase*
LLM-assisted fact-checking of Wikidata edits via SIFT (Stop/Investigate/Find/
Trace). 220 pytest tests. Writes only to `test.wikidata.org`.
- `scripts/run_verdict_fanout.py` — **two-phase loop**: Phase A investigate
  (model issues `web_search`/`web_fetch` in a tool loop), Phase B emit structured
  verdict. Checkpoint/resume, parallelism, per-model routing.
- `scripts/tool_executor.py` — `web_search` (Brave) + `web_fetch` (trafilatura),
  rate-limited, blocked-domain list, query-focused extraction.
- `scripts/sift_precheck.py` — deterministic P31/P279 ontology consistency checks.
- `config/sift_prompt_openrouter.md` — SIFT prompt: mandates live investigation,
  treats Wikipedia/Wikidata URLs as **non-evidence** (circular), special-cases
  value-changes ("is this an *improvement*?").
- Verdict scale: verified-high/low, plausible, unverifiable, suspect, incorrect.
**Liftable:** the investigate→verdict loop pattern, `tool_executor`,
`sift_precheck`, the SIFT prompt, the non-evidence/provenance discipline, the
eval/metrics rigor (scikit-learn PR-AUC/ROC-AUC, label-leakage controls).
*Note:* README/docker-compose say SearXNG but active code uses the Brave API —
confirm before depending.

### `schiste/SP42` (Rust) — *architecture & governance reference only; NO LLM*
A deterministic Wikipedia **patrolling** workbench (recent-changes → score →
human moderation). Important correction: **contains no LLM/agent/prompt code**;
"scoring" is rule-based + an optional Wikimedia LiftWing call. Value is the
*architecture and governance* — patterns adopted via `adr/0002` (accepted 2026-06-02,
option (a)); the concrete mechanics remain HOW — tentative until M0:
- **Pure I/O-free core** (`sp42-core`) behind **trait dependency-injection**
  (`HttpClient`, `EventSource`, `Storage`, `Clock`, `Rng`) with stub/replay/memory
  doubles → fully offline, deterministic tests.
- **`build_*_request` / `parse_*_response`** split + thin `execute_*` glue.
- Full **MediaWiki plumbing**: OAuth2+PKCE, server-side token custody, CSRF/edit
  tokens, `action=edit` with `baserevid`/`tags`/`minor`, recentchanges paging.
- **Policy-as-data governance:** YAML policies with a `suggested→candidate→active`
  lifecycle; promotion blocked until **eval gates** pass
  (`check-scoring-governance.sh`); fairness gates.
- **Explainability-first** + **append-only audit ledger** (`PublicAuditLedgerEntry`)
  + **human-supremacy** (no autonomous edits).
**Where we'd diverge (tentative):** add the entire LLM reasoning layer SP42 lacks; invert
the objective (improve, not flag); output proposed content edits, not reverts;
relax literal determinism to the non-LLM layers; engage the edit/sourcing policy
surface SP42 ignores.

## Ecosystem services & APIs (verified)

- **LLM-text policy** (RfC closed 2026-03-20): near-ban on LLM-generated/rewritten
  article *text*; narrow copyedit/translation exceptions; concerns = hallucinated
  statements + fabricated citations. *Drives metadata-first scope + anti-hallucination.*
- **Bot/assisted-editing policy:** OAuth2, descriptive User-Agent + contact, ≤3
  concurrent, maxlag, BRFA + bot account for automation, honest summaries.
- **Citoid** — Wikimedia citation generation from URL/DOI/PMID/ISBN/QID
  (`/api/rest_v1/data/citation/...`). A candidate backend for a citation-enrichment
  tool (`07`; HOW — tentative).
- **InternetArchiveBot / Wayback** — dead-link detection (`CheckIfDead`) + CDX +
  Save-Page-Now. MVP-1 uses Wayback **read-only** (find existing snapshots for
  citation quality); *creating* archives via Save-Page-Now is deferred post-MVP-1.
  Not generic dead-link repair — IABot owns that.
- **Wikidata** — `wbsearchentities`/`wbeditentity` via `api.php`; WDQS SPARQL.
- **IA Search-Inside + Open Library** — full-text search within scanned books →
  page-accurate `{{cite book}}` (MVP-2; see `09`).
- **Open GLAM CC0 APIs** — Met (P3634), Rijksmuseum (P13234), Smithsonian, Cleveland,
  AIC, Harvard (uploadable to Commons); Europeana/DPLA (read-only discovery).

## Framework bake-off (decided — full detail in ADR-0002)

*Decided 2026-06-02 via `adr/0002` (option (a)).* Candidate runtimes were scored
against our hard *requirements* (tool unit-testability, headless fake-model loop,
one core→CLI+web, durable HITL). **Decision** (`adr/0002`, accepted 2026-06-02,
option (a)): a thin pure-tool core; the Vercel
AI SDK (`ai`) for model calls only; durable HITL as a persisted approval state
machine; LangGraph.js as the sanctioned escape hatch. pi / opencode / Claude Agent
SDK were set aside as *base* (ephemeral permissions / subprocess+vendor lock /
coding-shaped / weak headless testability respectively), though each contributes
ideas.
