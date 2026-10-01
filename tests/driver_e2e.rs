//! plan-003 AC.7 — offline end-to-end driver session: the full pipeline
//! (sweep inventory → fetch/classify → operator attach + disposition →
//! model-authored findings → drafted proposal → gate → render → publish
//! through a SIMULATED confirmation) completes against fakes: the z.ai
//! model is an httpmock returning fixture-shaped completions, the wiki is
//! an httpmock, and Parsoid HTML comes from the recorded fixtures. The
//! same loop with a declining confirm source publishes NOTHING.
//!
//! One test, one process: cargo runs each test file as its own binary, so
//! the working-directory switch below cannot race other suites.

use std::path::Path;
use std::time::Duration;

use httpmock::MockServer;
use wikiloop::checks::gate::GateInput;
use wikiloop::checks::gate::run_gate;
use wikiloop::driver::model::ZaiClient;
use wikiloop::driver::steps::AssessContext;
use wikiloop::driver::steps::SourceDigest;
use wikiloop::driver::steps::assess;
use wikiloop::driver::steps::draft_proposal;
use wikiloop::ledger::Ledger;
use wikiloop::rules::RulesCorpus;
use wikiloop::sweep::SweepConfig;
use wikiloop::sweep::parse_citations;
use wikiloop::sweep::sweep_fetch_one;
use wikiloop::wikipedia::ConfirmSource;
use wikiloop::wikipedia::Wikipedia;

const BASE_WIKITEXT: &str = "The tower is old.\n\n==Sources==\n<ref name=\"a\">{{cite web|url=SRC1_URL|title=Tower History|website=Example}}</ref>\n<ref name=\"b\">{{cite web|url=SRC2_URL|title=Visitor Guide|website=Example}}</ref>\n";

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

struct ApproveSim;
impl ConfirmSource for ApproveSim {
    fn confirm<'a>(
        &'a mut self,
        _prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        // The simulated operator click: the wa-serve flow's approve button.
        Box::pin(std::future::ready(true))
    }
}

struct DeclineSim;
impl ConfirmSource for DeclineSim {
    fn confirm<'a>(
        &'a mut self,
        _prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        Box::pin(std::future::ready(false))
    }
}

fn completion(content: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{"finish_reason": "stop", "index": 0,
            "message": {"role": "assistant", "content": content}}]
    })
}

