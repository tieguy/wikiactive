# Phase 2 — Audit shape: render-on-pass, `--llm` consolidation, diagnosis vocabulary

Plan: `docs/design-plans/2026-09-30-loop-mechanization.md` (Phase 2).
Codebase state re-verified 2026-09-30 after Phase 1 (commit `45889ad`).

**Goal:** `wa audit` becomes the single draft-side examination: deterministic
gate, artifact on green (render-on-pass), LLM diagnosis pass by default
(fork config). The lavish legacy pair `wa render` / `wa poll` retires with
pointers.

**ACs covered:** `loopmech.AC7.1`–`AC7.3`, `AC8.1`–`AC8.4`.

## Verified current state

- `src/cli.rs`: `audit_cmd` (L1681) = gate preflight only;
  `rule_review_cmd` (L1432) = the `--llm` arm (requires a CURRENT
  artifact, appends `rule-reviewed` round entry, stores
  `rule-review.json`); `render_cmd` (L883, pub) = gate-inside-render
  pipeline with `Via::Tty` lavish open + `Via::Web` arms;
  `poll_cmd` (L1037); `Command::Render`/`Command::Poll` still live.
- `src/serve.rs`: `next_round` (L870) — Current/TextChanged ⇒ same round
  (replace-on-rerender), Published/CommentsApplied ⇒ +1; the audit POST
  handler calls `crate::cli::render_cmd(..., Via::Web)` then optionally
  `run_rule_review` when `llm=on` (Phase 1 wiring);
  `run_rule_review` (L~1680) duplicates `rule_review_cmd` with the
  state-injected zai client (`WIKIACTIVE_SERVE_TEST_ZAI` override).
- `src/rules.rs`: `HouseRules` (L19) — add an `[audit]` section
  (`#[serde(default)]`); `rules/house-rules.toml` has the `[zai]`
  precedent.
- `prompts/review.md` — says "review … advice"; checksum-pinned.
- Callers to migrate: `tests/driver_e2e.rs` L303 (`render_cmd` +
  `Via::Tty` ×3 → `audit_flow` with `LlmChoice::Off`), serve audit
  handler; `tests/readback.rs` uses `Via` for `publish_core` only (keep).
- Round-log phases: `rendered`, `rule-reviewed` (inert to
  `artifact_state` — pinned by serve test).
- Env hazard: `ZAI_API_KEY` IS present in the daemon environment — any
  CLI test that runs a default-on audit subprocess MUST `env_remove`
  the key (no live calls from tests).

## Decisions (locked)

- **One audit flow, both surfaces.** `pub async fn audit_flow(slug,
  opts: AuditOpts, zai: Option<&ZaiClient>) -> Result<()>` in `cli.rs`:
  1. gate preflight (blocked ⇒ print report, return `Err` carrying it,
     NO artifact — AC7.2);
  2. round := serve's `next_round(artifact_state)` logic (make
     `next_round` `pub(crate)` and reuse) — AC7.3;
  3. render pipeline (extracted from `render_cmd`, lavish/`Via` arms
     dropped): offline/live Parsoid, `registry_with_round`
     replace-on-rerender, write artifact, persist back-filled
     `rendered_span_id`s, append `rendered` entry;
  4. LLM diagnosis when `LlmChoice::On`: `diagnosis_pass` (refactor of
     `rule_review_cmd`; the client comes from `zai` or, when `None`,
     `ZaiClient::from_env`) — client-construction or transport failure
     ⇒ the artifact STANDS, the skip/failure is printed and recorded as
     a round-log entry (phase `rule-review-failed`, summary carries the
     reason) — AC8.4. Success stores `rule-review.json`, prints
     concerns, appends `rule-reviewed` — AC8.2/8.3 (never blocks).
- **Flag surface:** `wa audit <slug> [--llm|--no-llm] [--summary S]
  [--html-base P --html-propose P→--html-proposed P]`. `--llm`/
  `--no-llm` conflict; neither ⇒ config. `--summary` default
  `"proposed edit"` (render's old default; Phase 5 adds the rule check
  on it). The Phase-1 `--llm`-without-gate semantics disappear — audit
  always runs the deterministic gate first.
- **Config:** `rules/house-rules.toml` gains `[audit] llm_pass = true`;
  `HouseRules.audit: AuditRules { llm_pass: bool }` (`default_true`).
  Flipping the TOML flips the CLI default (AC8.1) and the serve
  checkbox's default state.
