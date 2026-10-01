//! loopmech.AC6.2 — `wa fetch`: inventory-then-fetch in ONE invocation;
//! `wa fetch dispose` / `wa fetch status` carry over; `wa sweep …` fails
//! with a pointer. The fixture cites only a URL-less ISBN book, which
//! inventory auto-dispositions (`print: no web text`) — so the whole
//! merged invocation runs offline (no network from tests).

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn setup_session() -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("wa-fetch-cli-{id}-{}", std::process::id()));
    let session = dir.join("sessions/test-article");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("session.json"),
        r#"{"article":"Test article","base_revid":1,"started":"2026-09-30T00:00:00Z","entry_loop":2}"#,
    )
    .unwrap();
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[],"quotes":[],"claims":[]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("cite.wikIText.txt"),
        "* {{Cite book|title=Never Come, Never Go!|year=1986|isbn=0961526106}}\n",
    )
    .unwrap();
    // The fetch leg loads rules/sweep.toml relative to the working dir.
    copy_rules(&dir);
    dir
}

fn copy_rules(dst: &Path) {
    fn rec(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let to = dst.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                rec(&entry.path(), &to);
            } else {
                std::fs::copy(entry.path(), to).unwrap();
            }
        }
    }
    rec(Path::new("rules"), &dst.join("rules"));
}

fn wa(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// One invocation registers the citation apparatus (inventory leg) and
/// runs the fetch leg; the URL-less book is auto-dispositioned, so the
/// fetch leg finds nothing pending and the command still succeeds.
#[test]
fn fetch_runs_inventory_then_fetch_in_one_invocation() {
    let dir = setup_session();
    let out = wa(
        &dir,
        &["fetch", "test-article", "--wikitext", "cite.wikIText.txt"],
    );
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
    let out_text = stdout(&out);
    assert!(
        out_text.contains("fetch inventory: 1 distinct cited source"),
        "inventory leg ran: {out_text}"
    );
    assert!(
        out_text.contains("fetch") && out_text.contains("pending"),
        "fetch leg reports its pass: {out_text}"
    );
    // The inventory leg persisted the registration.
    let ledger = std::fs::read_to_string(dir.join("sessions/test-article/ledger.json")).unwrap();
    assert!(ledger.contains("isbn:0961526106"), "{ledger}");
    assert!(ledger.contains("print: no web text"), "{ledger}");
}

/// `wa fetch` with neither slug nor subcommand is a usage error.
#[test]
fn fetch_without_slug_or_subcommand_fails() {
    let dir = setup_session();
    let out = wa(&dir, &["fetch"]);
    assert!(!out.status.success());
    let text = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("wa fetch <slug>"), "{text}");
}

/// The manifest and the operator disposition carry over.
#[test]
fn fetch_status_and_dispose_carry_over() {
    let dir = setup_session();
    let run = wa(
        &dir,
        &["fetch", "test-article", "--wikitext", "cite.wikIText.txt"],
    );
    assert!(run.status.success(), "{}", stdout(&run));

    let out = wa(&dir, &["fetch", "status", "test-article"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("fetch manifest"),
        "status output: {}",
        stdout(&out)
    );

    let out = wa(
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
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout(&out).contains("disposition recorded on S1"),
        "{}",
        stdout(&out)
    );
}

/// loopmech.AC6.2 — every `wa sweep` invocation fails with a pointer at
/// `wa fetch` and never dispatches.
#[test]
fn retired_sweep_command_fails_with_a_pointer() {
    let dir = setup_session();
    for args in [
        vec!["sweep", "inventory", "test-article"],
        vec!["sweep", "fetch", "test-article"],
        vec![
            "sweep",
            "dispose",
            "test-article",
            "--source",
            "S1",
            "--disposition",
            "x",
        ],
        vec!["sweep", "status", "test-article"],
        vec!["sweep"],
    ] {
        let out = wa(&dir, &args);
        assert!(!out.status.success(), "{args:?} must fail");
        let text = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
        assert!(
            text.contains("wa fetch"),
            "pointer to the replacement missing: {text}"
        );
    }
    // Refused dispatch wrote nothing.
    let ledger = std::fs::read_to_string(dir.join("sessions/test-article/ledger.json")).unwrap();
    assert!(ledger.contains(r#""sources":[]"#), "{ledger}");
}
