# Phase 5 — Edit-summary rule resolution

Plan Phase 5. State re-verified 2026-10-01 (`02f8b1b`):

- `rules/canonical/*.wikitext` (13 snapshots) carry `{{shortcut|A|B}}`
  templates (any case, multi-alias) and `shortcut=`/`shortcut1=…`
  infobox params; e.g. `mos.wikitext` → `MOS:VAR`, `citing-sources` →
  `WP:CITESHORT`.
- `publish_core` (CLI + serve chokepoint) re-runs the gate, then hands
  the summary to the Wikipedia client; `WikipediaError::BareSummary` is
  the empty-summary refusal precedent.
- `audit_flow` takes `opts.summary` (default "proposed edit").

**ACs:** `loopmech.AC5.1`–`AC5.5`.

## Decisions

- New module `src/checks/summary_rules.rs` (registered in
  `src/checks/mod.rs`): `ShortcutIndex` (built offline from
  `rules/canonical/*.wikitext`; token → canonical page; keys
  case-normalized/uppercased) and
  `summary_rule_check(summary, index) -> Vec<String>` — one line per
  shortcut-shaped token (`WP:`, `MOS:`, `H:`, `WT:`, `CAT:` prefixes,
  case-normalized, word-bounded) the index lacks. `regex` is already a
  dependency. No network at build or run (AC5.5).
- Parsing: `{{shortcut|…}}`-family templates (split aliases on `|`,
  skip params containing `=`), plus `shortcut…=<TOKEN>` params. The
  canonical page is the snapshot's file stem.
- **Chokepoints:** `audit_flow` runs the check on `opts.summary` before
  rendering (refusal ⇒ no artifact, tokens named — AC5.2's audit half);
  `publish_core` runs it before the gate/wiki work (refusal ⇒ no edit
  request — AC5.2's publish half). Token-free summaries are unaffected
  (the default "proposed edit" passes); disclosure-suffix append and
  idempotence untouched (suffix contains no shortcut tokens; the check
  reads the operator summary pre-suffix).
- House rules (AC5.4): no canonical shortcuts exist for house rules, so
  a made-up shortcut on a house-rule edit is simply unresolvable and
  refused; describing the same edit plainly passes — pinned by test
  wording. Whether a RESOLVABLE shortcut covers the edit stays judgment
  (ADVISORY, Phase 7's inventory row).

## Tasks

1. `src/checks/summary_rules.rs` + unit tests (offline build, known
   tokens resolve case-insensitively, alias fan-out, unresolvable named,
   token-free clean, prefix variants). Failing tests first.
   `Verifies: AC5.1, AC5.5`
2. `audit_flow` wiring + `tests/audit_cli.rs`: unresolvable token blocks
   with NO artifact + names the token; resolvable token passes; default
   summary unaffected. `Verifies: AC5.2 (audit half), AC5.3, AC5.4`
3. `publish_core` wiring + `tests/publish.rs`: unresolvable token sends
   NO edit request (mock call count 0) and names the token; token-free
   publish flows unchanged (existing suite). `Verifies: AC5.2 (publish
   half), AC5.3`
4. Phase gate: fmt/clippy/test fresh, sonar, commit.
