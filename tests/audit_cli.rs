//! loopmech.AC6.3 — `wa audit <slug>`: the deterministic gate run with
//! structured, disposition-grouped output. A gate-failing proposal exits
//! non-zero with the NEEDS ANCHOR / HARD BLOCK report and writes NO
//! artifact; a clean one passes without rendering.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
}

/// Temp working directory with an initialized session and the rules corpus
/// (the binary loads `rules/` relative to its working directory).
fn setup_session(findings: &str, base: &str, proposed: &str) -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("wa-audit-cli-{id}-{}", std::process::id()));
    let session = dir.join("sessions/test-article");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("session.json"),
        r#"{"article":"Test article","base_revid":1,"started":"2026-09-25T00:00:00Z","entry_loop":2}"#,
    )
    .unwrap();
    std::fs::write(session.join("assessments.json"), findings).unwrap();
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[],"quotes":[],"claims":[]}"#,
    )
    .unwrap();
    std::fs::write(session.join("base.wikitext"), base).unwrap();
    std::fs::write(session.join("proposed.wikitext"), proposed).unwrap();
    std::fs::write(dir.join("base-fixture.html"), html_of(base)).unwrap();
    std::fs::write(dir.join("proposed-fixture.html"), html_of(proposed)).unwrap();
    copy_dir(Path::new("rules"), &dir.join("rules"));
    dir
}

/// Offline Parsoid HTML fixture for one wikitext string (tests never hit
/// the live endpoint).
fn html_of(wt: &str) -> String {
    use std::fmt::Write as _;
    let mut body = String::new();
    for l in wt.lines() {
        let _ = write!(body, "<p>{l}</p>");
    }
    format!("<html><body>{body}</body></html>")
}

/// `wa audit --no-llm` with the offline fixtures (deterministic gate +
/// render, no model call). Tests never carry a live model key.
fn wa(dir: &Path) -> std::process::Output {
    wa_env(dir, &["--no-llm"])
}

/// Spawn `wa audit …` with the offline fixtures and NO model credentials
/// in the child environment (the default-on pass must degrade to a
/// reported skip, never a live call).
fn wa_env(dir: &Path, extra: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wa"));
    cmd.current_dir(dir)
        .env_remove("ZAI_API_KEY")
        .env_remove("ZAI_BASE_URL")
        .args(["audit"]);
    cmd.args(extra);
    cmd.args([
        "--html-base",
        "base-fixture.html",
        "--html-proposed",
        "proposed-fixture.html",
        "test-article",
    ]);
    cmd.output().unwrap()
}

#[test]
fn blocked_proposal_reports_disposition_groups_without_artifact() {
    // AS1 cites Q99 which is not in the ledger (NEEDS ANCHOR) and the
    // proposal introduces an unspaced heading (HARD BLOCK) — both groups
    // in one run.
    let dir = setup_session(
        r#"{"assessments":[{"id":"AS1","wikitext_anchor":"L1:C0-L1:C10","rules":["WP:V"],"evidence":["Q99"],"factual_note":"n","proposed_fix":"f","loop":2}]}"#,
        "He was born in 1919.\n",
        "==Life==\nHe was born in 1919.\n",
    );
    let out = wa(&dir);
    assert!(
        !out.status.success(),
        "blocked gate must exit non-zero; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("gate blocked"), "stdout: {stdout}");
    assert!(
        stdout.contains("NEEDS ANCHOR (1)"),
        "finding without evidence groups as anchor work: {stdout}"
    );
    assert!(
        stdout.contains("HARD BLOCK (1)"),
        "linter error groups as hard block: {stdout}"
    );
    assert!(
        stdout.contains("[at L1:C0-L1:C10]"),
        "finding span surfaced: {stdout}"
    );
    assert!(
        stdout.contains("no artifact written"),
        "a blocked audit writes nothing: {stdout}"
    );
    assert!(
        !dir.join("sessions/test-article/review.html").exists(),
        "no artifact written"
    );
}

/// loopmech.AC7.1 — a green audit produces the round artifact with no
/// separate render command: review.html + a `rendered` round entry.
#[test]
fn green_audit_renders_the_artifact_and_round_entry() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let out = wa(&dir);
    assert!(
        out.status.success(),
        "green audit succeeds; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("gate: PASS"), "stdout: {stdout}");
    let session = dir.join("sessions/test-article");
    assert!(session.join("review.html").exists(), "artifact written");
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(rounds.contains("\"phase\":\"rendered\""), "{rounds}");
    assert!(rounds.contains("\"round\":1"), "first round: {rounds}");
}

/// loopmech.AC7.3 — re-audit after comment resolution starts the NEXT
/// round; re-auditing an unchanged current round replaces that round's
/// artifact (registry replace-on-rerender preserved).
#[test]
fn re_audit_advances_rounds_and_replaces_on_rerender() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let session = dir.join("sessions/test-article");
    let first = wa(&dir);
    assert!(first.status.success());
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(rounds.contains("\"round\":1"), "{rounds}");

    // Comments were applied: the next audit is round 2.
    let resolved = r#"{"round":1,"timestamp":"2026-09-30T12:00:00Z","summary":"applied","phase":"comments-resolved","detail":[]}"#;
    std::fs::write(
        session.join("rounds.jsonl"),
        format!("{rounds}\n{resolved}\n"),
    )
    .unwrap();
    let second = wa(&dir);
    assert!(second.status.success());
    let html = std::fs::read_to_string(session.join("review.html")).unwrap();
    assert!(html.contains("Round 2"), "next round artifact: {html:.200}");

    // Re-audit with nothing advancing: SAME round replaces (registry
    // keeps round 1 once, round 2 once — not two round-2 entries).
    let third = wa(&dir);
    assert!(third.status.success());
    let html = std::fs::read_to_string(session.join("review.html")).unwrap();
    let r2 = html.matches("Round 2").count();
    assert_eq!(r2, 1, "replace-on-rerender: one round-2 entry");
    assert!(
        html.contains("Round 1"),
        "earlier rounds stay in the registry"
    );
}

