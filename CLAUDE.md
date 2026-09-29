# wikiactive / wikiloop — repo conventions

## What this is

`wikiloop` (binary `wa`) drives a structured Wikipedia improvement loop:
one logical edit at a time, every content change quote-anchored to a
fetched source, human review via the in-app console (`wa serve`:
block-anchored comments on the session page), publish only on
interactive confirmation. Read `PLAYBOOK.md` before doing any session work;
read `docs/design-plans/2026-09-24-mvp1-structured-loop.md` for the design.

## Working rules

- **Load the `wikimedia-api` skill before writing code that calls a
  Wikimedia host.** The User-Agent must identify the operator — it is
  hardcoded once in `src/lib.rs::USER_AGENT` and asserted by tests; a fork
  edits that constant (and `rules/house-rules.toml`), nowhere else.
- Etiquette is product-internalized (AC.13): UA, `maxlag=5`, `assert=user`,
  Retry-After backoff — codified in `docs/api-etiquette.md`, enforced in
  `src/wikipedia.rs` / `src/ledger/net.rs`, pinned by
  `tests/integrations.rs`. No runtime dependency on any external skill
  file.
- **Never edit from model memory**: every content change goes through the
  ledger (register → fetch → quote → claim) and the gate (`src/checks/
  gate.rs`). The quote-anchor locator is ported from SP42 with provenance
  headers — keep them.
- Toolchain: edition 2024, rust-version 1.96, `warnings=deny`,
  clippy pedantic denied. `cargo fmt` before every commit.
- License: GPL-3.0-only (SP42 copy-make it so; see LICENSE and the design
  plan's license note).
- lavish-axi is pinned at 0.1.78 (`vendor/lavish-axi-0.1.78.tgz` is the
  normative reference for payload/TOON shapes; captured fixtures under
  `fixtures/lavish/`).
- `sessions/` is gitignored (local state); `fixtures/` is committed.
- Re-record Parsoid fixtures with the REST endpoint documented in README
  when Parsoid HTML drifts.

## Commands

```
cargo test                 # everything (offline)
cargo clippy --all-targets # must be clean
wa session init --article "X" --entry-loop 2
wa analyze <slug>
wa findings add <slug> -
wa ledger register|fetch|archive|attach|quote|claim ...
wa sweep inventory|fetch|status <slug>   # fetch-or-dispose before analysis
wa sweep dispose <slug> --source S3 --disposition "..."
wa render <slug> --round 1 --summary "..."
wa check <slug>            # standalone gate preflight (no artifact)
wa comments list|add|resolve <slug> ...   # the review comment queue
wa publish <slug> --summary "..."
wa serve [--tsnet] [--port N]  # local web console (loopback default)
wa lint <wikitext-file>
```
