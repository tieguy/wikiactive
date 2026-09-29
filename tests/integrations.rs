//! AC.12 — external integration contracts (httpmock): Earwig compare/search
//! and archive.org save-page-now request shapes, UA header, error/cap
//! handling.
//! AC.13 — Wikimedia etiquette assertions: identifying UA on every request,
//! `maxlag` on API calls, backoff honored on 429/503, no external skill
//! files read.

use httpmock::MockServer;
use wikiloop::USER_AGENT;
use wikiloop::ledger::net::EarwigClient;
use wikiloop::ledger::net::SavePageNow;
use wikiloop::ledger::net::SourceFetcher;
use wikiloop::ledger::net::ssrf_guard;
use wikiloop::wikipedia::Wikipedia;

// ---------------------------------------------------------------- AC.12 Earwig

#[tokio::test]
async fn earwig_compare_contract() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api.json")
                .query_param("action", "compare")
                .query_param("project", "wikipedia")
                .query_param("lang", "en")
                .query_param("title", "Temple Fielding")
                .query_param("oldid", "1372827284")
                .query_param("url", "https://example.com/source")
                .header("User-Agent", USER_AGENT);
            then.status(200).json_body(serde_json::json!({
                "status": "ok",
                "result": {"url": "https://example.com/source", "verdict": "suspect", "ratio": 0.42}
            }));
        })
        .await;

    let earwig = EarwigClient::with_base(&server.url("/api.json"));
    let (verdict, ratio) = earwig
        .compare(
            "Temple Fielding",
            1_372_827_284,
            "https://example.com/source",
        )
        .await
        .expect("compare ok");
    assert_eq!(verdict, "suspect");
    assert!((ratio - 0.42).abs() < 1e-9);
    assert_eq!(mock.calls(), 1, "request shape mismatch");
}

#[tokio::test]
async fn earwig_search_contract() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api.json")
                .query_param("action", "search")
                .query_param_exists("oldid")
                .header("User-Agent", USER_AGENT);
            then.status(200).json_body(serde_json::json!({
                "status": "ok",
                "best": {"url": "https://copy.example/mirror", "confidence": 0.87}
            }));
        })
        .await;

    let earwig = EarwigClient::with_base(&server.url("/api.json"));
    let best = earwig
        .search("Temple Fielding", 123)
        .await
        .expect("search ok");
    assert_eq!(
        best,
        Some(("https://copy.example/mirror".to_string(), 0.87))
    );
    assert_eq!(mock.calls(), 1);
}

#[tokio::test]
async fn earwig_non_ok_status_is_an_error_not_a_panic() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/api.json");
            then.status(200)
                .json_body(serde_json::json!({"status": "error"}));
        })
        .await;
    let earwig = EarwigClient::with_base(&server.url("/api.json"));
    assert!(earwig.compare("X", 1, "https://e.com").await.is_err());
}

// ------------------------------------------------------------------ AC.12 SPN

#[tokio::test]
async fn spn_request_shape_and_archive_url() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/save")
                .body_includes("url=https%3A%2F%2Fexample.com%2Farticle")
                .header("User-Agent", USER_AGENT);
            then.status(200).header(
                "content-location",
                "/web/20260924180000/https://example.com/article",
            );
        })
        .await;

    let spn = SavePageNow::with_base(&server.url("/save"));
    let url = spn
        .save("https://example.com/article")
        .await
        .expect("saved");
    assert_eq!(
        url,
        "https://web.archive.org/web/20260924180000/https://example.com/article"
    );
    assert_eq!(mock.calls(), 1);
}

#[tokio::test]
async fn spn_rate_limit_is_an_error() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST).path("/save");
            then.status(429).body("rate limited");
        })
        .await;
    let spn = SavePageNow::with_base(&server.url("/save"));
    let err = spn.save("https://example.com/a").await.unwrap_err();
    assert!(err.to_string().contains("429"), "{err}");
}

#[tokio::test]
async fn spn_denial_is_an_error() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST).path("/save");
            then.status(403);
        })
        .await;
    let spn = SavePageNow::with_base(&server.url("/save"));
    assert!(spn.save("https://example.com/a").await.is_err());
}

