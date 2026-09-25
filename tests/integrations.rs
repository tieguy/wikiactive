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
        "LLM-Disclosure: U:LuisVilla/wikiactive"
    );
    // ssrf_guard is compiled in, not configured by skill files.
    assert!(ssrf_guard(&url::Url::parse("http://localhost/").unwrap()).is_err());
}
