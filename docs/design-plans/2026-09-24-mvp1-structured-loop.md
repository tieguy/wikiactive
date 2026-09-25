# wikiactive MVP-1 — Structured Wikipedia Improvement Loop (agent-driven)

## Goal

Build MVP-1 in `/home/louie/Projects/wiki/wikiactive`: a Claude-Code-driven editing loop for en.wikipedia articles that proposes one **logical edit** at a time, every content change grounded in a quoted source from a session source ledger ("never edit from model memory"), reviewed by the operator as a **visual diff in lavish-axi** with pinpoint comments, and **published per-edit via OAuth only on explicit human confirmation**. Validation: offline replay against frozen revisions, then one live article session.

## Implementation Summary

**Process model (the structured loop).** An article session is a state machine over a loop ladder, selected by triage:

- **L1 Rescue**: structure scaffold, MOS-compliant headings, lead sentence with first citation. Minimal touch by design.
- **L2 Mine existing sources**: verify current claims against the article's *existing* citations (claim ↔ quote check), extract more from those sources, fix citation formats, remove uncited material, Earwig-check suspicious passages, kill spamlinks.
- **L3 GA-style pass**: substantive criteria first (V, NOR/SYNTH, NPOV, coverage/UNDUE), stylistic later (MOS:LEAD, Words to watch, linter).
- **L4 New sources, one at a time**: each publish unit is one source → integrated content (possibly a new section); conflicts between sources become `{{efn}}` notes, never silent adjudication.
- **L5 Wikidata writeback** — deferred out of MVP-1; the ledger schema keeps the hooks.

**Triage selects the entry loop, not just whether L1 runs:** articles already at basic quality enter directly at L2 or L3 — notably articles the operator improved in the past and is now re-reviewing for accumulated drift, which enter at the L3 GA-style pass with particular attention to changes since the operator's last edits (the diff from the prior session is included in the analyze context bundle).

Disposition ladder for discovered material: *use in article with attribution → `{{efn}}` (workhorse) → talk page (only negative results and the end-of-session provenance note) → drop.*

**Granularity & history.** One publish = one logical edit = one scoped, specific edit summary (borrowed from code review). Transparent on-wiki history is a hard requirement; no wholesale replacements.

**Enforcement tiers (wiki rule → harness rule).**

| Tier | Content | Mechanism |
|---|---|---|
| 1 — Judgment core (always in context) | NPOV-as-attribution (WIKIVOICE/ATTRIBUTE/WEASEL, incl. BLP-by-analogy for the dead); V/SYNTH/PRIMARY at sentence level (claim-scope match, no source fusion, no negative-search claims); LEAD/SUMMARYSTYLE | Distilled operative clauses, standing checklist for the proposing model |
| 2 — Trigger cards (on demand) | UNDUE, NOTLITREVIEW, PROSELINE, RECENTISM, LEADCITE, CLOP, RS/RSP tiers, PRIMARY carve-outs, SUMMARY/SPINOFF, efn-for-conflicts | Per-rule card: operative clause + canonical link + failure examples |
| 3 — Mechanical (no judgment) | CITEVAR house style (named refs, no `:0` auto-names, no `{{sfn}}`, `{{rp}}` for pages), `\|page=`/`\|pages=` consistency, MOS:US, MOS:TENSE, MOS:TITLES italics, heading spacing, `{{see also}}` placement, **lead-vs-body shingle-overlap duplication check (mechanized MOS:LEAD: the lead summarizes, never duplicates a section)**, **house rule: no semicolons in drafted prose** | Wikitext linter + deterministic checks over every proposed diff |

**Anti-hallucination machinery.** Session **source ledger** (JSON): every source registered with URL, archive.org URL, fetched full text, access date; every added/changed claim carries `quote_ids` of verbatim quotes from ledger sources. **Quote-anchor validator**: verbatim-with-transcription-artifact-folding match of each quote against the fetched source text — ported from SP42's `locate_quote` (ADR-0007 §5): NFC, whitespace-run collapse, curly→straight quotes, dash unification, zero-width stripping, case folding; ellipsis-elision quotes match fragment-by-fragment in document order with minimum-fragment-length and bounded-window anti-stitching (SP42#25); reworded or fabricated quotes still never match. **Enforcement point:** `checks/gate.rs` is render's mandatory pre-flight — a finding whose evidence quotes fail validation blocks artifact emission entirely (no artifact written), and the same gate re-runs before publish's tty confirmation. Claims without anchors never reach the review diff (AC.11). **Paraphrase gate**: local shingle-similarity between added prose and its claimed source — flags both "too close" (CLOP) and "unsupported" directions. Dispositions: a *too-close* flag hard-blocks render exactly like an anchor-gate failure (proceeding requires revising the prose); a *no-support* flag requires anchor resolution before render. This is WP:V mechanically enforced. **Earwig** (`copyvios.toolforge.org/api.json`) runs *post-publish* (`action=compare` against each new web source; `action=search` for unknown copies) since it only sees on-wiki revisions; pre-publish close-paraphrase detection is the local gate.

