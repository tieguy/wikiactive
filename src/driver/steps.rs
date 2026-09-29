//! The pipeline steps: the ONLY places the model is called (findings
//! authoring, proposal drafting, comment resolution). Everything else —
//! loop control, the ledger, gates, publish — is deterministic Rust.
//!
//! Contract (plan-003 B.2 / AC.6):
//!
//! - Model output is schema-validated exactly like `wa findings add`
//!   (same [`Finding::validate`], same admission rules).
//! - Malformed output (unparseable OR failing validation OR citing
//!   unknown ledger quote ids) is retried exactly once with a corrective
//!   message, then the step is blocked — never waved through.
//! - No step writes session state: each returns data; the orchestrator
//!   persists and runs the publish gate (B.5 wires this; the driver
//!   cannot bypass it because the gate re-runs at render/publish on
//!   whatever state exists on disk).

use serde::Deserialize;

use crate::driver::model::ChatMessage;
use crate::driver::model::ZaiClient;
use crate::driver::model::ZaiError;
use crate::driver::model::strip_code_fence;
use crate::driver::prompts;
use crate::session::Finding;

/// Judgment-point temperature: low, deterministic drafting.
pub const STEP_TEMPERATURE: f64 = 0.2;

/// A step failure. `Transport` failures are the client's; everything here
/// is about the model's output failing admission.
#[derive(Debug, thiserror::Error)]
pub enum StepError {
    /// The z.ai call itself failed.
    #[error("model transport: {0}")]
    Transport(#[from] ZaiError),
    /// The step's output was malformed (unparseable, schema-invalid, or
    /// citing unknown quote ids) after one retry — blocked.
    #[error("step {step}: model output malformed after retry — blocked: {problems}")]
    Malformed {
        /// Step name (`author_findings` / `draft_proposal` / `resolve_comments`).
        step: &'static str,
        /// Everything wrong with the last attempt.
        problems: String,
    },
    /// A prompt template could not be loaded.
    #[error("prompt template: {0}")]
    Prompt(#[from] prompts::PromptError),
}

/// The deterministic context bundle for findings authoring, assembled by
/// the orchestrator from session state (never by the model).
#[derive(Debug, Clone)]
pub struct FindingsContext {
    /// Article title.
    pub article: String,
    /// Pinned base wikitext.
    pub base_wikitext: String,
    /// Per-source digest: id, title, fetched text, and registered quotes.
    pub sources: Vec<SourceDigest>,
    /// Quote ids that exist in the ledger (evidence must cite among
    /// these — fabricated ids are blocked).
    pub quote_ids: Vec<String>,
    /// Entry loop (1–5).
    pub entry_loop: u8,
    /// Cap on findings per run (prompt slot).
    pub max_findings: usize,
}

/// One source's digest for the findings prompt.
#[derive(Debug, Clone)]
pub struct SourceDigest {
    /// Ledger id (S1, S2, …).
    pub id: String,
    /// Source title (or URL when untitled).
    pub title: String,
    /// Fetched/attached text.
    pub text: String,
    /// Registered quotes (Q ids + verbatim text).
    pub quotes: Vec<(String, String)>,
}

/// A review comment resolved to its wikitext anchor (from `wa poll`).
#[derive(Debug, Clone)]
pub struct DriverComment {
    /// The operator's comment text.
    pub prompt: String,
    /// Resolved anchor (`L..C..-L..C..` into the proposed block, or
    /// `base:`-prefixed for removed wording).
    pub anchor: String,
}

/// A drafted proposal (`prompts/propose.md` output schema).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// The replacement wikitext block.
    pub proposed_wikitext_block: String,
    /// One-line edit summary.
    pub edit_summary: String,
}

/// A comment-resolution result (`prompts/resolve.md` output schema).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    /// The revised wikitext block.
    pub proposed_wikitext_block: String,
    /// One-line applied-comment summaries.
    pub applied: Vec<String>,
    /// One-line rejected-comment summaries with reasons.
    pub rejected: Vec<String>,
    /// Short operator-facing reply (review UI).
    pub reply: String,
}

/// Call the model and parse its output as JSON `T`; one corrective retry
/// on parse/validation failure, then [`StepError::Malformed`].
async fn call_json<T: serde::de::DeserializeOwned>(
    step: &'static str,
    client: &ZaiClient,
    messages: &[ChatMessage],
    validate: impl Fn(&T) -> Result<(), Vec<String>>,
) -> Result<T, StepError> {
    let first = client.chat(messages, STEP_TEMPERATURE).await?;
    if let Ok(parsed) = parse_and_validate::<T>(&first.content, &validate) {
        return Ok(parsed);
    }
    // One corrective retry: same conversation plus what was wrong.
    let problems_of = |content: &str| parse_problems::<T>(content, &validate);
    let retry_messages = {
        let mut m = messages.to_vec();
        m.push(ChatMessage::user(format!(
            "Your previous output was rejected: {}. Output the JSON this step's schema \
             expects — JSON only, no code fences, no commentary.",
            problems_of(&first.content)
        )));
        m
    };
    let second = client.chat(&retry_messages, STEP_TEMPERATURE).await?;
    match parse_and_validate::<T>(&second.content, &validate) {
        Ok(parsed) => Ok(parsed),
        Err(problems) => Err(StepError::Malformed {
            step,
            problems: problems.join("; "),
        }),
    }
}

