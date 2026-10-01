//! The pipeline steps: the ONLY places the model is called (findings
//! authoring, proposal drafting, comment resolution). Everything else —
//! loop control, the ledger, gates, publish — is deterministic Rust.
//!
//! Contract (plan-003 B.2 / AC.6):
//!
//! - Model output is schema-validated exactly like `wa findings add`
//!   (same [`Assessment::validate`], same admission rules).
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
use crate::session::Assessment;

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
        /// Step name (`assess` / `draft_proposal` / `resolve_comments`).
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
pub struct AssessContext {
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
    pub max_assessments: usize,
    /// The rules guidance for this entry loop (`rules::guidance_for_loop`
    /// output): tier-1 verbatim + the triage-selected cards. Built BEFORE
    /// the step runs — a corpus missing a card fails the handler, not the
    /// model call (rule-enforcement item 3).
    pub guidance: String,
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
    let problems = match parse_and_validate::<T>(&first.content, &validate) {
        Ok(parsed) => return Ok(parsed),
        Err(problems) => problems.join("; "),
    };
    // One corrective retry: the rejected output rides along as an
    // assistant turn (a stateless model must be able to SEE what it got
    // wrong), then the corrective instruction. An empty reply is named
    // as such — endpoints reject an empty assistant turn outright.
    let rejected = if first.content.trim().is_empty() {
        "(no output)".to_string()
    } else {
        first.content
    };
    let mut retry_messages = messages.to_vec();
    retry_messages.push(ChatMessage::assistant(rejected));
    retry_messages.push(ChatMessage::user(format!(
        "Your previous output was rejected: {problems}. Output the JSON this step's schema \
         expects — JSON only, no code fences, no commentary."
    )));
    let second = client.chat(&retry_messages, STEP_TEMPERATURE).await?;
    parse_and_validate::<T>(&second.content, &validate).map_err(|problems| StepError::Malformed {
        step,
        problems: problems.join("; "),
    })
}

/// Parse model output as `T` and validate it. The content is tried as raw
/// JSON first; code-fence stripping is only the fallback, so valid JSON
/// whose strings happen to contain a fence is never mangled.
fn parse_and_validate<T: serde::de::DeserializeOwned>(
    content: &str,
    validate: &impl Fn(&T) -> Result<(), Vec<String>>,
) -> Result<T, Vec<String>> {
    let parsed = serde_json::from_str::<T>(content.trim())
        .or_else(|_| serde_json::from_str::<T>(strip_code_fence(content)))
        .map_err(|e| vec![format!("not valid JSON for this step's schema: {e}")])?;
    validate(&parsed).map(|()| parsed)
}