// ------------------------------------------------------- AC.12 fetch caps/UA

#[tokio::test]
async fn source_fetch_sends_identifying_ua() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/page")
                .header("User-Agent", USER_AGENT);
            then.status(200).body("source text body");
        })
        .await;
    let fetcher = SourceFetcher::with_allow_local().unwrap();
    let text = fetcher
        .fetch_text(&server.url("/page"))
        .await
        .expect("fetch ok");
    assert_eq!(text, "source text body");
    assert_eq!(mock.calls(), 1);
}

#[tokio::test]
async fn source_fetch_size_cap_enforced() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/big");
            then.status(200).body("x".repeat(3 * 1024 * 1024));
        })
        .await;
    let fetcher = SourceFetcher::with_allow_local().unwrap();
    let err = fetcher.fetch_text(&server.url("/big")).await.unwrap_err();
    assert!(err.to_string().contains("exceeded"), "{err}");
}

#[tokio::test]
async fn source_fetch_backs_off_on_503_then_succeeds() {
    let server = MockServer::start_async().await;
    // Always-503 with Retry-After: 0; the fetcher must back off to its
    // attempt cap and then fail with a 503-status error.
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/flaky");
            then.status(503).header("retry-after", "0");
        })
        .await;
    let fetcher = SourceFetcher::with_allow_local().unwrap();
    let err = fetcher.fetch_text(&server.url("/flaky")).await.unwrap_err();
    assert!(err.to_string().contains("503"), "{err}");
    let hits = mock.calls();
    assert_eq!(hits, 3, "should back off exactly to the attempt cap");
}

// ------------------------------------------------------------- AC.13 etiquette

#[tokio::test]
async fn mwapi_client_sends_ua_and_maxlag() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param("maxlag", "5")
                .header("User-Agent", USER_AGENT);
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": {"1": {"pageid": 1, "ns": 0, "title": "Test",
                    "revisions": [{"revid": 700, "slots": {"main": {"content": "x"}}}]}}}
            }));
        })
        .await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .expect("connect");
    let revid = wiki.current_revid("Test").await.expect("revid");
    assert_eq!(revid, 700);
    assert_eq!(mock.calls(), 1, "UA + maxlag must both be present");
}

#[tokio::test]
async fn no_external_skill_files_are_read() {
    // AC.13: no code path reads external skill files. The binary has no
    // runtime dependency on any on-disk skill: assert the rule corpus and
    // house rules load only from the repo's rules/ directory, and that the
    // UA/etiquette constants are compiled in (not file-read).
    assert!(USER_AGENT.contains("User:LuisVilla"));
    assert!(USER_AGENT.contains("luis@lu.is"));
    let house = std::fs::read_to_string("rules/house-rules.toml").unwrap();
    assert!(house.contains("LLM-Disclosure"));
    // The UA string in house rules matches the compiled constant.
    assert!(
        house.contains("wikiactive/0.1"),
        "house rules must carry the UA"
    );
    assert_eq!(
        wikiloop::DISCLOSURE_SUFFIX,
        "LLM-Disclosure: [[User:LuisVilla/wikiactive]]"
    );
    // ssrf_guard is compiled in, not configured by skill files.
    assert!(ssrf_guard(&url::Url::parse("http://localhost/").unwrap()).is_err());
}

// ------------------------------------------------- AC.12 SSRF redirect guard

#[tokio::test]
async fn fetch_refuses_redirect_to_private_target() {
    // A public URL that 302s to the link-local metadata service: the
    // per-hop SSRF guard must refuse to follow the redirect.
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/redirector");
            then.status(302)
                .header("location", "http://169.254.169.254/latest/meta-data");
        })
        .await;
    let fetcher = SourceFetcher::with_allow_local().unwrap();
    let err = fetcher
        .fetch_text(&server.url("/redirector"))
        .await
        .unwrap_err();
    // The redirect is refused (transport error from the policy), never
    // followed to the metadata service.
    let msg = err.to_string();
    assert!(
        msg.contains("network") || msg.contains("redirect"),
        "redirect must not be followed: {msg}"
    );
}

// ------------------------------------------------ MVP-2 A.2.2 drift-review pin

