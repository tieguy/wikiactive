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
    std::fs::write(session.join("assessments.json"), r#"{"assessments":[]}"#).unwrap();
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
    mtime_gap();
    std::fs::write(
        session.join("context.md"),
        "# wa analyze — context bundle\n",
    )
    .unwrap();
    copy_dir(Path::new("rules"), &dir.join("rules"));
    copy_dir(Path::new("prompts"), &dir.join("prompts"));
    dir
}

/// Spawn `wa serve --port 0` in `dir`, returning the child and the bound
/// loopback port (parsed from the printed listening line). Every stdout
/// line is ALSO tee'd to `<dir>/serve-stdout.log` (the AC.1 oracle: the
/// web loop's stdout must carry no lavish invocation/URL).
fn spawn_serve(dir: &Path, env: &[(&str, &str)]) -> (Child, u16) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(dir)
        .env_remove("WIKIACTIVE_SERVE_TEST_API")
        // Tests never make live model calls: without an explicit
        // WIKIACTIVE_SERVE_TEST_ZAI override the client must fail to
        // construct (the reported-skip path), never reach an endpoint.
        .env_remove("ZAI_API_KEY")
        .env_remove("ZAI_BASE_URL")
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
    let mut log = std::fs::File::create(dir.join("serve-stdout.log")).expect("stdout log");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead as _;
        use std::io::Write as _;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        if reader.read_line(&mut line).is_ok() {
            let _ = writeln!(log, "{line}");
            let _ = log.flush();
            let _ = tx.send(line.clone());
        }
        for l in reader.lines().map_while(Result::ok) {
            // Unbuffered: the tests read this file while the server is
            // still running.
            let _ = writeln!(log, "{l}");
            let _ = log.flush();
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

/// Run `wa <args>` in `dir` synchronously (tests: the subprocess IS the
/// work under test; blocking on it is the point).
fn run_wa(dir: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_wa"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("wa runs")
}

/// A wall-clock gap so filesystem mtimes order strictly (nanosecond
/// resolution makes a few milliseconds sufficient).
fn mtime_gap() {
    std::thread::sleep(Duration::from_millis(4));
}

/// Stop a spawned `wa serve` child and reap it (blocking by design —
/// the test is finished with the process).
fn reap_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Bounded-poll a tee'd serve log until it carries `needle` (the drain
/// thread writes per-line asynchronously; asserting on a possibly
/// undrained log is a flake). Synchronous by design — the file IS the
/// thing under observation.
fn poll_log_for(log_path: &Path, needle: &str) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut stdout = String::new();
    while std::time::Instant::now() < deadline {
        stdout = std::fs::read_to_string(log_path).unwrap_or_default();
        if stdout.contains(needle) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    stdout
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
    assert!(console.contains("<h1>Sessions</h1>"), "{console}");
    assert!(console.contains("test-article"), "{console}");

    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("1 of 2 sources"), "{page}");
    assert!(page.contains("example.com/paywalled"), "{page}");
    assert!(page.contains("print: no web text"), "{page}");
    assert!(page.contains("Sign disposition"), "{page}");
    assert!(page.contains("Publish this edit"), "{page}");

    let pendings = reqwest::get(format!("{}/confirmations", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(pendings.contains("No publish is waiting"), "{pendings}");

    reap_child(&mut child);
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

    reap_child(&mut child);
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
    // A staged edit: base-identical sessions refuse up-front (revux.AC1.5).
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
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
        "Approve this publish",
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

    // Plan-005 D3: the pending prompt is confirm-source-neutral. The old
    // wording leaked tty mechanics onto the web surface ("This tty
    // confirmation … when you type yes") — pinned out.
    assert!(
        page.contains("This confirmation is the FINAL gate"),
        "neutral final-gate sentence: {page}"
    );
    assert!(!page.contains("tty confirmation"), "{page}");
    assert!(!page.contains("type yes"), "{page}");

    let resp = client
        .post(format!("{}{}", base_url(port), confirm_path))
        .form(&[("approve", "true")])
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "lands on the session page");

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

    // Plan-005 O.2: the web publish path logs plain URLs — no raw OSC-8
    // escapes (the serve log is not a terminal) — while still pointing at
    // the saved revision. The drain thread writes per-line asynchronously,
    // so bounded-poll for the line before asserting on the file (review
    // finding: asserting on a possibly-undrained log is a flake).
    let stdout = poll_log_for(&dir.join("serve-stdout.log"), "check it: ");
    assert!(
        stdout.contains("check it: http"),
        "plain permalink in the web log: {stdout}"
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "no escape sequences in the web serve log: {stdout}"
    );

    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A declined confirmation writes nothing.
#[tokio::test]
async fn declined_confirmation_never_edits() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
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
        "Approve this publish",
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

    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The driver judgment point through the app: `POST driver/assess`
/// calls the (mocked) model, and the validated finding lands in the
/// session's assessments.json via the same admission as `wa findings add`.
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
                    "[{\"id\":\"AS1\",\"wikitext_anchor\":\"L1:C0-L1:C19\",\"rules\":[\"WP:V\"],\"evidence\":[\"Q1\"],\"factual_note\":\"The history supports the age claim.\",\"proposed_fix\":\"Cite the age.\",\"loop\":2}]"}}]
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
            "{}/sessions/test-article/driver/assess",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    let findings = std::fs::read_to_string(session.join("assessments.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&findings).unwrap();
    let f1 = &parsed["assessments"][0];
    assert_eq!(f1["id"], "AS1", "{findings}");
    assert_eq!(f1["evidence"][0], "Q1", "{findings}");

    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- plan-004: the in-app review loop (comment queue, no lavish) ----

/// A gate-clean session with a REAL changed edit for offline rendering:
/// base/proposed wikitext plus matching HTML fixtures (each line a `<p>`),
/// so the render route can run fully offline through its fixture-path
/// form fields.
fn setup_review_session(base: &str, proposed: &str) -> PathBuf {
    let dir = setup_session(true);
    let session = dir.join("sessions/test-article");
    let html_of = |wt: &str| {
        use std::fmt::Write as _;
        let mut body = String::new();
        for l in wt.lines() {
            let _ = write!(body, "<p>{l}</p>");
        }
        format!("<html><body>{body}</body></html>")
    };
    std::fs::write(session.join("base.wikitext"), base).unwrap();
    std::fs::write(session.join("proposed.wikitext"), proposed).unwrap();
    std::fs::write(dir.join("base.html"), html_of(base)).unwrap();
    std::fs::write(dir.join("proposed.html"), html_of(proposed)).unwrap();
    dir
}

/// The artifact's embedded anchor table, read exactly as the app does
/// (AC.2 oracle: recorded targets must match these VERBATIM).
fn anchor_table(dir: &Path) -> Vec<(String, String)> {
    let html = std::fs::read_to_string(dir.join("sessions/test-article/review.html")).unwrap();
    wikiloop::render::read_anchor_table_str(&html)
        .unwrap()
        .into_iter()
        .map(|e| (e.element_id, e.wikitext_anchor))
        .collect()
}

/// POST the audit route with the offline fixture paths (the serve-side
/// form fields `html_base`/`html_proposed`, passed through to the render
/// pipeline) and NO LLM toggle.
async fn audit_offline(client: &reqwest::Client, port: u16, dir: &Path) {
    let resp = client
        .post(format!("{}/sessions/test-article/audit", base_url(port)))
        .form(&[
            ("summary", "test round"),
            (
                "html_base",
                dir.join("base.html").to_string_lossy().as_ref(),
            ),
            (
                "html_proposed",
                dir.join("proposed.html").to_string_lossy().as_ref(),
            ),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303, "render redirects");
}

/// Rule-enforcement item 4: warn-level lint findings (tense-drift on an
/// added line) show on the served review — under the block whose anchor
/// range covers them, in the warn palette with the rule's config
/// description — and never block the render.
#[tokio::test]
async fn lint_warnings_show_on_the_review_page_without_blocking() {
    let dir = setup_review_session(
        "The tower is old.\n",
        "The tower is old.\nThe railroad is now the largest employer in the county.\n",
    );
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    audit_offline(&client, port, &dir).await;
    assert!(
        dir.join("sessions/test-article/review.html").exists(),
        "warn findings do not block the render"
    );

    let page = reqwest::get(format!("{}/sessions/test-article/review", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("wa-comment lint-warning"),
        "warn styling present: {page}"
    );
    assert!(page.contains("tense-drift"), "rule id shown: {page}");
    assert!(
        page.contains("MOS:TENSE proxy"),
        "config description shown: {page}"
    );
    reap_child(&mut child);
}

/// loopmech.AC6.4 — the Audit action's LLM diagnosis toggle: one POST
/// renders the artifact AND runs the diagnosis pass against it, stores
/// the concerns with the round, and they render under the block they
/// name — while the `rule-reviewed` round entry leaves the artifact
/// CURRENT (only published / comments-resolved / text changes stale it;
/// pinned here).
#[tokio::test]
async fn audit_llm_toggle_renders_and_diagnoses_without_staling() {
    // The base carries a line the proposal deletes: pure-deletion blocks
    // are skipped by the step (review finding 3 — their spans cannot
    // validate), so the pass must still complete.
    let dir = setup_review_session(
        "The tower is old.\nA stale line follows.\n",
        "Critics have widely considered the tower the finest.\n",
    );
    let session = dir.join("sessions/test-article");
    let zai = MockServer::start_async().await;
    zai.mock_async(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/chat/completions")
            .body_includes("Cluster A"); // the guidance rides this prompt too
        then.status(200).json_body(serde_json::json!({
            "choices": [{"finish_reason": "stop", "index": 0,
                "message": {"role": "assistant", "content":
                    "[{\"clause\":\"A3\",\"verdict\":\"concern\",\"span\":\"widely considered\",\"note\":\"WEASEL wording needs attribution.\"}]"}}]
        }));
    })
    .await;

    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_ZAI", &zai.url(""))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // ONE request: render + the LLM diagnosis pass (the toggle).
    let resp = client
        .post(format!("{}/sessions/test-article/audit", base_url(port)))
        .form(&[
            ("round", "1"),
            ("summary", "test round"),
            (
                "html_base",
                dir.join("base.html").to_string_lossy().as_ref(),
            ),
            (
                "html_proposed",
                dir.join("proposed.html").to_string_lossy().as_ref(),
            ),
            ("llm", "on"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303, "to the fresh review");
    assert!(
        dir.join("sessions/test-article/review.html").exists(),
        "the artifact rendered in the same request"
    );

    // Stored with the CURRENT round, concern only.
    let file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(session.join("rule-review.json")).unwrap())
            .unwrap();
    assert_eq!(file["round"], 1, "{file}");
    assert_eq!(file["concerns"][0]["clause"], "A3", "{file}");

    // Shown under the block; the artifact did NOT go stale (comment
    // forms still render, no stale banner) — the entry is inert.
    let page = reqwest::get(format!("{}/sessions/test-article/review", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("rule-concern"), "advice shown: {page}");
    assert!(page.contains("Clause A3"), "{page}");
    assert!(page.contains("widely considered"), "span shown: {page}");
    assert!(
        page.contains("1 concern(s) shown under their blocks."),
        "{page}"
    );
    assert!(
        !page.contains("This review is out of date"),
        "rule-reviewed must not stale the artifact: {page}"
    );
    assert!(page.contains("Comment"), "comment forms still live: {page}");
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rule-enforcement item 3, review finding 4: a corpus missing a
/// triage-selected card fails the serve handler BEFORE any model call —
/// the paid endpoint is never hit.
#[tokio::test]
async fn missing_card_fails_the_handler_without_a_model_call() {
    let dir = setup_session(true);
    // Loop 2 selects rs-tiers/clop/primary-carveouts; remove one.
    let removed = dir.join("rules/cards/rs-tiers.md");
    assert!(removed.exists());
    std::fs::remove_file(&removed).unwrap();

    let zai = MockServer::start_async().await;
    let model = zai
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(500);
        })
        .await;
    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_ZAI", &zai.url(""))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .post(format!(
            "{}/sessions/test-article/driver/assess",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    assert_eq!(model.calls(), 0, "no model call on a broken corpus");
    // The outcome names the missing card.
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("rs-tiers") && page.contains("missing"),
        "the outcome names the missing card: {page}"
    );
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rule-enforcement item 5: a result from an EARLIER round is never
/// shown as current — labelled as out of date, concerns not rendered,
/// button still offered.
#[tokio::test]
async fn rule_review_result_from_an_earlier_round_is_labelled() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let session = dir.join("sessions/test-article");
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    audit_offline(&client, port, &dir).await;
    std::fs::write(
        session.join("rule-review.json"),
        r#"{"round":0,"timestamp":"2026-09-30T00:00:00Z","concerns":[{"clause":"A3","span":"widely considered","note":"stale advice","element_id":"wa-1"}]}"#,
    )
    .unwrap();

    let page = reqwest::get(format!("{}/sessions/test-article/review", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("Not checked for this round (last checked round 0)."),
        "{page}"
    );
    assert!(
        !page.contains("Clause A3") && !page.contains("stale advice"),
        "stale concerns are not rendered as current: {page}"
    );
    assert!(
        page.contains("wa-bar"),
        "the rules status line still renders: {page}"
    );
    assert!(
        !page.contains("/driver/rule-review"),
        "no standalone rule-review control anymore: {page}"
    );
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// AC.1 + AC.2 + AC.3: the whole review leg in-app — offline render
/// through the serve route, comment forms per changed block, submit →
/// list → resolve through `comments.jsonl`, and NO lavish anywhere (the
/// page, the artifact path prints, or the server's stdout).
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn in_app_review_flow_renders_comments_and_resolves_without_lavish() {
    let dir = setup_review_session(
        "The tower is old.\nThe keep is quiet.\n",
        "The tower is ancient.\nThe keep is quiet.\n",
    );
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    audit_offline(&client, port, &dir).await;
    assert!(
        dir.join("sessions/test-article/review.html").exists(),
        "artifact written"
    );

    // The session page: status + queue + pointer to the artifact (the
    // commenting surface); no lavish link or re-open (AC.1).
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("Round 1 is ready to read"), "{page}");
    assert!(page.contains("Open the review</a>"), "{page}");
    assert!(page.contains("No comments yet"), "{page}");
    assert!(
        !page.contains("lavish"),
        "session page must not link lavish: {page}"
    );

    // The SERVED ARTIFACT carries the comment forms inline under each
    // block, plain-language (no jargon labels, no quoted field), with the
    // hidden targets carrying the anchor-table anchors VERBATIM (AC.2).
    let artifact_page = reqwest::get(format!("{}/sessions/test-article/review", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        artifact_page.contains("Comment on the new wording"),
        "plain-language new-side form: {artifact_page}"
    );
    assert!(
        artifact_page.contains("id=\"wa-publish\""),
        "the publish action lives on the artifact page: {artifact_page}"
    );
    assert!(
        artifact_page.contains(">Publish this edit</button>"),
        "{artifact_page}"
    );
    assert!(
        artifact_page.contains("Comment on the removed wording"),
        "plain-language old-side affordance: {artifact_page}"
    );
    assert!(
        !artifact_page.contains("words you mean"),
        "the confusing quoted field is gone from the UI"
    );
    assert!(
        !artifact_page.contains("change ·"),
        "no jargon labels: {artifact_page}"
    );

    let table = anchor_table(&dir);
    let new_side = table
        .iter()
        .find(|(id, a)| id.starts_with("wa-") && !a.starts_with("base:"))
        .expect("new-side anchor");
    let old_side = table
        .iter()
        .find(|(_, a)| a.starts_with("base:"))
        .expect("old-side (base:) anchor");
    for anchor in [&new_side.1, &old_side.1] {
        assert!(
            artifact_page.contains(&format!("name=target value=\"{anchor}\"")),
            "form target must be the anchor verbatim: {artifact_page}"
        );
    }

    // Submit → the queue records the anchors verbatim (AC.2/AC.3).
    let post_comment = |target: String, text: String, quoted: Option<String>| {
        let client = client.clone();
        let url = format!("{}/sessions/test-article/comments", base_url(port));
        async move {
            let mut form = vec![("target", target), ("text", text)];
            if let Some(q) = quoted {
                form.push(("quoted", q));
            }
            let resp = client.post(&url).form(&form).send().await.unwrap();
            assert_eq!(resp.status().as_u16(), 303);
        }
    };
    post_comment(
        new_side.1.clone(),
        "make it 'very ancient'".into(),
        Some("ancient".into()),
    )
    .await;
    post_comment(
        old_side.1.clone(),
        "the old wording should be quoted in the reply".into(),
        None,
    )
    .await;

    let queue_text =
        std::fs::read_to_string(dir.join("sessions/test-article/comments.jsonl")).unwrap();
    let queue: Vec<serde_json::Value> = queue_text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(queue.len(), 2, "{queue_text}");
    assert_eq!(queue[0]["id"], "K1");
    assert_eq!(queue[0]["target"], new_side.1.as_str(), "{queue_text}");
    assert_eq!(queue[0]["quoted"], "ancient");
    assert_eq!(queue[0]["status"], "open");
    assert_eq!(queue[1]["target"], old_side.1.as_str(), "{queue_text}");

    // Manual resolution writes the note back (AC.3).
    let resp = client
        .post(format!(
            "{}/sessions/test-article/comments/resolve",
            base_url(port)
        ))
        .form(&[("id", "K2"), ("note", "handled by hand")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let queue_text =
        std::fs::read_to_string(dir.join("sessions/test-article/comments.jsonl")).unwrap();
    assert!(
        queue_text.contains("\"status\":\"resolved\""),
        "{queue_text}"
    );
    assert!(queue_text.contains("handled by hand"), "{queue_text}");

    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("handled by hand"), "resolution note renders");
    assert!(
        page.contains("name=id value=\"K1\""),
        "open comment carries its resolve form"
    );

    // AC.1: the web render path printed the in-app artifact path, and the
    // drained server stdout carries no lavish invocation or URL.
    let stdout = std::fs::read_to_string(dir.join("serve-stdout.log")).unwrap();
    assert!(
        stdout.contains("review: /sessions/test-article/review (in-app)"),
        "web render prints the in-app path: {stdout}"
    );
    assert!(
        !stdout.contains("lavish"),
        "no lavish on the web path: {stdout}"
    );

    // Staleness leg (the operator's shakedown catch): a publish after the
    // render makes the artifact history — the served artifact shows the
    // banner and NO comment forms, and the session page says out of date.
    std::fs::write(
        dir.join("sessions/test-article/rounds.jsonl.bak"),
        std::fs::read_to_string(dir.join("sessions/test-article/rounds.jsonl")).unwrap(),
    )
    .unwrap();
    {
        use std::io::Write as _;
        let rounds_path = dir.join("sessions/test-article/rounds.jsonl");
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&rounds_path)
            .unwrap();
        writeln!(
            f,
            "{{\"round\":0,\"timestamp\":\"2099-01-01T00:00:00Z\",\"summary\":\"published\",\
             \"phase\":\"published\",\"detail\":[]}}"
        )
        .unwrap();
    }
    let stale = reqwest::get(format!("{}/sessions/test-article/review", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        stale.contains("This review is out of date."),
        "stale banner: {stale}"
    );
    assert!(
        stale.contains("This edit was published, so the review is closed."),
        "{stale}"
    );
    assert!(
        !stale.contains("name=target"),
        "no comment forms on a stale artifact: {stale}"
    );
    assert!(
        !stale.contains("Publish this edit"),
        "no publish action on a stale artifact: {stale}"
    );
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("The round 1 review is out of date."),
        "{page}"
    );
    // And driver-resolve refuses the stale artifact.
    let resp = client
        .post(format!(
            "{}/sessions/test-article/driver/resolve",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let last_run = std::fs::read_to_string(dir.join("sessions/test-article/last-run.txt")).unwrap();
    assert!(
        last_run.contains("review artifact is out of date"),
        "{last_run}"
    );
    // Restore: the earlier queue assertions already ran; the artifact
    // returns to current once the publish entry is removed.
    std::fs::rename(
        dir.join("sessions/test-article/rounds.jsonl.bak"),
        dir.join("sessions/test-article/rounds.jsonl"),
    )
    .unwrap();

    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The mocked-model resolution payload for one group.
fn resolution_json(block: &str, applied: &str, reply: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{"finish_reason": "stop", "index": 0,
            "message": {"role": "assistant", "content": format!(
                "{{\"proposed_wikitext_block\":{block},\"applied\":[{applied}],\"rejected\":[],\"reply\":{reply}}}",
                block = serde_json::to_string(block).unwrap(),
                applied = serde_json::to_string(applied).unwrap(),
                reply = serde_json::to_string(reply).unwrap(),
            )}}]
    })
}

/// AC.4: driver-resolve over TWO block groups — a changed pair commented
/// on BOTH sides (one merged group, ONE model call) and an aligned pure
/// deletion — splicing the revised blocks once per group and writing the
/// combined applied/rejected/reply note back per group. An evidence
/// comment is excluded from the model and left open.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn driver_resolve_merges_pair_sides_into_one_call_and_splices_both_groups() {
    // Deletion at base L1 (aligned prefix: empty), changed pair at L2/L3.
    let dir = setup_review_session(
        "The moat is dry.\nThe keep is quiet.\nThe tower is old.\n",
        "The keep is quiet.\nThe tower is ancient.\n",
    );
    let session = dir.join("sessions/test-article");

    let zai = MockServer::start_async().await;
    // ONE mock for the merged pair group: the request must carry BOTH
    // sides' comment texts (body_includes is AND-semantics).
    let pair_mock = zai
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("very ancient")
                .body_includes("old wording");
            then.status(200).json_body(resolution_json(
                "The tower is very ancient.",
                "tightened the tower line",
                "Revised the tower line.",
            ));
        })
        .await;
    let deletion_mock = zai
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("restore the moat");
            then.status(200).json_body(resolution_json(
                "The moat is mostly dry.",
                "restored the moat line",
                "Restored the moat line.",
            ));
        })
        .await;

    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_ZAI", &zai.url(""))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    audit_offline(&client, port, &dir).await;
    let table = anchor_table(&dir);
    // base:L1 = the pure deletion (moat); base:L3 = the pair's old side;
    // the plain L2 anchor = the pair's new side.
    let by_prefix = |p: &str| {
        table
            .iter()
            .find(|(_, a)| a.starts_with(p))
            .unwrap_or_else(|| panic!("no anchor starting {p}: {table:?}"))
            .1
            .clone()
    };
    let deletion_anchor = by_prefix("base:L1");
    let pair_old_anchor = by_prefix("base:L3");
    let pair_new_anchor = by_prefix("L2");

    let add = |target: String, text: String| {
        let client = client.clone();
        let url = format!("{}/sessions/test-article/comments", base_url(port));
        async move {
            let resp = client
                .post(&url)
                .form(&[("target", target), ("text", text)])
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status().as_u16(), 303);
        }
    };
    add(pair_new_anchor.clone(), "make it 'very ancient'".into()).await;
    add(
        pair_old_anchor,
        "the old wording matters — keep it somewhere".into(),
    )
    .await;
    add(deletion_anchor, "restore the moat".into()).await;
    add("ledger:Q1".into(), "this quote looks trimmed".into()).await;

    let resp = client
        .post(format!(
            "{}/sessions/test-article/driver/resolve",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    // ONE model call for the merged pair (both sides in one request), one
    // for the deletion — and none for the evidence comment.
    assert_eq!(pair_mock.calls(), 1, "pair sides merge into ONE call");
    assert_eq!(deletion_mock.calls(), 1);

    // The revised blocks spliced once per group.
    let proposed = std::fs::read_to_string(session.join("proposed.wikitext")).unwrap();
    let lines: Vec<&str> = proposed.lines().collect();
    assert_eq!(
        lines,
        vec![
            "The moat is mostly dry.",
            "The keep is quiet.",
            "The tower is very ancient.",
        ],
        "deletion inserted at base L1, pair line replaced: {proposed}"
    );

    // Writeback: the pair's comments share the pair group's combined
    // note; the deletion has its own; the evidence comment stays open.
    let queue_text = std::fs::read_to_string(session.join("comments.jsonl")).unwrap();
    let queue: Vec<serde_json::Value> = queue_text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(queue.len(), 4, "{queue_text}");
    assert_eq!(queue[0]["status"], "resolved", "{queue_text}");
    assert_eq!(queue[1]["status"], "resolved", "{queue_text}");
    assert_eq!(queue[2]["status"], "resolved", "{queue_text}");
    assert_eq!(
        queue[3]["status"], "open",
        "evidence stays open: {queue_text}"
    );
    let pair_note = queue[0]["resolution"].as_str().unwrap();
    assert!(
        pair_note.contains("applied: tightened the tower line"),
        "{queue_text}"
    );
    assert!(
        pair_note.contains("reply: Revised the tower line."),
        "{queue_text}"
    );
    assert_eq!(
        queue[1]["resolution"].as_str(),
        queue[0]["resolution"].as_str(),
        "both sides of the pair share the group note"
    );
    assert!(
        queue[2]["resolution"]
            .as_str()
            .unwrap()
            .contains("restored the moat line"),
        "{queue_text}"
    );

    // The round log records the resolution phase.
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(rounds.contains("comments-resolved"), "{rounds}");

    // The outcome surfaces the evidence exclusion.
    let last_run = std::fs::read_to_string(session.join("last-run.txt")).unwrap();
    assert!(last_run.contains("3 comment(s) resolved"), "{last_run}");
    assert!(
        last_run.contains("evidence comments left for manual resolution: K4"),
        "{last_run}"
    );

    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Plan-005 O.3: driver-resolve over THREE disjoint groups — two separate
/// changed pairs plus a pure insertion — splicing each group's revised
/// block exactly once, with the combined proposed.wikitext pinned.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn driver_resolve_splices_three_groups_in_one_pass() {
    // Pair at L2, pair at L4, pure insertion at L6 — each separated by
    // unchanged lines so they stay three disjoint diff blocks.
    let dir = setup_review_session(
        "The keep is quiet.\nThe tower is old.\nThe gate is new.\nThe hall is grand.\nThe mill is stone.\n",
        "The keep is quiet.\nThe tower is ancient.\nThe gate is new.\nThe hall is grander.\nThe mill is stone.\nThe well is deep.\n",
    );
    let session = dir.join("sessions/test-article");

    let zai = MockServer::start_async().await;
    let tower_mock = zai
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("make the tower older");
            then.status(200).json_body(resolution_json(
                "The tower is very ancient.",
                "aged the tower",
                "Aged the tower line.",
            ));
        })
        .await;
    let hall_mock = zai
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("the hall needs more");
            then.status(200).json_body(resolution_json(
                "The hall is grandest.",
                "raised the hall",
                "Raised the hall line.",
            ));
        })
        .await;
    let well_mock = zai
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("describe the well");
            then.status(200).json_body(resolution_json(
                "The well is deep and cold.",
                "deepened the well",
                "Deepened the well line.",
            ));
        })
        .await;

    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_ZAI", &zai.url(""))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    audit_offline(&client, port, &dir).await;
    let table = anchor_table(&dir);
    let anchor_of = |line_prefix: &str| {
        table
            .iter()
            .find(|(_, a)| a.starts_with(line_prefix))
            .unwrap_or_else(|| panic!("no anchor starting {line_prefix}: {table:?}"))
            .1
            .clone()
    };
    let tower_anchor = anchor_of("L2");
    let hall_anchor = anchor_of("L4");
    let well_anchor = anchor_of("L6");

    for (target, text) in [
        (&tower_anchor, "make the tower older".to_string()),
        (&hall_anchor, "the hall needs more".to_string()),
        (&well_anchor, "describe the well".to_string()),
    ] {
        let resp = client
            .post(format!("{}/sessions/test-article/comments", base_url(port)))
            .form(&[("target", target.as_str()), ("text", text.as_str())])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 303);
    }

    let resp = client
        .post(format!(
            "{}/sessions/test-article/driver/resolve",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    assert_eq!(tower_mock.calls(), 1, "tower group: one call");
    assert_eq!(hall_mock.calls(), 1, "hall group: one call");
    assert_eq!(well_mock.calls(), 1, "insertion group: one call");

    // The exact spliced result: each group's revised block in place,
    // untouched lines preserved verbatim.
    let proposed = std::fs::read_to_string(session.join("proposed.wikitext")).unwrap();
    assert_eq!(
        proposed,
        "The keep is quiet.\nThe tower is very ancient.\nThe gate is new.\nThe hall is grandest.\nThe mill is stone.\nThe well is deep and cold.\n",
        "three-group splice: {proposed}"
    );

    let last_run = std::fs::read_to_string(session.join("last-run.txt")).unwrap();
    assert!(last_run.contains("3 comment(s) resolved"), "{last_run}");

    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// AC.4 (error leg): a pure-deletion group whose base anchor is
/// MISALIGNED (the changed pair at base L1 means base and proposed
/// disagree before the deletion line) surfaces the error, the comment
/// stays open, and nothing is spliced — never blind.
#[tokio::test]
async fn misaligned_deletion_group_surfaces_error_and_stays_open() {
    // Changed pair at L1, an Equal block at L2 (so the moat delete stays a
    // PURE deletion with its own base:L3 anchor), moat deleted at base L3:
    // the prefix check base[..2] != proposed[..2] must fail the splice.
    let dir = setup_review_session(
        "The tower is old.\nThe keep is quiet.\nThe moat is dry.\n",
        "The tower is ancient.\nThe keep is quiet.\n",
    );
    let session = dir.join("sessions/test-article");

    let zai = MockServer::start_async().await;
    zai.mock_async(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/chat/completions")
            .body_includes("restore the moat");
        then.status(200).json_body(resolution_json(
            "The moat is mostly dry.",
            "restored the moat line",
            "Restored the moat line.",
        ));
    })
    .await;

    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_ZAI", &zai.url(""))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    audit_offline(&client, port, &dir).await;
    let table = anchor_table(&dir);
    let deletion_anchor = table
        .iter()
        .find(|(_, a)| a.starts_with("base:L3"))
        .expect("pure-deletion anchor at base L3 (separated from the pair by the Equal block)")
        .1
        .clone();

    let resp = client
        .post(format!("{}/sessions/test-article/comments", base_url(port)))
        .form(&[
            ("target", deletion_anchor.as_str()),
            ("text", "restore the moat"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    // An unknown element id as target: never silently dropped — the
    // comment stays open with the error surfaced.
    let resp = client
        .post(format!("{}/sessions/test-article/comments", base_url(port)))
        .form(&[("target", "wa-99"), ("text", "where is this?")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    // Write-route gate (review finding 1): a traversal slug gets 404, no
    // directory is created, nothing appended.
    let resp = client
        .post(format!(
            "{}/sessions/..%2F..%2Ftmp%2Fwa-trav/comments",
            base_url(port)
        ))
        .form(&[("target", "L1:C0-L1:C5"), ("text", "hostile")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404, "traversal slug rejected");
    assert!(!std::path::Path::new("/tmp/wa-trav/comments.jsonl").exists());

    let resp = client
        .post(format!(
            "{}/sessions/test-article/driver/resolve",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    let last_run = std::fs::read_to_string(session.join("last-run.txt")).unwrap();
    assert!(
        last_run.contains("misaligned"),
        "the misalignment error surfaces: {last_run}"
    );
    assert!(
        last_run.contains("0 comment(s) resolved"),
        "nothing resolved: {last_run}"
    );
    assert!(
        last_run.contains("UNRESOLVED target K2 (target wa-99)"),
        "the unknown target surfaces: {last_run}"
    );
    let queue_text = std::fs::read_to_string(session.join("comments.jsonl")).unwrap();
    assert!(
        queue_text.matches("\"status\":\"open\"").count() == 2,
        "both comments stay open: {queue_text}"
    );
    // Nothing was spliced.
    let proposed = std::fs::read_to_string(session.join("proposed.wikitext")).unwrap();
    assert_eq!(
        proposed, "The tower is ancient.\nThe keep is quiet.\n",
        "no blind splice"
    );

    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------- loop-mechanization Phase 1 (AC6.4)

/// The session page offers the renamed controls (Assess, Audit) and the
/// audit form carries the LLM diagnosis toggle.
#[tokio::test]
async fn session_page_shows_assess_and_audit_controls() {
    let dir = setup_session(true);
    let (mut child, port) = spawn_serve(&dir, &[]);
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains(">Assess</button>"), "Assess control: {page}");
    assert!(
        page.contains("/sessions/test-article/driver/assess"),
        "assess action: {page}"
    );
    assert!(page.contains(">Audit</button>"), "Audit control: {page}");
    assert!(
        page.contains("/sessions/test-article/audit"),
        "audit action: {page}"
    );
    assert!(
        page.contains("name=llm") && page.contains("LLM diagnosis"),
        "the LLM toggle rides the audit form: {page}"
    );
    assert!(
        page.contains("name=llm checked"),
        "the fork config ships the toggle ON by default: {page}"
    );
    assert!(
        !page.contains("Write findings") && !page.contains("Render review"),
        "retired labels are gone: {page}"
    );
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The audit form WITHOUT the toggle renders the artifact and never
/// calls the model (no rule-review.json).
#[tokio::test]
async fn audit_without_the_toggle_makes_no_model_call() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let session = dir.join("sessions/test-article");
    // A mock that fails the test if hit: the pass is off.
    let zai = MockServer::start_async().await;
    zai.mock_async(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/chat/completions");
        then.status(500);
    })
    .await;
    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_ZAI", &zai.url(""))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    audit_offline(&client, port, &dir).await;
    assert!(session.join("review.html").exists(), "artifact written");
    assert!(
        !session.join("rule-review.json").exists(),
        "no diagnosis pass without the toggle"
    );
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The retired serve routes are gone: render, driver/findings, and the
/// standalone rule-review control all 404.
#[tokio::test]
async fn retired_serve_routes_are_gone() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    for path in [
        "/sessions/test-article/render",
        "/sessions/test-article/driver/findings",
        "/sessions/test-article/driver/rule-review",
    ] {
        let resp = client
            .post(format!("{}{path}", base_url(port)))
            .form(&[("round", "1"), ("summary", "x")])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 404, "{path} must be gone");
    }
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// loopmech.AC8.4 (serve half) — the toggle ON with NO constructible
/// model client: the audit still renders the artifact and the failed
/// pass is recorded in the round log.
#[tokio::test]
async fn audit_llm_on_without_an_endpoint_renders_and_records_the_skip() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let session = dir.join("sessions/test-article");
    let (mut child, port) = spawn_serve(&dir, &[]); // no key, no override
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .post(format!("{}/sessions/test-article/audit", base_url(port)))
        .form(&[
            ("summary", "test round"),
            (
                "html_base",
                dir.join("base.html").to_string_lossy().as_ref(),
            ),
            (
                "html_proposed",
                dir.join("proposed.html").to_string_lossy().as_ref(),
            ),
            ("llm", "on"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303, "to the fresh review");
    assert!(session.join("review.html").exists(), "the artifact stands");
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(
        rounds.contains("rule-review-failed"),
        "the failed pass is recorded: {rounds}"
    );
    assert!(
        !session.join("rule-review.json").exists(),
        "no stored concerns from a failed pass"
    );
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// loopmech.AC8.1 (serve half) — flipping the fork config flips the
/// toggle's default state on the page.
#[tokio::test]
async fn config_off_fork_unchecks_the_toggle() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let hr = dir.join("rules/house-rules.toml");
    let raw = std::fs::read_to_string(&hr).unwrap();
    std::fs::write(&hr, raw.replace("llm_pass = true", "llm_pass = false")).unwrap();
    let (mut child, port) = spawn_serve(&dir, &[]);
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !page.contains("name=llm checked"),
        "config off => unchecked: {page}"
    );
    assert!(page.contains("name=llm"), "the toggle still exists: {page}");
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------ loop-mechanization Phase 3 entry checks

/// A driver-assess mock model whose output cites an unknown quote id —
/// the step itself blocks (Malformed after retry); the refusal is
/// VISIBLE on the page and nothing lands in assessments.json.
#[tokio::test]
async fn driver_assess_unknown_quote_is_refused_visibly() {
    let dir = setup_session(true);
    let session = dir.join("sessions/test-article");
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[
            {"id":"S1","url":"https://example.com/s","access_date":"2026-09-29","sweep_status":"fetched","fetched_text":"The tower was built in stages."}],
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
                    "[{\"id\":\"AS1\",\"wikitext_anchor\":\"L1:C0-L1:C19\",\"rules\":[\"WP:V\"],\"evidence\":[\"Q99\"],\"factual_note\":\"n.\",\"proposed_fix\":\"f.\",\"loop\":2}]"}}]
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
            "{}/sessions/test-article/driver/assess",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("Q99") && page.contains("not a registered ledger quote"),
        "refusal visible: {page}"
    );
    let persisted = std::fs::read_to_string(session.join("assessments.json")).unwrap();
    assert_eq!(persisted, r#"{"assessments":[]}"#, "nothing saved");
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// loopmech.AC2.3 — the serve Assess path enforces the same freshness
/// state and refuses with the same message (stale relation + wa analyze).
#[tokio::test]
async fn driver_assess_refuses_stale_analyze_with_the_cli_message() {
    let dir = setup_session(true);
    let session = dir.join("sessions/test-article");
    // A quoted ledger so the fetch/evidence guards stay quiet.
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[
            {"id":"S1","url":"https://example.com/s","access_date":"2026-09-29","sweep_status":"fetched","fetched_text":"The tower was built in stages."}],
           "quotes":[{"id":"Q1","source_id":"S1","text":"The tower was built in stages.","located_at":0}],
           "claims":[]}"#,
    )
    .unwrap();
    // Stale: proposed rewritten AFTER the analyze bundle.
    mtime_gap();
    std::fs::write(session.join("proposed.wikitext"), "The tower is ancient.\n").unwrap();
    let zai = MockServer::start_async().await;
    zai.mock_async(|when, then| {
        when.method(httpmock::Method::POST).path("/chat/completions");
        then.status(200).json_body(serde_json::json!({
            "choices": [{"finish_reason": "stop", "index": 0,
                "message": {"role": "assistant", "content":
                    "[{\"id\":\"AS1\",\"wikitext_anchor\":\"L1:C0-L1:C19\",\"rules\":[\"WP:V\"],\"evidence\":[\"Q1\"],\"factual_note\":\"n.\",\"proposed_fix\":\"f.\",\"loop\":2}]"}}]
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
            "{}/sessions/test-article/driver/assess",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("stale") && page.contains("wa analyze"),
        "same freshness refusal as the CLI: {page}"
    );
    let persisted = std::fs::read_to_string(session.join("assessments.json")).unwrap();
    assert_eq!(persisted, r#"{"assessments":[]}"#);
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// loopmech.AC2.3 + phase-3 "done when" — BOTH entry surfaces refuse the
/// SAME fixture batch with the SAME refusal text (they share
/// `assess_entry_checks`); the identical sentence is asserted on both.
#[tokio::test]
async fn both_assess_surfaces_refuse_the_same_batch_identically() {
    let dir = setup_session(true);
    let session = dir.join("sessions/test-article");
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[
            {"id":"S1","url":"https://example.com/s","access_date":"2026-09-29","sweep_status":"fetched","fetched_text":"The tower was built in stages."}],
           "quotes":[{"id":"Q1","source_id":"S1","text":"The tower was built in stages.","located_at":0}],
           "claims":[]}"#,
    )
    .unwrap();
    // Stale: proposed rewritten AFTER the analyze bundle.
    mtime_gap();
    std::fs::write(session.join("proposed.wikitext"), "The tower is ancient.\n").unwrap();
    let batch = r#"[{"id":"AS1","wikitext_anchor":"L1:C0-L1:C19","rules":["WP:V"],"evidence":["Q1"],"factual_note":"n.","proposed_fix":"f.","loop":2}]"#;
    let refusal = "context.md (the analyze bundle) is stale — older than proposed.wikitext's last modification";

    // Surface 1: the serve Assess action.
    let zai = MockServer::start_async().await;
    zai.mock_async(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/chat/completions");
        then.status(200).json_body(serde_json::json!({
            "choices": [{"finish_reason": "stop", "index": 0,
                "message": {"role": "assistant", "content": batch}}]
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
            "{}/sessions/test-article/driver/assess",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains(refusal), "serve refusal verbatim: {page}");
    reap_child(&mut child);

    // Surface 2: the CLI, the SAME batch on the SAME fixture.
    let batch_path = dir.join("batch.json");
    std::fs::write(&batch_path, batch).unwrap();
    let out = run_wa(
        &dir,
        &[
            "assess",
            "add",
            "test-article",
            batch_path.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(refusal), "CLI refusal identical: {stderr}");
    let persisted = std::fs::read_to_string(session.join("assessments.json")).unwrap();
    assert_eq!(
        persisted, r#"{"assessments":[]}"#,
        "nothing saved by either surface"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// loopmech.AC3.4 — an unresolved fetch source refuses at the serve
/// Assess path and the message points at the on-page affordances.
#[tokio::test]
async fn driver_assess_unresolved_fetch_points_at_the_affordances() {
    let dir = setup_session(false); // S1 pending
    let session = dir.join("sessions/test-article");
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[
            {"id":"S1","url":"https://example.com/paywalled","access_date":"2026-09-29","sweep_status":"pending"},
            {"id":"S2","url":"https://example.com/s","access_date":"2026-09-29","sweep_status":"fetched","fetched_text":"The tower was built in stages."}],
           "quotes":[{"id":"Q1","source_id":"S2","text":"The tower was built in stages.","located_at":0}],
           "claims":[]}"#,
    )
    .unwrap();
    let zai = MockServer::start_async().await;
    zai.mock_async(|when, then| {
        when.method(httpmock::Method::POST).path("/chat/completions");
        then.status(200).json_body(serde_json::json!({
            "choices": [{"finish_reason": "stop", "index": 0,
                "message": {"role": "assistant", "content":
                    "[{\"id\":\"AS1\",\"wikitext_anchor\":\"L1:C0-L1:C19\",\"rules\":[\"WP:V\"],\"evidence\":[\"Q1\"],\"factual_note\":\"n.\",\"proposed_fix\":\"f.\",\"loop\":2}]"}}]
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
            "{}/sessions/test-article/driver/assess",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("S1 (pending)"),
        "source and status named: {page}"
    );
    assert!(
        page.contains("Sources table below"),
        "points at the on-page affordances: {page}"
    );
    let persisted = std::fs::read_to_string(session.join("assessments.json")).unwrap();
    assert_eq!(persisted, r#"{"assessments":[]}"#);
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------- review-ux-fixes (revux) Phase 1

/// loopmech-style revux.AC1.1 — POST /reject on a staged edit: parked
/// confirmations decline, proposed restores to base, an `aborted` round
/// entry records the rejection.
#[tokio::test]
async fn reject_restores_proposed_and_records_aborted() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let session = dir.join("sessions/test-article");
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    audit_offline(&client, port, &dir).await;
    let resp = client
        .post(format!(
            "{}/sessions/test-article/reject?from=review",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        303,
        "back to where the operator acted"
    );
    let base = std::fs::read_to_string(session.join("base.wikitext")).unwrap();
    let proposed = std::fs::read_to_string(session.join("proposed.wikitext")).unwrap();
    assert_eq!(proposed, base, "the staged edit is gone");
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(rounds.contains("\"phase\":\"aborted\""), "{rounds}");
    assert!(rounds.contains("rejected"), "{rounds}");
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// revux.AC1.4 — rejecting when nothing is staged is a visible no-op:
/// no round entry, files untouched.
#[tokio::test]
async fn reject_with_nothing_staged_is_a_noop() {
    let dir = setup_review_session("The tower is old.\n", "The tower is old.\n");
    let session = dir.join("sessions/test-article");
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let before = std::fs::read_to_string(session.join("proposed.wikitext")).unwrap();
    let resp = client
        .post(format!("{}/sessions/test-article/reject", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("nothing staged to reject"), "{page}");
    let after = std::fs::read_to_string(session.join("proposed.wikitext")).unwrap();
    assert_eq!(before, after, "files untouched");
    assert!(
        !session.join("rounds.jsonl").exists(),
        "no round entry for a no-op"
    );
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// revux.AC1.2 — after a reject the artifact stales under the REJECTED
/// banner (names the rejection, offers no re-render), and the aborted
/// round entry is inert to the published/comments-resolved staleness
/// legs (this test is that pin). Render precedes reject so the mtime
/// ordering is strict.
#[tokio::test]
async fn reject_stales_the_artifact_under_the_rejected_banner() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    audit_offline(&client, port, &dir).await;
    mtime_gap();
    let resp = client
        .post(format!(
            "{}/sessions/test-article/reject?from=review",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let page = reqwest::get(format!("{}/sessions/test-article/review", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("rejected by the operator") || page.contains("edit was rejected"),
        "the rejected banner names the rejection: {page}"
    );
    assert!(
        !page.contains("Render round"),
        "no re-render offer after a reject: {page}"
    );
    assert!(
        !page.contains("a hand edit"),
        "not the text-changed wording: {page}"
    );
    assert!(
        !page.contains("<form method=post action=\"/sessions/test-article/comments\""),
        "no live comment forms on a stale artifact: {page}"
    );
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// revux.AC1.6 — the race is closed end-to-end: publish parks a
/// confirmation holding the pre-reject text; a reject declines it; the
/// still-rendered Approve finds nothing — and NO edit request reaches
/// the wiki.
#[tokio::test]
async fn reject_declines_a_parked_publish_confirmation() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let wiki = MockServer::start_async().await;
    let edit_mock = mock_wiki(&wiki).await;
    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_API", &wiki.url("/"))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Publish parks the confirmation (no edit yet).
    let resp = client
        .post(format!("{}/sessions/test-article/publish", base_url(port)))
        .form(&[("summary", "web publish test")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let page = poll_page_contains(
        &client,
        &format!("{}/sessions/test-article", base_url(port)),
        "Approve this publish",
        Duration::from_secs(15),
    )
    .await
    .expect("pending confirmation renders");
    assert_eq!(edit_mock.calls(), 0);
    let confirm_path = page
        .split("action=\"")
        .find(|a| a.starts_with("/confirmations/"))
        .and_then(|a| a.split('"').next())
        .expect("confirmation form present")
        .to_string();

    // The operator rejects instead.
    let resp = client
        .post(format!("{}/sessions/test-article/reject", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);

    // The still-rendered Approve finds nothing to approve.
    let resp = client
        .post(format!("{}{}", base_url(port), confirm_path))
        .form(&[("approve", "true")])
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap_or_default();
    assert!(
        body.contains("Approval not found") || body.contains("Nothing was written"),
        "the declined confirmation surfaces: {body}"
    );
    assert_eq!(
        edit_mock.calls(),
        0,
        "the rejected text never reaches the wiki"
    );
    let proposed =
        std::fs::read_to_string(dir.join("sessions/test-article/proposed.wikitext")).unwrap();
    let base = std::fs::read_to_string(dir.join("sessions/test-article/base.wikitext")).unwrap();
    assert_eq!(proposed, base, "the reject restored the text");
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// revux.AC1.5 — publish on a session with NOTHING staged (proposed ==
/// base) refuses up front: no confirmation parks, no edit request.
#[tokio::test]
async fn publish_refuses_when_nothing_is_staged() {
    let dir = setup_session(true); // base == proposed
    let wiki = MockServer::start_async().await;
    let edit_mock = mock_wiki(&wiki).await;
    let (mut child, port) = spawn_serve(&dir, &[("WIKIACTIVE_SERVE_TEST_API", &wiki.url("/"))]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .post(format!("{}/sessions/test-article/publish", base_url(port)))
        .form(&[("summary", "should refuse")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303);
    let page = poll_page_contains(
        &client,
        &format!("{}/sessions/test-article", base_url(port)),
        "nothing staged",
        Duration::from_secs(10),
    )
    .await
    .expect("the refusal surfaces");
    assert!(!page.contains("Approve this publish"), "{page}");
    assert_eq!(edit_mock.calls(), 0);
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// revux.AC1.3 — the Reject control renders where the publish decision
/// lives: inside the review page's publish section and under the session
/// page's Audit form.
#[tokio::test]
async fn reject_control_renders_beside_publish() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    audit_offline(&client, port, &dir).await;
    let review = reqwest::get(format!("{}/sessions/test-article/review", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let reject_idx = review
        .find("Reject this edit")
        .unwrap_or_else(|| panic!("reject control on the review page: {review}"));
    let publish_idx = review
        .find("id=\"wa-publish\"")
        .unwrap_or_else(|| panic!("publish section present: {review}"));
    assert!(
        reject_idx > publish_idx,
        "the control sits inside the publish section"
    );
    assert!(review.contains("/sessions/test-article/reject"), "{review}");

    let session_page = reqwest::get(format!("{}/sessions/test-article", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let audit_idx = session_page
        .find(">Audit</button>")
        .unwrap_or_else(|| panic!("audit form present: {session_page}"));
    let reject_idx = session_page
        .find("Reject this edit")
        .unwrap_or_else(|| panic!("reject control on the session page: {session_page}"));
    assert!(reject_idx > audit_idx, "under the Audit form");
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// revux.AC2.1 + AC2.2 — the comments control says what it does
/// (Process comments; the model may decline with reasons) and sits at
/// the end of the diff, above the publish section — not in the top bar.
#[tokio::test]
async fn process_comments_control_is_labeled_and_placed() {
    let dir = setup_review_session("The tower is old.\n", "The tower is ancient.\n");
    // One open comment so the queue is live.
    run_wa(
        &dir,
        &[
            "comments",
            "add",
            "test-article",
            "--target",
            "L1:C0-L1:C21",
            "--text",
            "tighten this",
        ],
    );
    let (mut child, port) = spawn_serve(&dir, &[]);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    audit_offline(&client, port, &dir).await;
    let review = reqwest::get(format!("{}/sessions/test-article/review", base_url(port)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(review.contains(">Process comments</button>"), "{review}");
    assert!(
        review.contains("may decline with reasons"),
        "the control states the model can decline: {review}"
    );
    assert!(!review.contains("Apply comments"), "{review}");
    // Not in the top status bar: the first wa-bar carries no form.
    let bar_start = review.find("wa-bar").expect("status bar");
    let bar_end = review[bar_start..]
        .find("</div>")
        .map(|i| bar_start + i)
        .unwrap();
    let bar = &review[bar_start..bar_end];
    assert!(
        !bar.contains("<form"),
        "the top bar carries no button: {bar}"
    );
    // Placed at the end of the diff: after the comment threads, before
    // the publish section (search the body only — the appended
    // stylesheet also mentions the class).
    let main_end = review.find("</main>").expect("main closes");
    let body = &review[..main_end];
    let process_idx = body
        .find(">Process comments</button>")
        .expect("process control");
    let publish_idx = body.find("id=\"wa-publish\"").expect("publish section");
    let last_comment_idx = body.rfind("wa-comment").unwrap_or(0);
    assert!(process_idx > last_comment_idx, "after the comment threads");
    assert!(
        process_idx < publish_idx,
        "immediately above the publish section"
    );
    reap_child(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}
