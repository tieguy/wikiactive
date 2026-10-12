> **✅ FILED 2026-06-04 as [schiste/SP42 PR #1](https://github.com/schiste/SP42/pull/1) — renumbered ADR-0003** (0002 was taken by schiste's `local-dev-auth-bridge`). The canonical version is the PR (`SP42/docs/adr/0003-node-anchored-wikitext-editing.md`); this file is the working draft kept for history. The open questions below were moved into the PR description as acceptance gates.
>
> **Draft note.** Proposed ADR for `schiste/SP42`, drafted 2026-06-04 as the opening artifact of a wikiharness↔SP42 collaboration. Written to match SP42's ADR house style (cf. `docs/adr/0001-foundational-decisions.md`) so it can be pasted into `SP42/docs/adr/0002-…md` after review. Number `0002` is provisional (next free in SP42). It leads with Parsoid as the primary implementation because that is the better-justified call *in SP42's own foundational terms* (ADR-0001 §1/§3/§7) — but the WASM-`wikiparser-node` path is documented as a fully-considered alternative and the trait seam makes the choice reversible; flip the emphasis if the offline path is preferred.

---

# ADR-0002: Node-anchored wikitext editing for content actions

**Status:** Proposed
**Date:** 2026-06-04
**Author:** Luis Villa (proposed for SP42; pending review/acceptance by Christophe Henner)

## Context

SP42 today performs **content** edits via first-occurrence literal substring replacement. There are exactly two such sites, both in `sp42-server/src/action_routes.rs`, and neither uses a parser or even a regex:

- `InlineEdit` — `page_text.replacen(&original, &replacement, 1)` (`action_routes.rs:378`), where `original`/`replacement` come from the request's `selected_text`/`replacement_text`.
- `TagCitationNeeded` — `apply_citation_template` builds `{{template|text|date=…}}` and `page_text.replacen(selected_text, &tagged, 1)` (`action_routes.rs:411-423`).

The other action verbs — `Rollback`, `Undo`, `Patrol` — do not touch wikitext; they are server-side MediaWiki operations keyed by `rev_id` (`build_rollback_request` `action_executor.rs:152`; `build_undo_request` uses `undo`/`undoafter` `:240`; `patrol`). They are **out of scope** here.

`replacen(…, 1)` is adequate for SP42's patrol/revert mission, but inadequate for **citation-focused content improvement** — the motivation for this ADR, namely an incoming collaboration bringing LLM-assisted, human-confirmed citation verification and repair onto SP42. Four failure modes:

1. **Wrong-target.** First-occurrence matches the *first* literal span; when the same URL/phrase/citation recurs earlier in the article, it edits the wrong reference.
2. **Verbatim brittleness + TOCTOU.** It requires `selected_text` byte-identical to current page text, yet fetches *current* text while saving with `baserevid` = the patrolled rev — the needle can have moved or changed.
3. **No structural targeting.** It cannot express "wrap this bare URL in `<ref>`" or "set `access-date=` on the 3rd `{{cite web}}`" — operations that require knowing node boundaries.
4. **Silent corruption on commit.** First-occurrence + whole-page save means a mis-located swap writes a corrupted article with no guard.

Prior art exists and is reusable. Luis's wikiharness codebase already ships a node-anchored editing model: `findRefs` / `replaceRefNode` over a **document-order ordinal**, a hash-bound `{ kind: 'ext-ref'; ordinal }` locator, and a `String(node) === expectedText` **anti-drift** re-check (refuse-not-throw on drift/out-of-range), implemented over `wikiparser-node`. A follow-on extends it to cite-template parameters (`findCiteTemplates` / `setTemplateParams`).

## Decision

1. **Introduce a `WikitextEditor` trait** in `sp42-core/src/traits.rs`, with a deterministic in-crate test double — consistent with ADR-0001 §7 (trait-DI for all external I/O) and Constitution Art. 1/2 (mockable, deterministic). Its surface: enumerate `<ref>` / cite-template nodes in document order; replace or modify a node by ordinal; every operation **re-grounds on the expected node text** (refuse-not-throw on drift or out-of-range) and re-serializes the page **losslessly**.

2. **Route content-edit actions through it.** Replace the two `replacen` sites with node-anchored operations for structured (ref/template) edits.

3. **Extend the action contract.** `SessionActionExecutionRequest` gains an optional **node locator** (a node-kind + document-order ordinal + the expected node text used as the anti-drift anchor), alongside or replacing the literal `selected_text`. This is a protected contract/schema change; **this ADR is its record**.

4. **Immediate safety hardening (parser-independent, ship first).** For any remaining literal-span path, replace blind `replacen(…, 1)` with an **exactly-one-occurrence guard**: reject the edit unless the needle occurs exactly once. This closes the wrong-target / silent-corruption risk (failure modes 1 & 4) *today*, before the editor lands, at near-zero cost.

5. **Implementation choice: Parsoid REST is the primary `WikitextEditor`, gated by the checks in §Validation status; a WASM build of `wikiparser-node` is the documented fallback / offline successor** (§Alternatives).

6. **Implementation vehicle for the Parsoid impl: the `parsoid` crate** ([mwbot-rs](https://gitlab.wikimedia.org/repos/mwbot-rs/mwbot), Kunal Mehta; **GPL-3.0-or-later**). A maintained (47 releases; v0.11 Feb 2026) typed Rust wrapper over Parsoid HTML (Kuchiki-based) with **first-class `cite` / `Reference` / `ReferenceList` node types and a `Template` API (`filter_templates()`, `template.param("…")`)** — exactly our operations — fetching via `Client::get` (core REST `/w/rest.php`) and round-tripping via `Client::transform_to_wikitext`. Prefer it over hand-rolled `reqwest` + `data-mw` JSON munging (what the spike did). Caveats: (i) it is **GPL-3.0-or-later** — the same `deny.toml`/licensing action as the wikiparser-node option, **but Rust-native with no JS engine, so it does *not* incur ADR-0001 §1's "no-TypeScript" cost**; (ii) it still serializes via Parsoid REST (it is *not* an offline serializer) — and a source read (2026-06-04) found it uses `flavor=edit` HTML + revid, **not** the explicit stash/`If-Match` selective-serialization flow the raw spike proved, so its byte-level diff cleanliness is **unconfirmed** (see §Validation status); (iii) its `Wikicode` is `!Send` (Kuchiki/`Rc`) — use the provided `ImmutableWikicode` across `.await` points in the axum handler.

**Rationale (why Parsoid first, in SP42's own terms):**

- **Authoritative fidelity — validated by a spike.** Parsoid *is* MediaWiki's parser. Its selective serialization (html → wikitext, re-serializing only changed subtrees and copying untouched regions verbatim) is VisualEditor's own save path — so the "corrupts an article" risk is outsourced to the canonical implementation rather than re-created in a new parser. A round-trip test spike by Luis's LLM (2026-06-04, en.wp `Cosmic latte` rev 1357330394; see `wikiharness/docs/analysis/2026-06-04-parsoid-fidelity-spike.md`) confirmed: a no-op round-trip is **byte-for-byte lossless**; a targeted `{{cite}}` param modify/add re-serializes **only that one template** (all other refs/templates/prose byte-identical) and **inserts the new param in the template's existing style** — the very property `#14` had to hand-build over wikiparser-node. For an article-integrity-critical write path, this is the conservative choice.
- **Congruent with ADR-0001.** It is an **HTTP edge behind a trait** (§7) that adds **no new language** (§1, "no TypeScript layer"; "wasm-bindgen bridge eliminated"); SP42 already depends on external Wikimedia services — EventStreams (§8), LiftWing (§9), the Action API — so a Parsoid REST dependency fits the existing posture. Dependency footprint depends on the vehicle (Decision 6): a bespoke `reqwest` client adds **no crate** (`deny.toml` clean), while the recommended `parsoid` crate adds **one GPL-3.0 Rust dependency** (a `deny.toml`/licensing action — but no JS, unlike the WASM-wikiparser path).
- **Nothing to build at the grammar level** — fastest path to de-risking the whole question.

**Trade-off:** adds an external-service dependency and latency in the edit path (fetch Parsoid HTML with `?stash=true`, then POST the modified HTML with `If-Match: <etag>` for selective serialization — two round-trips per edit). Acceptable for a human-confirmed, one-edit-at-a-time flow; but may be a challenge for a bulk path. The one residual fidelity cost (measured in the spike): Parsoid re-serializes the **whole edited template** from `data-mw`, lightly **normalizing that template's own internal whitespace** toward its dominant style (e.g. `|year=2002|title=` → `|year=2002 |title=`) — so the reviewed diff is template-scoped but **not always minimal**. It never touches other templates or prose. For a human-confirmed flow this is acceptable (often an improvement); a minimal-diff editor (wikiparser-node) avoids it.

## Alternatives considered

- **(a) A WASM build of `wikiparser-node`** — the offline path. **Pros:** deterministic and offline (no per-edit network or availability dependency — the strongest fit for SP42's determinism ethos, Constitution Art. 2); reuses wikiharness's already-built node-anchored logic essentially verbatim; one WASM module could serve **both** `sp42-server` and the Leptos browser frontend (enabling live edit-preview). **Cons in SP42 specifically:** it reintroduces JavaScript — a JS engine embedded in WASM (e.g. Javy/QuickJS) or a reimplementation — in tension with ADR-0001 §1 ("no TypeScript layer," bridge eliminated); `wikiparser-node` is **GPL-3.0-or-later**, against the permissive-only `deny.toml` (§3) — resolvable, but a real licensing/governance action; and it carries a heavier build and maintenance surface. **Recommended as the successor** if Parsoid's latency, availability, or drivability disappoint; the trait seam (Decision 1) makes that a one-implementation change. Note: the `parsoid` crate (Decision 6) already delivers a GPL, Rust-native, typed Parsoid-backed editor **without** a JS engine — so (a)'s only remaining edge over the recommended path is **offline operation** (no per-edit network), which is its real and only reason to exist.

- **(b) A pure-Rust structured editor** over an offset AST (`parse-wiki-text-2` supplies positions; byte-splice + anti-drift). **Pros:** pure Rust, offline, no GPL, no JS — the cleanest fit on paper. **Cons:** no existing Rust crate does round-trip-**lossless** structured `<ref>`/template editing today (the crates are read-only/offset-only); the MediaWiki-grammar edge-case tail is the dominant risk and the long pole, and SP42 would become the maintainer of a parser the ecosystem currently lacks. **Defer** unless both (a) and Parsoid are rejected.

- **(c) Keep `replacen`, hardened with the ambiguity guard.** **Adopted as the interim** (Decision 4) but **rejected as the end state**: even guarded, it cannot express structural operations (failure mode 3) and cannot anchor an LLM-proposed citation edit to a specific node.

## Validation status (as of 2026-06-04)

This decision is **provisional pending the acceptance gates in the merge request** (below). Evidence to date — recorded here because it is stable (it states what was tested and remains true):

- **Proven.** Param **modify/add** on an existing `{{cite …}}` round-trips **lossless + node-scoped + style-preserving** via the raw RESTBase `/api/rest_v1/` transform with `?stash=true` + `If-Match` (`wikiharness/docs/analysis/2026-06-04-parsoid-fidelity-spike.md`). The one residual cost is light whitespace normalization *within the edited template only*.
- **Mechanism known, fidelity unconfirmed.** A source read of the `parsoid` crate (`parsoid/src/api.rs`) found it fetches `flavor=edit` HTML (inline `data-parsoid` present) and transforms via core REST **without** the explicit stash/`If-Match` selser flow — so its byte-level cleanliness is not yet proven equal to the raw spike's. If it under-preserves, the fallback (behind the trait, Decision 1) is to contribute selser upstream or pair the crate's manipulation with a bespoke stash/`If-Match` transform.

The still-unresolved items are tracked as **acceptance gates in the MR description** (below), not in this body, so the merged record stays a *decision* rather than a TODO list (cf. ADR-0001's single fallback-stated open question).

## Consequences

- A new `WikitextEditor` trait + deterministic double in `sp42-core`; the two `replacen` content-edit sites rewired; `SessionActionExecutionRequest` and its schema gain a locator (a protected contract/schema change — recorded by this ADR).
- **Parsoid implementation:** a new HTTP dependency on a Parsoid REST endpoint; operationally a GET (`?stash=true`) + a stateless transform POST (`If-Match: <etag>`) per edit; reviewed diffs are template-scoped with occasional within-template whitespace normalization (per the spike). Dependency footprint is a sub-choice (Decision 6): the **`parsoid` crate** (recommended) adds **one GPL-3.0 Rust dep** — a `deny.toml`/licensing action, but Rust-native, typed, and Wikimedia-maintained; a **bespoke `reqwest` client** adds **no crate** (`deny.toml` clean) at the cost of hand-maintaining `data-mw` manipulation. **WASM-`wikiparser-node` fallback:** also GPL, but additionally embeds a JS engine; its sole advantage is offline/minimal-diff operation.
- SP42's content-edit path gains structural correctness and anti-drift; combined with the Decision-4 hardening, the silent-corruption-on-commit risk is closed immediately.
- The incoming citation-verification work can anchor its human-confirmed, hash-bound edits to specific nodes through this contract — the clean integration seam — instead of flattening to a lossy literal span.
- The `Rollback` / `Undo` / `Patrol` verbs are unaffected (no wikitext manipulation).
- **Non-goal (deferred):** reusing this editor in the Leptos frontend for live edit-preview is out of scope for this ADR; if wanted, a later ADR decides it (and would tilt toward the offline WASM-`wikiparser-node` vehicle).

---

> **Merge-request description (review-time — paste into the GitLab MR, NOT into the merged ADR).** These are the items to close in review; fold each resolution into the body before merge so the immutable record carries no open TODOs.
>
> **Acceptance gates (resolve before merge):**
> 1. **Bare-URL-wrap round-trip** — confirm inserting a *new* node (wrap a bare-URL `<ref>` body into `{{cite web|url=…}}`) round-trips cleanly; the 2026-06-04 spike covered param modify/add only.
> 2. **`parsoid`-crate serialization fidelity** — run the crate-level spike: does `transform_to_wikitext` preserve untouched wikitext byte-for-byte, or normalize more than selser? If it under-preserves, choose the fallback (upstream selser vs bespoke transform) and record it in Decision 6.
> 3. **Locator shape (ratify)** — model both the `<ref>` and cite-template ordinal spaces in the locator union now, or ship `<ref>` first and extend? Decide in review and fold into Decision 3.
> 4. **Stash-expiry fallback** — confirm the behaviour when an `If-Match` stash has aged out (pass the full `original` in the POST body).
