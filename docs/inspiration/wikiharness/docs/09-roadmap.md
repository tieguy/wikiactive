# 09 — Roadmap (post-MVP-1)

> **Staging — HOW (tentative).** Architecture, contracts, and sequencing here are **exploratory, not locked** — a current best guess, revisable freely until M0 (base framework now settled — `adr/0002`, accepted 2026-06-02). The settled *what* lives in `00`–`02`/`11`. See [Staging](../README.md#staging).

MVP-1 (interactive citation verification) is specified in `08`. This document
sequences what comes after and grounds each phase in concrete, verified APIs so
the plan is buildable, not hand-wavy. Each phase adds tools to the catalog (`07`)
and must meet the same testability bar (`04`).

## Phase ordering rationale

We lead with improvements that are **verifiable, additive, and low-controversy**,
and we *skip* problems already well-served by existing bots.

- **Skip: generic dead-link repair.** `IABot` already does this at scale and well.
  We use archiving only in service of citation *quality* (MVP-1), not as a goal.
- **Skip (for now): generic Commons image-matching.** Relevance is fuzzy and hard
  to make crisply testable. We instead target *structured* GLAM sources where an
  object is tied to a subject via an identifier (below).

## MVP-2 — Internet Archive book-scan sourcing

**Goal:** given a weakly-sourced or unsourced (but plausibly true, existing)
claim, find a *scanned book and specific page* that supports it, and propose a
page-accurate `{{cite book}}` — with the supporting quote string-located in the
OCR text (same anti-hallucination gate as MVP-1).

**Why this and not text generation:** it is pure *sourcing* — we add a citation to
an existing claim, never new prose. It makes the Internet Archive a first-class
citizen and is highly testable (deterministic search + OCR + quote-location).

**Pipeline (all steps are individually testable tools):**

1. `search_books` *(read)* — Open Library Search
   (`https://openlibrary.org/search.json?q=…`, fields incl. `ia`, `ocaid`,
   ISBN/title/author/publisher/year) to find candidate books for the claim's
   topic. Each hit's `ia`/`ocaid` is the Internet Archive item id.
2. `search_inside_book` *(read)* — IA "Search Inside": full-text search *within*
   an item to get matching **leaf/page numbers + snippet + box coordinates**.
   Production path is the BookReader search endpoint on the item's data node
   (discovered via `https://archive.org/metadata/{id}` → server host); the
   experimental stable wrapper is
   `https://api.archivelab.org/books/{id}/searchinside?q=…`. **Confirm the exact
   JSON shape empirically against a live item** (the precise schema lives in a
   staff-only repo) and pin it behind the `edges` adapter + fixtures.
3. `verify_book_support` *(read)* — claim + OCR snippet/page text →
   SUPPORTED/PARTIAL/NOT, with the quote **located in the fetched OCR**
   (`locate_quote_in_source`). Reuses the MVP-1 verify prompt/parser.
4. `propose_book_citation` *(read+write → Wikipedia)* — emit a `{{cite book}}`
   `ProposedEdit`:
   - `title`,`author`,`publisher`,`publication-place`,`date(year)`,`isbn`,
     `pages`/`page=` (the matched leaf/page), and a deep link
     `https://archive.org/details/{id}/page/{n}` (or `|id={{Internet Archive|…}}`).
   - Sourced from Open Library Edition JSON
     (`/books/OLxxxM.json`, `/isbn/{isbn}.json`) + the matched page.
5. *(optional)* `propose_book_edition_item` *(read+write → Wikidata)* — create/link
   a book *edition* item (P31→Q3331189, P629 to the work) carrying Open Library ID
   (**P648**), ISBN-13 (**P212**) / ISBN-10 (**P957**), Internet Archive ID
   (**P724**).

**Constraints:** no API key for IA/Open Library, but a descriptive `User-Agent`
with contact is required and request rate must be gentle (use bulk dumps for big
jobs). *Citing* from any scan is fine (a citation is a fact, not reuse); only
*reuse/upload* of page images is gated to clearly public-domain items
(`possible-copyright-status`/Open Library `ebook_access`).

## MVP-3 — codify one NLP-requiring on-wiki rule (ADR-0007)

Sequenced **after MVP-2 and before the Phase 3–5 enhancers.** Codifies a single
Manual-of-Style "words to watch" rule that traditional non-NLP bots can't evaluate.
**Rule: MOS:PEACOCK (puffery)** — chosen because the data exists (`12 §C-bis`) and
it uniquely reuses our citation signal. **MOS:WEASEL is the immediate follow-on
enhancer**, reusing the same pipeline (below) on the larger WikiWeasel/CoNLL-2010
benchmark — that reuse *is* the demonstration that the enhancer framework
(`adr/0005`) composes.

**Why it matters:** it is the first deliberate, hard-gated step into *content*
(text-adjacent) edits, proving the two safety mechanisms that make that step
defensible:
- **Blocking hooks** (`06`, `adr/0007`): PreToolUse scope/policy gate; PostToolUse
  verification; stub/placeholder scan.
- **Multi-model voting:** N-of-M agreement before a content proposal is surfaced
  (the SIFT ensemble idea applied to content); disagreement suppresses or escalates.

**Action is flag-first** (inline cleanup template) with any minimal text change
multi-model-agreed, human-confirmed, and disclosed. This is a *single codified rule*
— it is **not** Phase 5 (general text); it is the bounded precursor that earns
evidence toward it.

**Pipeline (individually testable tools; the *rule* is a parameter, which is what
lets weasel reuse it):** `detect_puffery` (NLP judgment over prose) →
`verify_puffery` (multi-model agreement) → `propose_puffery_fix` (flag
`{{Peacock term}}` or minimal concise edit, `ProposedEdit`). The follow-on swaps in
`detect_weasel` / `{{Weasel inline}}` over the same `verify`/`propose` machinery.
Exit gate per `10` M6.

**Datasets exist (this gated the rule choice — `12 §C-bis`).** All candidate rules
have labeled data via the same trick as the citation work: **distant supervision
from Wikipedia's own inline cleanup templates** (`{{Peacock term}}`, `{{Weasel
inline}}`, `{{By whom}}`) plus the NPOV edit history. The two best-supported:
**peacock** (public code+data, ~0.963 F1 RoBERTa baseline, citation features help —
direct synergy with our core) and **weasel** (WikiWeasel/CoNLL-2010, ~20.7k
sentence-level labels with shared-task baselines).

## MVP-4 — naive "grumpy Wikipedian" review (discovery, *not* editing)

Sequenced **after MVP-3.** A deliberately simple, *ungated* enhancer: point an LLM at
an article with a "review this like a grumpy, exacting Wikipedian" prompt and collect
its complaints. Unlike MVP-1–3, its goal is **not to improve the article** — it is to
**discover what to build next**: recurring gripes, clustered across many articles,
become candidate rules / future enhancers / MVPs. It is, in effect, a data-driven
backlog generator for the enhancer framework (`adr/0005` Decision 4 already anticipates
non-edit "discovery" consumers of the outputs layer).

**Load-bearing fence (do not remove even though this is numbered MVP-4):** this tool
*inverts* the project's central discipline — it is naive, subjective, and **not**
evidence-gated, the opposite of the source-only/anti-hallucination posture in
`02`/`04`/`11`. That is tolerable **only** because its output is **suggestions for
humans/roadmap, never article edits**. It must never become a path to ungated editing.
Numbering it alongside the gated MVPs carries some risk of implying a shared safety
posture; that risk is accepted for now (owner decision), with the fence kept explicit.

- **Output:** structured findings ("candidate rule", "smells like X", free-text gripe)
  into the same consumer-agnostic outputs layer (`adr/0005`), tagged as *discovery*,
  routed to roadmap review — **not** to the edit-commit consumer.
- **Why naive is fine here:** the cost of a wrong suggestion is a human ignoring it,
  not a bad edit. The rigor lives downstream, when a clustered gripe graduates into a
  *real* gated enhancer (the MVP-3 treatment).

## Composition — the `review` meta-command (run enhancers in sequence)

`adr/0005` frames the harness as hosting **and composing** many enhancers, but defers
the *generalized* compose-UI as hard (Decision 5). The simplest useful composition is a
**fixed pipeline**: `wikiharness review <article>` runs the registered *edit-producing*
enhancers (MVP-1 → MVP-2 → MVP-3) in sequence over one article; MVP-1's `verify` is just
the single-enhancer case. (The MVP-4 discovery tool is a *separate* invocation — its
output type is different, so it is not folded into the edit-proposal pipeline.)

- **Value:** a forcing function for the **enhancer registry + outputs store** (`06`,
  `adr/0005`) — it only works if enhancers genuinely compose and outputs are addressable.
- **Review volume is inherent, not a bug to throttle away.** Three enhancers over a long
  article produce many proposals; rather than engineer the volume down, the UX answer is
  a **"save my position" / pin-and-resume** affordance — see `03 §4`. The meta-command
  is the main reason that affordance has to exist.
- Sequenced **after the individual enhancers** (it composes them); it does not need its
  own gated milestone beyond the registry/outputs work it exercises.

## Phase 3 — Wikidata enrichment

Reuses SIFT most directly (the investigate→verdict loop, `ontology_precheck`,
`web_search`/`web_fetch`, the SIFT prompt). Tools: `sparql_query`,
`search_entities`, `ontology_precheck`, `propose_statement` (every statement
carries its own reference — no unsourced statements). Dev writes target
`test.wikidata.org` (as SIFT already does). Focus: missing identifiers, sitelinks,
and well-sourced statements for an article's subject.

## Phase 4 — Open GLAM media & structured data

> **Scope guard (ADR-0004):** this phase is **text & metadata only — no image
> parsing / no computer vision.** Media are matched by their *catalog records,
> descriptions, existing SDC, and the depicted object's Wikidata context* — never
> by their pixels. "Image→article" and "depicts" are recast as text→entity /
> metadata→article matching. We reuse infrastructure (pywikibot-sdc, OpenRefine
> reconciliation API, QuickStatements, Pattypan, ISA) and apply the LLM only as the
> metadata-grounded disambiguation layer, with anti-rubber-stamping metrics (`11`)
> and the CAT failure (`12 §E`) explicitly designed against.

Target *structured* sources where an object is tied to a subject by an identifier,
making relevance testable. **First-class (CC0 → uploadable to Commons + linkable
on Wikidata):**

| Source | API | Auth | License | Wikidata ID prop |
|---|---|---|---|---|
| Met | `collectionapi.metmuseum.org/public/collection/v1/` | none | CC0 | **P3634** |
| Rijksmuseum | `data.rijksmuseum.nl` (Search + IIIF/OAI-PMH; legacy `/api` deprecated) | free key | PD img / CC0 meta | **P13234** |
| Smithsonian | `api.si.edu/openaccess/api/v1.0/` | api.data.gov key | CC0 (where released) | dept-specific (e.g. SAAM **P4704**) |
| Cleveland (CMA) | `openaccess-api.clevelandart.org/api/artworks/?cc0=1` | none | CC0 | CMA id prop |
| Art Institute Chicago | `api.artic.edu/api/v1/artworks` | none | CC0 (PD) | AIC id prop |
| Harvard Art Museums | `api.harvardartmuseums.org/object` | free key | varies | — |

**Read-only enrichment / discovery aggregators** (per-record rights vary; *find*
then decide per item): **Europeana** (`api.europeana.eu`, `reusability=open`
filter) and **DPLA** (`api.dp.la/v2`).

**Tools:** `search_open_collections`, `check_media_license` (hard gate before any
Commons proposal — only CC0/PD), `propose_commons_upload` (image + SDC metadata),
`propose_article_image`, `propose_external_id` (link subject via the existing
Wikidata id property). **Resolution-first strategy:** resolve the article's
Wikidata item → if it already carries P3634/P13234/etc., fetch the object directly;
else search the CC0 collections; for CC0/PD hits, upload to Commons with structured
metadata and add/confirm the Wikidata identifier statement, then use the image.

## Phase 5 — Text (last, hardest — because it is hardest to *prove*)

Text comes last because it is the hardest place to demonstrate safety, **not**
because it is permanently off-limits. We reach it the same way we reach every
other expansion: by accumulating the evidence that retires the community's
specific fears (`11-evidence-and-policy-strategy.md`). Under current policy
(`02 §1`) this starts strictly bounded — copyedit-only, diff-bounded, never free
generation, always flagged, always human-confirmed, *suggested* word-by-word with
full provenance/audit. Whether and how far it expands beyond that is an
**evidence-gated, consensus-driven** question (the ladder in `11`), including
adversarial evals on text specifically, not a fixed verdict.

## Cross-phase: graduating a write path to production (and *moving the boundary*)

Two distinct gates, not to be conflated:

**(A) Enabling a write path that current policy already permits** (per sink) —
only after:
1. its tools pass Tiers 1–4 with fixtures + the relevant eval gates (`05`),
2. its YAML policy reaches `active` (passing declared `required_gates`),
3. a human has run it end-to-end on a test wiki,
4. and — where the cadence/automation crosses assisted-editing norms — a **BRFA**
   is filed and approved with a dedicated bot account (`02 §3`).

**(B) Asking the community to *expand* what is permitted** (e.g. a new edit class,
or reduced per-edit confirmation for a well-proven class) — only after the
relevant rung of the **evidence ladder** is met: pre-declared, reproducible
quality data (introduced-error/hallucination rates, acceptance, reversion vs.
human baselines, adversarial results) is published and taken through the
community's own process (RfC/BRFA). See `11-evidence-and-policy-strategy.md`. We
never reach gate (B) by quietly operating past the current rule — only by moving
the rule with evidence and consensus.
