//! AC.7 / AC.10 — publish path safety against a mocked Action API:
//! tty-confirm absent → refuses; base revid moved → aborts with conflict;
//! summary always carries the disclosure suffix; `assert=user` on the edit;
//! disclosure-page log append is idempotent; dry-run validates without
//! posting.

use httpmock::MockServer;
use wikiloop::wikipedia::ConfirmSource;
use wikiloop::wikipedia::DenyConfirm;
use wikiloop::wikipedia::EditRequest;
use wikiloop::wikipedia::Wikipedia;
use wikiloop::wikipedia::WikipediaError;

/// A confirm source that answers yes (tests the happy path only).
struct Approve;
impl ConfirmSource for Approve {
    fn confirm<'a>(
        &'a mut self,
        _prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        Box::pin(std::future::ready(true))
    }
}

/// Mock GET for `wikitext_at_revid` (queries by revids only).
async fn mock_wikitext_at_revid(server: &MockServer, revid: u64, content: &str) {
    let revid_str = revid.to_string();
    server
        .mock_async(move |when, then| {
            when.method(httpmock::Method::GET)
                .query_param("revids", revid_str.clone());
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": [{
                    "pageid": 1,
                    "ns": 2,
                    "title": "User:LuisVilla/wikiactive",
                    "revisions": [{"revid": revid, "slots": {"main": {"content": content}}}]
                }]}
            }));
        })
        .await;
}
/// Mock GET for the csrf token fetch (`post_with_token` step).
async fn mock_csrf_token(server: &MockServer) {
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("meta", "tokens")
                .query_param("type", "csrf");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"tokens": {"csrftoken": "+\\"}}
            }));
        })
        .await;
}

/// Mock GET that answers `current_revid` for the given page.
async fn mock_revid<'a>(server: &'a MockServer, revid: u64, title: &str) -> httpmock::Mock<'a> {
    let title = title.to_string();
    server
        .mock_async(move |when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param("titles", title.clone());
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": [{"pageid": 1, "ns": 0, "title": title,
                    "revisions": [{"revid": revid, "slots": {"main": {"content": "old wikitext"}}}]}]}
            }));
        })
        .await
}

/// Mock GET+POST pair for a successful edit (matchers assert the body shape).
async fn mock_edit_ok(server: &MockServer) -> httpmock::Mock<'_> {
    mock_csrf_token(server).await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .body_includes("action=edit")
                .body_includes("notminor=1")
                .body_includes("assert=user")
                .body_includes("LLM-Disclosure%3A+%5B%5BUser%3ALuisVilla%2Fwikiactive%5D%5D")
                .body_includes("baserevid=500");
            then.status(200).json_body(serde_json::json!({
                "edit": {"result": "Success", "newrevid": 501}
            }));
        })
        .await
}

#[tokio::test]
async fn absent_confirmation_refuses_without_edit() {
    let server = MockServer::start_async().await;
    let revid_mock = mock_revid(&server, 500, "User:LuisVilla/wikiactive/smoke").await;
    let edit_mock = mock_edit_ok(&server).await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let req = EditRequest {
        title: "User:LuisVilla/wikiactive/smoke",
        base_revid: 500,
        wikitext: "new text",
        summary: "Smoke test edit",
        review_artifact: None,
        dry_run: false,
    };
    let mut absent = DenyConfirm;
    let err = wiki.edit(req, &mut absent).await.unwrap_err();
    assert!(matches!(err, WikipediaError::Declined), "{err}");
    assert_eq!(edit_mock.calls(), 0, "no edit may be posted");
    assert_eq!(revid_mock.calls(), 1, "pre-check happened");
}

#[tokio::test]
async fn test_stale_base_revid_aborts() {
    let server = MockServer::start_async().await;
    // Current revid (505) has moved past the pinned base (500).
    mock_revid(&server, 505, "Article").await;
    let edit_mock = mock_edit_ok(&server).await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let req = EditRequest {
        title: "Article",
        base_revid: 500,
        wikitext: "new text",
        summary: "edit",
        review_artifact: None,
        dry_run: false,
    };
    let mut approve = Approve;
    let err = wiki.edit(req, &mut approve).await.unwrap_err();
    assert!(
        matches!(
            err,
            WikipediaError::EditConflict {
                base: 500,
                current: 505
            }
        ),
        "{err}"
    );
    assert_eq!(edit_mock.calls(), 0, "conflict must never write");
}