/// loopmech.AC8.1 — `--no-llm` skips the pass entirely.
#[test]
fn no_llm_flag_skips_the_pass() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let out = wa(&dir);
    assert!(out.status.success());
    assert!(
        !dir.join("sessions/test-article/rule-review.json").exists(),
        "no diagnosis without the pass"
    );
}

/// loopmech.AC8.4 — an unconfigured model endpoint never blocks the
/// deterministic outcome: the artifact renders, the skip is reported in
/// the output AND recorded in the round log.
#[test]
fn unconfigured_endpoint_skips_the_pass_without_blocking() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    // No flags: the default-on pass finds no ZAI_API_KEY (removed by the
    // spawn helper) and must degrade to a reported skip.
    let out = wa_env(&dir, &[]);
    assert!(
        out.status.success(),
        "the audit outcome is deterministic; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_lowercase().contains("diagnosis") && stdout.contains("skip"),
        "the skip is reported: {stdout}"
    );
    let session = dir.join("sessions/test-article");
    assert!(session.join("review.html").exists(), "artifact stands");
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(
        rounds.contains("rule-review-failed") || rounds.contains("rule-review-skipped"),
        "the failed pass is recorded: {rounds}"
    );
}

/// loopmech.AC8.1 — flipping the fork config flips the default: with
/// `llm_pass = false` a flagless audit makes no pass attempt at all
/// (no rule-review.json, no skip entry — the pass is simply off).
#[test]
fn config_flip_turns_the_default_off() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let hr = dir.join("rules/house-rules.toml");
    let raw = std::fs::read_to_string(&hr).unwrap();
    std::fs::write(&hr, raw.replace("llm_pass = true", "llm_pass = false")).unwrap();
    let out = wa_env(&dir, &[]);
    assert!(out.status.success());
    let session = dir.join("sessions/test-article");
    assert!(
        !session.join("rule-review.json").exists(),
        "pass off by config"
    );
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(
        !rounds.contains("rule-review"),
        "no pass attempt, no skip record: {rounds}"
    );
}

/// Rule-enforcement item 4: `wa audit` prints warn-level lint findings
/// under a WARNINGS heading after the gate report — advisory, the gate
/// itself still passes (established: `run_gate` keeps only error-severity
/// findings as reasons, so warnings were invisible before).
#[test]
fn warn_level_findings_print_after_the_gate_report_without_blocking() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is old.\nThe railroad is now the largest employer in the county.\n",
    );
    let out = wa(&dir);
    assert!(
        out.status.success(),
        "warn findings do not block; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("gate: PASS"), "stdout: {stdout}");
    assert!(
        stdout.contains("WARNINGS (advisory — they do not block)"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("tense-drift"), "rule id shown: {stdout}");
    assert!(
        stdout.contains("MOS:TENSE proxy"),
        "config description shown: {stdout}"
    );
}

/// MVP-2 A.2.2 — a session with the review-since drift pin embeds the
/// drift diff (with provenance label) in the analyze bundle, without
/// `--prior-base`.
#[test]
fn review_since_drift_embedded_in_analyze_bundle() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "She was a railroad president.\n",
        "She was a railroad president.\n",
    );
    let session = dir.join("sessions/test-article");
    std::fs::write(
        session.join("session.json"),
        r#"{"article":"Test article","base_revid":900,"started":"2026-09-25T00:00:00Z","entry_loop":3,"review_since_revid":8900001,"review_since_user":"LuisVilla"}"#,
    )
    .unwrap();
    std::fs::write(
        session.join("review-since.wikitext"),
        "She was a railroad president.\n",
    )
    .unwrap();
    // The base has drifted since the operator's last edit.
    std::fs::write(
        session.join("base.wikitext"),
        "She was president of the railroad.\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["analyze", "test-article"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let ctx = std::fs::read_to_string(session.join("context.md")).unwrap();
    assert!(
        ctx.contains("drift (prior session or operator's last edit)"),
        "drift section present: {ctx}"
    );
    assert!(
        ctx.contains("since LuisVilla's last edit (revid 8900001)"),
        "provenance label present: {ctx}"
    );
    assert!(
        ctx.contains("president of the railroad"),
        "diff content embedded: {ctx}"
    );
}

