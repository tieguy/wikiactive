//! plan-003 AC.8 — `wa serve`: loopback-only bind; console and
//! session/manifest routes render; publish requires the explicit operator
//! confirmation action (no auto-publish — the edit posts only when a
//! pending confirmation is approved); a declined or absent confirmation
//! never writes. The CLI tty path keeps passing `tests/publish.rs`.
//!
//! Drives the real `wa serve` binary (spawned with its own working
//! directory, so session/rules paths are isolated — the `check_cli`
//! pattern), with `WIKIACTIVE_SERVE_TEST_API` pointing the wiki calls at
//! an httpmock server when a publish flow is exercised.

use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

use httpmock::MockServer;

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

/// A gate-clean session with sweep state: S1 (resolved when
/// `resolve_sweep`: dispositioned — publish must not dead-end on the
/// sweep gate), S2 an auto-dispositioned print source.
fn setup_session(resolve_sweep: bool) -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("wa-serve-{id}-{}", std::process::id()));
    let session = dir.join("sessions/test-article");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("session.json"),
        r#"{"article":"Test article","base_revid":500,"started":"2026-09-29T00:00:00Z","entry_loop":2}"#,
    )
    .unwrap();
    std::fs::write(session.join("findings.json"), r#"{"findings":[]}"#).unwrap();
    let s1 = if resolve_sweep {
        r#"{"id":"S1","url":"https://example.com/paywalled","access_date":"2026-09-29","sweep_status":"needs_operator","disposition":"dropped: paywall"}"#
    } else {
        r#"{"id":"S1","url":"https://example.com/paywalled","access_date":"2026-09-29","sweep_status":"pending"}"#
    };
    std::fs::write(
        session.join("ledger.json"),
        format!(
            r#"{{"schema_version":1,"sources":[{s1},
                {{"id":"S2","url":"isbn:0961526106","access_date":"2026-09-29","sweep_status":"no_text","disposition":"print: no web text"}}],
               "quotes":[],"claims":[]}}"#
        ),
    )
    .unwrap();
    // Gate-clean proposal: identical base and proposed, no findings.
    std::fs::write(session.join("base.wikitext"), "The tower is old.\n").unwrap();
    std::fs::write(session.join("proposed.wikitext"), "The tower is old.\n").unwrap();
    copy_dir(Path::new("rules"), &dir.join("rules"));
    copy_dir(Path::new("prompts"), &dir.join("prompts"));
    dir
}

/// Spawn `wa serve --port 0` in `dir`, returning the child and the bound
/// loopback port (parsed from the printed listening line).
fn spawn_serve(dir: &Path, env: &[(&str, &str)]) -> (Child, u16) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(dir)
        .env_remove("WIKIACTIVE_SERVE_TEST_API")
        .envs(env.iter().copied())
        .args(["serve", "--port", "0"])
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(dir.join("serve-stderr.log")).unwrap())
        .spawn()
        .expect("spawn wa serve");
    let stdout = child.stdout.take().expect("piped stdout");
    // Drain stdout for the child's lifetime: closing the pipe after the
    // first line would make the server's next println! panic on a broken
    // pipe and take the whole process down.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead as _;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        if reader.read_line(&mut line).is_ok() {
            let _ = tx.send(line);
        }
        for l in reader.lines() {
            drop(l);
        }
    });
    let line = rx.recv().expect("listening line");
    // "wa serve listening on http://127.0.0.1:PORT"
    let port: u16 = line
        .rsplit("127.0.0.1:")
        .next()
        .unwrap_or_default()
        .split([' ', '('])
        .next()
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("could not parse listening line: {line}"));
    (child, port)
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

async fn poll_page_contains(
    client: &reqwest::Client,
    url: &str,
    needle: &str,
    timeout: Duration,
) -> Option<String> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(resp) = client.get(url).send().await
            && let Ok(text) = resp.text().await
            && text.contains(needle)
        {
            return Some(text);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    None
}

