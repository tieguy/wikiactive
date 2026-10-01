//! Read-back verification (rule-enforcement plan item 1): after a
//! successful save, what the wiki recorded is compared with what
//! `publish_core` sent. A mismatch warns and records — the publish itself
//! still returns Ok in every case (the edit is already live).
//!
//! Own test binary, not part of tests/publish.rs: the `publish_core`
//! fixture switches the process working directory, which must not race
//! the other publish tests.

use httpmock::MockServer;
use std::path::Path;
use std::path::PathBuf;
use wikiloop::cli::Via::Tty;
use wikiloop::cli::publish_core;
use wikiloop::wikipedia::ConfirmSource;
use wikiloop::wikipedia::Wikipedia;

/// A confirm source that answers yes (the edit is the point; the
/// read-back is what varies per scenario).
struct Approve;
impl ConfirmSource for Approve {
    fn confirm<'a>(
        &'a mut self,
        _prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        Box::pin(std::future::ready(true))
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
}

/// The final summary `publish_core` sends (summary + disclosure suffix) —
/// what the read-back's comment check expects on a clean save.
const FINAL_SUMMARY: &str = "Test summary (LLM-Disclosure: [[User:LuisVilla/wikiactive]])";

/// One scenario world: a scratch cwd holding a gate-clean session pinned
/// to base 500, and a mocked wiki whose edit saves revid 501. The
/// read-back mocks are the scenario's business; the disclosure-log upsert
/// has no mock (its failure is the tolerated warning path).
async fn publish_world(name: &str) -> (PathBuf, PathBuf, MockServer) {
    let root = std::env::temp_dir().join(format!("wa-readback-{name}-{}", std::process::id()));
    let dir = root.join("w");
    let session = dir.join("sessions/readback-article");
    std::fs::create_dir_all(&session).unwrap();
    copy_dir(Path::new("rules"), &dir.join("rules"));
    std::fs::write(
        session.join("session.json"),
        r#"{"article":"Readback Article","base_revid":500,"started":"2026-09-30T00:00:00Z","entry_loop":2}"#,
    )
    .unwrap();
    std::fs::write(session.join("base.wikitext"), "old text\n").unwrap();
    std::fs::write(session.join("proposed.wikitext"), "new text\n").unwrap();
    std::fs::write(session.join("assessments.json"), r#"{"assessments":[]}"#).unwrap();
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[],"quotes":[],"claims":[]}"#,
    )
    .unwrap();

    let server = MockServer::start_async().await;
    // The article's revid pre-check (the edit's base currency).
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param("titles", "Readback Article");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": [{"pageid": 1, "ns": 0, "title": "Readback Article",
                    "revisions": [{"revid": 500, "slots": {"main": {"content": "old text"}}}]}]}
            }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("meta", "tokens")
                .query_param("type", "csrf");
            then.status(200)
                .json_body(serde_json::json!({"query": {"tokens": {"csrftoken": "+\\"}}}));
        })
        .await;
    // The edit POST mock is scenario business (mock_edit): counted there.
    (root, dir, server)
}

/// The edit POST (returns the mock so scenarios can count calls).
async fn mock_edit(server: &MockServer) -> httpmock::Mock<'_> {
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .body_includes("action=edit");
            then.status(200)
                .json_body(serde_json::json!({"edit": {"result": "Success", "newrevid": 501}}));
        })
        .await
}

/// The read-back flags query (one mock per scenario, matched by rvprop so
/// it cannot collide with the text query).
async fn mock_flags_query(server: &MockServer, revision: serde_json::Value) {
    server
        .mock_async(move |when, then| {
            when.method(httpmock::Method::GET)
                .query_param("revids", "501")
                .query_param("rvprop", "ids|flags|comment|user");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": [{"pageid": 1, "ns": 0, "title": "Readback Article",
                    "revisions": [revision]}]}
            }));
        })
        .await;
}

/// The read-back text query, returning `content` for revid 501.
async fn mock_text_query(server: &MockServer, content: &str) {
    let content = content.to_string();
    server
        .mock_async(move |when, then| {
            when.method(httpmock::Method::GET)
                .query_param("revids", "501")
                .query_param("rvprop", "ids|content");
            then.status(200).json_body(serde_json::json!({
                "query": {"pages": [{"pageid": 1, "ns": 0, "title": "Readback Article",
                    "revisions": [{"revid": 501, "slots": {"main": {"content": content}}}]}]}
            }));
        })
        .await;
}

