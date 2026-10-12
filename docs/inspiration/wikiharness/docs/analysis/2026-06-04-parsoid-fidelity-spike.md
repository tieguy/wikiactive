# Parsoid round-trip fidelity spike (for SP42 ADR-0002)

**Date:** 2026-06-04 · **Article:** en.wikipedia.org `Cosmic latte`, rev **1357330394** · **Endpoint:** RESTBase `/api/rest_v1/` (Parsoid HTML profile 2.8.0).

## Question

Can targeted citation edits be expressed as Parsoid DOM edits that **selective-serialize losslessly** (untouched wikitext byte-preserved) and produce a **clean, template-scoped diff**? This gates the "Parsoid REST as primary `WikitextEditor` impl" recommendation in the draft SP42 ADR-0002.

## Method (reproducible)

All calls read-only; the `transform/html/to/wikitext` endpoint is **stateless** (HTML→wikitext conversion, nothing is saved to any wiki). Descriptive UA per Wikimedia policy.

1. `GET /page/html/Cosmic_latte?stash=true` → Parsoid HTML + an ETag whose key references a server-side **stash** of the original (`data-parsoid` is held in the stash, not inline). The `?stash=true` + `If-Match` ETag is what enables **selective** serialization (vs lossy full serialization).
2. Edit a specific node in the HTML by mutating its single-quoted `data-mw='{…}'` JSON (the transclusion's template name + params).
3. `POST /transform/html/to/wikitext/Cosmic_latte/1357330394` with `If-Match: <etag>` and the modified HTML → Parsoid re-serializes **only changed subtrees**, copying everything else verbatim from the stashed original.
4. `diff` the result against the exact original wikitext (`action=raw&oldid=1357330394`).

## Results

| Test | Edit | Result |
|---|---|---|
| **0 — no-op** | POST unmodified stashed HTML | **Byte-for-byte identical** (7666 → 7666, empty `diff`). Foundational losslessness holds. |
| **A — modify param** | change a `{{cite magazine}}` `title` value | Only that template re-serialized; all other templates/refs/prose byte-identical. |
| **B — add param** | add `access-date=2026-06-04` to a `{{cite journal}}` | Param appended as ` \|access-date=2026-06-04` in the template's dominant ` \|key=val` style; only that template changed. |

**Proven:**
- Selective serialization is **globally lossless** — anything you don't touch is preserved exactly (Test 0).
- Edits are **node-scoped** — only the edited template is re-serialized, even when several refs share one wikitext line (Tests A, B).
- New-param **insertion follows the template's existing style** for free (Test B) — the property `#14`/`setTemplateParams` had to hand-build over wikiparser-node.

**Caveat (the one real cost):** Parsoid re-serializes the **whole edited template** from `data-mw`, lightly **normalizing that template's own internal whitespace** toward its dominant style:
- Test A: `|url= https://…` → `|url=https://…` (dropped a stray leading space).
- Test B: `|year=2002|title=` → `|year=2002 |title=` (added a space to an inconsistent separator).

So the diff an editor reviews is **template-scoped but not always minimal** — it can include a cosmetic whitespace tidy of the *edited* template (never other templates, never prose). This is the VisualEditor "dirty diff" effect, confined to the edited node. Here both normalizations were arguably improvements (more consistent spacing).

## Operational details discovered

- Must use `?stash=true` on the GET **and** `If-Match: <etag>` on the transform POST — without them you get lossy *full* serialization.
- `data-mw` is a **single-quoted** attribute (`data-mw='{"parts":…}'`); ref nodes carry `{"name":…}`, transclusions carry `{"parts":[{"template":…}]}`.
- The stash has a TTL; if a human confirmation arrives after it expires, fall back to passing the full `original` (html + data-parsoid + wikitext) in the POST body instead of relying on `If-Match`.
- Endpoint is RESTBase `/api/rest_v1/` today; the Wikimedia REST surface is mid-migration to core REST `/w/rest.php/` — pin the exact endpoint/version at implementation time.

## Not yet tested (recommended follow-up before locking the impl)

- **Inserting a brand-new node** — the bare-URL-repair case (`#9`): wrap `<ref>http://x</ref>` into `<ref>{{cite web|url=http://x|…}}</ref>`, i.e. replace a ref's *body*, not just a template param. Param modify/add is proven; new-node insertion is plausibly fine but unverified (hand-constructing a new Parsoid ref node risks a false negative, so do it via the real editor path).

## Bottom line for ADR-0002

The gating fidelity question is **answered in the affirmative for the param-edit class** (the most common citation-repair operation and the `#14` case): lossless, node-scoped, style-preserving, with only a minor within-template cosmetic normalization that is acceptable for a human-confirmed, one-edit-at-a-time flow. Parsoid REST stands as the primary `WikitextEditor` implementation; the new-node-insertion case remains to confirm.
