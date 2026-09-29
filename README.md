# wikiactive — structured Wikipedia improvement loop (MVP-1)

A Claude-Code-driven editing loop for en.wikipedia articles: one **logical
edit** at a time, every content change grounded in a verbatim quote from a
session source ledger ("never edit from model memory"), reviewed by the
operator as a **visual diff in lavish-axi**, and **published per-edit via
OAuth only on explicit human confirmation**.

The loop spec lives in **[PLAYBOOK.md](PLAYBOOK.md)** — the durable process
artifact. Design and rationale:
[docs/design-plans/2026-09-24-mvp1-structured-loop.md](docs/design-plans/2026-09-24-mvp1-structured-loop.md).

## Setup

### Rust toolchain

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.96.0
```

Edition 2024, rust-version 1.96, `warnings=deny` + clippy pedantic.

### Node ≥ 22 + pinned lavish

The review surface runs [lavish-axi], pinned:

```
npx -y lavish-axi@0.1.78 --help
```

Node is required *only* for lavish. The tarball is vendored in-repo
(`vendor/lavish-axi-0.1.78.tgz`) as the normative reference for its
annotation payload and TOON output shapes; captured real output lives under
`fixtures/lavish/`.

### OAuth2 owner-only consumer (publish path)

1. Log in at meta.wikimedia.org and open
   [Special:OAuthConsumerRegistration/propose].
2. Propose a consumer:
   - **Application name**: `wikiactive — disclosed AI-assisted editing
     loop` (descriptive on purpose: it appears as the automatic tool tag
     on edits).
   - **Consumer version**: 0.1.
   - **Wiki**: en.wikipedia.org only.
   - **Grants**: *Basic rights* + *Edit existing pages*. **No upload.**
   - **Owner-only consumer**: check it (the "OAuth 2.0" token is issued
     immediately at registration — no authorize/token exchange is ever
     performed by this tool).
3. Copy the issued **Access token** (the `jwt` / OAuth2 token, not the
   1.0a key pair).
4. Export it (never commit):

```
export WIKIACTIVE_OAUTH2_TOKEN="..."
```

**Smoke-test fallback (userspace only):** a [Bot Password] works for the
userspace smoke publish without OAuth — create one at
en.wikipedia.org [[Special:BotPasswords]] (grant: edit existing pages) and:

```
export WIKIACTIVE_BOTPASSWORD="LuisVilla@wikiloop:password"
```

Plan policy: BotPasswords is for the smoke test ONLY — mainspace requires
the OAuth consumer (it tags edits with the consumer name and is revocable
without touching your password).

Read actions work unauthenticated; publish requires a credential.

### Etiquette (product-internalized)

The client hardcodes an identifying User-Agent
(`wikiactive/0.1 (en.wikipedia User:LuisVilla; luis@lu.is)`), `maxlag=5`,
`assert=user`, and Retry-After backoff. See
[docs/api-etiquette.md](docs/api-etiquette.md). **A fork edits
`src/lib.rs::USER_AGENT` and `rules/house-rules.toml` once** — carrying
someone else's contact address is a bug.

### Smoke test (first publish)

```
wa session init --article "User:LuisVilla/wikiactive/smoke" --entry-loop 1
# (make proposed.wikitext a one-line page)
wa render <slug> --round 1 --summary "smoke test" --no-open
wa publish <slug> --summary "wikiactive smoke test"
# then blank the page the same way
```

## Usage

See [PLAYBOOK.md](PLAYBOOK.md) for the full session protocol. Fast path:

```
wa session init --article "Temple Fielding" --entry-loop 2
wa analyze temple-fielding            # step 0: ALWAYS read this first
wa ledger register <slug> --url https://… --title "…"
wa ledger fetch <slug> --source S1
wa ledger archive <slug> --source S1
wa ledger quote <slug> --source S1 --text "verbatim words…"
wa findings add <slug> - <<'JSON'
[{"id":"F1","wikitext_anchor":"L3:C0-L3:C120","rules":["WP:V"],
  "evidence":["Q1"],"factual_note":"…","proposed_fix":"…","loop":2}]
JSON
# edit sessions/<slug>/proposed.wikitext — ONE logical edit
wa render <slug> --round 1 --summary "Fix ref-name formats"
wa poll <slug>                          # waits for operator comments
wa publish <slug> --summary "Format citations, fix auto ref names"
```

## Parsoid fixture re-record procedure

Render/replay tests run fully offline against recorded Parsoid HTML
(`fixtures/parsoid/`). When Parsoid HTML drifts ( MediaWiki bumps
`mw:htmlVersion`), re-record:

```
UA='wikiactive/0.1 (en.wikipedia User:LuisVilla; luis@lu.is)'
curl -sS "https://en.wikipedia.org/api/rest_v1/page/html/Commitment_device/1343452323" \
     -A "$UA" -o fixtures/parsoid/commitment-device@1343452323.html
```

Wikitext fixtures are fetched by `revid` (Action API,
`prop=revisions&revids=…`); HTML by the REST `page/html/<title>/<revid>`
endpoint. Update both together; the tests assert consistency.

## Tests

```
cargo test          # 150+ tests, fully offline (httpmock for HTTP contracts)
cargo clippy --all-targets
```

Acceptance-criteria mapping: `tests/` files are named per AC
(`quote_anchor`=AC.2, `paraphrase`=AC.3, `linter`=AC.4, `render`=AC.5,
`anchors`=AC.6, `publish`=AC.7/10, `replay`=AC.8, `gate`=AC.11,
`integrations`=AC.12/13, `rules_corpus`=AC.1, `findings_cli`=field
ownership).

## License

GPL-3.0-only. The quote-anchor locator is copied with provenance from
SP42 (GPL-3.0-only), which sets this project's effective license; see the
design plan's license note.

## Status / next steps

**MVP-2 Phase A complete (`v0.5.0`, 2026-09-29).** The L2 content loop is
proven live: two article sessions under full operator review — Sarah
Kidder (three published edits: marriage-year correction round-trip
1874→1870→1874 resolved by operator-captured sources, lead citation,
Ohio-birthplace `{{cn}}` resolved, discrepancy footnote) and Temple
Fielding (two published edits) — drove the review surface to
reviewer-grade (deletion anchors, enwiki link affordances, single-column
diff with evidence rail, consulted-source manifest, clickable
citations), hardened the gate (`wa check` fail-fast, drift pin,
config-tuned paraphrase thresholds, attributed-quote deferral,
`wa ledger attach` for operator captures of unfetchable sources), and
kept the disclosure log current per session. Phase B (in progress,
targeting 0.9.0) ports the loop to `wa serve` — an axum, loopback-only
local console with a direct z.ai (GLM) driver on the three judgment
points (findings, proposals, comment resolution), restructured around a
**source sweep** (fetch-or-dispose every cited source before textual
analysis; the ledger stays text-first). Design record:
[docs/design-plans/2026-09-25-mvp2-addendum.md](docs/design-plans/2026-09-25-mvp2-addendum.md).

[lavish-axi]: https://www.npmjs.com/package/lavish-axi
[Special:OAuthConsumerRegistration/propose]: https://meta.wikimedia.org/wiki/Special:OAuthConsumerRegistration/propose
