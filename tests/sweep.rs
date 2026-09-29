//! plan-003 AC.2/AC.3 — the source sweep. AC.2: the frozen pre-session
//! Kidder fixture yields every distinct cited source (hand-enumerated
//! manifest beside it), with URL-less books auto-dispositioned. AC.3:
//! httpmock classification into `fetched` / `needs_operator` /
//! `snapshot_available` / `no_text` (403, marker-configured paywall, CDX
//! availability), with the CDX client's request shape/UA pinned per the
//! Earwig/SPN pattern.

use httpmock::MockServer;
use wikiloop::USER_AGENT;
use wikiloop::ledger::Ledger;
use wikiloop::ledger::net::CdxClient;
use wikiloop::ledger::net::SourceFetcher;
use wikiloop::sweep::SweepConfig;
use wikiloop::sweep::parse_citations;
use wikiloop::sweep::status;
use wikiloop::sweep::sweep_fetch_one;

fn fixture(path: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/fixtures/sweep/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

// ------------------------------------------------------------------- AC.2

/// The frozen Kidder base yields EXACTLY the hand-enumerated manifest:
/// every distinct URL (the dead True West link resolved to its citation's
/// own archive-url), plus the URL-less book — nothing missed, nothing
/// invented.
#[test]
fn kidder_fixture_yields_the_expected_manifest() {
    let wikitext = fixture("sarah-kidder@1370213000.wikitext");
    let manifest: serde_json::Value =
        serde_json::from_str(&fixture("sarah-kidder@1370213000.expected.json")).unwrap();
    let candidates = parse_citations(&wikitext);
    let keys: Vec<String> = candidates.iter().map(|c| c.ledger_url().unwrap()).collect();

    assert_eq!(
        candidates.len(),
        usize::try_from(manifest["expected_count"].as_u64().unwrap_or(u64::MAX))
            .expect("count fits"),
        "candidate count matches the manifest"
    );
    for s in manifest["sources"].as_array().unwrap() {
        let expected_key = s["url"]
            .as_str()
            .map(str::to_string)
            .or_else(|| s["isbn"].as_str().map(|i| format!("isbn:{i}")))
            .unwrap();
        assert!(
            keys.contains(&expected_key),
            "manifest source missing from inventory: {expected_key}"
        );
    }
    // Nothing invented: every inventory key is on the manifest.
    for key in &keys {
        let on_manifest = manifest["sources"].as_array().unwrap().iter().any(|s| {
            if s["url"].as_str() == Some(key.as_str()) {
                return true;
            }
            s["isbn"].as_str().map(|i| format!("isbn:{i}")) == Some(key.clone())
        });
        assert!(on_manifest, "inventory invented a source: {key}");
    }

    // The dead link resolves to its archive-url as the fetch target.
    let dead = candidates
        .iter()
        .find(|c| {
            c.url
                .as_deref()
                .is_some_and(|u| u.contains("web.archive.org"))
        })
        .expect("dead-original candidate");
    assert!(
        dead.dead_original
            .as_deref()
            .is_some_and(|u| u.contains("truewestmagazine.com"))
    );
}

/// Inventory → ledger: 13 URL sources pending, the URL-less book
/// auto-dispositioned `print: no web text` (the gate never dead-ends on
/// non-web sources), and re-inventory is a no-op (dedupe by URL).
#[test]
fn inventory_registers_pending_with_print_auto_disposition() {
    let wikitext = fixture("sarah-kidder@1370213000.wikitext");
    let candidates = parse_citations(&wikitext);
    let mut ledger = Ledger::default();
    let mut added = 0;
    for c in &candidates {
        let (_, was_added) = ledger.register_sweep_source(&c.ledger_url().unwrap(), None);
        added += usize::from(was_added);
    }
    assert_eq!(added, 14);
    // Re-inventory: nothing new.
    for c in &candidates {
        let (_, was_added) = ledger.register_sweep_source(&c.ledger_url().unwrap(), None);
        assert!(!was_added);
    }
    assert_eq!(ledger.sources.len(), 14);

    let pending = ledger
        .sources
        .iter()
        .filter(|s| s.sweep_status.as_deref() == Some(status::PENDING))
        .count();
    assert_eq!(pending, 13, "URL sources pending");
    let book = ledger
        .sources
        .iter()
        .find(|s| s.url == "isbn:0961526106")
        .expect("URL-less book registered");
    assert_eq!(book.disposition.as_deref(), Some("print: no web text"));
    // The auto-dispositioned book is NOT unresolved; the 13 pending are.
    assert_eq!(ledger.sweep_unresolved().len(), 13);
}

// ------------------------------------------------------------------- AC.3

fn ledger_with(url: &str) -> (Ledger, String) {
    let mut ledger = Ledger::default();
    let (id, _) = ledger.register_sweep_source(url, None);
    (ledger, id)
}

fn fetcher() -> SourceFetcher {
    SourceFetcher::with_allow_local().expect("test fetcher")
}

#[tokio::test]
async fn classification_fetched_on_clean_text() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/page");
            then.status(200)
                .body(format!("Article body. {}", "sentence. ".repeat(60)));
        })
        .await;
    let (mut ledger, id) = ledger_with(&server.url("/page"));
    let cdx = CdxClient::with_base("http://127.0.0.1:9/cdx"); // unreachable: not consulted
    let out = sweep_fetch_one(&fetcher(), &cdx, &mut ledger, &id, &SweepConfig::default())
        .await
        .expect("classifies");
    assert_eq!(out.status, status::FETCHED);
    assert!(
        ledger
            .source_text(&id)
            .is_some_and(|t| t.contains("sentence"))
    );
    assert!(ledger.sweep_unresolved().is_empty());
}