// ------------------------------------------- plan-003 AC.4 sweep gate (B.3)

fn wa_args(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn write_ledger_with_sweep(dir: &Path, status: &str) {
    let ledger = format!(
        r#"{{"schema_version":1,"sources":[{{"id":"S1","url":"https://example.com/paywalled","access_date":"2026-09-29","sweep_status":"{status}"}}],"quotes":[],"claims":[]}}"#
    );
    std::fs::write(dir.join("sessions/test-article/ledger.json"), ledger).unwrap();
}

/// A swept session with an unresolved source blocks at `wa audit` with a
/// `SweepSourceUnresolved` reason naming source and status, grouped as
/// anchor work; the operator's `wa fetch dispose` unblocks.
#[test]
fn sweep_unresolved_blocks_check_and_dispose_unblocks() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    write_ledger_with_sweep(&dir, "pending");

    let out = wa(&dir);
    assert!(!out.status.success(), "unresolved sweep source blocks");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("sweep source S1 unresolved (pending)"),
        "reason names source and status: {stdout}"
    );
    assert!(
        stdout.contains("NEEDS ANCHOR"),
        "grouped as anchor work: {stdout}"
    );
    assert!(
        stdout.contains("wa fetch dispose"),
        "the reason says how to resolve: {stdout}"
    );

    let out = wa_args(
        &dir,
        &[
            "fetch",
            "dispose",
            "test-article",
            "--source",
            "S1",
            "--disposition",
            "dropped: paywall",
        ],
    );
    assert!(
        out.status.success(),
        "dispose succeeds: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = wa(&dir);
    assert!(
        out.status.success(),
        "disposition unblocks: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Attaching operator-captured text (`wa ledger attach`) resolves the
/// sweep by supplying the text — the Kidder SF-Call path.
#[test]
fn sweep_attach_text_unblocks() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    write_ledger_with_sweep(&dir, "needs_operator");
    let capture = dir.join("capture.txt");
    std::fs::write(
        &capture,
        "The operator's browser captured this paywalled text verbatim.\n",
    )
    .unwrap();

    let out = wa(&dir);
    assert!(
        !out.status.success(),
        "needs_operator blocks until resolved"
    );

    let out = wa_args(
        &dir,
        &[
            "ledger",
            "attach",
            "test-article",
            "--source",
            "S1",
            "--file",
            "capture.txt",
        ],
    );
    assert!(
        out.status.success(),
        "attach succeeds: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = wa(&dir);
    assert!(
        out.status.success(),
        "attached text unblocks: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// A source the sweep auto-dispositioned (`print: no web text`) never
/// blocks — the gate must not dead-end on non-web sources.
#[test]
fn sweep_auto_dispositioned_print_source_passes() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    std::fs::write(
        dir.join("sessions/test-article/ledger.json"),
        r#"{"schema_version":1,"sources":[{"id":"S1","url":"isbn:0961526106","access_date":"2026-09-29","sweep_status":"no_text","disposition":"print: no web text"}],"quotes":[],"claims":[]}"#,
    )
    .unwrap();
    let out = wa(&dir);
    assert!(
        out.status.success(),
        "auto-dispositioned print source passes: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ------------------------------------------- loop-mechanization AC6.3 retirements

/// The retired `wa check` and `wa review` fail with pointers at the audit
/// surface; they never dispatch.
#[test]
fn retired_check_and_review_fail_with_pointers() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    for args in [
        vec!["check", "test-article"],
        vec!["check"],
        vec!["review", "test-article"],
        vec!["review"],
    ] {
        let out = wa_args(&dir, &args);
        assert!(!out.status.success(), "{args:?} must fail");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            text.contains("wa audit"),
            "pointer to the replacement missing: {text}"
        );
    }
    // The refused dispatches wrote no artifact.
    assert!(!dir.join("sessions/test-article/review.html").exists());
}

/// loopmech.AC7.1 — the lavish legacy pair is retired: both fail with
/// pointers to the audit surface and never dispatch.
#[test]
fn retired_render_and_poll_fail_with_pointers() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    for args in [
        vec!["render", "test-article", "--round", "1"],
        vec!["render"],
        vec!["poll", "test-article"],
        vec!["poll"],
    ] {
        let out = wa_args(&dir, &args);
        assert!(!out.status.success(), "{args:?} must fail");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            text.contains("wa audit") || text.contains("wa serve"),
            "pointer to the replacement missing: {text}"
        );
    }
    assert!(!dir.join("sessions/test-article/review.html").exists());
}

/// loopmech.AC4.4 — the claim-sequencing reason groups under NEEDS
/// ANCHOR in `wa audit`'s report, naming the claim id and prose.
#[test]
fn claim_not_staged_groups_under_needs_anchor_in_audit_output() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    std::fs::write(
        dir.join("sessions/test-article/ledger.json"),
        r#"{"schema_version":1,"sources":[{"id":"S1","url":"https://example.com/s","access_date":"2026-09-30","fetched_text":"The keep was rebuilt in stone."}],"quotes":[{"id":"Q1","source_id":"S1","text":"The keep was rebuilt in stone.","located_at":0}],"claims":[{"id":"C1","prose":"The keep was rebuilt in stone.","quote_ids":["Q1"]}]}"#,
    )
    .unwrap();
    let out = wa(&dir);
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("NEEDS ANCHOR"), "{stdout}");
    assert!(stdout.contains("prose staged nowhere"), "{stdout}");
    assert!(stdout.contains("C1"), "{stdout}");
}