/// Judgment point 1: author findings from the context bundle. Output is
/// validated exactly like `wa findings add` (same
/// [`Assessment::validate`]), plus evidence ids must exist in the ledger.
///
/// # Errors
/// [`StepError::Transport`] or [`StepError::Malformed`] (after one
/// corrective retry).
pub async fn assess(client: &ZaiClient, ctx: &AssessContext) -> Result<Vec<Assessment>, StepError> {
    let template = prompts::load(prompts::ASSESS)?;
    let system = prompts::render(
        &template,
        &[
            ("max_assessments", &ctx.max_assessments.to_string()),
            ("entry_loop", &ctx.entry_loop.to_string()),
            ("guidance", &ctx.guidance),
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

    let findings = call_json::<Vec<Assessment>>(
        "assess",
        client,
        &[ChatMessage::system(system), ChatMessage::user(user)],
        |parsed| {
            let mut problems = Vec::new();
            for (i, f) in parsed.iter().enumerate() {
                if parsed[..i].iter().any(|earlier| earlier.id == f.id) {
                    problems.push(format!("finding id {} is used more than once", f.id));
                }
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
            if parsed.len() > ctx.max_assessments {
                problems.push(format!(
                    "{} findings exceed the cap of {}",
                    parsed.len(),
                    ctx.max_assessments
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
/// `evidence` is the finding's quotes as the model must see them — the
/// verbatim text with its source, one entry per quote (never bare ids:
/// the draft has to be written from the quoted words, not from memory).
/// `base_block` is the wikitext the finding's anchor spans, which the
/// returned block replaces.
///
/// # Errors
/// [`StepError::Transport`] or [`StepError::Malformed`] (after one
/// corrective retry).
pub async fn draft_proposal(
    client: &ZaiClient,
    finding: &Assessment,
    evidence: &[String],
    base_block: &str,
    named_refs: &[String],
    guidance: &str,
) -> Result<Proposal, StepError> {
    let system = prompts::render(&prompts::load(prompts::PROPOSE)?, &[("guidance", guidance)]);
    let user = format!(
        "Assessment {} (loop {}): {}\nProposed fix: {}\nEvidence quotes (verbatim):\n{}\n\nBase wikitext block:\n{}\n\nNamed refs on the page: {}",
        finding.id,
        finding.loop_id,
        finding.factual_note,
        finding.proposed_fix,
        evidence.join("\n"),
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
    guidance: &str,
) -> Result<Resolution, StepError> {
    let system = prompts::render(&prompts::load(prompts::RESOLVE)?, &[("guidance", guidance)]);
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

/// The verdict a rule-review entry carries (`prompts/review.md` output
/// schema). `Ok` entries are accepted then discarded by the pipeline —
/// the display is concerns-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConcernVerdict {
    Ok,
    Concern,
}

/// One rule-review entry (`prompts/review.md` output schema):
/// clause-by-clause advice on the drafted text (rule-enforcement item
/// 5). Never a gate.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConcern {
    /// A clause id the guidance names (tier-1 `A1`-style, or a card
    /// slug) — validated against a corpus-built allowlist.
    pub clause: String,
    pub verdict: ConcernVerdict,
    /// Verbatim text from a block's proposed text — must locate
    /// (the quote-anchor discipline: model output is untrusted).
    pub span: String,
    /// What the clause demands and how the span may fail it.
    pub note: String,
}

/// One changed block for the rule-review step (rule-enforcement item 5):
/// the base text, the proposed text, and the verbatim evidence with
/// sources — the same inputs `bucket_groups` assembles for resolve.
#[derive(Debug, Clone)]
pub struct ReviewBlock {
    /// The block's element id in the review artifact (carried back onto
    /// each concern so the display can place it under its block).
    pub element_id: String,
    pub base: String,
    pub proposed: String,
    /// Formatted evidence: `- "quote" — source <url>` per line.
    pub evidence: Vec<String>,
}

/// Judgment point 4 (rule-enforcement item 5): a separate model pass
/// reads the drafted text and reports, clause by clause, where it may
/// break the tier-1 rules and the loop's cards. Advice shown beside the
/// diff — NEVER a gate.
///
/// Validation (same discipline as quotes — model output is untrusted):
/// `clause` must be in `clauses` (corpus-built), `span` must locate
/// verbatim in one of the blocks' proposed text, and a `concern` needs a
/// non-empty note. One corrective retry, then the step fails.
///
/// # Errors
/// [`StepError::Transport`] or [`StepError::Malformed`] (after the
/// corrective retry).
pub async fn review_draft(
    client: &ZaiClient,
    guidance: &str,
    clauses: &[String],
    blocks: &[ReviewBlock],
) -> Result<Vec<RuleConcern>, StepError> {
    use std::fmt::Write as _;
    let system = prompts::render(&prompts::load(prompts::REVIEW)?, &[("guidance", guidance)]);
    let mut user = format!(
        "Clause ids you may cite (from the guidance): {}\n\nChanged blocks:\n",
        clauses.join(", ")
    );
    for (n, b) in blocks.iter().enumerate() {
        let _ = write!(
            user,
            "\nBlock {}:\nBase text:\n{}\nProposed text:\n{}\n",
            n + 1,
            b.base,
            b.proposed
        );
        if b.evidence.is_empty() {
            let _ = writeln!(user, "Evidence: (none quoted)");
        } else {
            let _ = write!(user, "Evidence:\n{}\n", b.evidence.join("\n"));
        }
    }
    call_json::<Vec<RuleConcern>>(
        "review_draft",
        client,
        &[ChatMessage::system(system), ChatMessage::user(user)],
        |parsed| {
            let mut problems = Vec::new();
            for c in parsed {
                if !clauses.contains(&c.clause) {
                    problems.push(format!(
                        "clause {:?} is not one the guidance names",
                        c.clause
                    ));
                }
                if c.span.trim().is_empty() {
                    problems.push(format!("clause {}: span must be non-empty", c.clause));
                } else if !blocks.iter().any(|b| b.proposed.contains(c.span.trim())) {
                    problems.push(format!(
                        "clause {}: span is not verbatim in any block's proposed text",
                        c.clause
                    ));
                }
                if c.verdict == ConcernVerdict::Concern && c.note.trim().is_empty() {
                    problems.push(format!(
                        "clause {}: a concern needs a non-empty note",
                        c.clause
                    ));
                }
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
