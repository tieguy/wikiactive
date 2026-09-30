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
/// loopback port (parsed from the printed listening line). Every stdout
/// line is ALSO tee'd to `<dir>/serve-stdout.log` (the AC.1 oracle: the
/// web loop's stdout must carry no lavish invocation/URL).
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

/// POST the render route with the offline fixture paths (the serve-side
/// form fields `html_base`/`html_proposed`, passed through to
/// `render_cmd`).
async fn render_offline(client: &reqwest::Client, port: u16, dir: &Path) {
    let resp = client
        .post(format!("{}/sessions/test-article/render", base_url(port)))
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
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 303, "render redirects");
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

    render_offline(&client, port, &dir).await;
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
    assert!(page.contains("Review comments"), "{page}");
    assert!(page.contains("0 open."), "{page}");
    assert!(page.contains("Open the review artifact"), "{page}");
    assert!(page.contains("queue empty"), "{page}");
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
        artifact_page.contains("leave a comment on the new (highlighted) wording"),
        "plain-language new-side form: {artifact_page}"
    );
    assert!(
        artifact_page.contains("comment on the removed (struck-through) wording"),
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
    assert!(page.contains("K1 OPEN"), "open comment highlighted");

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

    let _ = child.kill();
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

    render_offline(&client, port, &dir).await;
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

    let _ = child.kill();
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

    render_offline(&client, port, &dir).await;
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

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
}