- **Serve:** the audit POST calls `audit_flow` with
  `Some(state.zai_client())` when constructible (else `None` → the
  reported-skip path); `run_rule_review` is deleted (its logic is
  `diagnosis_pass`); the checkbox renders `checked` when config
  `llm_pass` is true. The round/summary form fields stay (operator
  input, not a render step).
- **Retirements:** hidden `Command::Render`/`Command::Poll` rest-args
  variants bail with pointers to `wa audit` / in-app review;
  `render_cmd`, `poll_cmd`, the `Via::Tty` render arms, and
  `print_lavish_output` are deleted. `Via` survives for
  `publish_core` only (doc updated). The `lavish` module itself stays
  (pub, pinned fixtures; only its CLI entry points retire).
- **Prompt:** `prompts/review.md` wording moves advice → diagnosis
  ("You diagnose a drafted Wikipedia edit…"; "a diagnosis, never a
  decision: you never approve, block, or rewrite — the deterministic
  gate and the human reviewer decide"); checksum re-pinned with reason.

## Tasks

### Task 1 — `[audit]` fork config
`rules/house-rules.toml` `[audit] llm_pass = true`; `AuditRules` struct
(+ `default_true`) in `src/rules.rs`; unit test: absent section ⇒ true;
explicit false ⇒ false. `Verifies: loopmech.AC8.1` (config half)

### Task 2 — `audit_flow` (gate → render-on-pass → diagnosis rider)
Extract the render pipeline from `render_cmd`; implement `audit_flow`
+ `AuditOpts`/`LlmChoice` + `diagnosis_pass` per Decisions; wire
`Command::Audit` flags; `next_round` shared. Failing tests FIRST in
`tests/audit_cli.rs` (subprocess tests `env_remove("ZAI_API_KEY")`):
- green audit (offline fixtures) ⇒ `review.html` exists + `rendered`
  round entry; no separate command (`Verifies: loopmech.AC7.1`);
- blocked audit ⇒ report + no artifact (`Verifies: loopmech.AC7.2`);
- re-audit after a `comments-resolved` entry ⇒ next round number, and
  the revisions registry keeps earlier rounds + replaces on re-rerender
  of the same round (`Verifies: loopmech.AC7.3`);
- `--no-llm` ⇒ no `rule-review.json` (`Verifies: loopmech.AC8.1`);
- default (no flags, key removed) ⇒ artifact + skip reported in stdout
  AND a round-log entry names the failed pass (`AC8.4`);
- config flip: copy rules/, set `llm_pass = false` ⇒ default run makes
  no pass (`AC8.1`).
`Verifies: loopmech.AC7.1–AC7.3, AC8.1–AC8.4 (CLI half)`

### Task 3 — serve rides the same flow
Serve audit handler → `audit_flow`; delete `run_rule_review`; checkbox
default from config. Update `tests/serve.rs`: the toggle test still
passes (llm=on, mock zai); NEW: config-off fork ⇒ checkbox unchecked;
the audit POST without llm in a config-off fork makes no model call;
llm=on with an UNCONSTRUCTIBLE client (no env key, no override) still
renders + records the skip (`AC8.4` serve half).
`Verifies: loopmech.AC8.4 (serve half), AC7.1 (serve ends at review)`

### Task 4 — retire `wa render` / `wa poll`
Hidden variants + pointers; delete `render_cmd`/`poll_cmd`/lavish tty
arms; migrate `tests/driver_e2e.rs` to `audit_flow` (`LlmChoice::Off`).
Failing tests FIRST: `wa render …`/`wa poll …` fail with pointers to
`wa audit`. `Verifies: loopmech.AC7.1 (retirement half)`

### Task 5 — prompts/review.md diagnosis vocabulary; re-pin
Wording per Decisions; update the pin with reason in the commit
message. `Verifies: loopmech.AC8.2 (schema unchanged — validation stays)`

### Task 6 — docs touch-up + phase gate
CLAUDE.md/README.md: `wa render`/`wa poll` rows become `wa audit`
(keep the lavish pinned-fixture note accurate); smoke test uses
`wa audit`. Fresh `cargo fmt`/`clippy --all-targets`/`cargo test`,
output read; `sonar analyze secrets` before each commit.
`Verifies: loopmech.AC6.5 (surface), None (gate)`