#[tokio::test]
async fn console_and_manifest_routes_render_on_loopback() {
    let dir = setup_session(false);
    let (mut child, port) = spawn_serve(&dir, &[]);

    let console = reqwest::get(base_url(port))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(console.contains("session console"), "{console}");
    assert!(console.contains("test-article"), "{console}");

    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("Source sweep (1 unresolved)"), "{page}");
    assert!(page.contains("example.com/paywalled"), "{page}");
    assert!(page.contains("print: no web text"), "{page}");
    assert!(page.contains("sign disposition"), "{page}");
    assert!(page.contains("start publish"), "{page}");

    let pendings = reqwest::get(format!("{}/confirmations", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(pendings.contains("none"), "{pendings}");

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn web_dispose_records_the_operator_disposition() {
    let dir = setup_session(false);
    let (mut child, port) = spawn_serve(&dir, &[]);

    // No-redirect client: the PRG pattern must surface the 303 itself.
    let resp = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .post(format!(
            "{}/sessions/test-article/sweep-dispose",
            base_url(port)
        ))
        .form(&[("source", "S1"), ("disposition", "attested-unreachable")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303, "PRG redirect");

    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("attested-unreachable"), "{page}");

    let ledger = std::fs::read_to_string(dir.join("sessions/test-article/ledger.json")).unwrap();
    assert!(ledger.contains("attested-unreachable"), "{ledger}");

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The mock wiki: revid pre-check + csrf + the edit POST (call count is
/// the no-auto-publish oracle).
async fn mock_wiki(server: &MockServer) -> httpmock::Mock<'_> {
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param("titles", "Test article");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": [{"pageid": 1, "ns": 0, "title": "Test article",
                    "revisions": [{"revid": 500, "slots": {"main": {"content": "old"}}}]}]}
            }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("meta", "tokens")
                .query_param("type", "csrf");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "", "query": {"tokens": {"csrftoken": "+\\"}}
            }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .body_includes("action=edit");
            then.status(200).json_body(serde_json::json!({
                "edit": {"result": "Success", "newrevid": 501}
            }));
        })
        .await
}