async fn minor_flag_yields_a_mismatch_line() {
    let (root, dir, server) = publish_world("minor").await;
    let edit_mock = mock_edit(&server).await;
    // The regression this item exists for: the server saved the edit as
    // minor despite notminor=1.
    mock_flags_query(
        &server,
        serde_json::json!({
            "revid": 501, "parentid": 500, "flags": ["minor"],
            "comment": FINAL_SUMMARY, "user": "LuisVilla"
        }),
    )
    .await;
    mock_text_query(&server, "new text\n").await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    assert!(std::env::set_current_dir(&dir).is_ok());
    let mut approve = Approve;
    let outcome = publish_core("readback-article", "Test summary", &wiki, &mut approve, Tty)
        .await
        .expect("publish completes despite the mismatch");
    assert!(outcome.created_revision);
    assert_eq!(edit_mock.calls(), 1);
    assert!(
        outcome.verification.iter().any(|l| l.contains("MINOR")),
        "minor flag must surface: {:?}",
        outcome.verification
    );
    // The round entry keeps the diff URL first, then the mismatch lines.
    let rounds =
        std::fs::read_to_string(dir.join("sessions/readback-article/rounds.jsonl")).unwrap();
    let entry: serde_json::Value = rounds.lines().last().unwrap().parse().unwrap();
    let detail = entry["detail"].as_array().expect("detail array");
    assert!(
        detail[0].as_str().unwrap().contains("diff=501"),
        "diff URL first: {detail:?}"
    );
    assert!(
        detail
            .iter()
            .skip(1)
            .any(|d| d.as_str().unwrap().contains("MINOR")),
        "mismatch recorded: {detail:?}"
    );
    assert!(std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

async fn matching_revision_yields_no_lines() {
    let (root, dir, server) = publish_world("clean").await;
    let edit_mock = mock_edit(&server).await;
    mock_flags_query(
        &server,
        serde_json::json!({
            "revid": 501, "parentid": 500,
            "comment": FINAL_SUMMARY, "user": "LuisVilla"
        }),
    )
    .await;
    mock_text_query(&server, "new text\n").await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    assert!(std::env::set_current_dir(&dir).is_ok());
    let mut approve = Approve;
    let outcome = publish_core("readback-article", "Test summary", &wiki, &mut approve, Tty)
        .await
        .expect("clean publish");
    assert!(outcome.created_revision);
    assert_eq!(edit_mock.calls(), 1);
    assert!(
        outcome.verification.is_empty(),
        "clean read-back: {:?}",
        outcome.verification
    );
    assert!(std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

async fn failed_query_reports_could_not_verify() {
    let (root, dir, server) = publish_world("query-error").await;
    let _edit_mock = mock_edit(&server).await;
    // No read-back mocks at all: every read-back query fails — reported
    // as lines, never as a publish error.
    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    assert!(std::env::set_current_dir(&dir).is_ok());
    let mut approve = Approve;
    let outcome = publish_core("readback-article", "Test summary", &wiki, &mut approve, Tty)
        .await
        .expect("publish completes despite the failed read-back");
    assert!(outcome.created_revision);
    assert!(
        !outcome.verification.is_empty(),
        "the failed read-back must be reported"
    );
    assert!(
        outcome
            .verification
            .iter()
            .any(|l| l.contains("could not verify the saved revision")),
        "{:?}",
        outcome.verification
    );
    assert!(std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn read_back_verifies_the_saved_revision() {
    // Sequential on purpose: each scenario switches the process cwd.
    minor_flag_yields_a_mismatch_line().await;
    matching_revision_yields_no_lines().await;
    failed_query_reports_could_not_verify().await;
    unresolvable_summary_shortcut_refuses_publish_without_edit().await;
    unstaged_claim_blocks_publish_without_edit().await;
    nothing_staged_refuses_publish_without_edit().await;
}

/// loopmech.AC5.2 (publish half) — a summary citing an unresolvable
/// shortcut is refused BEFORE any request: no edit, the token named.
/// A scenario (not a test): the process cwd switches.
async fn unresolvable_summary_shortcut_refuses_publish_without_edit() {
    let (root, dir, server) = publish_world("summary-refusal").await;
    let edit_mock = mock_edit(&server).await;
    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    assert!(std::env::set_current_dir(&dir).is_ok());
    let mut approve = Approve;
    let err = publish_core(
        "readback-article",
        "restore the date per MOS:NOTREAL",
        &wiki,
        &mut approve,
        Tty,
    )
    .await
    .expect_err("unresolvable shortcut refuses");
    assert!(err.to_string().contains("MOS:NOTREAL"), "{err}");
    assert_eq!(
        edit_mock.calls(),
        0,
        "no edit request from a refused summary"
    );
    assert!(std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

/// loopmech.AC4.1 (publish half) — a claim registered without staged
/// prose blocks PUBLISH too (the gate re-runs there): no edit request.
/// A scenario (not a test): the process cwd switches.
async fn unstaged_claim_blocks_publish_without_edit() {
    let (root, dir, server) = publish_world("claim-not-staged").await;
    let edit_mock = mock_edit(&server).await;
    // The ledger carries one claim whose prose is in neither base
    // ("old text") nor proposed ("new text").
    std::fs::write(
        dir.join("sessions/readback-article/ledger.json"),
        r#"{"schema_version":1,"sources":[],"quotes":[],"claims":[{"id":"C1","prose":"The keep was rebuilt in stone.","quote_ids":[]}]}"#,
    )
    .unwrap();
    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    assert!(std::env::set_current_dir(&dir).is_ok());
    let mut approve = Approve;
    let err = publish_core("readback-article", "Test summary", &wiki, &mut approve, Tty)
        .await
        .expect_err("unstaged claim blocks publish");
    assert!(
        err.to_string().contains("gate blocked publish"),
        "gate reason surfaces: {err}"
    );
    assert_eq!(edit_mock.calls(), 0, "no edit while a claim is unstaged");
    assert!(std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

/// revux.AC1.5 (CLI half) — `publish_core` refuses up front when nothing
/// is staged (proposed == base): no confirmation prompt, no edit. A
/// scenario (not a test): the process cwd switches.
async fn nothing_staged_refuses_publish_without_edit() {
    let (root, dir, server) = publish_world("nothing-staged").await;
    let edit_mock = mock_edit(&server).await;
    // Nothing staged: proposed identical to base.
    let base_text =
        std::fs::read_to_string(dir.join("sessions/readback-article/base.wikitext")).unwrap();
    std::fs::write(
        dir.join("sessions/readback-article/proposed.wikitext"),
        &base_text,
    )
    .unwrap();
    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    assert!(std::env::set_current_dir(&dir).is_ok());
    let mut approve = Approve;
    let err = publish_core("readback-article", "Test summary", &wiki, &mut approve, Tty)
        .await
        .expect_err("nothing staged refuses");
    assert!(
        err.to_string().contains("nothing staged"),
        "refusal names the state: {err}"
    );
    assert_eq!(edit_mock.calls(), 0, "no request of any kind");
    assert!(std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}
