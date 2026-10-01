//! `wa assess add` — schema-validated assessment authoring: malformed
//! entries are rejected (non-zero exit, nothing written); valid entries are
//! accepted and persisted.

use std::process::Command;

/// Create a temp working directory with a minimal initialized session.
fn setup_session() -> std::path::PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("wa-assess-cli-{id}-{}", std::process::id()));
    let session = dir.join("sessions/test-article");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("session.json"),
        r#"{"article":"Test article","base_revid":1,"started":"2026-09-24T00:00:00Z","entry_loop":2}"#,
    )
    .unwrap();
    std::fs::write(session.join("assessments.json"), r#"{"assessments":[]}"#).unwrap();
    // A ledger whose S1 carries text and quote Q1 (verbatim-verified), and
    // a fresh analyze bundle (context.md newer than proposed.wikitext).
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[{"id":"S1","url":"https://example.com/s","access_date":"2026-09-30","fetched_text":"verbatim words"}],"quotes":[{"id":"Q1","source_id":"S1","text":"verbatim words","located_at":0}],"claims":[]}"#,
    )
    .unwrap();
    std::fs::write(session.join("proposed.wikitext"), "base text\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(4));
    std::fs::write(
        session.join("context.md"),
        "# wa analyze — context bundle\n",
    )
    .unwrap();
    dir
}

/// Make the analyze bundle stale: proposed.wikitext written AFTER
/// context.md.
fn make_stale(dir: &std::path::Path) {
    std::thread::sleep(std::time::Duration::from_millis(4));
    std::fs::write(
        dir.join("sessions/test-article/proposed.wikitext"),
        "new draft\n",
    )
    .unwrap();
}

/// Simulate `wa analyze` re-running: context.md rewritten now.
fn reanalyze(dir: &std::path::Path) {
    std::thread::sleep(std::time::Duration::from_millis(4));
    std::fs::write(
        dir.join("sessions/test-article/context.md"),
        "# wa analyze — context bundle\n",
    )
    .unwrap();
}

fn write_batch(dir: &std::path::Path, evidence: &str) -> std::path::PathBuf {
    let json = format!(
        r#"{{"id":"AS1","wikitext_anchor":"L1:C0-L1:C5","rules":["WP:V"],"evidence":[{evidence}],"factual_note":"n","proposed_fix":"f","loop":2}}"#
    );
    let path = dir.join("batch.json");
    std::fs::write(&path, json).unwrap();
    path
}

const VALID: &str = r#"{
    "id": "AS1",
    "wikitext_anchor": "L3:C0-L3:C120",
    "rules": ["WP:V"],
    "evidence": ["Q1"],
    "factual_note": "The count is scope-widened vs the quote.",
    "proposed_fix": "Restore the 'in Japan by 1986' qualifier.",
    "loop": 2
}"#;

#[test]
fn valid_finding_is_accepted_and_persisted() {
    let dir = setup_session();
    let json_path = dir.join("assessment.json");
    std::fs::write(&json_path, VALID).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", json_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("accepted assessment AS1"));
    let persisted =
        std::fs::read_to_string(dir.join("sessions/test-article/assessments.json")).unwrap();
    assert!(persisted.contains("\"AS1\""), "{persisted}");
}

#[test]
fn malformed_json_is_rejected() {
    let dir = setup_session();
    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"{not json at all")?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(!out.status.success());
    // Nothing written.
    let persisted =
        std::fs::read_to_string(dir.join("sessions/test-article/assessments.json")).unwrap();
    assert_eq!(persisted, r#"{"assessments":[]}"#);
}

#[test]
fn schema_violations_are_rejected_with_all_problems() {
    // Missing evidence, bad id shape, loop out of range.
    let bad = r#"{
        "id": "finding-1",
        "wikitext_anchor": "L3:C0-L3:C120",
        "rules": ["WP:V"],
        "evidence": [],
        "factual_note": "n",
        "proposed_fix": "f",
        "loop": 9
    }"#;
    let dir = setup_session();
    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.as_mut().unwrap().write_all(bad.as_bytes())?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("AS<number>"), "{stderr}");
    assert!(stderr.contains("evidence"), "{stderr}");
    assert!(stderr.contains("loop"), "{stderr}");
}

#[test]
fn duplicate_assessment_id_is_rejected() {
    let dir = setup_session();
    let json_path = dir.join("assessment.json");
    std::fs::write(&json_path, VALID).unwrap();
    // Add once.
    let first = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", json_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(first.status.success());
    // Add again: same id.
    let second = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", json_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("duplicate"));
}

