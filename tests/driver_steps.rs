//! plan-003 AC.6 — driver pipeline steps: model output is schema-validated
//! exactly like `wa findings add`; malformed output is retried exactly once
//! (with a corrective message) then blocked; fabricated evidence ids (the
//! gate-bypass attempt) are blocked. All against httpmock with the real
//! prompt templates rendered into the wire body.

use std::time::Duration;

use httpmock::MockServer;
use wikiloop::driver::model::ZaiClient;
use wikiloop::driver::steps::AssessContext;
use wikiloop::driver::steps::DriverComment;
use wikiloop::driver::steps::SourceDigest;
use wikiloop::driver::steps::StepError;
use wikiloop::driver::steps::assess;
use wikiloop::driver::steps::draft_proposal;
use wikiloop::driver::steps::resolve_comments;
use wikiloop::driver::steps::review_draft;
use wikiloop::session::Assessment;

/// Wrap model `content` in the chat-completions response shape.
fn completion(content: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{"finish_reason": "stop", "index": 0,
            "message": {"role": "assistant", "content": content}}]
    })
}

fn client(server: &MockServer) -> ZaiClient {
    ZaiClient::with_base(&server.url(""), "glm-5.3", "test-key").with_retry_delays(vec![])
}

/// The guidance the serve handlers pass (rule-enforcement item 3):
/// tier-1 verbatim + this loop's cards, from the real corpus.
fn guidance() -> String {
    let corpus = wikiloop::rules::RulesCorpus::load(std::path::Path::new("rules")).unwrap();
    wikiloop::rules::guidance_for_loop(&corpus, 2).unwrap()
}

fn ctx() -> AssessContext {
    // Real corpus guidance: what the serve handlers pass — tier-1
    // verbatim + this loop's cards.
    AssessContext {
        article: "Sarah Kidder".into(),
        base_wikitext: "Born Sarah A. Clark in Ohio, Kidder married John Flint Kidder in 1874."
            .into(),
        sources: vec![SourceDigest {
            id: "S1".into(),
            title: "SF Call obituary".into(),
            text: "…was married in 1874…".into(),
            quotes: vec![(
                "Q1".into(),
                "was married in 1874 to Miss S. L. A. Clark".into(),
            )],
        }],
        quote_ids: vec!["Q1".into()],
        entry_loop: 2,
        max_assessments: 2,
        guidance: guidance(),
    }
}

const VALID_FINDINGS: &str = r#"```json
[{"id":"AS1","wikitext_anchor":"L1:C0-L1:C74","rules":["WP:V"],
  "evidence":["Q1"],"factual_note":"The obituary supports 1874.",
  "proposed_fix":"Cite the marriage year to the obituary.","loop":2}]