// ------------------------------------ loop-mechanization Phase 5 summary rules

/// loopmech.AC5.2 — an audit summary citing an unresolvable shortcut is
/// refused: no artifact, the token named.
#[test]
fn summary_with_unresolvable_shortcut_blocks_the_audit() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let out = wa_env(
        &dir,
        &["--no-llm", "--summary", "tighten the lead per MOS:NOTREAL"],
    );
    assert!(
        !out.status.success(),
        "unresolvable shortcut refuses: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("MOS:NOTREAL"), "{text}");
    assert!(
        !dir.join("sessions/test-article/review.html").exists(),
        "no artifact from a refused summary"
    );
}

/// loopmech.AC5.1 — a summary citing a resolvable shortcut passes; the
/// default summary (no tokens) is unaffected.
#[test]
fn summary_with_resolvable_shortcut_and_default_pass() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let out = wa_env(
        &dir,
        &["--no-llm", "--summary", "set the variety per MOS:VAR"],
    );
    assert!(
        out.status.success(),
        "resolvable shortcut passes: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("sessions/test-article/review.html").exists());
}

/// loopmech.AC8.1 — `--llm` overrides the fork config: in a config-off
/// fork, the flag still ATTEMPTS the pass (observable as the recorded
/// skip: no key in the test env, so the attempt degrades to
/// `rule-review-failed` — config-off without the flag attempts nothing).
#[test]
fn llm_flag_forces_the_pass_on_over_config() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let hr = dir.join("rules/house-rules.toml");
    let raw = std::fs::read_to_string(&hr).unwrap();
    std::fs::write(&hr, raw.replace("llm_pass = true", "llm_pass = false")).unwrap();

    // Config-off, no flag: no attempt, no record.
    let out = wa_env(&dir, &[]);
    assert!(out.status.success());
    let rounds = std::fs::read_to_string(dir.join("sessions/test-article/rounds.jsonl")).unwrap();
    assert!(!rounds.contains("rule-review"), "no attempt: {rounds}");

    // Config-off, --llm: the pass is attempted (and, keyless, recorded
    // as the failed/skipped pass) — the flag beat the config.
    let out = wa_env(&dir, &["--llm"]);
    assert!(out.status.success());
    let rounds = std::fs::read_to_string(dir.join("sessions/test-article/rounds.jsonl")).unwrap();
    assert!(
        rounds.contains("rule-review-failed"),
        "the flag forced the attempt: {rounds}"
    );
}

/// loopmech.AC8.4 (unreachable variant) — a CONFIGURED but unreachable
/// endpoint never blocks the deterministic outcome: the client
/// constructs, the transport fails, the skip is reported and recorded,
/// and the artifact stands.
#[test]
fn unreachable_endpoint_skips_the_pass_without_blocking() {
    let dir = setup_session(
        r#"{"assessments":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        // Constructible client, dead endpoint (nothing listens on port 1).
        .env("ZAI_API_KEY", "test-key-not-real")
        .env("ZAI_BASE_URL", "http://127.0.0.1:1")
        .args([
            "audit",
            "--html-base",
            "base-fixture.html",
            "--html-proposed",
            "proposed-fixture.html",
            "test-article",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "the audit outcome is deterministic: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("diagnosis pass"),
        "the transport failure is reported: {stdout}"
    );
    let session = dir.join("sessions/test-article");
    assert!(session.join("review.html").exists(), "artifact stands");
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(rounds.contains("rule-review-failed"), "recorded: {rounds}");
}