/// `wa session init --review-since-user` (MVP-2 A.2.2): usercontribs are
/// paged newest-first and filtered client-side (the API has no `uctitle`
/// filter — the first live run pinned the operator's most recent edit
/// ANYWHERE until this filtering landed). The newest matching entry is the
/// recorded revid; a user who never edited the title pins nothing (None).
#[tokio::test]
async fn review_since_user_records_revid_and_drift_summary() {
    let server = MockServer::start_async().await;
    // Page 2 (created first so the more-specific mock wins): the Sarah
    // Kidder entry, reached via the continuation cursor.
    let page2 = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("list", "usercontribs")
                .query_param("ucuser", "LuisVilla")
                .query_param("uccontinue", "2026|1370000000")
                .header("User-Agent", USER_AGENT);
            then.status(200).json_body(serde_json::json!({
                "query": {"usercontribs": [
                    {"revid": 8_900_001, "user": "LuisVilla", "title": "Sarah Kidder"}
                ]}
            }));
        })
        .await;
    // Page 1: a newer contribution to a DIFFERENT article plus the
    // continuation cursor — must not be pinned for Sarah Kidder.
    let page1 = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("list", "usercontribs")
                .query_param("ucuser", "LuisVilla")
                .query_param("uclimit", "500")
                .header("User-Agent", USER_AGENT);
            then.status(200).json_body(serde_json::json!({
                "query": {"usercontribs": [
                    {"revid": 1_376_716_598, "user": "LuisVilla", "title": "Temple Fielding"}
                ]},
                "continue": {"uccontinue": "2026|1370000000", "continue": "-||"}
            }));
        })
        .await;
    let wt_mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param("revids", "8900001")
                .query_param("maxlag", "5")
                .header("User-Agent", USER_AGENT);
            then.status(200).json_body(serde_json::json!({
                "query": {"pages": {"1": {"pageid": 1, "ns": 0, "title": "Sarah Kidder",
                    "revisions": [{"revid": 8_900_001, "slots": {"main": {"content": "She was a railroad president."}}}]}}}
            }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("ucuser", "NobodyElse")
                .header("User-Agent", USER_AGENT);
            then.status(200)
                .json_body(serde_json::json!({"query": {"usercontribs": []}}));
        })
        .await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .expect("connect");
    let revid = wiki
        .last_edit_revid("LuisVilla", "Sarah Kidder")
        .await
        .expect("usercontribs query");
    assert_eq!(
        revid,
        Some(8_900_001),
        "the pinned revid is the newest edit TO THE TITLE, not the user's newest edit"
    );
    let prior = wiki
        .wikitext_at_revid("Sarah Kidder", 8_900_001)
        .await
        .expect("wikitext at the pinned revid");
    assert_eq!(prior, "She was a railroad president.");
    assert_eq!(page1.calls(), 1);
    assert_eq!(page2.calls(), 1);
    assert_eq!(wt_mock.calls(), 1);

    // Never edited the title: no pin, no error.
    assert_eq!(
        wiki.last_edit_revid("NobodyElse", "Sarah Kidder")
            .await
            .expect("empty contribs is not an error"),
        None
    );
}

// ------------------------------------------------ plan-003 AC.5 z.ai client

use std::time::Duration;
use wikiloop::driver::model::ChatMessage;
use wikiloop::driver::model::ZaiClient;
use wikiloop::driver::model::ZaiError;

fn zai_ok_body() -> serde_json::Value {
    serde_json::json!({
        "choices": [{"finish_reason": "stop", "index": 0,
            "message": {"role": "assistant", "content": "{\"ok\":true}",
                        "reasoning_content": "thinking…"}}],
        "usage": {"total_tokens": 42}
    })
}

