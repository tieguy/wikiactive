---
description: Load before writing any code that calls a Wikimedia host (wikipedia.org, wikidata.org, commons, meta, any *.wikimedia.org or sister project) or fetching their content. Covers User-Agent policy, the wm-fetch tool, identities, and where etiquette lives in this repo.
---

# Wikimedia API etiquette (wikiactive)

Any code or fetch touching a Wikimedia host follows this. A forked repo carries the
upstream author's contact info by default — that is a bug every time; these values
are the operator's.

## Identities

| | |
|---|---|
| On-wiki username | `LuisVilla` |
| Toolforge shell user | `luisvilla-personal` (differs from the local Unix username) |
| Toolforge SSH | `ssh luisvilla-personal@login.toolforge.org`, then `become <toolname>` |
| SSH key | `~/.ssh/id_ed25519` (comment `luis@lu.is`), registered at idm.wikimedia.org |
| GitHub | `tieguy` |
| UA contact | `luis@lu.is` |

## Fetching: wm-fetch, never web_fetch/curl

- The `web_fetch` tool is **hard-denied for Wikimedia domains** by this project's
  `wmfetch-only-for-wikimedia` hook — it cannot set a User-Agent.
- Use `wm-fetch <url>` from a shell (`~/.local/bin/wm-fetch`, canonical checkout
  `~/Projects/wiki/wm-fetch`). Since 2.0 it is a Rust binary with WMF policy
  enforced by construction: UA contact gate, maxlag including the HTTP-200 error
  form, pacing/serialization, scoped robots.txt. It refuses to run unconfigured;
  `wm-fetch --init` sets contact info once.
- Never hand-roll `curl`/`reqwest` calls to WM hosts in new code — route through
  the existing clients.

## Where etiquette lives in this repo

Etiquette is product-internalized (AC.13), not a runtime skill dependency:
`USER_AGENT` is defined **once** in `src/lib.rs` and asserted by tests; maxlag=5,
`assert=user`, Retry-After backoff live in `src/wikipedia.rs` / `src/ledger/net.rs`
per `rules/house-rules.toml [etiquette]` and `docs/api-etiquette.md`, pinned by
`tests/integrations.rs`. A fork edits `USER_AGENT` (and house-rules) — nowhere else.

When planning or reviewing changes that touch these paths, treat any new
direct-to-WMF network call without the shared client as a Critical finding.