#[tokio::test]
async fn edit_carries_summary_suffix_and_assert_user() {
    let server = MockServer::start_async().await;
    mock_revid(&server, 500, "User:LuisVilla/wikiactive/smoke").await;
    // The mock's matchers ARE the assertion: body must contain the encoded
    // disclosure suffix, assert=user, and baserevid. A request missing any
    // of them will not match (no 200) and the edit fails the test.
    let edit_mock = mock_edit_ok(&server).await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let req = EditRequest {
        title: "User:LuisVilla/wikiactive/smoke",
        base_revid: 500,
        wikitext: "new text",
        summary: "Smoke test edit",
        review_artifact: None,
        dry_run: false,
    };
    let mut approve = Approve;
    let outcome = wiki.edit(req, &mut approve).await.expect("edit succeeds");
    assert_eq!(outcome.new_revid, 501);
    assert!(
        outcome.diff_url.contains("diff=501"),
        "{}",
        outcome.diff_url
    );
    assert_eq!(edit_mock.calls(), 1);
}

#[tokio::test]
async fn bare_summary_refused_before_any_request() {
    let server = MockServer::start_async().await;
    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let req = EditRequest {
        title: "Article",
        base_revid: 500,
        wikitext: "text",
        summary: "   ",
        review_artifact: None,
        dry_run: false,
    };
    let mut approve = Approve;
    let err = wiki.edit(req, &mut approve).await.unwrap_err();
    assert!(matches!(err, WikipediaError::BareSummary), "{err}");
}

#[tokio::test]
async fn dry_run_validates_but_never_posts() {
    let server = MockServer::start_async().await;
    mock_revid(&server, 500, "User:LuisVilla/wikiactive/smoke").await;
    let edit_mock = mock_edit_ok(&server).await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let req = EditRequest {
        title: "User:LuisVilla/wikiactive/smoke",
        base_revid: 500,
        wikitext: "new text",
        summary: "Smoke",
        review_artifact: None,
        dry_run: true,
    };
    let mut approve = Approve;
    wiki.edit(req, &mut approve).await.expect("dry run ok");
    assert_eq!(edit_mock.calls(), 0, "dry run never posts");
}

#[tokio::test]
async fn disclosure_log_append_is_idempotent() {
    let server = MockServer::start_async().await;
    // Page exists and already contains the session marker.
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param_exists("titles")
                .query_param("rvprop", "ids");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": [{
                    "pageid": 1,
                    "ns": 2,
                    "title": "User:LuisVilla/wikiactive",
                    "revisions": [{"revid": 900, "slots": {"main": {
                        "content": "== Sessions ==\n<!-- wa-session:2026-09-24-cd -->\nalready logged"
                    }}}]
                }]}
            }));
        })
        .await;
    let edit_mock = mock_edit_ok(&server).await;
    mock_wikitext_at_revid(
        &server,
        900,
        "== Sessions ==\n<!-- wa-session:2026-09-24-cd -->\nalready logged",
    )
    .await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let mut approve = Approve;
    let appended = wiki
        .append_disclosure_log(
            "User:LuisVilla/wikiactive",
            "<!-- wa-session:2026-09-24-cd -->\n* 2026-09-24 Commitment device (3 edits)",
            "wa-session:2026-09-24-cd",
            &mut approve,
        )
        .await
        .expect("idempotent append");
    assert!(appended.is_none(), "marker present: no second append");
    assert_eq!(edit_mock.calls(), 0);
}

#[tokio::test]
async fn disclosure_log_appends_when_marker_absent() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param_exists("titles")
                .query_param("rvprop", "ids");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": [{
                    "pageid": 1,
                    "ns": 2,
                    "title": "User:LuisVilla/wikiactive",
                    "revisions": [{"revid": 900, "slots": {"main": {
                        "content": "== Sessions =="
                    }}}]
                }]}
            }));
        })
        .await;
    mock_csrf_token(&server).await;
    mock_wikitext_at_revid(&server, 900, "== Sessions ==").await;
    // The append edit posts to the disclosure page at its current revid.
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .body_includes("action=edit")
                .body_includes("baserevid=900")
                .body_includes("LLM-Disclosure");
            then.status(200).json_body(serde_json::json!({
                "edit": {"result": "Success", "newrevid": 901}
            }));
        })
        .await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let mut approve = Approve;
    let appended = wiki
        .append_disclosure_log(
            "User:LuisVilla/wikiactive",
            "<!-- wa-session:new -->\n* entry",
            "wa-session:new",
            &mut approve,
        )
        .await
        .expect("append");
    let outcome = appended.expect("Some on append");
    assert_eq!(outcome.new_revid, 901);
    assert!(outcome.permalink().ends_with("901"));
}