```"#;

/// Happy path: fenced valid findings parse, validate, and the rendered
/// prompt template reached the wire (the mock only matches a body that
/// carries the filled slots and the ledger quote-id list).
#[tokio::test]
async fn assess_accepts_valid_output() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("at most 2") // max_assessments slot
                .body_includes("Loop 2 discipline") // rendered template body
                .body_includes("Cluster A") // tier-1 guidance rides the prompt
                .body_includes("Registered quote ids (evidence must cite among these): Q1");
            then.status(200).json_body(completion(VALID_FINDINGS));
        })
        .await;

    let findings = assess(&client(&server), &ctx())
        .await
        .expect("valid output");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].id, "AS1");
    assert_eq!(findings[0].evidence, vec!["Q1".to_string()]);
    assert_eq!(mock.calls(), 1);
}

/// AC.6: malformed output is retried exactly once with a corrective
/// message, then the step is blocked — never waved through.
#[tokio::test]
async fn malformed_output_retried_once_then_blocked() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(200)
                .json_body(completion("I think the best finding here is AS1 because…"));
        })
        .await;

    let err = assess(&client(&server), &ctx())
        .await
        .expect_err("garbage stays garbage");
    match err {
        StepError::Malformed { step, problems } => {
            assert_eq!(step, "assess");
            assert!(problems.contains("not valid JSON"), "{problems}");
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
    assert_eq!(mock.calls(), 2, "exactly one corrective retry");
}

/// AC.6: the retry is a real recovery path — garbage once, then valid.
#[tokio::test]
async fn malformed_then_valid_recovers() {
    let server = MockServer::start_async().await;
    let garbage = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            // Response delay = the deterministic race window: the request
            // is counted on arrival, so the test sees the hit and deletes
            // this mock while the (garbage) response is still in flight;
            // the corrective retry then matches only the good mock. The
            // corrective call itself is immediate — no delay to lean on
            // (the prior 750ms retry-delay fix covered transport retries,
            // not this path; the test flaked ~1-in-5 in isolation).
            then.status(200)
                .delay(Duration::from_millis(300))
                .json_body(completion("sure, here are my thoughts:"));
        })
        .await;
    let good = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                // The corrective retry carries the rejected output as an
                // assistant turn — pin it on the wire.
                .body_includes("sure, here are my thoughts:");
            then.status(200).json_body(completion(VALID_FINDINGS));
        })
        .await;

    // The step runs in a task; after the garbage hit we delete that mock
    // so the retry matches the good one (httpmock: first match wins). The
    // good mock REQUIRES the rejected output in the request body: the
    // corrective retry must carry the model's prior turn (review finding:
    // a context-less retry just repeats the failure).
    // Own client with a REAL retry delay: the retry must not fire before
    // the garbage mock is deleted (the shared zero-delay client made this
    // racy under full-suite load).
    let c = ZaiClient::with_base(&server.url(""), "glm-5.3", "test-key")
        .with_retry_delays(vec![Duration::from_millis(750)]);
    let task = tokio::spawn(async move { assess(&c, &ctx()).await });
    while garbage.calls() == 0 {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let garbage_hits = garbage.calls();
    garbage.delete_async().await;
    let findings = task.await.expect("joins").expect("recovers on retry");
    assert_eq!(findings.len(), 1);
    assert_eq!(garbage_hits, 1);
    assert_eq!(good.calls(), 1);
}

/// AC.6: the gate-bypass attempt — well-formed findings citing a quote id
/// that does not exist in the ledger — is blocked after the retry.
#[tokio::test]
async fn fabricated_quote_id_is_blocked() {
    let server = MockServer::start_async().await;
    let fabricated = VALID_FINDINGS.replace("\"Q1\"", "\"Q99\"");
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(200).json_body(completion(&fabricated));
        })
        .await;

    let err = assess(&client(&server), &ctx())
        .await
        .expect_err("Q99 is not in the ledger");
    match err {
        StepError::Malformed { step, problems } => {
            assert_eq!(step, "assess");
            assert!(problems.contains("Q99"), "{problems}");
            assert!(
                problems.contains("not a registered ledger quote"),
                "{problems}"
            );
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
    assert_eq!(mock.calls(), 2);
}

/// `draft_proposal`: valid shape parses; schema violations (empty summary)
/// retry then block.
#[tokio::test]
async fn draft_proposal_contract() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("Cluster A"); // tier-1 guidance rides the prompt
            then.status(200).json_body(completion(
                r#"{"proposed_wikitext_block":"A [[ref]] here.","edit_summary":"Cite the year."}"#,
            ));
        })
        .await;
    let finding = Assessment {
        id: "AS1".into(),
        wikitext_anchor: "L1:C0-L1:C74".into(),
        rendered_span_id: None,
        rules: vec!["WP:V".into()],
        evidence: vec!["Q1".into()],
        factual_note: "n".into(),
        proposed_fix: "f".into(),
        loop_id: 2,
    };
    let p = draft_proposal(
        &client(&server),
        &finding,
        &[],
        "base block",
        &["sfcall".into()],
        &guidance(),
    )
    .await
    .expect("parses");
    assert_eq!(p.edit_summary, "Cite the year.");
    assert_eq!(mock.calls(), 1);

    let server2 = MockServer::start_async().await;
    let bad = server2
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(200).json_body(completion(
                r#"{"proposed_wikitext_block":"ok","edit_summary":"  "}"#,
            ));
        })
        .await;
    let err = draft_proposal(
        &client(&server2),
        &finding,
        &[],
        "base block",
        &[],
        &guidance(),
    )
    .await
    .expect_err("empty summary is invalid");
    assert!(err.to_string().contains("edit_summary"), "{err}");
    assert_eq!(bad.calls(), 2, "retried once then blocked");
}

/// Rule-enforcement item 5: the rule-review schema is validated with the
/// quote-anchor discipline — an invented clause, a span that does not
/// locate verbatim in any block, and an empty note each reject the
/// output (one corrective retry, then blocked).
#[tokio::test]
async fn review_draft_rejects_unknown_clause_unlocating_span_empty_note() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(200).json_body(completion(
                r#"[{"clause":"Z9","verdict":"concern","span":"nowhere in the text","note":""}]"#,
            ));
        })
        .await;
    let clauses = {
        let corpus = wikiloop::rules::RulesCorpus::load(std::path::Path::new("rules")).unwrap();
        wikiloop::rules::guidance_clauses(&corpus, 2)
    };
    let blocks = vec![wikiloop::driver::steps::ReviewBlock {
        element_id: "wa-1".into(),
        base: "The tower is old.".into(),
        proposed: "The tower is ancient.".into(),
        evidence: vec![],
    }];
    let err = review_draft(&client(&server), &guidance(), &clauses, &blocks)
        .await
        .expect_err("invalid output stays rejected");
    match err {
        StepError::Malformed { step, problems } => {
            assert_eq!(step, "review_draft");
            assert!(
                problems.contains("not one the guidance names"),
                "{problems}"
            );
            assert!(problems.contains("not verbatim"), "{problems}");
            assert!(problems.contains("non-empty note"), "{problems}");
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
    assert_eq!(mock.calls(), 2, "exactly one corrective retry");
}

/// Rule-enforcement item 5 happy path: a concern whose clause the
/// guidance names and whose span locates verbatim parses through — the
/// guidance demonstrably rides the prompt.
#[tokio::test]
async fn review_draft_accepts_a_locating_concern() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("Cluster A"); // tier-1 guidance rides the prompt
            then.status(200).json_body(completion(
                r#"[{"clause":"A3","verdict":"concern","span":"widely considered","note":"WEASEL wording needs attribution."}]"#,
            ));
        })
        .await;
    let clauses = {
        let corpus = wikiloop::rules::RulesCorpus::load(std::path::Path::new("rules")).unwrap();
        wikiloop::rules::guidance_clauses(&corpus, 2)
    };
    let blocks = vec![wikiloop::driver::steps::ReviewBlock {
        element_id: "wa-1".into(),
        base: "The tower is old.".into(),
        proposed: "Critics have widely considered the tower the finest.".into(),
        evidence: vec![],
    }];
    let concerns = review_draft(&client(&server), &guidance(), &clauses, &blocks)
        .await
        .expect("valid concerns");
    assert_eq!(concerns.len(), 1);
    assert_eq!(concerns[0].clause, "A3");
    assert_eq!(
        concerns[0].verdict,
        wikiloop::driver::steps::ConcernVerdict::Concern
    );
    assert_eq!(mock.calls(), 1);
}

/// `resolve_comments`: valid shape parses; unknown fields are schema
/// violations (`deny_unknown_fields`) — a model inventing its own shape is
/// blocked.
#[tokio::test]
async fn resolve_comments_contract() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("Cluster A"); // tier-1 guidance rides the prompt
            then.status(200).json_body(completion(
                r#"{"proposed_wikitext_block":"revised","applied":["tightened"],"rejected":[],
                    "reply":"done"}"#,
            ));
        })
        .await;
    let comments = vec![DriverComment {
        prompt: "tighten".into(),
        anchor: "L1:C0-L1:C5".into(),
    }];
    let r = resolve_comments(&client(&server), "proposed", "base", &comments, &guidance())
        .await
        .expect("parses");
    assert_eq!(r.applied, vec!["tightened".to_string()]);
    assert_eq!(mock.calls(), 1);

    let server2 = MockServer::start_async().await;
    let invented = server2
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(200).json_body(completion(
                r#"{"proposed_wikitext_block":"x","reply":"y","notes":"invented field"}"#,
            ));
        })
        .await;
    let err = resolve_comments(
        &client(&server2),
        "proposed",
        "base",
        &comments,
        &guidance(),
    )
    .await
    .expect_err("invented fields are schema violations");
    assert!(err.to_string().contains("malformed"), "{err}");
    assert_eq!(invented.calls(), 2);
}
