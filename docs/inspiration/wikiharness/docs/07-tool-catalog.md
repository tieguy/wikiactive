# 07 — Tool Catalog & Contracts

> **Staging — HOW (tentative).** Architecture, contracts, and sequencing here are **exploratory, not locked** — a current best guess, revisable freely until M0 (base framework now settled — `adr/0002`, accepted 2026-06-02). The settled *what* lives in `00`–`02`/`11`. See [Staging](../README.md#staging).

Each capability is a **tool**: a typed, independently unit-testable unit composing
pure `core/` logic with injected `edges/`. Tools never reach the network or a model
directly — they receive injected clients (`HttpClient`, `WikiClient`,
`ModelClient`, `Clock`, `Storage`) so every tool is testable with
stubs/fixtures (see `04-testing-strategy.md`).

## Tool contract (uniform shape)

```ts
interface Tool<Params, Result> {
  name: string;                 // stable id, e.g. "verify_citation"
  access: "read" | "read+write";
  params: Schema<Params>;       // runtime+TS schema (Zod/TypeBox — per adr/0002)
  // PURE where possible; all side effects via injected deps in `ctx`.
  execute(params: Params, ctx: ToolContext): Promise<ToolResult<Result>>;
}
interface ToolResult<R> {
  data: R;
  provenance: ProvenanceRecord;     // sources, hashes, timestamps, model, quote
  evidence?: Evidence[];            // evidence-before-verdict envelope (see below); rides with
                                    //   each proposal/finding in the SSE event
  proposedEdits?: ProposedEdit[];   // edit-path output: targets a sink; never auto-applied
  findings?: Finding[];             // non-edit output (e.g. MVP-4 discovery): suggestions
                                    //   for humans/roadmap; never a wiki write
  rationale: string;                // human-readable; required
}
```

Tools don't act on their own: they compose into **enhancers** (registrable plugins,
`adr/0005` / `06`; the `review` meta-command runs the edit-producing enhancers in
sequence — `09`) and emit into a **consumer-agnostic outputs layer** (`adr/0005`
Decision 4) — structured, addressable results that different consumers ingest. Two
output types flow through it:

- **`ProposedEdit`** — the currency of the **edit path**: a *diff-able, confirmable*
  description of a change to some sink (Wikipedia/Wikidata/Commons; IA writes are
  post-MVP-1), **never applied without a human confirmation token**
  (`02-constraints-policy.md` §2). Edit-commit is *one* consumer of the outputs layer.
- **`Finding`** — a **non-edit** output (e.g. the MVP-4 discovery tool): a suggestion
  for humans/roadmap (candidate rule, smell, gripe), routed to discovery consumers,
  **never a wiki write** and not confirmation-gated, because it changes nothing on-wiki.

## Evidence payload — the evidence-before-verdict contract (decided 2026-06-02)

