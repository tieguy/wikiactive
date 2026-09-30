# Wikimedia API etiquette — codified (AC.13)

This document codifies the Wikimedia API etiquette that `wikiloop` enforces
**in code**. The tool has no runtime dependency on any external skill file:
the rules below are compiled into the client (`src/wikipedia.rs`,
`src/ledger/net.rs`, `src/lib.rs`) and asserted by `tests/integrations.rs`.
If you change behavior, change this document in the same commit.

Canonical sources: [meta:User-Agent policy], [mw:API:Etiquette],
[mw:Manual:Maxlag], [mw:API:Assert]. Fetch conventions for archive.org and
Toolforge follow the same identifying-UA discipline.

## User-Agent (identifying, hardcoded)

Every outbound HTTP request — Action API, REST/Parsoid, source fetch,
archive.org save-page-now, Earwig — sends:

```
wikiactive/0.1 (en.wikipedia User:LuisVilla; luis@lu.is)
```

- Defined once in `src/lib.rs::USER_AGENT`, mirrored in
  `rules/house-rules.toml` `[user_agent]`. A fork MUST edit this constant —
  inheriting someone else's contact address is a policy violation, which is
  why it is not configurable via environment.
- Set on the `mwapi` builder (`.set_user_agent`) and on every `reqwest`
  client the tool constructs. Asserted by
  `tests/integrations.rs::mwapi_client_sends_ua_and_maxlag` and the
  Earwig/SPN contract tests (mock servers match on the exact UA header).

## Maxlag

- `maxlag=5` on all Action API requests (via `.set_maxlag(5)` on the mwapi
  builder — applied client-wide, visible in every request URL).
- On a `maxlag` error response the mwapi client's retry logic backs off; the
  edit path treats it as a retryable failure, never a silent skip.

## Rate limits and backoff (429/503, Retry-After)

- The plain-HTTP layer (`SourceFetcher`, `SavePageNow`, `EarwigClient`)
  honors `Retry-After` on 429/503: sleep for the header's seconds (capped at
  60s), retry up to 3 attempts, then surface a status error. Default backoff
  is 2s × attempt when the header is absent or unparseable.
- Asserted by `tests/integrations.rs::source_fetch_backs_off_on_503_then_succeeds`
  (exactly 3 attempts) and `spn_rate_limit_is_an_error`.
- Concurrency is 1 (`.set_concurrency(1)`); no parallel request fan-out
  against Wikimedia hosts.

## assert=user and authentication

- `assert=user` is set client-wide (`.set_assert(Assert::User)`) **and**
  passed explicitly on every `action=edit` POST: an edit that is not
  authenticated is an error, not a fallback to anonymous editing.
- Asserted by `tests/publish.rs::edit_carries_summary_suffix_and_assert_user`
  (the mock server matches the body containing `assert=user`).
- Authentication is OAuth2 **owner-only** (token issued at consumer
  registration, supplied via `WIKIACTIVE_OAUTH2_TOKEN`); no authorization
  exchange is performed by this tool.

## Edit-path safety (see also PLAYBOOK.md)

- `action=edit` always posts with `baserevid` pinned to the session's base;
  the client pre-checks currency and aborts on conflict — a moved base never
  writes (`tests/publish.rs::test_stale_base_revid_aborts`).
- Creating a page (no base revision) posts with `createonly`, so a page
  that appeared in the meantime is never overwritten.
- Every edit posts `notminor`: `minor` is a presence flag, and no edit this
  tool makes is minor.
- Every edit summary carries the disclosure suffix
  (`LLM-Disclosure: U:LuisVilla/wikiactive`); a bare summary is refused.
- No publish happens without an interactive `/dev/tty` confirmation
  (`ConfirmSource`; `DenyConfirm` proves refusal in tests).
- After a successful save, the revision is read back once (flags, parent,
  comment, user, text) and compared with what the edit path sent: a
  mismatch — a minor flag above all, since `minor` is a presence flag —
  is shown to the operator under `VERIFY` and recorded in the round log,
  never silently absorbed. The tool verifies outcomes, not only inputs
  (`tests/readback.rs`).

The edit parameters come from one typed shape
(`EditParams`/`Base` in `src/wikipedia.rs`; its `to_pairs` is the only
place that names these keys, and the exact key set is pinned by a unit
test — rule-enforcement item 2):

| Parameter | Why it is sent |
|---|---|
| [`action=edit`](https://www.mediawiki.org/wiki/API:Edit) | the write module |
| `title`, `text`, `summary` | the edit itself; the summary is validated (non-bare) and carries the disclosure suffix |
| [`assert=user`](https://www.mediawiki.org/wiki/API:Assert) | fail the request if not authenticated — on the edit itself, not only client-wide (AC.7) |
| [`notminor=1`](https://www.mediawiki.org/wiki/API:Edit#Parameters) | `minor` is a presence flag (any value, `"0"` included, marks the edit minor); `notminor` is the explicit opposite and also overrides a mark-all-minor account preference. There is deliberately no `minor` field in the typed shape |
| [`baserevid` + `nocreate`](https://www.mediawiki.org/wiki/API:Edit#Parameters) | existing-page pin (client pre-checks currency; a moved base aborts without writing) and never-create on an update |
| [`createonly`](https://www.mediawiki.org/wiki/API:Edit#Parameters) | create pin: a page that appeared since the session saw it missing is never overwritten |

## Source fetching (non-Wikimedia hosts)

- SSRF guard: only `http`/`https`, ports 80/443, no loopback/private/
  link-local/metadata hosts. Redirects capped at 5; response bodies capped
  at 2 MiB.
- Identifying UA (same string) on archive.org and Toolforge requests;
  archive.org save-page-now denials (429/403) are surfaced as errors and the
  ledger records **no** archive URL rather than a fabricated one.

## What is deliberately NOT done

- No scraping around the Action API (no HTML scraping of article pages for
  content; Parsoid REST endpoints are the supported transform surface).
- No unattended operation: the tty confirmation gate means no scheduled
  publish runs, ever.
- No Commons upload (OAuth grants exclude upload; screenshots are manual in
  MVP-1).

[meta:User-Agent policy]: https://meta.wikimedia.org/wiki/User-Agent_policy
[mw:API:Etiquette]: https://www.mediawiki.org/wiki/API:Etiquette
[mw:Manual:Maxlag]: https://www.mediawiki.org/wiki/Manual:Maxlag
[mw:API:Assert]: https://www.mediawiki.org/wiki/API:Assert
