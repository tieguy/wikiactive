# Phase 3 — Assess entry checks

Plan: `docs/design-plans/2026-09-30-loop-mechanization.md` (Phase 3).
State re-verified 2026-09-30 after Phase 2 (`ad8e5af`).

**Goal:** both assess entry paths (CLI `wa assess add`, serve Assess save)
refuse unknown-quote, stale-analyze, and unresolved-fetch batches;
bypasses exist for the latter two (CLI flags) and are recorded.

**ACs:** `loopmech.AC1.1`–`AC1.3`, `AC2.1`–`AC2.5`, `AC3.1`–`AC3.5`.

## Verified state

- `src/session.rs` — `RoundEntry` (phases free-form strings; `artifact_state`
  reacts only to `rendered`/`published`/`comments-resolved`), session
  paths incl. `rounds()`.
- `src/ledger.rs` — `quote(id) -> Option<&Quote>` (L579),
  `sweep_unresolved() -> Vec<(&SourceEntry, &str)>` (L269),
  `has_sweep_state()` (L284).
- `src/cli.rs` — `assess_add` (batch-atomic validate-then-save),
  `AssessCmd::Add` flags.
- `src/serve.rs` — `run_driver_assess` saves the step output directly
  (L~1900): the path Phase 3 hardens; its outcome strings reach the
  operator via `note_outcome` (never silent).
- `now_iso()` = RFC3339 seconds (`src/comments.rs` L217); chrono is a
  dep for parsing entry timestamps.
- `analyze` writes `context.md` (`src/cli.rs` `analyze`).

## Decisions

- **One check function** in `src/session.rs` (session-domain, beside
  `RoundEntry`):
  `pub fn assess_entry_checks(dir, ledger, evidence: &[String], opts: &EntryChecks) -> Result<(), Vec<EntryRefusal>>`
  with `EntryChecks { allow_stale_analyze, allow_unresolved_fetch }` and
  `EntryRefusal { UnknownQuote{ids}, StaleAnalyze{older_than},
  UnresolvedFetch{sources: Vec<(String,String)>} }` + Display. Both
  surfaces call it; a test pins that the same fixture batch is refused
  by both.
- **Freshness:** `context.md` must be NEWER than `proposed.wikitext`'s
  mtime AND newer than the newest `rendered`|`published`|
  `comments-resolved` entry timestamp (parsed RFC3339; missing
  `context.md` is stale "missing — run `wa analyze`"; missing
  `proposed.wikitext` skips that comparison; no advancing entries ⇒ only
  the proposed comparison — AC2.4). Equality counts as stale (the safe
  direction).
- **UnknownQuote has NO bypass.** Batch-atomic: any refusal ⇒ nothing
  saved (the existing shape).
- **Bypasses:** `--allow-stale-analyze`, `--allow-unresolved-fetch` on
  `wa assess add`. A bypass that was actually needed (its guard fired)
  appends a round-log entry `phase: "assess-bypass"` with a detail line
  per guard. Serve has no bypass flags (refuses with the same message).
- **Messages:** every refusal names the offending ids and prescribes the
  fix (`wa analyze <slug>`; fetch/attach/dispose for unresolved). The
  serve variant appends the on-page affordance pointer ("the Sources
  table below: attach or disposition") for `UnresolvedFetch`.

## Tasks

1. `EntryRefusal`/`EntryChecks`/`assess_entry_checks` in `src/session.rs`
   with unit tests (freshness predicate against temp dirs, quote
   existence, sweep gating incl. AC3.5 no-sweep-state). Failing tests
   first. `Verifies: AC1.1, AC2.1, AC2.4, AC3.1, AC3.5 (logic)`
2. CLI wiring: flags, print-all-refusals error, nothing saved, bypass
   round-log entries. Tests in `tests/assess_cli.rs` (fixtures write
   `context.md`/`proposed.wikitext`/`rounds.jsonl` with ordered mtimes):
   unknown ids named + nothing saved (AC1.1); happy path unchanged
   (AC1.3); stale vs proposed / vs published entry (AC2.1); re-analyze
   then identical batch accepted (AC2.2); first-iteration case (AC2.4);
   bypass flag proceeds + recorded (AC2.5, AC3.2); unresolved listed
   (AC3.1); resolved-then-clean proceed (AC3.3).
   `Verifies: AC1.1, AC1.3, AC2.1, AC2.2, AC2.4, AC2.5, AC3.1–AC3.3`
3. Serve wiring: `run_driver_assess` runs the checks before saving,
   refuses with the same message (+ affordance pointer). Tests:
   serve driver assess refuses an unknown-quote batch visibly and saves
   nothing (AC1.2); refuses stale with the same wording as the CLI
   (AC2.3); unresolved message names the affordances (AC3.4); existing
   driver tests' fixtures gain a fresh `context.md` + valid quotes.
   `Verifies: AC1.2, AC2.3, AC3.4`
4. Phase gate: fmt/clippy/test fresh, sonar before commits.