/// Userspace smoke: base revid 0 means create — no baserevid/nocreate
/// params, no currency pre-check, one POST.
#[tokio::test]
async fn create_from_base_zero_posts_without_baserevid() {
    let server = MockServer::start_async().await;
    mock_csrf_token(&server).await;
    let edit_mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .body_includes("action=edit")
                .body_includes("assert=user")
                .body_includes("LLM-Disclosure%3A+%5B%5BUser%3ALuisVilla%2Fwikiactive%5D%5D")
                .body_excludes("baserevid")
                .body_excludes("nocreate");
            then.status(200).json_body(serde_json::json!({
                "edit": {"result": "Success", "newrevid": 42, "new": ""}
            }));
        })
        .await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let req = EditRequest {
        title: "User:LuisVilla/wikiactive/smoke",
        base_revid: 0,
        wikitext: "smoke test page content",
        summary: "wikiactive smoke test",
        review_artifact: Some("sessions/user-luisvilla-wikiactive-smoke/review.html"),
        dry_run: false,
    };
    let mut approve = Approve;
    let outcome = wiki.edit(req, &mut approve).await.expect("create ok");
    assert_eq!(outcome.new_revid, 42);
    assert_eq!(edit_mock.calls(), 1);
}

/// A successful save of identical content returns result=Success with no
/// newrevid: the tool reports a no-change outcome, not a shape error.
#[tokio::test]
async fn null_edit_reports_no_change() {
    let server = MockServer::start_async().await;
    mock_csrf_token(&server).await;
    mock_revid(&server, 700, "Article").await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .body_includes("action=edit");
            then.status(200).json_body(serde_json::json!({
                "edit": {"result": "Success", "nochange": true}
            }));
        })
        .await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let req = EditRequest {
        title: "Article",
        base_revid: 700,
        wikitext: "identical text",
        summary: "same again",
        review_artifact: None,
        dry_run: false,
    };
    let mut approve = Approve;
    let outcome = wiki.edit(req, &mut approve).await.expect("null edit ok");
    assert_eq!(outcome.new_revid, 700, "base stands; no revision created");
    assert!(outcome.diff_url.is_empty());
}

/// The marker rides in the entry text (invisible comment) so idempotency
/// survives hand-edited log pages.
#[tokio::test]
async fn disclosure_log_embeds_marker_in_entry() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param_exists("titles");
            then.status(200).json_body(serde_json::json!({
                "query": {"pages": [{"pageid": 1, "ns": 2,
                    "title": "User:LuisVilla/wikiactive/log",
                    "revisions": [{"revid": 900, "slots": {"main": {"content": "== Log =="}}}]}
                ]}
            }));
        })
        .await;
    mock_wikitext_at_revid(&server, 900, "== Log ==").await;
    mock_csrf_token(&server).await;
    let edit_mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .body_includes("action=edit")
                // Encoding-agnostic: the marker comment and entry text ride in
                // the posted wikitext in any form-encoding.
                .body_includes("wa-session")
                .body_includes("2026");
            then.status(200).json_body(serde_json::json!({
                "edit": {"result": "Success", "newrevid": 901}
            }));
        })
        .await;

    let wiki = Wikipedia::connect_with_api_url(&server.url("/"), None)
        .await
        .unwrap();
    let mut approve = Approve;
    let marked = "* 2026 entry";
    let out = wiki
        .append_disclosure_log(
            "User:LuisVilla/wikiactive/log",
            marked,
            "wa-session:test",
            &mut approve,
        )
        .await
        .expect("append")
        .expect("Some");
    assert_eq!(out.new_revid, 901);
    assert_eq!(edit_mock.calls(), 1);
}