fn parse_and_validate<T: serde::de::DeserializeOwned>(
    content: &str,
    validate: &impl Fn(&T) -> Result<(), Vec<String>>,
) -> Result<T, Vec<String>> {
    let parsed = serde_json::from_str::<T>(strip_code_fence(content))
        .map_err(|e| vec![format!("not valid JSON for this step's schema: {e}")])?;
    match validate(&parsed) {
        Ok(()) => Ok(parsed),
        Err(problems) => Err(problems),
    }
}

fn parse_problems<T: serde::de::DeserializeOwned>(
    content: &str,
    validate: &impl Fn(&T) -> Result<(), Vec<String>>,
) -> String {
    parse_and_validate::<T>(content, validate)
        .err()
        .unwrap_or_default()
        .join("; ")
}

/// Judgment point 1: author findings from the context bundle. Output is
/// validated exactly like `wa findings add` (same
/// [`Finding::validate`]), plus evidence ids must exist in the ledger.
///
/// # Errors
/// [`StepError::Transport`] or [`StepError::Malformed`] (after one
/// corrective retry).
pub async fn author_findings(
    client: &ZaiClient,
    ctx: &FindingsContext,
) -> Result<Vec<Finding>, StepError> {
    let template = prompts::load(prompts::AUTHOR_FINDINGS)?;
    let system = prompts::render(
        &template,
        &[
            ("max_findings", &ctx.max_findings.to_string()),
            ("entry_loop", &ctx.entry_loop.to_string()),
        ],
    );
    let mut user = format!(
        "Article: {}\n\nBase wikitext:\n{}\n",
        ctx.article, ctx.base_wikitext
    );
    {
        use std::fmt::Write as _;
        for s in &ctx.sources {
            let _ = write!(user, "\nSource {} — {}:\n{}\n", s.id, s.title, s.text);
            for (qid, text) in &s.quotes {
                let _ = writeln!(user, "Quote {qid} (verbatim): {text}");
            }
        }
        let _ = write!(
            user,
            "\nRegistered quote ids (evidence must cite among these): {}\nEntry loop: {}.\n",
            ctx.quote_ids.join(", "),
            ctx.entry_loop
        );
    }

    let findings = call_json::<Vec<Finding>>(
        "author_findings",
        client,
        &[ChatMessage::system(system), ChatMessage::user(user)],
        |parsed| {
            let mut problems = Vec::new();
            for f in parsed {
                if let Err(p) = f.validate() {
                    problems.push(format!("finding {}: {}", f.id, p.join("; ")));
                }
                for qid in &f.evidence {
                    if !ctx.quote_ids.contains(qid) {
                        problems.push(format!(
                            "finding {}: evidence {qid} is not a registered ledger quote",
                            f.id
                        ));
                    }
                }
            }
            if parsed.len() > ctx.max_findings {
                problems.push(format!(
                    "{} findings exceed the cap of {}",
                    parsed.len(),
                    ctx.max_findings
                ));
            }
            if problems.is_empty() {
                Ok(())
            } else {
                Err(problems)
            }
        },
    )
    .await?;
    Ok(findings)
}

/// Judgment point 2: draft the scoped edit for one approved finding.
///
/// # Errors
/// [`StepError::Transport`] or [`StepError::Malformed`] (after one
/// corrective retry).
pub async fn draft_proposal(
    client: &ZaiClient,
    finding: &Finding,
    base_block: &str,
    named_refs: &[String],
) -> Result<Proposal, StepError> {
    let system = prompts::load(prompts::PROPOSE)?;
    let user = format!(
        "Finding {} (loop {}): {}\nProposed fix: {}\nEvidence quotes: {}\n\nBase wikitext block:\n{}\n\nNamed refs on the page: {}",
        finding.id,
        finding.loop_id,
        finding.factual_note,
        finding.proposed_fix,
        finding.evidence.join(", "),
        base_block,
        named_refs.join(", "),
    );
    call_json::<Proposal>(
        "draft_proposal",
        client,
        &[ChatMessage::system(system), ChatMessage::user(user)],
        |p| {
            let mut problems = Vec::new();
            if p.proposed_wikitext_block.trim().is_empty() {
                problems.push("proposed_wikitext_block must be non-empty".into());
            }
            if p.edit_summary.trim().is_empty() {
                problems.push("edit_summary must be non-empty".into());
            }
            if problems.is_empty() {
                Ok(())
            } else {
                Err(problems)
            }
        },
    )
    .await
}

/// Judgment point 3: apply operator review comments to the proposal.
///
/// # Errors
/// [`StepError::Transport`] or [`StepError::Malformed`] (after one
/// corrective retry).
pub async fn resolve_comments(
    client: &ZaiClient,
    proposed_block: &str,
    base_block: &str,
    comments: &[DriverComment],
) -> Result<Resolution, StepError> {
    let system = prompts::load(prompts::RESOLVE)?;
    let mut user = format!(
        "Proposed wikitext block:\n{proposed_block}\n\nBase wikitext block:\n{base_block}\n\nComments:\n"
    );
    {
        use std::fmt::Write as _;
        for (n, c) in comments.iter().enumerate() {
            let _ = writeln!(user, "{}. [{}] {}", n + 1, c.anchor, c.prompt);
        }
    }
    call_json::<Resolution>(
        "resolve_comments",
        client,
        &[ChatMessage::system(system), ChatMessage::user(user)],
        |r| {
            let mut problems = Vec::new();
            if r.proposed_wikitext_block.trim().is_empty() {
                problems.push("proposed_wikitext_block must be non-empty".into());
            }
            if r.reply.trim().is_empty() {
                problems.push("reply must be non-empty (the operator sees it)".into());
            }
            if problems.is_empty() {
                Ok(())
            } else {
                Err(problems)
            }
        },
    )
    .await
}