Following the `03` UX redline, "evidence before verdict" (`03 §3`, UX principle 2; the CAT
anti-rubber-stamping lesson, `11`) is realized as a **hybrid envelope**: a small *required
common shape* the core/hooks understand and enforce, plus an *enhancer-specific extension*
with a registered per-kind renderer. **The contract is fixed at M1; evidence *kinds* are
added as enhancers land** (MVP-1 ships only `located-quote`). Chosen over a closed core union
(too rigid for `0005`'s first-class plugins) and an open opaque blob (un-enforceable; fails
`04`'s deeply-testable bar).

```ts
interface Evidence {
  kind: string;                 // enhancer-declared: "located-quote" | "in-article-span" | "diff" | …
  primary: EvidenceDisplay;     // shown FIRST, before the verdict — evidence-before-verdict
  provenance: ProvenanceRecord; // sources/hashes/timestamps; circular WP/WD URLs marked non-evidence
  grounding: GroundingAssertion;// machine-checkable; a PostToolUse hook re-verifies it (adr/0007)
  detail?: unknown;             // enhancer-specific; rendered by a registered per-kind renderer
}
```

The **verdict is not part of `Evidence`** — the graded categorical verdict
(fully/partial/not/unavailable) is a separate, secondary field on the proposal, rendered
*after* `primary`. **No model-generated confidence/support number is produced or shown** — it
is false precision (the model invents it the same way it invents prose); the honest signals
are the **graded categorical verdict**, **measured ensemble (dis)agreement** (`adr/0007`), and
**abstention**. That is what makes evidence-before-verdict **structural**, not a styling
convention.

`grounding` **generalizes the anti-hallucination gate** (`04`, `02 §1`): `locateQuoteInSource`
is just the predicate for `located-quote`. Every kind carries its own pure, Tier-1-tested
grounding predicate the harness re-checks against retrieved bytes / the article revision:
- `located-quote` → the quote string-locates in a source fetched **this session**;
- `in-article-span` (e.g. MOS:PEACOCK — no external source) → the flagged span exists in the
  cited article revision **and** the rule id is valid;
- `diff` → the shown diff equals the actual computed diff.

A PostToolUse hook (`adr/0007`) rejects, pre-human, any proposal whose evidence fails its
grounding predicate — so **"no verdict without grounded evidence" holds across every enhancer**,
not just citations.

**Grounding is fabrication-resistance, not judgment-correctness — and its strength is a
gradient, not a guarantee.** The predicates above check that the model pointed at *real*
retrieved bytes, a *real* article span, a *real* rule id, and that the diff shown equals the
diff computed. They do **not** check that the model's *judgment* is right. Critically, that
strength **decays along a fact→judgment axis**:
- `located-quote` is **strong** — the quote must string-locate in fetched bytes, anchoring the
  verdict tightly to retrieved text; fabricating a source is structurally impossible.
- `in-article-span` (e.g. MOS:PEACOCK puffery) is **weak** — the machine can confirm the span
  exists and the rule id is valid, but it *cannot* evaluate "is this actually puffery." That
  judgment is irreducibly the model's; there is no deterministic oracle for it.

For the judgment end of the axis, grounding is **necessary but far from sufficient**. The slack
is taken up by a **second, separate layer**: **measured ensemble agreement** (N-of-M model
voting, `adr/0007 §4`) plus **human confirmation** — never by grounding pretending to certify a
judgment it cannot, and never by a model-generated confidence number (`11`). So the honest
contract is: grounding stops fabrication; the verdict + measured agreement + HITL carry
correctness. (This is why MOS:PEACOCK is the `adr/0007 §5` demonstrator for "blocking hooks +
multi-model voting," not "blocking hooks alone.")

## Read/write classification

| Sink | Tools write here? | Gate |
|---|---|---|
| Wikipedia (article space) | Yes — assisted, confirm-each | OAuth2, provenance, disclosed summary, dry-run default |
| Wikidata | Yes — assisted | same; `test.wikidata.org` in dev |
| Wikimedia Commons | Yes — assisted (uploads) | license check tool must pass |
| Internet Archive (Save Page Now) | Deferred — post-MVP-1 | MVP-1 reads Wayback only (no IA writes) |
| Open Library metadata | Maybe (later) | scoped |
| CrossRef / museum APIs / SPARQL / search | **Read only** | — |

## Catalog (grouped)

### A. Wiki I/O (foundation; reuses SP42 plumbing patterns)
- **`get_article`** *(read)* — fetch article wikitext + parsed HTML + current
  `revid` via MediaWiki Action API / REST. Pure `buildGetArticleRequest` +
  `parseArticle`. Reuse citation-checker's REST-HTML approach
  (`/api/rest_v1/page/html/<title>[/<oldid>]`).
- **`get_entity`** *(read)* — fetch a Wikidata item (incl. revision-specific via
  `Special:EntityData`, as SIFT does). Resolve labels.
- **`propose_wiki_edit`** *(read+write)* — build an `action=edit` `ProposedEdit`
  (with `baserevid`, `tags`, `minor`, disclosed summary). Port SP42's
  `WikiPageSaveRequest` / `build_*_request` + token flow. **Never auto-commits.**
- **`commit_confirmed_edit`** *(read+write)* — the *only* function that calls
  `action=edit` for real; requires a valid human confirmation token bound to the
  exact `ProposedEdit` hash. Centralizes OAuth2, CSRF token fetch, `maxlag`,
  User-Agent, rate limits.

### B. Citation tools (MVP-1 core; reuses `citation-checker-script`)
- **`extract_claim`** *(read, pure)* — given article HTML + a citation index,
  extract the claim text attached to that citation (the "between adjacent
  citations" algorithm). **Direct port of `core/claim.js`.**
- **`resolve_citation_url`** *(read, pure)* — citation → best source URL
  (live-over-archive preference, Harvard/`sfn` `#CITEREF` following, page-number
  extraction). **Direct port of `core/urls.js`.**
- **`fetch_source`** *(read)* — retrieve source content (HTML→text via a
  trafilatura-equivalent; PDF page extraction; archive-snapshot handling).
  Combines citation-checker's worker-proxy approach + SIFT's `web_fetch`
  (trafilatura). Records content hash + fetch timestamp into provenance.
- **`verify_citation`** *(read)* — claim + fetched source → verdict
  `{SUPPORTED | PARTIAL | NOT_SUPPORTED | SOURCE_UNAVAILABLE}` (graded categorical; **no
  numeric confidence** — false precision, see the Evidence contract / `03`) +
  **string-located supporting quote**. Reuses citation-checker's tuned system
  prompt (9 few-shots, "use only provided source" gate) + robust verdict parser.
  The quote MUST be `locateQuoteInSource`-verifiable (anti-hallucination gate).
- **`enrich_citation`** *(read+write)* — turn a bare URL/DOI/PMID/ISBN into a
  filled citation template via **Citoid** (`/api/rest_v1/data/citation/...`),
  optionally cross-checked with CrossRef. Emits a `ProposedEdit` replacing the
  bare cite with a full `{{cite ...}}`.
- **`repair_citation`** *(read+write)* — fix malformed/incomplete existing cites
  (missing access-date, broken template params) → `ProposedEdit`.

### C. Internet Archive tools (first-class; reuses IABot concepts)
- **`check_url_liveness`** *(read, pure-ish)* — is a citation URL dead? (port of
  IABot's `CheckIfDead` heuristics; low false-positive design.)
- **`find_archive_snapshot`** *(read)* — query Wayback **CDX**/Availability for an
  existing snapshot of a URL near a date. No API key.
- **`archive_url`** *(read+write → IA; **post-MVP-1**)* — trigger **Save Page Now**
  for a live citation URL; return the resulting Wayback URL. Rate-limited. Deferred
  so MVP-1 makes no writes to the Internet Archive (read-only Wayback only).
- **`add_archive_to_citation`** *(read+write → Wikipedia)* — attach
  `archive-url`/`archive-date` to an existing cite → `ProposedEdit`. In MVP-1 the
  `archive-url` comes from an **existing** snapshot (`find_archive_snapshot`), not a
  freshly created one.
  (Note: we *intentionally* do not chase generic dead-link repair — IABot owns
  that. We use archiving in service of citation *quality*.)
- **`find_book_support`** *(read; MVP-2)* — full-text "Search Inside" of IA
  scanned books + Open Library to find a book **page** supporting a claim; emits a
  page-accurate `{{cite book}}` `ProposedEdit`. (Detailed in `09-roadmap.md`.)

### D. Wikidata / open-data tools (later; reuses SIFT)
- **`sparql_query`** *(read)* — run a SPARQL query against WDQS; typed results.
- **`search_entities`** *(read)* — `wbsearchentities` label/alias search.
- **`ontology_precheck`** *(read, pure)* — cheap P31/P279 consistency checks.
  **Direct port of SIFT's `sift_precheck.py`.**
- **`propose_statement`** *(read+write → Wikidata)* — build a `ProposedEdit` for a
  missing statement **with its own reference** (no unsourced statements). Emit as
  **QuickStatements 3.0 / Wikibase REST** (reuse, don't write a writer); validate
  with **`wbcheckconstraints`/ShEx** before surfacing (ADR-0004).
- **`reconcile_entity`** *(read)* — the LLM disambiguation layer behind the standard
  **Reconciliation Service API** facade: deterministic external-ID join → candidate
  generation (`wikidata.reconci.link` / `search_entities` / **Mix'n'match**) → LLM
  re-rank with context → **a graded match judgment + rationale, or abstain** (auto-accept
  gated on **measured ensemble agreement**, not a self-reported score). (`12 §D`)
- **`web_search` / `web_fetch`** *(read)* — investigation tools for the
  investigate→verdict loop. **Port of SIFT's `tool_executor.py`** (rate-limited,
  blocked-domain list, query-focused extraction). Pluggable search backend.

### E. Open GLAM media/data tools (later; read-only enrichment + Commons sink)
> **Scope guard (ADR-0004): text & metadata only — no image parsing / no CV.**
> Media are matched by catalog text, descriptions, existing SDC, and the depicted
> object's Wikidata context — never pixels. Writes go via **pywikibot-sdc**; human
> front-ends are **Pattypan / OpenRefine / ISA** (we fill their inputs). All
> proposals carry a graded judgment (**no numeric confidence**), support **abstention**, and
> are subject to anti-rubber-stamping measurement (`11`).
- **`search_open_collections`** *(read)* — query Met / Rijksmuseum / Smithsonian /
  Europeana / CMA / DPLA for openly-licensed objects related to a subject.
- **`check_media_license`** *(read, pure)* — determine if an item is CC0 / PD /
  Commons-compatible. **Gate before any Commons proposal.**
- **`propose_commons_upload`** *(read+write → Commons)* — build an upload
  `ProposedEdit` (only for license-cleared items) with structured-data metadata.
- **`propose_article_image`** *(read+write → Wikipedia)* — propose adding an
  existing Commons/just-uploaded image to an article infobox/section.
- **`propose_external_id`** *(read+write → Wikidata)* — link a subject to an
  external collection via the appropriate Wikidata external-id property (e.g. Met
  object ID, Rijksmuseum ID), where such properties already exist.

### F. Cross-cutting (pure, heavily unit-tested)
- **`assemble_provenance`** *(pure)* — build the `ProvenanceRecord`.
- **`format_edit_summary`** *(pure)* — honest summary + AI-assistance disclosure.
- **`evaluate_policy`** *(pure)* — given active YAML policy + a `ProposedEdit`,
  allow/deny + reasons.
- **`locate_quote_in_source`** *(pure)* — the anti-hallucination primitive.
- **`render_diff`** *(pure)* — produce the human-facing diff for confirmation.

### G. Discovery (MVP-4; emits `Finding`s, not edits)
- **`review_grumpy`** *(read; **MVP-4**)* — run a deliberately naive "review this like a
  grumpy, exacting Wikipedian" pass over an article; emit `Finding`s (candidate rules,
  smells, free-text gripes), tagged *discovery*. **Ungated and subjective by design** —
  the inverse of the evidence-gated tools above — tolerable **only** because its output
  is suggestions for humans/roadmap, **never** a `ProposedEdit` and never a wiki write
  (`09` MVP-4, fence). Clustered across many articles, its findings seed future
  enhancers. Net-new (not a port), so it is absent from the reuse map below.

## Reuse map (so we port, not reinvent)

| New tool | Source repo / service | Artifact reused |
|---|---|---|
| extract_claim, resolve_citation_url, verify_citation, parse verdict | citation-checker-script | `core/claim.js`, `core/urls.js`, `core/prompts.js`, `core/parsing.js`, `core/verdicts.js`, `core/providers.js` |
| web_search/web_fetch, ontology_precheck, investigate→verdict loop | wikidata-SIFT | `tool_executor.py`, `sift_precheck.py`, `run_verdict_fanout.py`, SIFT prompt |
| propose_wiki_edit, commit_confirmed_edit, OAuth2+PKCE, token flow, policy-as-data, eval gates | SP42 | `action_executor.rs`, `oauth.rs`, `recent_changes.rs`, scoring-governance pattern |
| enrich_citation | Citoid | REST citation API |
| find_archive_snapshot, check_url_liveness (`archive_url` post-MVP-1) | IABot / Wayback APIs | CDX / Availability / `CheckIfDead` (Save-Page-Now post-MVP-1) |
| find_book_support | Internet Archive / Open Library | Search-Inside + OL APIs (see 07) |

Every cell above is an independently testable unit with its own fixtures.