**Finding model** (unified for Tier 1/2; linter output stays flat): `{id, wikitext_anchor, rendered_span_id, rules[], evidence: ledger quotes + source ids, factual_note, proposed_fix, loop}`. **Authoring & field ownership:** findings enter via `wa findings add --json` (schema-validated; malformed entries rejected); the model authors `id`, `rules[]`, `evidence` (ledger quote ids), `factual_note`, `proposed_fix`, and a draft `wikitext_anchor`; the pipeline back-fills `rendered_span_id` at render time and verifies/resolves anchors at poll time.

**Stack: Rust** (recommended over Go; rationale recorded). Toolchain mirrors SP42 conventions: edition 2024, rust-version 1.96, workspace lints `warnings=deny`/clippy pedantic, `tracing` for logs. Core libraries: **`mwapi`** (Action API client: OAuth2 auth, `post_with_token` CSRF, `assert=user`, rate-limit etiquette) for all authenticated API work including `action=edit` with `baserevid`; **`parsoid`** crate (v0.10.x, already proven in SP42's `sp42-parsoid`) for wikitext↔HTML transforms; **`similar`** for diffing (also in SP42's dep set); `reqwest` (rustls) for plain HTTP (source fetch, Earwig, archive.org save-page-now); `scraper`/html5ever for anchor resolution; `clap` CLI; `serde`/`serde_json` throughout; `httpmock` (or a small axum test server) for API tests. **SP42** (`~/Projects/Volunteering-Consulting/SP42`) is the **reuse source, not just a pattern reference** (the operator maintains SP42 and explicitly welcomes code reuse): (a) **copy-with-provenance** from `sp42-citation` the domain-agnostic citation primitives — `locate_quote`/`FuzzyLocate` (ADR-0007 §5 anti-fabrication locator), `segment_sentences`, `source_fetch` (`html_to_text`, wayback body recovery), `citoid` metadata handling, `extract_use_sites`, and the verify-gate shape from `verify.rs` — each copied file carrying a provenance header (crate, commit, license); also reference `sp42-assessment`'s `render_ga_appendix` (pure wikitext GA-reviewer evidence-appendix builder — a ready-made model for the Loop-3 report artifact) and `sp42-devtools`' deterministic-fixture pattern; (b) **consult-as-anti-example** where SP42 is patrol-shaped (verdict voting, scoring — wikiactive needs hard-block dispositions, not scores); (c) **post-MVP convergence** — file SP42 issues proposing promotion of shared primitives to `sp42-platform` or a standalone crate/CLI subcommand wikiactive can call unix-style; explicitly not an MVP-1 blocker; (d) **no build-time cross-repo dependency** (no path/git deps — reuse is by copy until a shared crate exists). Additional pattern references: `crates/sp42-fetch`'s SSRF-guarded fetch edge + Wikimedia UA discipline, `crates/sp42-app/src/platform/auth.rs`'s OAuth2 endpoint/config conventions (note: SP42 implements the authorization-code flow, whereas wikiactive uses `mwapi`'s documented OAuth2 **owner-only** path — a token issued at consumer registration, no authorize/token exchange), and `sp42-parsoid` usage. Static musl release builds cover the portability rationale for the eventual MVP-2 server. **License consequence:** `mwapi` and `parsoid` are GPL-3.0-or-later. Because SP42 (the copy source) is **GPL-3.0-only**, copying its code makes wikiactive's effective license **GPL-3.0-only**; set LICENSE accordingly when the first copy lands (operator has accepted GPL either way). Recorded as an accepted decision below.

**Review surface: lavish-axi** (recon verdict: reuse; pinned `npx -y lavish-axi@0.1.78`; Node ≥ 22 required *only* for lavish). One artifact file per article session, regenerated in place each round (live reload, per-round revisions legend via `data-lavish-revisions`). The artifact is a two-pane (old/new) rendered-article diff **with an evidence rail**: each changed block that carries findings or claims emits an expandable evidence view — full source citation, the verbatim ledger quotes supporting it, archive URL, and attached findings — so the reviewer verifies quote→prose mapping in place; evidence blocks carry `data-wiki-anchor="ledger:Q<n>"` ids so operator comments on a citation map to the ledger entry rather than to wikitext. Every changed block gets `id="wa-N"` + `data-wiki-anchor` (wikitext line:col range) so human comment selectors root at unique ids. Comments resolve back to wikitext spans in Rust (html5ever DOM; replicating lavish's id-rooted CSS selector semantics and text-range path+offset model). **Normative reference:** the lavish-axi v0.1.78 source itself (npm tarball vendored in-repo) plus captured real poll/TOON output and at least one real annotation payload committed as fixtures in Phase 0 — the public README documents neither the payload schema nor TOON concretely, so observed data is the contract the parser and `anchors.rs` are built against.

**Publish path: OAuth2 owner-only consumer + `mwapi`** (endpoint/config conventions informed by SP42's auth scaffolding; the flow itself is mwapi's owner-only path; credentials via env, never committed). Session pins `base_revid`; `action=edit` with `baserevid` + `assert=user` aborts on conflict (current revid ≠ base). Publish command requires an interactive `/dev/tty` confirmation — a model-initiated run stalls without the human. Post-publish: Earwig compare; citations carry `|archive-url=` from ledger provenance; end-of-session talk-page note offers source scans on request. **Disclosure (operator decided):** every published edit summary automatically carries the suffix `LLM-Disclosure: U:LuisVilla/wikiactive`, appended by the publish path (mechanically enforced, format in `house-rules.toml`), and the tool maintains that userspace page as the full disclosure artifact: standing methodology section (loop ladder, rule tiers, quote-anchoring, per-edit human gate), per-session log (article, date, drafting model + version, per-edit diff links; default on, config toggle), and a discussion section for other editors. The OAuth consumer is named descriptively (e.g., "wikiactive — disclosed AI-assisted editing loop") so the automatic tool tag on edits is self-explanatory. The disclosure page also carries **screenshots of the review UI** — captured from the live artifact page (it is itself a browser page), uploaded to Commons as own work, with the evidence rail as the canonical demonstration of citation review. MVP-1 ships no Commons upload client (OAuth grants deliberately exclude upload): the operator captures and uploads screenshots manually, declaring licensing per Commons practice for screenshots that depict CC BY-SA-licensed article text.

**Repo layout:**

```
wikiactive/
  Cargo.toml              # single crate `wikiloop` (bin+lib), edition 2024, rust-version 1.96
  LICENSE                 # GPL-3.0-or-later
  CLAUDE.md               # repo conventions: product-internalized UA/etiquette rules (docs/api-etiquette.md), PLAYBOOK pointer
  README.md               # setup: rust toolchain, Node>=22 + pinned lavish, OAuth2 consumer runbook, env vars
  PLAYBOOK.md             # the loop driver: session protocol, ladder, per-loop rule packs
  docs/design-plans/2026-09-24-mvp1-structured-loop.md   # this design + loop spec
  src/
    main.rs  cli.rs       # clap: session init / analyze (context bundle) / findings add / render / poll / publish
    ledger.rs             # source ledger: register, fetch, archive.org SPN, quotes, claims
    checks/               # quote_anchor.rs, paraphrase.rs, linter.rs, gate.rs (render pre-flight enforcement)
    render.rs             # parsoid transforms, similar-diff, artifact emitter (ids, anchors, revisions registry)
    anchors.rs            # selector/text-range → wikitext anchor resolution (html5ever)
    lavish.rs             # artifact file mgmt, poll (TOON parse), agent-reply invocation
    wikipedia.rs          # mwapi wrapper: fetch@revid, parse, edit(confirm, baserevid, assert)
  rules/
    canonical/            # verbatim rule pages fetched for reference (disk only)
    tier1-core.md  cards/*.md  linter.toml  house-rules.toml  sources/rsp-seed.tsv
  sessions/<article-slug>/   # ledger.json, findings.json, review.html, round log
  fixtures/replay/        # frozen wikitext + expected findings
  fixtures/parsoid/       # recorded Parsoid transform responses (offline render/replay tests)
  fixtures/lavish/        # captured poll/TOON output + real annotation payload (Phase 0)
  vendor/lavish-axi-0.1.78.tgz   # vendored normative reference (npm tarball)
  docs/spike-notes.md     # Phase-0 manual round-trip log
  tests/                  # cargo integration tests (unit tests inline in modules)
```

**Scope boundaries (non-goals for MVP-1):** local web app + z.ai model API (MVP-2; all core logic is framework-free Rust so the binary grows a server later via axum, per SP42's stack), Wikidata writeback execution (schema hooks only; `mwapi` works against Wikidata too when the time comes), other wikis/languages, bot flags or unattended operation of any kind, multi-article batch, cross-repo dependencies on SP42 crates.

**Standing constraints (product-internalized, per operator direction):** the tool must not depend on any external/on-disk skill — the completion state is that `wikiloop` has the `wikimedia-api` skill's substance **built in**: UA construction identifying the operator (`wikiactive/<ver> (en.wikipedia User:LuisVilla; luis@lu.is)`) hardcoded in the HTTP client and asserted by tests; API:Etiquette as code (`maxlag` on all API requests, Retry-After/429/503 backoff, `assert=user`, error-code handling); the codified rules written to `docs/api-etiquette.md` for maintainers. During development the execute agent may load the `wikimedia-api` skill as a *source* from which to derive these rules, but the deliverable enforces them itself (AC.13). `~/.lavish-axi/` state is local and unauthenticated — keep the server loopback-only.

## Implementation Plan

### Phase 0 — Scaffold + lavish spike (decision gate)

1. `cargo init`, git baseline, `.gitignore` (sessions/, .env, target/); license file.
2. Empirical TOON verification: run one real `lavish-axi` session headless (`LAVISH_AXI_NO_OPEN=1`), capture `poll` stdout; **vendor the lavish-axi@0.1.78 npm tarball in-repo as the normative payload/TOON reference**, and commit the captured TOON output plus at least one real annotation payload under `fixtures/lavish/`; write the TOON parser against these observed fixtures with a fixture test.
3. Anchor round-trip hello world (throwaway spike — production `anchors.rs` lands in Phase 4): generate a small artifact with `id`/`data-wiki-anchor` spans, annotate by hand once, resolve selector + text-range payloads to expected anchors; log the manual round-trip in `docs/spike-notes.md`.
4. **Live interop roundtrip (operator-requested):** fetch a real userspace page (`User:LuisVilla/wikiactive/spike`) or a testwiki page → parsoid wikitext→HTML transform → render a review artifact from it → lavish annotate → poll → resolve anchors. This exercises the full lavish + mwapi + parsoid chain against live infrastructure before build-out. The *write* roundtrip (an actual page edit) rides the Phase 3 smoke-publish unless OAuth credentials already exist when Phase 0 runs.
5. **Gate:** if anchor round-trip is unworkable, fall back to a minimal home-built viewer (pre-agreed with operator); otherwise proceed with lavish.

### Phase 1 — Rules corpus

1. Fetch canonical rule pages to `rules/canonical/` (WP:V, NOR, NPOV, GACR, MOS:LEAD, MOS:LAYOUT, CLOSEPARAPHRASE, Words to watch, LEADCITE, RSP, plus Loop-3/4 cards' targets) — disk only; distillates go in context.
2. Author `tier1-core.md`: the three clusters as operative clauses, with fixtures mixing the operator's own failure examples (3-million-copies scope; Danish survey fusion; Gouldner wikivoice; lead/body duplication) **and upstream examples from the canonical documentation** (NPOV attribution examples, CLOSEPARAPHRASE samples, Words-to-watch entries, MOS:LEAD guidance) so the corpus doesn't over-index on one editor's experience.
3. Author ≥ 10 trigger cards; `linter.toml` + `house-rules.toml`; `rsp-seed.tsv` seeded from the archaeology hit list (deny: Grokipedia-class/predatory; caution: TED talks, personal blogs, extension blogs; complement-only: Find a Grave).

### Phase 2 — Core checks (pure logic, fully unit-tested)

1. `checks/quote_anchor.rs`: **ported from SP42 `sp42-citation::locate_quote`** (copy-with-provenance per the reuse policy) — verbatim-with-artifact-folding match of quotes against source text; per-claim anchor verification; tests re-cover SP42's documented edge cases (re-cased quote locates; reworded or fabricated quote does not; ellipsis-elision bounded).
2. `checks/paraphrase.rs`: shingle/n-gram similarity (`similar`-adjacent logic) between added prose and ledger source text; thresholds for too-close and no-support.
3. `checks/linter.rs`: Tier-3 rules in **two scopes** — whole-page scan mode (run by `wa analyze` and replay to report pre-existing defects; this is how AC.8's frozen-article fixtures are surfaced) and added-lines gating mode (over proposed diffs).

### Phase 3 — Ledger, archives, API client

1. `ledger.rs`: register/fetch/archive (web.archive.org save-page-now, record archive URL), quotes, claims. Fetch layer mirrors `sp42-fetch` discipline (redirect/size caps, UA, retry-after) at pattern level, reimplemented locally. Earwig and SPN clients get httpmock contract tests (request shape, UA header, error/cap handling) — AC.12.
2. `wikipedia.rs` on `mwapi`: OAuth2 owner-only consumer auth (token issued at registration; no authorize/token exchange — do not copy SP42's authorization-code flow); fetch wikitext at pinned revid; parse wikitext→HTML via `parsoid`; `action=edit` with `baserevid`, `assert=user`, tty confirm (read through an injected `ConfirmSource` so tests simulate absent/declined confirmation), conflict abort, summary with disclosure suffix; validate-only dry-run mode. The client hardcodes etiquette (identifying UA, `maxlag`, Retry-After backoff) and codifies it in `docs/api-etiquette.md` — the product-internalized equivalent of the wikimedia-api skill (AC.13); during implementation, the `wikimedia-api` skill is read as the derivation source, not a runtime dependency.
3. OAuth runbook executed by operator: create owner-only OAuth2 consumer (meta.wikimedia.org `Special:OAuthConsumerRegistration/propose`, grants: basic + edit existing pages; name it descriptively — e.g., "wikiactive — disclosed AI-assisted editing loop" — per the disclosure design), export env vars; smoke-publish to `User:LuisVilla/wikiactive/smoke`, then blank it. During this runbook (or the first session), author the disclosure page's standing methodology section once; subsequent sessions only append log entries.

### Phase 4 — Renderer + anchor resolver + evidence rail

1. `render.rs`: base+proposed wikitext → HTML (parsoid transform); paragraph-aligned diff (`similar`); two-pane scroller artifact (no clipping, plain scrollers, desktop viewport env); changed blocks wrapped with `id="wa-N"`, `data-wiki-anchor`, embedded anchor table, `data-lavish-revisions` registry per round. Render invokes `checks/gate.rs` as mandatory pre-flight (AC.11).
2. **Evidence rail (accelerated from MVP-2 at operator request — the disclosure page's canonical screenshot):** each changed block with findings/claims emits an expandable evidence view — full source citation, highlighted verbatim ledger quotes, archive URL, attached findings. Evidence blocks carry `id="ev-N"` + `data-wiki-anchor="ledger:Q<n>"` so comments on a citation map to the ledger entry. Same scroller-safety rules as the diff panes.
3. `anchors.rs`: resolve lavish comment payloads (id-rooted CSS selectors; text-range `{selector, path, offset}`) against the exact artifact DOM → wikitext anchors (diff panes) or ledger quote ids (evidence rail), per the vendored source and Phase-0 captured payloads.
4. **Offline test fixtures:** record Parsoid transform responses for the fixture wikitext into `fixtures/parsoid/` (re-record procedure for Parsoid drift documented in README); render and replay tests run fully offline against recorded HTML.
5. Golden tests: anchor ids unique; table↔wikitext ranges round-trip; revisions registry matching the documented shape ({id,label,timestamp,summary} script block + `data-lavish-revision` per changed block — the legend lists at most 6 rounds and silently ignores malformed registries, so rounds beyond 6 stay recorded in the registry but are unlisted; noted in PLAYBOOK); synthetic payload resolution; gate refusal; evidence-rail completeness (every gated finding shows at least one quote whose id resolves into the session ledger).

### Phase 5 — Loop playbook + CLI

1. `PLAYBOOK.md`: session protocol (pin revid → triage ladder → per-iteration: **step 0 = run `wa analyze` and load its output before proposing anything**; analyze emits a *context bundle* — pinned-revid article state, tier1-core clauses verbatim, triage-selected trigger cards, findings/ledger summary — the mechanical guarantee behind "Tier 1 always in context" → findings → propose logical edit → checks green → render → lavish open/poll → resolve comments → revise → operator confirms → publish → re-pin → next), comment conventions, publish gate invariant (model never self-publishes; tty confirm; anchor-gate re-run), end-of-session artifacts (TALK provenance note; disclosure-page session log append **with screenshots** — captured from the live artifact page, uploaded to Commons as own work, with an evidence-rail round as the canonical citation-review demonstration — and Earwig post-checks).
2. `cli.rs` wiring: `wa findings add --json` (schema-validated findings authoring per the field-ownership contract; `tests/findings_cli.rs` asserts malformed entries are rejected and valid ones accepted) alongside session init / analyze / render / poll / publish; repo `CLAUDE.md` pointing at PLAYBOOK + UA rules.

### Phase 6 — Replay validation (offline)

1. Freeze fixtures: `Commitment device@1343452323`, `Temple Fielding@1372827284` (pre-session 2026-09-24).
2. **Replay = a full offline PLAYBOOK session** (model in the loop, publishing disabled) over each frozen revision, producing the findings report. Mechanically-asserted fixtures (`tests/replay.rs` via the deterministic checks): TF `:N`-style auto ref names; TF unspaced `==` headings; CD lead/"Concept and mechanisms" duplication via the lead-vs-body shingle check.
3. Soft report for operator grading against an expected-findings checklist (judgment findings — e.g., Nudgewise/see-also spamlink, game-theory/UNDUE absence, Robert Frank synthesis, recentism — surfaced by the model, never asserted by code; findings graded, never prose-matched — training-data contamination caveat).

### Phase 7 — One live session (operator picks a low-traffic article)

Full loop, ≥3 logical edits published; post-publish Earwig per source; TALK provenance note; operator sign-off. Blast radius minimized: low-traffic, non-BLP, non-contentious article (operator's final call), watchlist set, cadence mirrors the operator's manual practice (scoped summaries, ~10–45 min rhythm as applicable).

## Acceptance Criteria

- **AC.1** Rules corpus + context bundle load and validate: `tests/rules_corpus.rs` asserts tier1-core exists with the three clusters, ≥10 cards, linter config parses, house rules include the semicolon ban; and that the `wa analyze` context bundle embeds the tier1 clauses verbatim plus the triage-selected cards (fails if the corpus is silently dropped from the bundle).
- **AC.2** Quote-anchor validator behaves per spec: `tests/quote_anchor.rs` — verbatim quote passes; absent quote fails; punctuation/whitespace-varied quote passes; wrong-source quote fails.
- **AC.3** Paraphrase gate discriminates: `tests/paraphrase.rs` — near-verbatim passage flags too-close; clean paraphrase with supporting quote passes; unrelated text flags no-support.
- **AC.4** Linter catches every Tier-3 rule on added lines only: `tests/linter.rs` derives one generated case per rule declared in the parsed `linter.toml` (no enumeration drift between config and test), with fixed fixtures for `:0` ref name, `{{sfn}}` usage, `|page=`/`|pages=` inconsistency, MOS:US mix, MOS:TENSE, `{{rp}}` usage, heading spacing, `{{see also}}` misuse, semicolon in drafted prose, mismatched italics, and lead-vs-body duplication.
- **AC.5** Renderer emits resolvable artifacts: `tests/render.rs` golden test, offline against recorded Parsoid fixtures — unique ids, revisions registry matching the documented shape, anchor table maps every id to a non-empty wikitext range in the proposed wikitext, and the evidence rail renders for every gated finding at least one quote whose id resolves into the session ledger.
- **AC.6** Anchor resolution round-trips: `tests/anchors.rs` — synthetic selector and text-range payloads in lavish's documented format resolve to expected `data-wiki-anchor` values. Plus a one-time manual lavish round-trip (Phase 0 gate, logged in session notes) — manual because annotation requires a human hand.
- **AC.7** Publish path is safe: `tests/publish.rs` (httpmock/fake API server) — tty-confirm absent → refuses; base revid moved → aborts with conflict; summary always present and carrying the configured `LLM-Disclosure:` suffix (publish refuses a bare summary); `assert=user` included on edit; disclosure-page log append is idempotent. Integration: one real publish to `User:LuisVilla/wikiactive/smoke` then blank (operator-observed).
- **AC.8** Replay surfaces known findings: `tests/replay.rs` — all mechanically-checkable fixtures (TF auto ref names, TF unspaced headings, CD lead/body duplication) reported by the deterministic checks over frozen wikitext; judgment findings captured in the replay soft report (full offline PLAYBOOK run, publishing disabled) and graded by operator against the expected-findings checklist (manual review, logged).
- **AC.9** One live session completes: ≥3 logical edits published with scoped summaries; each round's comments demonstrably incorporated (session round log + on-wiki diffs); operator signs off in writing.
- **AC.10** Freshness invariant: publish against any base other than current revid never writes — asserted by `tests/publish.rs::test_stale_base_revid_aborts` (mock) and by the `wikipedia.rs` code path.
- **AC.11** Anchor-gate enforcement (the "never edit from model memory" invariant): `tests/gate.rs` — a finding whose evidence quotes fail quote-anchor validation against the session ledger blocks render (no artifact written) and blocks publish; the gate is render's mandatory pre-flight and re-runs before publish's tty confirmation.
- **AC.12** External integration contracts: `tests/integrations.rs` (httpmock) — Earwig compare and archive.org save-page-now request shapes, UA header, and error/cap handling.
- **AC.13** Wikimedia etiquette is product-internalized: `tests/integrations.rs` (httpmock, mwapi client) — the identifying UA header is sent on every request, `maxlag` is included on API calls, Retry-After/backoff is honored on 429/503, no code path reads external skill files; `docs/api-etiquette.md` documents the codified rules.

## Test Strategy

| AC | Test | Layer |
|---|---|---|
| AC.1 | `tests/rules_corpus.rs` (corpus + context bundle) | Unit |
| AC.2 | `tests/quote_anchor.rs` | Unit (pure logic) |
| AC.3 | `tests/paraphrase.rs` | Unit |
| AC.4 | `tests/linter.rs` (config-derived cases) | Unit |
| AC.5 | `tests/render.rs` (offline golden files) | Unit + golden files |
| AC.6 | `tests/anchors.rs` + manual Phase-0 round-trip | Unit + manual procedure (annotating needs a human; limitation accepted and logged) |
| AC.7 | `tests/publish.rs` (mocked API) + real userspace smoke publish | Integration |
| AC.8 | `tests/replay.rs` + operator grading session | Integration (offline PLAYBOOK run) + manual |
| AC.9 | Live session artifacts + operator sign-off | Manual acceptance (inherent: the criterion is a human work product) |
| AC.10 | `tests/publish.rs::test_stale_base_revid_aborts` | Unit/mock |
| AC.11 | `tests/gate.rs` | Unit |
| AC.12 | `tests/integrations.rs` (httpmock) | Unit/contract |
| AC.13 | `tests/integrations.rs` (client etiquette assertions) | Unit/contract |

Gaps stated plainly: no automated UI test exists for the lavish review experience (mitigated by the Phase-0 manual round-trip and the live session); soft judgment findings cannot be auto-graded (operator grading is the test); TOON/payload formats are verified empirically in Phase 0 and pinned as committed fixtures under `fixtures/lavish/`; render/replay tests run fully offline against recorded Parsoid responses; live-publish behavior beyond the userspace smoke test is validated only by AC.9's manual session.

## Review Strategy

Plan-mode: `plan-reviewer` subagent before handoff (this pass). Implementation: after all automated tests pass, dispatch a `general-purpose` subagent to review the completed work against this plan and the PLAYBOOK; fix or rebut all critical findings; re-review until clean or operator decides. Operator is the final reviewer of the live session.

## Documentation Strategy

`README.md` (setup: rust toolchain, Node ≥22, pinned lavish, OAuth2 owner-only consumer runbook with exact meta.wikimedia.org steps including descriptive consumer naming, env vars, UA policy (product-internalized; see docs/api-etiquette.md), Parsoid fixture re-record procedure), `docs/api-etiquette.md` (the codified Wikimedia API etiquette the client enforces), `PLAYBOOK.md` (the loop spec — the durable process artifact this project exists to produce), `docs/design-plans/2026-09-24-mvp1-structured-loop.md` (this design incl. the loop ladder and enforcement tiers), repo `CLAUDE.md` (conventions: load `wikimedia-api` skill before Wikimedia-calling code; UA string; PLAYBOOK pointer). MVP-2 migration notes live at the end of the design doc (what ports: the whole `wikiloop` binary; what's new: axum server UI, z.ai driver in place of the Claude-Code session).

## Risks, Blockers, and Required Decisions

**Risks**
- Anchor mis-mapping is silent (comment resolves to wrong span). Mitigation: unique ids everywhere; the agent quotes the resolved wikitext span back in `--agent-reply` so mis-maps are visible; golden tests.
- lavish pre-1.0 contract churn → pin exact version; TOON verified once, fixture-pinned.
- Layout-checker noise on dense diff pages (no kill switch) → scroller-only CSS, no clipping, `LAVISH_AXI_DIAGNOSTIC_VIEWPORTS=desktop`; findings never reach the agent.
- `mwapi` OAuth2 owner-only flow is documented but unproven in this family (SP42's Phase 4 live integration is still pending) → Phase 3 userspace smoke test isolates this early; fallback documented: BotPasswords auth for the smoke test only, never for mainspace.
- **WP:AIARTICLE (March 2026 guideline) prohibits LLM generation/rewriting of article content** outside two narrow exemptions; the tool's prose-drafting loops sit against its letter even though they mechanize its stated rationale (core-policy compliance via quote-anchoring; feasible review via one human-gated edit at a time; no NOTLAB problem). Mitigation: staged posture (see Required decisions) — offline replay first, always-on disclosure with the userspace disclosure page, userspace/low-traffic demonstration, operator-led community conversation. The guideline's own evaluation note ("consider the full pattern of the editor's recent edits and whether the edits comply with core content policies") is the spirit-argument the disclosure page makes concrete and auditable.
- Edit conflicts on the live article → base-revid pinning + abort (AC.7/AC.10); low-traffic article choice.
- Rust/`parsoid` version drift → pin parsoid 0.10.x (the version proven in SP42); record any bump in the design doc.
- Replay contamination (finished articles may be in training data) → grade findings, never prose.
- `~/.lavish-axi/state.json` grows unbounded → acceptable for MVP; note MVP-2 migration.

**Required operator steps**
- Create the OAuth2 owner-only consumer (Phase 3) and supply credentials via env.
- Pick the live-session article (Phase 7; low-traffic, non-BLP, non-contentious — operator's final call).
- Grade the replay soft-findings report (Phase 6).
- Capture and upload disclosure screenshots to Commons (manual in MVP-1; end of each live session, licensing per Commons practice for depicted CC BY-SA text).

**Required decisions (flagged, defaults chosen)**
- **Language: Rust** (over Go). Rationale: SP42 pattern library + mwbot-rs ecosystem (`mwapi`, `parsoid`), `similar` diffing, SP42-conventions continuity; static musl builds satisfy the portability motivation. Go would forfeit both the MediaWiki crate ecosystem and the SP42 reference. *(Operator raised "Rust or possibly Go"; Rust confirmed by operator, 2026-09-24.)*
- **License: GPL** (operator confirmed, 2026-09-24) — `mwapi`/`parsoid` are GPL-3.0-or-later; copying SP42 code (GPL-3.0-only) makes wikiactive effectively GPL-3.0-only, and LICENSE is set accordingly at first copy. Only avoidable by hand-rolling both the API client and the quote locator, which is not worth it.
- **AI-assistance disclosure (operator decided)**: automatic `LLM-Disclosure: U:LuisVilla/wikiactive` suffix on every edit summary + tool-maintained userspace disclosure page (methodology, per-session logs with model name/version and diff links, discussion section). Pointer-format per operator preference over per-summary model names; satisfies WP:LLMDISCLOSE's substance on a page instead of cramming it into summaries.
- **Compliance posture re WP:AIARTICLE** (March 2026 RfC guideline: LLM generation/rewriting of article content prohibited outside two narrow exemptions — copyedit corrections and translation): default = **staged** — build + offline replay now; disclosure always-on; the single live session runs in userspace or on one low-traffic article as the transparent demonstration; operator opens the community conversation (AI noticeboard / Village pump) before broader mainspace use. Account-level risk explicitly accepted by the operator. Confirm at handoff.

**Deferred**
- L5 Wikidata writeback (ledger schema keeps claim↔quote hooks; `mwapi` speaks to Wikidata when needed).
- MVP-2: local web app + z.ai driver; portability constraint honored by keeping all core logic framework-free with an axum growth path.