/// AC.5: bearer auth, model id in the body, identifying UA on the z.ai
/// call site, response parsed (content + reasoning + usage).
#[tokio::test]
async fn zai_chat_contract_shape() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .header("Authorization", "Bearer test-key")
                .header("User-Agent", USER_AGENT)
                .json_body(serde_json::json!({
                    "model": "glm-5.3",
                    "temperature": 0.2,
                    "messages": [
                        {"role": "system", "content": "sys"},
                        {"role": "user", "content": "ctx"}
                    ]
                }));
            then.status(200).json_body(zai_ok_body());
        })
        .await;

    let client =
        ZaiClient::with_base(&server.url(""), "glm-5.3", "test-key").with_retry_delays(vec![]);
    let resp = client
        .chat(&[ChatMessage::system("sys"), ChatMessage::user("ctx")], 0.2)
        .await
        .expect("chat succeeds");
    assert_eq!(resp.content, "{\"ok\":true}");
    assert_eq!(resp.reasoning.as_deref(), Some("thinking…"));
    assert_eq!(resp.total_tokens, Some(42));
    assert_eq!(mock.calls(), 1);
}

/// AC.5: a rate-limit 429 (no 1113) is retried per the schedule and then
/// succeeds. httpmock has no per-hit response sequences, so the reject
/// mock (created first — first match wins) is deleted after its first
/// hit; the generous retry delay makes the delete land first.
#[tokio::test]
async fn zai_backs_off_on_429_then_succeeds() {
    let server = MockServer::start_async().await;
    let reject = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(429)
                .header("Retry-After", "0")
                .json_body(serde_json::json!({"error": {"code": "1001", "message": "rate"}}));
        })
        .await;
    let accept = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(200).json_body(zai_ok_body());
        })
        .await;

    let client = ZaiClient::with_base(&server.url(""), "glm-5.3", "test-key")
        .with_retry_delays(vec![Duration::from_millis(250)]);
    let task = tokio::spawn(async move { client.chat(&[ChatMessage::user("x")], 0.2).await });
    while reject.calls() == 0 {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let reject_hits = reject.calls();
    reject.delete_async().await;
    let resp = task.await.expect("task joins").expect("retry succeeds");
    assert_eq!(resp.content, "{\"ok\":true}");
    assert_eq!(reject_hits, 1);
    assert_eq!(accept.calls(), 1);
}

/// AC.5 / B.0: 429 with error code 1113 (no resource package) is TERMINAL
/// — exactly one request, no retries, a [`ZaiError::NoPackage`] with the
/// Coding-Plan hint.
#[tokio::test]
async fn zai_no_package_429_is_terminal_not_retried() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(429).json_body(serde_json::json!({
                "error": {"code": "1113",
                          "message": "Insufficient balance or no resource package."}
            }));
        })
        .await;

    let client = ZaiClient::with_base(&server.url(""), "glm-5.3", "test-key")
        .with_retry_delays(vec![Duration::from_millis(1); 3]);
    let err = client
        .chat(&[ChatMessage::user("x")], 0.2)
        .await
        .expect_err("1113 is terminal");
    assert!(matches!(err, ZaiError::NoPackage { .. }));
    assert!(err.to_string().contains("ZAI_BASE_URL"));
    assert_eq!(mock.calls(), 1, "terminal 429 must not be retried");
}

/// AC.5: exhausted retries surface [`ZaiError::RateLimited`]; non-retryable
/// 4xx surfaces [`ZaiError::Http`].
#[tokio::test]
async fn zai_error_mapping() {
    let server = MockServer::start_async().await;
    let rate = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(429).json_body(serde_json::json!({
                "error": {"code": "1001", "message": "rate"}}));
        })
        .await;

    let client = ZaiClient::with_base(&server.url(""), "glm-5.3", "test-key")
        .with_retry_delays(vec![Duration::from_millis(1); 2]);
    let err = client
        .chat(&[ChatMessage::user("x")], 0.2)
        .await
        .expect_err("always 429");
    assert!(matches!(err, ZaiError::RateLimited { attempts: 3 }));
    assert_eq!(rate.calls(), 3);

    let server2 = MockServer::start_async().await;
    let unauthorized = server2
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(401).body("denied");
        })
        .await;
    let client2 = ZaiClient::with_base(&server2.url(""), "glm-5.3", "test-key")
        .with_retry_delays(vec![Duration::from_millis(1); 2]);
    let err2 = client2
        .chat(&[ChatMessage::user("x")], 0.2)
        .await
        .expect_err("401");
    assert!(matches!(err2, ZaiError::Http { status: 401, .. }));
    assert_eq!(unauthorized.calls(), 1, "4xx is not retried");
}