#[tokio::test]
async fn classification_403_is_needs_operator() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/blocked");
            then.status(403).body("forbidden");
        })
        .await;
    let (mut ledger, id) = ledger_with(&server.url("/blocked"));
    let cdx = CdxClient::with_base("http://127.0.0.1:9/cdx");
    let out = sweep_fetch_one(&fetcher(), &cdx, &mut ledger, &id, &SweepConfig::default())
        .await
        .expect("classifies");
    assert_eq!(out.status, status::NEEDS_OPERATOR);
    assert!(out.note.as_deref().is_some_and(|n| n.contains("403")));
    assert_eq!(
        ledger.sweep_unresolved().len(),
        1,
        "still demands resolution"
    );
}

/// Marker behavior only: the marker string comes from the CONFIG, not an
/// open-ended heuristic — and text without the configured marker passes.
#[tokio::test]
async fn classification_paywall_marker_from_config() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/paywalled");
            then.status(200).body(
                "<html><body>Subscribe Today for unlimited access to this story and everything else we publish.</body></html>",
            );
        })
        .await;
    let cfg = SweepConfig {
        paywall_markers: vec!["subscribe today".into()],
        lending_markers: vec![],
    };
    let (mut ledger, id) = ledger_with(&server.url("/paywalled"));
    let cdx = CdxClient::with_base("http://127.0.0.1:9/cdx");
    let out = sweep_fetch_one(&fetcher(), &cdx, &mut ledger, &id, &cfg)
        .await
        .expect("classifies");
    assert_eq!(out.status, status::NEEDS_OPERATOR);
    assert!(
        out.note
            .as_deref()
            .is_some_and(|n| n.contains("paywall marker"))
    );

    // Marker behavior only, both directions: marker-free text with the
    // default config is plain fetched (no open-ended heuristic).
    let clean = MockServer::start_async().await;
    clean
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/plain");
            then.status(200)
                .body(format!("<p>{}</p>", "Ordinary article prose. ".repeat(40)));
        })
        .await;
    let (mut ledger2, id2) = ledger_with(&clean.url("/plain"));
    let out2 = sweep_fetch_one(
        &fetcher(),
        &cdx,
        &mut ledger2,
        &id2,
        &SweepConfig::default(),
    )
    .await
    .expect("classifies");
    assert_eq!(out2.status, status::FETCHED);
}

