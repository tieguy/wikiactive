//! AC.3 (MVP-2 A.2.1) — `wa check <slug>`: standalone gate run with
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
    let dir = std::env::temp_dir().join(format!("wa-check-cli-{id}-{}", std::process::id()));
    let session = dir.join("sessions/test-article");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("session.json"),
        r#"{"article":"Test article","base_revid":1,"started":"2026-09-25T00:00:00Z","entry_loop":2}"#,
    )
    .unwrap();
    std::fs::write(session.join("findings.json"), findings).unwrap();
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[],"quotes":[],"claims":[]}"#,
    )
    .unwrap();
    std::fs::write(session.join("base.wikitext"), base).unwrap();
    std::fs::write(session.join("proposed.wikitext"), proposed).unwrap();
    copy_dir(Path::new("rules"), &dir.join("rules"));
    dir
}

fn wa(dir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(dir)
        .args(["check", "test-article"])
        .output()
        .unwrap()
}

#[test]
fn blocked_proposal_reports_disposition_groups_without_artifact() {
    // F1 cites Q99 which is not in the ledger (NEEDS ANCHOR) and the
    // proposal introduces an unspaced heading (HARD BLOCK) — both groups
    // in one run.
    let dir = setup_session(
        r#"{"findings":[{"id":"F1","wikitext_anchor":"L1:C0-L1:C10","rules":["WP:V"],"evidence":["Q99"],"factual_note":"n","proposed_fix":"f","loop":2}]}"#,
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
        "check never renders: {stdout}"
    );
    assert!(
        !dir.join("sessions/test-article/review.html").exists(),
        "no artifact written"
    );
}

#[test]
fn clean_proposal_passes_without_artifact() {
    let dir = setup_session(
        r#"{"findings":[]}"#,
        "The tower is old.\n",
        "The tower is older than it looks.\n",
    );
    let out = wa(&dir);
    assert!(
        out.status.success(),
        "clean gate must pass; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("gate: PASS"), "stdout: {stdout}");
    assert!(
        !dir.join("sessions/test-article/review.html").exists(),
        "check writes no artifact on pass either"
    );
}

/// Rule-enforcement item 4: `wa check` prints warn-level lint findings
/// under a WARNINGS heading after the gate report — advisory, the gate
/// itself still passes (established: `run_gate` keeps only error-severity
/// findings as reasons, so warnings were invisible before).
#[test]
fn warn_level_findings_print_after_the_gate_report_without_blocking() {
    let dir = setup_session(
        r#"{"findings":[]}"#,
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
        r#"{"findings":[]}"#,
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

/// A swept session with an unresolved source blocks at `wa check` with a
/// `SweepSourceUnresolved` reason naming source and status, grouped as
/// anchor work; the operator's `wa sweep dispose` unblocks.
#[test]
fn sweep_unresolved_blocks_check_and_dispose_unblocks() {
    let dir = setup_session(
        r#"{"findings":[]}"#,
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
        stdout.contains("wa sweep dispose"),
        "the reason says how to resolve: {stdout}"
    );

    let out = wa_args(
        &dir,
        &[
            "sweep",
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
        r#"{"findings":[]}"#,
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
        r#"{"findings":[]}"#,
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
