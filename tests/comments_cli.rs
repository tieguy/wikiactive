//! plan-004 AC.3 (CLI leg) — `wa comments list/add/resolve` round-trips
//! the queue (`sessions/<slug>/comments.jsonl`) with statuses and
//! resolution notes. Drives the real binary in an isolated working
//! directory (the `check_cli` pattern).

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

fn setup_session() -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("wa-comments-cli-{id}-{}", std::process::id()));
    let session = dir.join("sessions/test-article");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("session.json"),
        r#"{"article":"Test article","base_revid":1,"started":"2026-09-29T00:00:00Z","entry_loop":2}"#,
    )
    .unwrap();
    std::fs::write(session.join("assessments.json"), r#"{"assessments":[]}"#).unwrap();
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[],"quotes":[],"claims":[]}"#,
    )
    .unwrap();
    std::fs::write(session.join("base.wikitext"), "Old text.\n").unwrap();
    copy_dir(Path::new("rules"), &dir.join("rules"));
    dir
}

fn wa(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn comments_cli_round_trips_the_queue() {
    let dir = setup_session();

    // add: an anchor target (the changed-block form's spelling) and an
    // element-id target (the `ev-N` path).
    let out = wa(
        &dir,
        &[
            "comments",
            "add",
            "test-article",
            "--target",
            "L3:C0-L3:C120",
            "--text",
            "tighten this",
            "--quoted",
            "several hundred",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("queued K1"), "{stdout}");

    let out = wa(
        &dir,
        &[
            "comments",
            "add",
            "test-article",
            "--target",
            "ev-1",
            "--text",
            "quote looks trimmed",
        ],
    );
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("queued K2"));

    // list: open first with the highlighted span.
    let out = wa(&dir, &["comments", "list", "test-article"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("2 open / 2 total"), "{stdout}");
    assert!(stdout.contains("[OPEN] K1 → L3:C0-L3:C120"), "{stdout}");
    assert!(stdout.contains("\"several hundred\""), "{stdout}");
    assert!(stdout.contains("quote looks trimmed"), "{stdout}");

    // resolve: flips the status and persists the note.
    let out = wa(
        &dir,
        &[
            "comments",
            "resolve",
            "test-article",
            "--id",
            "K1",
            "--note",
            "applied; block spliced at L3",
        ],
    );
    assert!(out.status.success());
    let out = wa(&dir, &["comments", "list", "test-article"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("1 open / 2 total"), "{stdout}");
    assert!(stdout.contains("[resolved] K1"), "{stdout}");
    assert!(
        stdout.contains("resolution: applied; block spliced at L3"),
        "{stdout}"
    );

    // The queue file itself carries statuses + notes (the storage, not the
    // rendering).
    let queue = std::fs::read_to_string(dir.join("sessions/test-article/comments.jsonl")).unwrap();
    assert!(queue.contains("\"status\":\"open\""), "{queue}");
    assert!(queue.contains("\"status\":\"resolved\""), "{queue}");
    assert!(queue.contains("applied; block spliced at L3"), "{queue}");

    // An unknown id fails loudly instead of resolving nothing silently.
    let out = wa(
        &dir,
        &[
            "comments",
            "resolve",
            "test-article",
            "--id",
            "K9",
            "--note",
            "x",
        ],
    );
    assert!(!out.status.success(), "unknown id must fail");

    let _ = std::fs::remove_dir_all(&dir);
}