#[tokio::test]
async fn classification_dead_link_gets_cdx_snapshot() {
    let page = MockServer::start_async().await;
    page.mock_async(|when, then| {
        when.method(httpmock::Method::GET).path("/gone");
        then.status(404).body("not found");
    })
    .await;
    let cdx = MockServer::start_async().await;
    cdx.mock_async(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/cdx/search/cdx")
            .query_param("url", page.url("/gone"))
            .query_param("output", "json")
            .query_param("filter", "statuscode:200")
            .header("User-Agent", USER_AGENT);
        then.status(200).json_body(serde_json::json!([
            ["timestamp", "original"],
            ["20130202035759", "http://gone.example/original"]
        ]));
    })
    .await;

    let (mut ledger, id) = ledger_with(&page.url("/gone"));
    let client = CdxClient::with_base(&cdx.url("/cdx/search/cdx"));
    let out = sweep_fetch_one(
        &fetcher(),
        &client,
        &mut ledger,
        &id,
        &SweepConfig::default(),
    )
    .await
    .expect("classifies");
    assert_eq!(out.status, status::SNAPSHOT_AVAILABLE);
    let entry = ledger.sources.iter().find(|s| s.id == id).unwrap();
    assert_eq!(
        entry
            .metadata
            .as_ref()
            .and_then(|m| m.snapshot_url.as_deref()),
        Some("https://web.archive.org/web/20130202035759/http://gone.example/original")
    );
}

#[tokio::test]
async fn classification_dead_without_snapshot_needs_operator() {
    let page = MockServer::start_async().await;
    page.mock_async(|when, then| {
        when.method(httpmock::Method::GET).path("/gone2");
        then.status(404).body("not found");
    })
    .await;
    let cdx = MockServer::start_async().await;
    cdx.mock_async(|when, then| {
        when.method(httpmock::Method::GET).path("/cdx/search/cdx");
        then.status(200).body("");
    })
    .await;

    let (mut ledger, id) = ledger_with(&page.url("/gone2"));
    let client = CdxClient::with_base(&cdx.url("/cdx/search/cdx"));
    let out = sweep_fetch_one(
        &fetcher(),
        &client,
        &mut ledger,
        &id,
        &SweepConfig::default(),
    )
    .await
    .expect("classifies");
    assert_eq!(out.status, status::NEEDS_OPERATOR);
}

#[tokio::test]
async fn classification_no_text_on_shell() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/pdf-shell");
            then.status(200).body("%PDF-1.4 binary-ish");
        })
        .await;
    let (mut ledger, id) = ledger_with(&server.url("/pdf-shell"));
    let cdx = CdxClient::with_base("http://127.0.0.1:9/cdx");
    let out = sweep_fetch_one(&fetcher(), &cdx, &mut ledger, &id, &SweepConfig::default())
        .await
        .expect("classifies");
    assert_eq!(out.status, status::NO_TEXT);
}

/// The CDX client's contract per the Earwig/SPN pattern: UA, request
/// shape, and the no-snapshot case is `None`, not an error.
#[tokio::test]
async fn cdx_client_contract() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/cdx/search/cdx")
                .query_param("url", "http://x.example/a")
                .query_param("output", "json")
                .query_param("fl", "timestamp,original")
                .query_param("limit", "1")
                .header("User-Agent", USER_AGENT);
            then.status(200).json_body(serde_json::json!([
                ["timestamp", "original", "statuscode"],
                ["20010101000000", "http://x.example/a", "200"]
            ]));
        })
        .await;
    let cdx = CdxClient::with_base(&server.url("/cdx/search/cdx"));
    let snap = cdx
        .closest_snapshot("http://x.example/a")
        .await
        .expect("ok")
        .expect("snapshot found");
    assert_eq!(
        snap,
        "https://web.archive.org/web/20010101000000/http://x.example/a"
    );
    assert_eq!(mock.calls(), 1);
}

#[tokio::test]
async fn cdx_no_capture_is_none_not_error() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/cdx/search/cdx");
            then.status(200).body("");
        })
        .await;
    let cdx = CdxClient::with_base(&server.url("/cdx/search/cdx"));
    assert!(
        cdx.closest_snapshot("http://never-archived.example/")
            .await
            .unwrap()
            .is_none()
    );
}

/// The config file and the compiled defaults are pinned together (the
/// paraphrase/linter pattern): live tuning is config-only.
#[test]
fn sweep_toml_matches_default_markers() {
    let cfg = SweepConfig::load(std::path::Path::new("rules/sweep.toml")).expect("loads");
    assert_eq!(cfg, SweepConfig::default());
}