/// loopmech.AC6.1 — the retired `wa findings` invocation fails with a
/// pointer at `wa assess`, never dispatches.
#[test]
fn retired_findings_command_fails_with_a_pointer() {
    let dir = setup_session();
    for args in [
        vec!["findings", "add", "test-article", "-"],
        vec!["findings", "list", "test-article"],
        vec!["findings"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_wa"))
            .current_dir(&dir)
            .args(&args)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?} must fail");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("wa assess"),
            "pointer to the replacement missing: {stderr}"
        );
    }
    // And nothing was written by the refused dispatch.
    let persisted =
        std::fs::read_to_string(dir.join("sessions/test-article/assessments.json")).unwrap();
    assert_eq!(persisted, r#"{"assessments":[]}"#);
}

// ----------------------------------- loop-mechanization Phase 3 entry checks

/// loopmech.AC1.1 — an unknown evidence id refuses the batch, names the
/// id, and saves NOTHING.
#[test]
fn entry_refuses_unknown_quote_names_ids_saves_nothing() {
    let dir = setup_session();
    let batch = write_batch(&dir, r#""Q9""#);
    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", batch.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Q9"), "{stderr}");
    assert!(stderr.contains("not in the ledger"), "{stderr}");
    let persisted =
        std::fs::read_to_string(dir.join("sessions/test-article/assessments.json")).unwrap();
    assert_eq!(
        persisted, r#"{"assessments":[]}"#,
        "batch-atomic: nothing saved"
    );
}

/// loopmech.AC2.1 + AC2.2 — a stale analyze bundle refuses with the
/// relation named and `wa analyze` prescribed; re-running analyze admits
/// the IDENTICAL batch.
#[test]
fn stale_analyze_refuses_then_reanalyze_admits_identical_batch() {
    let dir = setup_session();
    make_stale(&dir);
    let batch = write_batch(&dir, r#""Q1""#);
    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", batch.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("stale"), "{stderr}");
    assert!(stderr.contains("proposed.wikitext"), "{stderr}");
    assert!(stderr.contains("wa analyze"), "{stderr}");

    reanalyze(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", batch.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "identical batch admitted after re-analyze: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// loopmech.AC2.5 / AC3.2 — the bypass flags proceed past their guards
/// and the bypass is recorded in the round log.
#[test]
fn bypass_flags_proceed_and_are_recorded() {
    let dir = setup_session();
    make_stale(&dir);
    // A swept ledger with an unresolved source joins the fun.
    std::fs::write(
        dir.join("sessions/test-article/ledger.json"),
        r#"{"schema_version":1,"sources":[{"id":"S1","url":"https://example.com/s","access_date":"2026-09-30","fetched_text":"verbatim words"},{"id":"S2","url":"https://example.com/paywalled","access_date":"2026-09-30","sweep_status":"pending"}],"quotes":[{"id":"Q1","source_id":"S1","text":"verbatim words","located_at":0}],"claims":[]}"#,
    )
    .unwrap();
    let batch = write_batch(&dir, r#""Q1""#);
    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args([
            "assess",
            "add",
            "test-article",
            batch.to_str().unwrap(),
            "--allow-stale-analyze",
            "--allow-unresolved-fetch",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "bypasses proceed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rounds_path = dir.join("sessions/test-article/rounds.jsonl");
    let rounds = std::fs::read_to_string(rounds_path).unwrap();
    assert!(rounds.contains("assess-bypass"), "{rounds}");
    assert!(rounds.contains("stale-analyze"), "{rounds}");
    assert!(rounds.contains("unresolved-fetch"), "{rounds}");
}

/// loopmech.AC3.1 + AC3.3 — an unresolved fetch source refuses with the
/// source and status named; once resolved the same command proceeds
/// without the flag.
#[test]
fn unresolved_fetch_refuses_then_resolved_proceeds() {
    let dir = setup_session();
    let swept = r#"{"schema_version":1,"sources":[{"id":"S1","url":"https://example.com/s","access_date":"2026-09-30","fetched_text":"verbatim words"},{"id":"S2","url":"https://example.com/paywalled","access_date":"2026-09-30","sweep_status":"needs_operator"}],"quotes":[{"id":"Q1","source_id":"S1","text":"verbatim words","located_at":0}],"claims":[]}"#;
    std::fs::write(dir.join("sessions/test-article/ledger.json"), swept).unwrap();
    let batch = write_batch(&dir, r#""Q1""#);

    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", batch.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("S2 (needs_operator)"), "{stderr}");

    // Resolved by a signed disposition: no flag needed.
    let resolved = r#"{"schema_version":1,"sources":[{"id":"S1","url":"https://example.com/s","access_date":"2026-09-30","fetched_text":"verbatim words"},{"id":"S2","url":"https://example.com/paywalled","access_date":"2026-09-30","sweep_status":"needs_operator","disposition":"dropped: paywall"}],"quotes":[{"id":"Q1","source_id":"S1","text":"verbatim words","located_at":0}],"claims":[]}"#;
    std::fs::write(dir.join("sessions/test-article/ledger.json"), resolved).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(&dir)
        .args(["assess", "add", "test-article", batch.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "resolved source proceeds without the flag: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