/// One linear scenario by design: the e2e's value is the whole loop in
/// order; splitting it into helpers would hide exactly that ordering.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn full_offline_driver_session_completes_and_cannot_publish_unconfirmed() {
    // ---- World: temp workspace (sessions/, rules/), mocked sources,
    // mocked z.ai, mocked wiki. ----
    let dir = std::env::temp_dir().join(format!("wa-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let session = dir.join("sessions/e2e-article");
    std::fs::create_dir_all(&session).unwrap();
    copy_dir(Path::new("rules"), &dir.join("rules"));
    copy_dir(Path::new("prompts"), &dir.join("prompts"));
    assert!(std::env::set_current_dir(&dir).is_ok());

    // Two mocked sources: one clean, one bot-blocked (403).
    let src1 = MockServer::start_async().await;
    src1.mock_async(|when, then| {
        when.method(httpmock::Method::GET).path("/history");
        then.status(200).body(format!(
            "<html><p>{}</p></html>",
            "The tower was built in stages. ".repeat(30)
        ));
    })
    .await;
    let src2 = MockServer::start_async().await;
    src2.mock_async(|when, then| {
        when.method(httpmock::Method::GET).path("/guide");
        then.status(403).body("forbidden");
    })
    .await;
    // The CDX availability service (not consulted here, but wired).
    let cdx_server = MockServer::start_async().await;
    let cdx = wikiloop::ledger::net::CdxClient::with_base(&cdx_server.url("/cdx"));

    // The z.ai model: findings then proposal (fixture-shaped completions).
    let model = MockServer::start_async().await;
    let findings_completion = r#"```json
[{"id":"AS1","wikitext_anchor":"L1:C0-L1:C19","rules":["WP:V"],
  "evidence":["Q1"],
  "factual_note":"The fetched history supports the age claim.",
  "proposed_fix":"Cite the tower's age to the fetched history.","loop":2}]
```"#;
    model
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("You author Wikipedia improvement assessments");
            then.status(200).json_body(completion(findings_completion));
        })
        .await;
    let proposal_completion = r#"{"proposed_wikitext_block":"The tower is old.\n","edit_summary":"Cite the tower's age to the fetched history."}"#;
    model
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .body_includes("You draft ONE scoped Wikipedia edit");
            then.status(200).json_body(completion(proposal_completion));
        })
        .await;
    let zai = ZaiClient::with_base(&model.url(""), "glm-5.3", "test-key").with_retry_delays(vec![]);

    // The wiki: revid pre-check, csrf, edit POST (counted).
    let wiki_api = MockServer::start_async().await;
    wiki_api
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("action", "query")
                .query_param("titles", "E2E Article");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "",
                "query": {"pages": [{"pageid": 1, "ns": 0, "title": "E2E Article",
                    "revisions": [{"revid": 500, "slots": {"main": {"content": "old"}}}]}]}
            }));
        })
        .await;
    wiki_api
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .query_param("meta", "tokens");
            then.status(200).json_body(serde_json::json!({
                "batchcomplete": "", "query": {"tokens": {"csrftoken": "+\\"}}
            }));
        })
        .await;
    let edit_mock = wiki_api
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .body_includes("action=edit");
            then.status(200).json_body(serde_json::json!({
                "edit": {"result": "Success", "newrevid": 501}
            }));
        })
        .await;
    let wiki = Wikipedia::connect_with_api_url(&wiki_api.url("/"), None)
        .await
        .unwrap();

    // ---- Session bootstrap. ----
    let base = BASE_WIKITEXT
        .replace("SRC1_URL", &src1.url("/history"))
        .replace("SRC2_URL", &src2.url("/guide"));
    std::fs::write(session.join("session.json"),
        r#"{"article":"E2E Article","base_revid":500,"started":"2026-09-29T00:00:00Z","entry_loop":2}"#).unwrap();
    std::fs::write(session.join("base.wikitext"), &base).unwrap();
    std::fs::write(
        session.join("ledger.json"),
        r#"{"schema_version":1,"sources":[],"quotes":[],"claims":[]}"#,
    )
    .unwrap();

    // ---- Step 0: the source sweep. ----
    let mut ledger = Ledger::default();
    for candidate in parse_citations(&base) {
        ledger.register_sweep_source(&candidate.ledger_url().unwrap(), None);
    }
    assert_eq!(ledger.sources.len(), 2, "both cited sources inventoried");
    ledger.save(&session.join("ledger.json")).unwrap();

    let cfg = SweepConfig::default();
    let fetcher = wikiloop::ledger::net::SourceFetcher::with_allow_local().unwrap();
    let outcomes: Vec<String> = {
        let mut out = Vec::new();
        for s in ["S1", "S2"] {
            let o = sweep_fetch_one(&fetcher, &cdx, &mut ledger, s, &cfg)
                .await
                .unwrap();
            out.push(o.status);
        }
        out
    };
    assert_eq!(
        outcomes,
        vec!["fetched".to_string(), "needs_operator".to_string()]
    );
    assert_eq!(
        ledger.sweep_unresolved().len(),
        1,
        "the 403 source still demands resolution"
    );
    ledger.save(&session.join("ledger.json")).unwrap();

    // Operator resolution: attach a capture for S2 (the SF-Call path).
    std::fs::write(
        session.join("capture.txt"),
        "The visitor guide describes the tower. ",
    )
    .unwrap();
    wikiloop::cli::ledger_attach("e2e-article", "S2", "sessions/e2e-article/capture.txt").unwrap();
    let mut ledger = Ledger::load(&session.join("ledger.json")).unwrap();
    assert!(
        ledger.source_text("S2").is_some(),
        "attach supplied the text"
    );
    assert!(
        ledger.sweep_unresolved().is_empty(),
        "sweep resolved: fetch + capture"
    );

    // ---- Judgment point 1: findings from the fetched sources. ----
    let text = ledger.source_text("S1").unwrap().to_string();
    let quote_text = "The tower was built in stages.".to_string();
    assert!(text.contains(&quote_text), "quote locates in fetched text");
    let qid = ledger.add_quote("S1", &quote_text).unwrap();
    ledger.save(&session.join("ledger.json")).unwrap();
    // Rules guidance (rule-enforcement item 3): tier-1 + loop-2 cards.
    let corpus = RulesCorpus::load(Path::new("rules")).unwrap();
    let guidance = wikiloop::rules::guidance_for_loop(&corpus, 2).unwrap();
    let ctx = AssessContext {
        article: "E2E Article".into(),
        base_wikitext: base.clone(),
        sources: vec![SourceDigest {
            id: "S1".into(),
            title: "Tower History".into(),
            text: text.clone(),
            quotes: vec![(qid.clone(), quote_text)],
        }],
        quote_ids: vec![qid.clone()],
        entry_loop: 2,
        max_assessments: 3,
        guidance: guidance.clone(),
    };
    let assessments = assess(&zai, &ctx)
        .await
        .expect("model assessments validate");
    assert_eq!(assessments.len(), 1);
    assert_eq!(assessments[0].evidence, vec![qid.clone()]);

    // Persist findings exactly like `wa findings add` (schema-validated).
    let file = wikiloop::session::AssessmentsFile {
        assessments: assessments.clone(),
    };
    file.save(&session.join("assessments.json")).unwrap();

    // ---- Judgment point 2: the scoped proposal. ----
    let proposal = draft_proposal(
        &zai,
        &assessments[0],
        &[],
        "The tower is old.",
        &[],
        &guidance,
    )
    .await
    .expect("proposal");
    let proposed = base.replace(
        "The tower is old.",
        proposal.proposed_wikitext_block.trim_end(),
    );
    std::fs::write(session.join("proposed.wikitext"), &proposed).unwrap();

    // ---- Gate (the same one render/publish re-run). ----
    let corpus = RulesCorpus::load(Path::new("rules")).unwrap();
    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        assessments: &assessments,
        base_wikitext: &base,
        proposed_wikitext: &proposed,
        linter_config: &corpus.linter,
        paraphrase_config: &corpus.paraphrase,
    });
    assert!(!verdict.blocked, "gate: {:?}", verdict.reasons);

    // ---- Audit offline (recorded Parsoid fixtures; render fidelity has
    // its own suite — here we exercise the offline path completing).
    // The LLM pass is off: this e2e is about the deterministic loop.
    let parsoid_fixture = format!(
        "{}/fixtures/parsoid/temple-fielding@1372827284.html",
        env!("CARGO_MANIFEST_DIR")
    );
    wikiloop::cli::audit_flow(
        "e2e-article",
        &wikiloop::cli::AuditOpts {
            llm: wikiloop::cli::LlmChoice::Off,
            summary: "e2e round".into(),
            html_base: Some(Path::new(&parsoid_fixture).to_path_buf()),
            html_proposed: Some(Path::new(&parsoid_fixture).to_path_buf()),
        },
        None,
    )
    .await
    .expect("offline audit renders");

    // ---- Publish WITHOUT confirmation: nothing writes. ----
    let mut declined = DeclineSim;
    let err = wikiloop::cli::publish_core(
        "e2e-article",
        &proposal.edit_summary,
        &wiki,
        &mut declined,
        wikiloop::cli::Via::Tty,
    )
    .await
    .expect_err("declined confirmation must fail");
    assert!(err.to_string().contains("declined"), "{err}");
    assert_eq!(edit_mock.calls(), 0, "no edit without confirmation");
    let meta = std::fs::read_to_string(session.join("session.json")).unwrap();
    assert!(
        meta.contains("\"base_revid\":500"),
        "no re-pin on decline: {meta}"
    );

    // ---- Publish THROUGH the simulated confirmation. ----
    let mut approved = ApproveSim;
    let outcome = wikiloop::cli::publish_core(
        "e2e-article",
        &proposal.edit_summary,
        &wiki,
        &mut approved,
        wikiloop::cli::Via::Tty,
    )
    .await
    .expect("publish completes");
    assert!(outcome.created_revision);
    assert_eq!(edit_mock.calls(), 1, "exactly one edit");
    assert_eq!(outcome.new_revid, 501);
    let meta = std::fs::read_to_string(session.join("session.json")).unwrap();
    assert!(meta.contains("\"base_revid\": 501"), "re-pinned: {meta}");
    let rounds = std::fs::read_to_string(session.join("rounds.jsonl")).unwrap();
    assert!(rounds.contains("published:"), "round log: {rounds}");

    // Cleanup.
    assert!(std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
    let _: Duration = Duration::from_millis(0);
}
