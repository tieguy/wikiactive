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