/// No auto-publish: starting the publish flow parks on a pending
/// confirmation; the edit posts only after the explicit approve click,
/// then the session re-pins.
#[tokio::test]
async fn publish_requires_the_explicit_web_confirmation() {
    let dir = setup_session(true);
    let wiki = MockServer::start_async().await;
    let edit_mock = mock_wiki(&wiki).await;
    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_API", &wiki.url("/"))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .post(format!("{}/sessions/test-article/publish", base_url(port)))
        .form(&[("summary", "web publish test")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    // The pending confirmation appears on the session page…
    let page = poll_page_contains(
        &client,
        &format!("{}/sessions/test-article", base_url(port)),
        "PENDING PUBLISH CONFIRMATION",
        Duration::from_secs(15),
    )
    .await;
    let page = page.unwrap_or_else(|| {
        let dump = std::fs::read_to_string(dir.join("serve-stderr.log"))
            .unwrap_or_else(|_| String::from("(no stderr captured)"));
        panic!("pending confirmation must render; serve stderr:\n{dump}")
    });
    // …and NO edit was posted (no auto-publish).
    assert_eq!(edit_mock.calls(), 0, "no edit without the approve click");

    let confirm_path = page
        .split("action=\"")
        .find(|a| a.starts_with("/confirmations/"))
        .and_then(|a| a.split('"').next())
        .unwrap_or_else(|| panic!("confirmation form missing: {page}"))
        .to_string();
    assert!(
        confirm_path.starts_with("/confirmations/"),
        "{confirm_path}"
    );

    let resp = client
        .post(format!("{}{}", base_url(port), confirm_path))
        .form(&[("approve", "true")])
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let published = poll_page_contains(
        &client,
        &format!("{}/sessions/test-article", base_url(port)),
        "published:",
        Duration::from_secs(15),
    )
    .await;
    assert!(published.is_some(), "the approved publish completes");
    assert_eq!(edit_mock.calls(), 1, "exactly one edit after approval");
    let meta = std::fs::read_to_string(dir.join("sessions/test-article/session.json")).unwrap();
    assert!(meta.contains("\"base_revid\": 501"), "re-pinned: {meta}");

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A declined confirmation writes nothing.
#[tokio::test]
async fn declined_confirmation_never_edits() {
    let dir = setup_session(true);
    let wiki = MockServer::start_async().await;
    let edit_mock = mock_wiki(&wiki).await;
    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_API", &wiki.url("/"))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    client
        .post(format!("{}/sessions/test-article/publish", base_url(port)))
        .form(&[("summary", "web publish test")])
        .send()
        .await
        .unwrap();

    let page = poll_page_contains(
        &client,
        &format!("{}/sessions/test-article", base_url(port)),
        "PENDING PUBLISH CONFIRMATION",
        Duration::from_secs(15),
    )
    .await;
    let page = page.unwrap_or_else(|| {
        let dump = std::fs::read_to_string(dir.join("serve-stderr.log"))
            .unwrap_or_else(|_| String::from("(no stderr captured)"));
        panic!("pending confirmation must render; serve stderr:\n{dump}")
    });

    let confirm_path = page
        .split("action=\"")
        .find(|a| a.starts_with("/confirmations/"))
        .and_then(|a| a.split('"').next())
        .unwrap()
        .to_string();
    client
        .post(format!("{}{}", base_url(port), confirm_path))
        .form(&[("approve", "false")])
        .send()
        .await
        .unwrap();

    let declined = poll_page_contains(
        &client,
        &format!("{}/sessions/test-article", base_url(port)),
        "publish failed",
        Duration::from_secs(15),
    )
    .await;
    assert!(declined.is_some(), "decline outcome surfaces");
    assert_eq!(edit_mock.calls(), 0, "a decline never edits");
    let meta = std::fs::read_to_string(dir.join("sessions/test-article/session.json")).unwrap();
    assert!(meta.contains("\"base_revid\":500"), "no re-pin: {meta}");

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The driver judgment point through the app: `POST driver/findings`
/// calls the (mocked) model, and the validated finding lands in the
/// session's findings.json via the same admission as `wa findings add`.
#[tokio::test]
async fn driver_findings_endpoint_runs_the_model_and_admits_findings() {
    let dir = setup_session(true);
    // Give S1 fetched text + a quote so the model has evidence to cite.
    let session = dir.join("sessions/test-article");
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[
            {"id":"S1","url":"https://example.com/paywalled","access_date":"2026-09-29","sweep_status":"fetched","fetched_text":"The tower was built in stages.","metadata":{"title":"Tower History"}},
            {"id":"S2","url":"isbn:0961526106","access_date":"2026-09-29","sweep_status":"no_text","disposition":"print: no web text"}],
           "quotes":[{"id":"Q1","source_id":"S1","text":"The tower was built in stages.","located_at":0}],
           "claims":[]}"#,
    )
    .unwrap();

    let zai = MockServer::start_async().await;
    zai.mock_async(|when, then| {
        when.method(httpmock::Method::POST).path("/chat/completions");
        then.status(200).json_body(serde_json::json!({
            "choices": [{"finish_reason": "stop", "index": 0,
                "message": {"role": "assistant", "content":
                    "[{\"id\":\"F1\",\"wikitext_anchor\":\"L1:C0-L1:C19\",\"rules\":[\"WP:V\"],\"evidence\":[\"Q1\"],\"factual_note\":\"The history supports the age claim.\",\"proposed_fix\":\"Cite the age.\",\"loop\":2}]"}}]
        }));
    })
    .await;

    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_ZAI", &zai.url(""))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .post(format!(
            "{}/sessions/test-article/driver/findings",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    let findings = std::fs::read_to_string(session.join("findings.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&findings).unwrap();
    let f1 = &parsed["findings"][0];
    assert_eq!(f1["id"], "F1", "{findings}");
    assert_eq!(f1["evidence"][0], "Q1", "{findings}");

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
}
