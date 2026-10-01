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
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[],"quotes":[],"claims":[]}"#,
    )
    .unwrap();
    dir
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
