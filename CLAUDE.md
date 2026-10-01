# wikiactive / wikiloop — repo conventions

## What this is

`wikiloop` (binary `wa`) drives a structured Wikipedia improvement loop:
one logical edit at a time, every content change quote-anchored to a
fetched source, human review via the in-app console (`wa serve`:
block-anchored comments on the session page), publish only on
interactive confirmation. Article drafting runs inside `wa serve`: the model is
called at exactly the three judgment points with the versioned, checksum-pinned
prompts in `prompts/` — rules that govern drafted output live there and in the
gate, never in the coding harness's context. The coding harness (this session
type) maintains the tool, rules corpus, and prompts. `PLAYBOOK.md` is the
loop's higher-level spec (stages, guarantees, ownership — ADR-0002): specific
rules live only in `wa` or the pinned prompts; PLAYBOOK is paired with code
changes and audited, never runtime reading. Read
`docs/design-plans/2026-09-24-mvp1-structured-loop.md` for the design.

## Working rules

- The User-Agent is hardcoded once in `src/lib.rs::USER_AGENT` and
  asserted by tests; a fork edits that constant (and
  `rules/house-rules.toml`), nowhere else.
- Etiquette is product-internalized (AC.13): UA, `maxlag=5`, `assert=user`,
  Retry-After backoff — codified in `docs/api-etiquette.md`, enforced in
  `src/wikipedia.rs` / `src/ledger/net.rs`, pinned by
  `tests/integrations.rs`. No runtime dependency on any external skill
  file. Any new direct-to-WMF network call that bypasses the shared client
  is a Critical review finding.
- **Never edit from model memory**: every content change goes through the
  ledger (register → fetch → quote → claim) and the gate (`src/checks/
  gate.rs`). The quote-anchor locator is ported from SP42 with provenance
  headers — keep them.
- Toolchain: edition 2024, rust-version 1.96, `warnings=deny`,
  clippy pedantic denied. `cargo fmt` before every commit.
- SonarQube gate: `sonar analyze secrets` over changed files before
  committing; after push CI analyzes the branch — read `sonar list issues
  -p tieguy_wikiactive --new-code --format toon` on the next gate pass.
  BLOCKER/HIGH findings are must-fix before work counts as done.
- License: GPL-3.0-only (SP42 copy-make it so; see LICENSE and the design
  plan's license note).
- lavish-axi is pinned at 0.1.78 for the retired tty path (`wa render`/
  `wa poll` are gone — audit renders; the pin and fixtures stay as the
  recorded reference: `vendor/lavish-axi-0.1.78.tgz` is the
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
wa fetch <slug>             # fetch stage: inventory + fetch in one go
wa fetch dispose <slug> --source S3 --disposition "..."
wa fetch status <slug>
wa analyze <slug>
wa assess add <slug> -      # schema-validated assessment admission
wa assess list <slug>
wa ledger register|fetch|archive|attach|quote|claim ...
wa audit <slug> [--llm|--no-llm] [--summary "…"]
                             # gate; green ⇒ round artifact (render-on-pass);
                             # LLM diagnosis default per rules/house-rules.toml [audit]
wa comments list|add|resolve <slug> ...   # the review comment queue
wa publish <slug> --summary "..."
wa serve [--tsnet] [--port N]  # local web console (loopback default)
wa lint <wikitext-file>
```
