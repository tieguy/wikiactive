# ADR-0011 — Source HTML→text extraction is an injected edge (Defuddle), with a pure fallback

Status: **Accepted** — owner-directed 2026-06-02 (scraper-completeness scope choice for #3).
Date: 2026-06-02

## Context

Live-mode benchmarking (`adr/0010`/#3) measures **scraper completeness** on top of model quality:
the body-classifier attribution split charges a fetch/extraction failure to the *pipeline*, not the
*model*. The live fetch path was the naive `http.fetch(url) → htmlToText(body)`
(`packages/evals/src/benchmark.ts`, `tools/citation/fetch-source.ts`). `htmlToText` is a pure,
dependency-free converter — it does **not** strip navigation, ads, cookie walls, or footer chrome,
so the model often judges against page furniture instead of the article. That depresses the
scraper hit-rate and mis-attributes recoverable cases.

The reuse source (`alex-cite-checker`, `00`/`12`) gets its hit-rate from three techniques we are
porting under #3:

1. **Wayback `id_` rewrite** — request the raw archived page, not the Wayback wrapper (pure URL
   transform → `core/citation/urls.ts`).
2. **Wayback chrome recovery** — salvage the inner article when a banner was served anyway (pure
   text transform → `core/citation/source-fetch.ts`).
3. **Defuddle main-content extraction** — strip boilerplate to the article body. This is the big
   lever, and the only one that is **not pure**: it needs a DOM.

(1) and (2) drop straight into pure `core`. (3) is the subject of this ADR.

## Decision

1. **Main-content extraction is an EDGE, behind a `core` interface.** Add
   `HtmlExtractor { extract(html, url): Promise<string> }` to `core/edges.ts` (a pure interface,
   no DOM/library types leak in), implemented by `DefuddleHtmlExtractor` in `edges/`. This keeps
   `core` pure (the no-`Date.now`/no-fetch/no-DOM discipline of `04`) and confines the DOM
   dependency to the adapter layer, exactly as the model edge confines the Vercel SDK.

2. **Adopt `defuddle` + `linkedom` as `edges` runtime dependencies.** `defuddle@0.18.1` is the
   maintained boilerplate-stripper alex uses; `linkedom@0.18.12` is a lightweight, Node/Worker-safe
   DOM parser. The adapter parses with linkedom and passes the `Document` to `defuddle/node`'s
   `Defuddle(doc, url, …)` (the non-deprecated, library-agnostic entry point). These live only in
   `edges` (which already carries `ai`); `core`, `tools`, and `evals` stay dependency-light.

3. **Graceful degradation to the pure fallback.** Defuddle is a heuristic: it can throw, return
   nothing, or over-strip. When the extract is empty or thinner than a floor
   (`DEFAULT_MIN_USEFUL_CHARS = 200`), the adapter falls back to `htmlToText(html)` on the **full
   page**. A whole-page extract beats almost-nothing for completeness; the body-classifier still
   gates true garbage. The Defuddle call is **injectable**, so the fallback logic is unit-tested
   with no real DOM and the real libraries are smoke-tested hermetically (no network).

4. **Injected, default-off in pure contexts.** Both the live benchmark (`scripts/benchmark.ts`
   composition root) and `fetch_source` (via an **optional** `ctx.htmlExtractor`) use the extractor
   when provided; absent it, behavior is unchanged (`htmlToText`). So the hermetic CI gate and
   existing tool tests are untouched, and only the opt-in live composition roots wire Defuddle.

## Consequences

- Live-mode scraper completeness rises (Wayback raw content + boilerplate-stripped bodies), and the
  attribution split now reflects real extraction quality rather than `htmlToText`'s floor.
- `edges` gains two runtime deps. They are isolated to one adapter and excluded from the pure layers
  by the import-boundary checker (which governs cross-package imports) and by `core` staying free of
  DOM types.
- No new hermeticity risk: `DefuddleHtmlExtractor` makes **no** network calls; it transforms bytes
  already fetched by the (read-only) `HttpClient`. Tests stay offline.
- Not in scope here: the query-aware relevance-chunking pipeline (`extract-relevant-content`) and a
  CDX/Availability dead-link→snapshot fallback (absent in the source too). Both remain future work.

## Status of related invariants

Unaffected: anti-hallucination gate (quotes still located against the fetched bytes — now
better-extracted bytes), human-confirms-every-edit, test-wiki-only writes, no model confidence,
assess-the-edit-not-the-editor, open-model posture. This ADR only changes *how source text is
extracted before the model sees it*.
