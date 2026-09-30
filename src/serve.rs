//! `wa serve` (plan-003 B.4) — the Phase-B local web console: axum,
//! loopback-only. Hosts the session console, the sweep/source manifest
//! (per-source status, attach ingestion, disposition signing), and the
//! publish confirmation as an EXPLICIT operator action backed by
//! [`WebConfirm`] — a pending-confirmation state resolved by a web click.
//!
//! Invariants (test-pinned):
//!
//! - The publish path is [`crate::cli::publish_core`] — the same
//!   gate → confirm → edit → re-pin → disclosure flow the tty CLI runs,
//!   with the confirm source injected. `BundledConsent` backs only the
//!   disclosure-log upsert bundled into the one yes, never the article
//!   edit.
//! - No auto-publish: without `POST /confirmations/{id}` resolving a
//!   pending confirmation to approve, the article edit never posts.
//! - Bind is `127.0.0.1` only (never configurable to another host).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use axum::Form;
use axum::Router;
use axum::extract::Path;
use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse as _;
use axum::response::Redirect;
use axum::routing::get;
use axum::routing::post;
use tokio::sync::oneshot;

use crate::ledger::Ledger;
use crate::session::SessionMeta;
use crate::ui::esc;
use crate::ui::plural;
use crate::wikipedia::ConfirmSource;
use crate::wikipedia::Wikipedia;

/// How long a pending publish confirmation waits for the operator before
/// declining (the flow then aborts without writing).
pub const CONFIRM_TIMEOUT: Duration = Duration::from_mins(10);

/// The only host `wa serve` will ever bind.
pub const LOOPBACK_HOST: &str = "127.0.0.1";

/// One pending publish confirmation: the exact prompt the flow is parked
/// on and the channel that resolves it.
struct PendingConfirm {
    slug: String,
    summary: String,
    prompt: String,
    responder: oneshot::Sender<bool>,
}

/// Shared server state: pending confirmations and per-session last
/// publish outcome (for the console).
#[derive(Default)]
pub struct ServeState {
    confirmations: Mutex<HashMap<String, PendingConfirm>>,
    outcomes: Mutex<HashMap<String, String>>,
    /// Per-slug completed-run counter: the publish POST waits for the
    /// pending confirmation OR the next outcome before responding, so the
    /// page the operator lands on always shows what happened.
    runs: Mutex<HashMap<String, u64>>,
    /// Test hook: override the wiki API endpoint (`None` in production —
    /// the live operator never sets it).
    api_override: Option<String>,
    /// Test hook for the z.ai model endpoint (`None` in production).
    zai_override: Option<String>,
    confirm_timeout: Option<Duration>,
}

impl ServeState {
    /// Production state (live wiki endpoint, standard timeout). The
    /// `WIKIACTIVE_SERVE_TEST_API` env var is a TEST hook (spawns of the
    /// binary point at an httpmock wiki); the live operator never sets it,
    /// and the server is loopback-only regardless.
    #[must_use]
    pub fn new() -> Self {
        let api_override = std::env::var("WIKIACTIVE_SERVE_TEST_API")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let zai_override = std::env::var("WIKIACTIVE_SERVE_TEST_ZAI")
            .ok()
            .filter(|s| !s.trim().is_empty());
        Self {
            api_override,
            zai_override,
            ..Self::default()
        }
    }

    /// Test state: fixed wiki and z.ai endpoints and a short confirm
    /// timeout.
    #[must_use]
    pub fn with_api_override(api_url: &str, confirm_timeout: Duration) -> Self {
        Self {
            api_override: Some(api_url.to_string()),
            confirm_timeout: Some(confirm_timeout),
            ..Self::default()
        }
    }

    /// The z.ai client for the driver judgment points (key from env;
    /// endpoint from house-rules `[zai]`, env overriding; the test hook
    /// points at a mock).
    fn zai_client(
        &self,
    ) -> Result<crate::driver::model::ZaiClient, crate::driver::model::ZaiError> {
        let model = crate::rules::RulesCorpus::load(std::path::Path::new("rules"))
            .ok()
            .and_then(|c| c.house_rules.zai.and_then(|z| z.model))
            .unwrap_or_else(|| "glm-5.3".to_string());
        match &self.zai_override {
            Some(base) => Ok(crate::driver::model::ZaiClient::with_base(
                base, &model, "test-key",
            )),
            None => crate::driver::model::ZaiClient::from_env(&model),
        }
    }

    fn timeout(&self) -> Duration {
        self.confirm_timeout.unwrap_or(CONFIRM_TIMEOUT)
    }

    fn resolve(&self, id: &str, approve: bool) -> bool {
        let pending = self.confirmations.lock().expect("confirmations").remove(id);
        match pending {
            Some(p) => p.responder.send(approve).is_ok(),
            None => false,
        }
    }

    fn note_outcome(&self, slug: &str, outcome: &str) {
        self.outcomes
            .lock()
            .expect("outcomes")
            .insert(slug.to_string(), outcome.to_string());
        let mut runs = self.runs.lock().expect("runs");
        *runs.entry(slug.to_string()).or_default() += 1;
        // Persist too: a failed publish's error must be diagnosable from
        // the session directory, not only from the live server's memory.
        let _ = std::fs::write(session_dir(slug).join("last-run.txt"), outcome);
    }

    fn run_count(&self, slug: &str) -> u64 {
        self.runs
            .lock()
            .expect("runs")
            .get(slug)
            .copied()
            .unwrap_or_default()
    }

    /// The last action's outcome for a session ("" when none).
    fn outcome(&self, slug: &str) -> String {
        self.outcomes
            .lock()
            .expect("outcomes")
            .get(slug)
            .cloned()
            .unwrap_or_default()
    }

    /// This session's pending publish approvals: `(id, summary, prompt)`.
    fn pending_for(&self, slug: &str) -> Vec<(String, String, String)> {
        self.confirmations
            .lock()
            .expect("confirmations")
            .iter()
            .filter(|(_, p)| p.slug == slug)
            .map(|(id, p)| (id.clone(), p.summary.clone(), p.prompt.clone()))
            .collect()
    }

    fn has_pending_for(&self, slug: &str) -> bool {
        self.confirmations
            .lock()
            .expect("confirmations")
            .values()
            .any(|p| p.slug == slug)
    }

    async fn connect_wiki(&self) -> anyhow::Result<Wikipedia> {
        match &self.api_override {
            Some(url) => Wikipedia::connect_with_api_url(url, None)
                .await
                .map_err(|e| anyhow::anyhow!("{e}")),
            None => Wikipedia::connect()
                .await
                .map_err(|e| anyhow::anyhow!("{e}")),
        }
    }
}

/// The web-backed confirm source: registers a pending confirmation and
/// parks until the operator clicks (or the timeout declines).
pub struct WebConfirm {
    state: Arc<ServeState>,
    slug: String,
    summary: String,
}

impl ConfirmSource for WebConfirm {
    fn confirm<'a>(
        &'a mut self,
        prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        let state = Arc::clone(&self.state);
        let slug = self.slug.clone();
        let summary = self.summary.clone();
        let prompt = prompt.to_string();
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            let id = format!("{}-{}", slug, chrono::Utc::now().timestamp_millis());
            state.confirmations.lock().expect("confirmations").insert(
                id.clone(),
                PendingConfirm {
                    slug,
                    summary,
                    prompt,
                    responder: tx,
                },
            );
            // Timeout declines: no click, no write. An unanswered entry is
            // dropped so no page keeps offering an approval that can no
            // longer do anything.
            let answer = tokio::time::timeout(state.timeout(), rx).await;
            state
                .confirmations
                .lock()
                .expect("confirmations")
                .remove(&id);
            matches!(answer, Ok(Ok(true)))
        })
    }
}

fn session_dir(slug: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("sessions").join(slug)
}

fn read_meta(slug: &str) -> Option<SessionMeta> {
    let text = std::fs::read_to_string(session_dir(slug).join("session.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write-route gate (plan-004 review finding): the slug comes from the
/// URL path and axum percent-decodes it, so an unvalidated slug can
/// traverse outside `sessions/` (and browser form POSTs are "simple
/// requests" — any site can fire them at loopback). Every mutating route
/// must reference a session that actually exists on disk.
fn known_session(slug: &str) -> bool {
    !slug.contains(['/', '\\'])
        && !slug.split('/').any(|seg| seg == ".." || seg == ".")
        && read_meta(slug).is_some()
}

fn list_sessions() -> Vec<String> {
    let mut slugs: Vec<String> = std::fs::read_dir("sessions")
        .map(|rd| {
            rd.filter_map(std::result::Result::ok)
                .filter(|e| e.path().is_dir())
                .filter(|e| e.path().join("session.json").exists())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    slugs.sort();
    slugs
}

/// Console sort rank: smaller = more attention (live reviews with open
/// comments first, then live reviews, then history/no-review rows).
/// Module-level so the ordering is unit-testable (plan-005 O.3).
fn console_rank(sort_key: &str) -> u8 {
    match sort_key {
        "current+comments" => 0,
        "current" => 1,
        _ => 2,
    }
}

/// Query-string parameters on form actions: `from=review` returns the
/// operator to the review page they acted on, `at` names the block to
/// scroll back to, and `notice` asks the review page to show the last
/// outcome.
type Params = axum::extract::Query<HashMap<String, String>>;

fn from_review(q: &HashMap<String, String>) -> bool {
    q.get("from").is_some_and(|v| v == "review")
}

/// Where a form POST lands: back on the review page when it was sent from
/// there (at the block it concerned, or showing the outcome `notice`),
/// else on the session page.
fn back_to(slug: &str, q: &HashMap<String, String>, notice: Option<&str>) -> Redirect {
    if !from_review(q) {
        return Redirect::to(&format!("/sessions/{slug}"));
    }
    let query = notice.map(|n| format!("?notice={n}")).unwrap_or_default();
    let fragment = match notice {
        Some("publish") => "#wa-publish".to_string(),
        Some(_) => String::new(),
        None => q
            .get("at")
            .filter(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            .map(|a| format!("#{a}"))
            .unwrap_or_default(),
    };
    Redirect::to(&format!("/sessions/{slug}/review{query}{fragment}"))
}

/// The last action's outcome as a message box (empty when there is none).
/// Failures get the error treatment; URLs in the text become links.
fn notice_html(outcome: &str) -> String {
    if outcome.is_empty() {
        return String::new();
    }
    let lower = outcome.to_lowercase();
    let failed = [
        "failed",
        "blocked",
        "error",
        "not started",
        "out of date",
        "unresolved",
    ]
    .iter()
    .any(|w| lower.contains(w));
    format!(
        "<div class=\"notice{}\" role=\"status\"><strong>Last action</strong>\
         <div class=\"msg\">{}</div></div>\n",
        if failed { " error" } else { "" },
        crate::ui::linkify(outcome)
    )
}

/// One pending publish approval: the exact prompt the flow is parked on
/// and the two answers. `from_review` returns a decline to the review
/// page.
fn approval_html(pending: &(String, String, String), from_review: bool) -> String {
    let (id, summary, prompt) = pending;
    format!(
        "<section class=\"approval\" id=\"wa-publish\"><h2>Approve this publish</h2>\
         <p>Edit summary: <strong>{}</strong></p><pre>{}</pre>\
         <form method=post action=\"/confirmations/{}{}\" class=\"row\">\
         <button name=approve value=true class=\"primary\">Approve and publish</button>\
         <button name=approve value=false>Don't publish</button></form>\
         <p class=\"meta\">Nothing is written to Wikipedia until you approve.</p></section>\n",
        if summary.is_empty() {
            "(none)".to_string()
        } else {
            esc(summary)
        },
        esc(prompt),
        esc(id),
        if from_review { "?from=review" } else { "" },
    )
}

/// One console row's status: `(label, needs attention, detail, sort key)`.
fn console_status(
    pending: bool,
    artifact: Option<&ArtifactState>,
    open: usize,
) -> (String, bool, String, &'static str) {
    if pending {
        (
            "Publish waiting for your approval".to_string(),
            true,
            String::new(),
            "current+comments",
        )
    } else {
        match artifact {
            Some(ArtifactState::Current { round }) if open > 0 => (
                format!("Review open, round {round}"),
                true,
                format!("{} to apply", plural(open, "open comment")),
                "current+comments",
            ),
            Some(ArtifactState::Current { round }) => (
                format!("Review open, round {round}"),
                true,
                String::new(),
                "current",
            ),
            Some(ArtifactState::Stale {
                kind: StaleKind::Published,
                ..
            }) => ("Published".to_string(), false, String::new(), "stale"),
            Some(ArtifactState::Stale { kind, .. }) => (
                "Needs a new render".to_string(),
                false,
                kind.short().to_string(),
                "stale",
            ),
            None => ("No review yet".to_string(), false, String::new(), "none"),
        }
    }
}

/// GET / — the console: every session with what actually decides "does
/// this need me?" — the review state (open / needs a new render and why /
/// published / none), the open-comment count, and a waiting publish
/// approval — sorted attention-first (live reviews with open comments,
/// then live reviews, then history).
async fn console(State(state): State<Arc<ServeState>>) -> Html<String> {
    struct Row {
        slug: String,
        article: String,
        label: String,
        attention: bool,
        detail: String,
        sort_key: &'static str,
    }
    let mut rows: Vec<Row> = list_sessions()
        .into_iter()
        .map(|slug| {
            let dir = session_dir(&slug);
            let article = read_meta(&slug).map_or_else(|| slug.clone(), |m| m.article);
            let open = crate::comments::CommentQueue::load(&dir.join("comments.jsonl"))
                .map_or(0, |q| q.open().len());
            let (label, attention, detail, sort_key) = console_status(
                state.has_pending_for(&slug),
                artifact_state(&dir).as_ref(),
                open,
            );
            Row {
                slug,
                article,
                label,
                attention,
                detail,
                sort_key,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        console_rank(a.sort_key)
            .cmp(&console_rank(b.sort_key))
            .then_with(|| a.slug.cmp(&b.slug))
    });

    let mut body = String::from("<h1>Sessions</h1>\n");
    if rows.is_empty() {
        body.push_str(
            "<p class=\"empty\">No sessions in this directory yet. Start one from a terminal: \
             <code>wa session init --article \"Article title\" --entry-loop 2</code></p>",
        );
    } else {
        body.push_str("<ul class=\"sessions\">\n");
        for row in rows {
            let outcome = state.outcome(&row.slug);
            let last = outcome.lines().next().unwrap_or_default();
            let last: String = if last.chars().count() > 120 {
                last.chars().take(120).chain(['…']).collect()
            } else {
                last.to_string()
            };
            let mut sub = format!("<code>{}</code>", esc(&row.slug));
            if !row.detail.is_empty() {
                let _ = write!(sub, "<span>{}</span>", esc(&row.detail));
            }
            if !last.is_empty() {
                let _ = write!(sub, "<span>Last action: {}</span>", esc(&last));
            }
            let _ = writeln!(
                body,
                "<li><a class=\"title\" href=\"/sessions/{slug}\">{article}</a>\
                 <span class=\"state{attn}\">{label}</span>\
                 <span class=\"sub\">{sub}</span></li>",
                slug = esc(&row.slug),
                article = esc(&row.article),
                attn = if row.attention { " attn" } else { "" },
                label = esc(&row.label),
            );
        }
        body.push_str("</ul>");
    }
    Html(crate::ui::page("Sessions", &[], &body))
}

/// GET /sessions/{slug} — one session, laid out in the order the work
/// happens: sources → draft → review → publish. The last action's outcome
/// and any publish approval waiting on the operator sit at the top.
/// Server-rendered string HTML (no templating dependency).
async fn session_page(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
) -> Html<String> {
    let Some(meta) = known_session(&slug).then(|| read_meta(&slug)).flatten() else {
        return Html(crate::ui::page(
            "Session not found",
            &[("Sessions", "/")],
            &format!(
                "<h1>Session not found</h1><p>There is no session named <code>{}</code> in \
                 this directory.</p>",
                esc(&slug)
            ),
        ));
    };
    let dir = session_dir(&slug);
    let based_on = if meta.base_revid == 0 {
        "a page that does not exist yet".to_string()
    } else {
        format!(
            "based on revision <a href=\"https://en.wikipedia.org/w/index.php?oldid={0}\">{0}</a>",
            meta.base_revid
        )
    };
    let mut page = format!(
        "<h1>{}</h1>\n<p class=\"meta\">Session <code>{}</code>, {based_on}, entry loop {}.</p>\n",
        esc(&meta.article),
        esc(&slug),
        meta.entry_loop
    );
    page.push_str(&notice_html(&state.outcome(&slug)));
    for pending in state.pending_for(&slug) {
        page.push_str(&approval_html(&pending, false));
    }
    if let Ok(ledger) = Ledger::load(&dir.join("ledger.json"))
        && ledger.has_sweep_state()
    {
        page.push_str(&sources_section(&slug, &ledger));
    }
    page.push_str(&draft_section(&slug, &dir));
    page.push_str(&review_section(&slug, &dir));
    page.push_str(&publish_section(&slug, &meta));
    Html(crate::ui::page(&meta.article, &[("Sessions", "/")], &page))
}

/// A source's sweep state in plain words.
fn source_status_label(s: &crate::ledger::SourceEntry) -> String {
    use crate::sweep::status;
    if s.fetched_text.as_deref().is_some_and(|t| !t.is_empty()) {
        return if s
            .fetched_via
            .as_deref()
            .is_some_and(|v| v.starts_with("operator"))
        {
            "Text attached by you".into()
        } else {
            "Fetched".into()
        };
    }
    match s.sweep_status.as_deref() {
        Some(status::PENDING) => "Not fetched yet".into(),
        Some(status::FETCHED) => "Fetched".into(),
        Some(status::NEEDS_OPERATOR) => "Could not be fetched".into(),
        Some(status::SNAPSHOT_AVAILABLE) => "Archived snapshot found, not fetched yet".into(),
        Some(status::NO_TEXT) => "No readable text".into(),
        Some(other) => other.replace('_', " "),
        None => "Not part of the sweep".into(),
    }
}

/// One row of the sources table; `needs` marks a source the sweep gate is
/// still waiting on.
fn source_row(slug: &str, s: &crate::ledger::SourceEntry, needs: bool) -> String {
    let link = if s.url.starts_with("http") {
        format!(
            "<a href=\"{}\">{}</a>",
            esc(&s.url),
            esc(&crate::render::source_link_text(s))
        )
    } else {
        esc(&s.url)
    };
    let form = format!(
        "<form method=post action=\"/sessions/{slug}/sweep-dispose\" class=\"row\">\
         <input type=hidden name=source value=\"{id}\">\
         <input name=disposition list=\"dispositions\" required \
         placeholder=\"Why the text cannot be had\" aria-label=\"Disposition for {id}\">\
         <button>Sign disposition</button></form>",
        id = esc(&s.id)
    );
    let disposition = match (&s.disposition, needs) {
        (Some(d), _) => format!(
            "{} <details><summary>Change</summary>{form}</details>",
            esc(d)
        ),
        (None, true) => form,
        (None, false) => "Not needed".to_string(),
    };
    format!(
        "<tr><td><span class=\"sid\">{}</span>{link}</td><td{}>{}</td><td>{disposition}</td></tr>\n",
        esc(&s.id),
        if needs { " class=\"needs\"" } else { "" },
        esc(&source_status_label(s)),
    )
}

/// The source sweep: per-source status, the disposition form for every
/// source that still needs one, the batch fetch, and operator capture.
fn sources_section(slug: &str, ledger: &Ledger) -> String {
    use crate::sweep::status;
    let unresolved: Vec<&str> = ledger
        .sweep_unresolved()
        .iter()
        .map(|(s, _)| s.id.as_str())
        .collect();
    let total = ledger.sources.len();
    let mut html = String::from("<h2>Sources</h2>\n");
    if unresolved.is_empty() {
        let _ = writeln!(
            html,
            "<p>All {} have text or a signed disposition.</p>",
            plural(total, "source")
        );
    } else {
        let _ = writeln!(
            html,
            "<p><strong>{} of {}</strong> still need text or a signed disposition. Fetch \
             them, attach text you captured yourself, or record why the text cannot be had.</p>",
            unresolved.len(),
            plural(total, "source")
        );
    }
    html.push_str(
        "<datalist id=\"dispositions\"><option value=\"attested-unreachable\">\
         <option value=\"dropped: paywall\"><option value=\"dropped: dead link\"></datalist>\n\
         <div class=\"scroll\"><table><tr><th>Source</th><th>Status</th><th>Disposition</th></tr>\n",
    );
    for s in &ledger.sources {
        html.push_str(&source_row(slug, s, unresolved.contains(&s.id.as_str())));
    }
    html.push_str("</table></div>\n");
    let fetchable = ledger.sources.iter().any(|s| {
        matches!(
            s.sweep_status.as_deref(),
            Some(status::PENDING | status::SNAPSHOT_AVAILABLE)
        )
    });
    if fetchable {
        let _ = writeln!(
            html,
            "<form method=post action=\"/sessions/{slug}/sweep-fetch\" class=\"row\">\
             <button>Fetch pending sources</button></form>"
        );
    }
    let mut options = String::new();
    for s in ledger
        .sources
        .iter()
        .filter(|s| s.fetched_text.as_deref().is_none_or(str::is_empty))
    {
        let _ = write!(
            options,
            "<option value=\"{id}\">{id}: {}</option>",
            esc(&crate::render::source_link_text(s)),
            id = esc(&s.id)
        );
    }
    if !options.is_empty() {
        let _ = writeln!(
            html,
            "<details class=\"panel\"><summary>Attach text you captured yourself</summary>\
             <form method=post action=\"/sessions/{slug}/attach\" class=\"stack\">\
             <label class=\"field\">Source<select name=source>{options}</select></label>\
             <label class=\"field\">Text of the page\
             <textarea name=text rows=6 required></textarea></label>\
             <button>Attach text</button></form></details>"
        );
    }
    html
}

/// The model-drafting step: how many findings exist, whether an edit is
/// staged, and the two judgment-point actions.
fn draft_section(slug: &str, dir: &std::path::Path) -> String {
    let findings = crate::session::FindingsFile::load(&dir.join("findings.json"))
        .map_or(0, |f| f.findings.len());
    let base = std::fs::read_to_string(dir.join("base.wikitext")).unwrap_or_default();
    let proposed = std::fs::read_to_string(dir.join("proposed.wikitext")).unwrap_or_default();
    let staged = !proposed.trim().is_empty() && proposed != base;
    format!(
        "<h2>Draft</h2>\n<p>{} {}</p>\n<div class=\"row\">\
         <form method=post action=\"/sessions/{slug}/driver/findings\">\
         <button>Write findings</button></form>\
         <form method=post action=\"/sessions/{slug}/driver/propose\">\
         <button{}>Draft the edit</button></form></div>\n\
         <p class=\"meta\">Both call the drafting model. A finding is kept only if it quotes a \
         fetched source, and the draft is checked when you render the review.{}</p>\n",
        if findings == 0 {
            "No findings yet.".to_string()
        } else {
            format!("{}.", plural(findings, "finding"))
        },
        if staged {
            "An edit is staged."
        } else {
            "No edit is staged yet."
        },
        if findings == 0 { " disabled" } else { "" },
        if findings == 0 {
            " Drafting the edit needs at least one finding."
        } else {
            ""
        },
    )
}

/// The review step: where the review stands, the render form (round
/// prefilled with the one that makes sense next), and the comment queue.
fn review_section(slug: &str, dir: &std::path::Path) -> String {
    let state = artifact_state(dir);
    let mut html = String::from("<h2>Review</h2>\n");
    match &state {
        Some(ArtifactState::Current { round }) => {
            let _ = writeln!(
                html,
                "<p>Round {round} is ready to read. Comment under any paragraph there; \
                 publish from the same page when it reads right.</p>\
                 <p><a class=\"btn primary\" href=\"/sessions/{slug}/review\">Open the review</a></p>"
            );
        }
        Some(ArtifactState::Stale { round, kind }) => {
            let _ = writeln!(
                html,
                "<p>The round {round} review is out of date. {} \
                 <a href=\"/sessions/{slug}/review\">View it anyway</a></p>",
                kind.sentence()
            );
        }
        None => html.push_str("<p>Nothing has been rendered for review yet.</p>\n"),
    }
    let _ = writeln!(
        html,
        "<form method=post action=\"/sessions/{slug}/render\" class=\"row\">\
         <label class=\"field\">Round<input name=round type=number min=1 value={} \
         style=\"width:5rem\"></label>\
         <label class=\"field grow\">What changed this round\
         <input name=summary placeholder=\"e.g. corrected the marriage date\"></label>\
         <button>Render review</button></form>",
        next_round(state.as_ref())
    );
    html.push_str("<h3>Comments</h3>\n");
    html.push_str(&queue_html(slug, dir));
    html
}

/// The publish step on the session page (the review page carries its own
/// copy, next to the evidence).
fn publish_section(slug: &str, meta: &SessionMeta) -> String {
    let mut html = String::from("<h2>Publish</h2>\n");
    if let Some(last) = &meta.last_published_diff_url {
        let _ = writeln!(
            html,
            "<p>Last published: <a href=\"{0}\">{0}</a></p>",
            esc(last)
        );
    }
    let _ = writeln!(html, "{}", publish_form(slug, false, false));
    html
}

/// The publish form and what pressing it does.
fn publish_form(slug: &str, from_review: bool, primary: bool) -> String {
    format!(
        "<form method=post action=\"/sessions/{slug}/publish{}\" class=\"row\">\
         <label class=\"field grow\">Edit summary\
         <input name=summary required placeholder=\"Shown in the page history\"></label>\
         <button{}>Publish this edit</button></form>\
         <p class=\"meta\">The checks run first. You then see the exact edit and approve it \
         before anything is written to Wikipedia.</p>",
        if from_review { "?from=review" } else { "" },
        if primary { " class=\"primary\"" } else { "" },
    )
}

/// GET /sessions/{slug}/review — the review artifact, served in-app with
/// the comment UI and — when the review is live — the publish action and
/// any pending publish approval injected, so the whole review workflow
/// (read → comment → apply → re-render → publish → approve) happens on
/// THIS page, not a hop away (operator shakedown catches: the footer
/// pointed at an "approve" that lived on a different page under a
/// different name, greyed-out as metadata).
async fn review_artifact(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    axum::extract::Query(q): Params,
) -> axum::response::Response {
    let path = session_dir(&slug).join("review.html");
    let artifact = known_session(&slug)
        .then(|| std::fs::read_to_string(&path).ok())
        .flatten();
    match artifact {
        Some(html) => Html(inject_comment_ui(
            &slug,
            &html,
            &state.pending_for(&slug),
            q.get("notice").map(String::as_str),
            &state.outcome(&slug),
        ))
        .into_response(),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Html(crate::ui::page(
                "No review yet",
                &[("Sessions", "/")],
                &format!(
                    "<h1>No review yet</h1><p>Nothing has been rendered for this session. \
                     <a href=\"/sessions/{}\">Render a review from the session page.</a></p>",
                    esc(&slug)
                ),
            )),
        )
            .into_response(),
    }
}

/// The review artifact's freshness relative to the CURRENT session state
/// (plan-004 staleness guard, from the operator's shakedown catch: an old
/// page-creation draft was served as if it were the live review). No new
/// state: reconciled from facts the session already records —
/// `rounds.jsonl` (append-only, file order = event order: a `published`
/// or `comments-resolved` entry after the last `rendered` makes the
/// artifact history) and file mtimes (a `proposed.wikitext`/`base`
/// touched after the render is a hand edit the artifact never saw).
enum ArtifactState {
    /// The render is the session's latest event — commenting is live.
    Current { round: u32 },
    /// Do not comment against this artifact: forms suppressed, banner
    /// shown, driver-resolve refuses.
    Stale { round: u32, kind: StaleKind },
}

/// Why a rendered review no longer describes the session.
#[derive(Clone, Copy)]
enum StaleKind {
    Published,
    CommentsApplied,
    TextChanged,
}

impl StaleKind {
    /// Clause form, for the console and the driver's refusal message.
    fn short(self) -> &'static str {
        match self {
            Self::Published => "the edit was published",
            Self::CommentsApplied => "comments were applied after the render",
            Self::TextChanged => "the text changed after the render",
        }
    }

    /// Full sentence with the next step, for banners.
    fn sentence(self) -> &'static str {
        match self {
            Self::Published => "This edit was published, so the review is closed.",
            Self::CommentsApplied => {
                "Comments were applied after it was rendered. Render again to review the \
                 revised text."
            }
            Self::TextChanged => {
                "The session's text changed after it was rendered (a hand edit?). Render \
                 again before commenting."
            }
        }
    }
}

/// The round a render started now should carry: the first round, the same
/// round again while nothing was reviewed in between, else the next one.
fn next_round(state: Option<&ArtifactState>) -> u32 {
    match state {
        None => 1,
        Some(
            ArtifactState::Current { round }
            | ArtifactState::Stale {
                round,
                kind: StaleKind::TextChanged,
            },
        ) => (*round).max(1),
        Some(ArtifactState::Stale { round, .. }) => round + 1,
    }
}

fn artifact_state(dir: &std::path::Path) -> Option<ArtifactState> {
    // The render under discussion is the LAST `rendered` round entry.
    let rounds: Vec<crate::session::RoundEntry> = std::fs::read_to_string(dir.join("rounds.jsonl"))
        .ok()?
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let render_idx = rounds.iter().rposition(|e| e.phase == "rendered")?;
    let round = rounds[render_idx].round;
    // Anything the loop did AFTER that render makes the artifact history.
    let later: &[crate::session::RoundEntry] = &rounds[render_idx + 1..];
    let stale = |kind| Some(ArtifactState::Stale { round, kind });
    if later.iter().any(|e| e.phase == "published") {
        return stale(StaleKind::Published);
    }
    if later.iter().any(|e| e.phase == "comments-resolved") {
        return stale(StaleKind::CommentsApplied);
    }
    // A hand edit (or anything else) touched the session text after the
    // artifact was written: mtime is the basic on-disk fact.
    let artifact_mtime = std::fs::metadata(dir.join("review.html"))
        .ok()?
        .modified()
        .ok()?;
    let text_touched_after = ["proposed.wikitext", "base.wikitext"]
        .iter()
        .filter_map(|f| std::fs::metadata(dir.join(f)).ok())
        .filter_map(|m| m.modified().ok())
        .any(|t| t > artifact_mtime);
    if text_touched_after {
        return stale(StaleKind::TextChanged);
    }
    Some(ArtifactState::Current { round })
}

/// The per-block insertion plan for [`inject_comment_ui`]: for each
/// changed block and evidence card, the comment thread to insert directly
/// after that element's closing `</div>` (blocks are flat — the next
/// `</div>` after the opening tag is the element's own close), plus the
/// anchors that received a form (everything else lands in the stray
/// footer). A thread is the block's existing comments, then the collapsed
/// "Comment" form(s) — one for a plain block, new-wording and
/// removed-wording forms for a changed pair.
fn block_insertions(
    slug: &str,
    artifact_html: &str,
    queue: &crate::comments::CommentQueue,
) -> (Vec<(usize, String)>, Vec<String>) {
    let blocks = crate::render::review_targets(artifact_html);
    let evidence = crate::render::evidence_targets(artifact_html);
    let cards = |anchor: &str, place: &str, back: &str| -> String {
        comment_cards(
            slug,
            queue.comments.iter().filter(|c| c.target == anchor),
            |_| place.to_string(),
            back,
        )
    };
    let div_end = |id: &str| -> Option<usize> {
        let needle = format!("id=\"{id}\"");
        let i = artifact_html.find(&needle)?;
        let close = artifact_html[i..].find("</div>")? + i + "</div>".len();
        Some(close)
    };

    let mut insertions: Vec<(usize, String)> = Vec::new();
    let mut placed: Vec<String> = Vec::new();
    for block in &blocks {
        let Some(end) = div_end(&block.element_id) else {
            continue;
        };
        let back = format!("?from=review&amp;at={}", block.element_id);
        let mut html = String::from("<div class=\"wa-thread\">");
        for old in &block.old_sides {
            html.push_str(&cards(
                &old.wikitext_anchor,
                " on the removed wording",
                &back,
            ));
            placed.push(old.wikitext_anchor.clone());
        }
        html.push_str(&cards(&block.wikitext_anchor, "", &back));
        placed.push(block.wikitext_anchor.clone());
        if block.old_sides.is_empty() {
            html.push_str(&comment_form(
                slug,
                &block.wikitext_anchor,
                &back,
                "Comment",
                "What should change here?",
            ));
        } else {
            html.push_str(&comment_form(
                slug,
                &block.wikitext_anchor,
                &back,
                "Comment on the new wording",
                "What should change in the new wording?",
            ));
            for old in &block.old_sides {
                html.push_str(&comment_form(
                    slug,
                    &old.wikitext_anchor,
                    &back,
                    "Comment on the removed wording",
                    "What about the removed wording?",
                ));
            }
        }
        html.push_str("</div>");
        insertions.push((end, html));
    }
    for ev in &evidence {
        let Some(end) = div_end(&ev.element_id) else {
            continue;
        };
        let back = format!("?from=review&amp;at={}", ev.element_id);
        let mut html = String::from("<div class=\"wa-thread\">");
        html.push_str(&cards(&ev.wikitext_anchor, "", &back));
        html.push_str(&comment_form(
            slug,
            &ev.wikitext_anchor,
            &back,
            "Comment on this source",
            "What is wrong with this source or its quote?",
        ));
        html.push_str("</div>");
        placed.push(ev.wikitext_anchor.clone());
        insertions.push((end, html));
    }
    (insertions, placed)
}

/// The banner on a review that no longer describes the session: why, and
/// (unless it was published) the render form for the round that replaces
/// it, right there.
fn stale_banner(slug: &str, round: u32, kind: StaleKind) -> String {
    let action = if matches!(kind, StaleKind::Published) {
        format!(" <a href=\"/sessions/{slug}\">Back to the session</a>")
    } else {
        let next = next_round(Some(&ArtifactState::Stale { round, kind }));
        format!(
            "<form method=post action=\"/sessions/{slug}/render?from=review\" class=\"row\">\
             <input type=hidden name=round value={next}>\
             <label class=\"field grow\">What changed this round\
             <input name=summary placeholder=\"e.g. applied the review comments\"></label>\
             <button class=\"primary\">Render round {next}</button></form>"
        )
    };
    format!(
        "<div class=\"notice warn\"><strong>This review is out of date.</strong> {}{action}</div>\n",
        kind.sentence()
    )
}

/// Serve-time injection of the review UI into the artifact: breadcrumbs,
/// a status bar with the apply-comments action, each changed block's and
/// evidence card's comment thread DIRECTLY beneath it (the block itself
/// is the context), the publish section, and the current stylesheet. The
/// on-disk artifact stays pristine (self-contained, structurally
/// unchanged — plan-004's invariant); this is presentation only.
///
/// `notice` says where to show the last action's `outcome`: `top` (under
/// the breadcrumbs) or `publish` (in the publish section).
fn inject_comment_ui(
    slug: &str,
    artifact_html: &str,
    pendings: &[(String, String, String)],
    notice: Option<&str>,
    outcome: &str,
) -> String {
    let dir = session_dir(slug);
    let queue =
        crate::comments::CommentQueue::load(&dir.join("comments.jsonl")).unwrap_or_default();
    let state = artifact_state(&dir);
    let article = read_meta(slug).map_or_else(|| slug.to_string(), |m| m.article);
    let mut head = crate::ui::crumbs(
        &[("Sessions", "/"), (&article, &format!("/sessions/{slug}"))],
        "Review",
    );
    if notice == Some("top") {
        head.push_str(&notice_html(outcome));
    }

    let mut out = artifact_html.to_string();
    if let Some(ArtifactState::Stale { round, kind }) = state {
        // A STALE artifact is history: no comment forms (never comment
        // against an old anchor table), just the banner saying what
        // happened and what to do. Everything below stays read-only.
        head.push_str(&stale_banner(slug, round, kind));
    } else {
        let (mut insertions, placed_anchors) = block_insertions(slug, artifact_html, &queue);

        // Apply right-to-left so earlier offsets stay valid.
        insertions.sort_by_key(|(pos, _)| std::cmp::Reverse(*pos));
        for (pos, html) in insertions {
            out.insert_str(pos, &html);
        }

        let open_count = queue.open().len();
        if open_count == 0 {
            head.push_str(
                "<div class=\"wa-bar\"><span>No open comments. Use <strong>Comment</strong> \
                 under any paragraph or source to ask for a change, or publish at the bottom \
                 of the page.</span></div>\n",
            );
        } else {
            let _ = writeln!(
                head,
                "<div class=\"wa-bar\"><span><strong>{}.</strong> Applying them has the \
                 drafting model revise each commented paragraph; you then render the next \
                 round. Comments on sources stay open for you to resolve by hand.</span>\
                 <form method=post action=\"/sessions/{slug}/driver/resolve?from=review\">\
                 <button class=\"primary\">Apply comments</button></form></div>",
                plural(open_count, "open comment")
            );
        }

        // The publish leg lives HERE: a pending approval, if one exists,
        // renders as the prominent block (approve/decline); otherwise the
        // publish form sits at the end of the diff — the reviewer decides
        // with the evidence in view, not a page-hop away.
        let publish_html = if pendings.is_empty() {
            format!(
                "<section class=\"wa-publish\" id=\"wa-publish\"><h2>Publish</h2>{}{}{}</section>\n",
                if notice == Some("publish") {
                    notice_html(outcome)
                } else {
                    String::new()
                },
                if open_count == 0 {
                    String::new()
                } else {
                    format!(
                        "<p>{} still open. Publishing now sends the text as shown above.</p>",
                        plural(open_count, "comment")
                    )
                },
                publish_form(slug, true, open_count == 0)
            )
        } else {
            pendings.iter().map(|p| approval_html(p, true)).collect()
        };
        if let Some(i) = out.rfind("</main>") {
            out.insert_str(i, &publish_html);
        }

        // Any comments that did NOT land under a block (unknown
        // targets, manual anchors): a catch-all footer before </main>.
        let stray = comment_cards(
            slug,
            queue
                .comments
                .iter()
                .filter(|c| !placed_anchors.contains(&c.target)),
            |c| format!(" on {}", target_label(&c.target)),
            "?from=review",
        );
        if !stray.is_empty() {
            let footer = format!(
                "<section class=\"wa-stray\"><h2>Other comments</h2>\
                 <p class=\"meta\">These are not attached to a paragraph or source shown on \
                 this page.</p>{stray}</section>\n"
            );
            if let Some(i) = out.rfind("</main>") {
                out.insert_str(i, &footer);
            }
        }
    }
    if let Some(i) = out.find("<main>") {
        out.insert_str(i + "<main>".len(), &head);
    }

    // The current stylesheet, after the artifact's own: an artifact
    // rendered by an older build still gets today's look.
    if let Some(i) = out.find("</head>") {
        out.insert_str(i, &format!("<style>\n{}</style>\n", crate::ui::CSS));
    }
    out
}

/// One collapsed comment form; the hidden target is the anchor, verbatim
/// (AC.2). `back` is the query string that returns the operator to this
/// block.
fn comment_form(slug: &str, target: &str, back: &str, label: &str, placeholder: &str) -> String {
    format!(
        "<details><summary>{label}</summary>\
         <form method=post action=\"/sessions/{slug}/comments{back}\">\
         <input type=hidden name=target value=\"{target}\">\
         <textarea name=text rows=2 required placeholder=\"{}\" aria-label=\"{label}\"></textarea>\
         <button>Add comment</button></form></details>",
        esc(placeholder)
    )
}

/// What a comment target points at, in reviewer language.
fn target_label(target: &str) -> String {
    if target.starts_with("ledger:") {
        return "a source".to_string();
    }
    crate::render::human_line_label(target)
        .unwrap_or_else(|| format!("<code>{}</code>", esc(target)))
}

/// Comment cards: open ones first (each with its by-hand resolve form),
/// then resolved ones with their notes. `place` says where a comment sits
/// (" on line 11", " on the removed wording", or nothing when the card is
/// already under its block).
fn comment_cards<'a>(
    slug: &str,
    comments: impl Iterator<Item = &'a crate::comments::Comment>,
    place: impl Fn(&crate::comments::Comment) -> String,
    back: &str,
) -> String {
    use crate::comments::CommentStatus;
    let (open, resolved): (Vec<_>, Vec<_>) =
        comments.partition(|c| c.status == CommentStatus::Open);
    let mut html = String::new();
    for c in open {
        let _ = write!(
            html,
            "<div class=\"wa-comment\"><p>{text}</p>{quoted}<div class=\"who\">\
             <span>{id}{place}</span><details><summary>Resolve by hand</summary>\
             <form method=post action=\"/sessions/{slug}/comments/resolve{back}\">\
             <input type=hidden name=id value=\"{id}\">\
             <input name=note placeholder=\"What you did (optional)\" aria-label=\"Resolution note\">\
             <button>Mark resolved</button></form></details></div></div>",
            text = esc(&c.text),
            quoted = c
                .quoted
                .as_ref()
                .map(|q| format!("<p class=\"meta\">Highlighted: “{}”</p>", esc(q)))
                .unwrap_or_default(),
            id = esc(&c.id),
            place = place(c),
        );
    }
    for c in resolved {
        let _ = write!(
            html,
            "<div class=\"wa-comment resolved\"><p>{}</p><div class=\"who\">\
             <span>{}{}, resolved{}</span></div></div>",
            esc(&c.text),
            esc(&c.id),
            place(c),
            c.resolution
                .as_deref()
                .filter(|r| !r.is_empty())
                .map(|r| format!(": {}", esc(r)))
                .unwrap_or_default()
        );
    }
    html
}

/// The session page's comment queue: the apply action when anything is
/// open, then every comment (open first) with where it points. Commenting
/// itself happens on the review page, under the text.
fn queue_html(slug: &str, dir: &std::path::Path) -> String {
    let queue = match crate::comments::CommentQueue::load(&dir.join("comments.jsonl")) {
        Ok(queue) if queue.comments.is_empty() => {
            return "<p class=\"meta\">No comments yet. They are added on the review page, \
                    under the paragraph they concern.</p>\n"
                .into();
        }
        Ok(queue) => queue,
        Err(e) => {
            return format!(
                "<div class=\"notice error\">The comment queue could not be read: {}</div>\n",
                esc(&e)
            );
        }
    };
    let mut html = String::new();
    let open = queue.open().len();
    if open > 0 {
        let _ = writeln!(
            html,
            "<form method=post action=\"/sessions/{slug}/driver/resolve\" class=\"row\">\
             <button>Apply {}</button></form>\
             <p class=\"meta\">The drafting model revises each commented paragraph. Comments \
             on sources stay open for you to resolve by hand.</p>",
            plural(open, "open comment")
        );
    }
    html.push_str(&comment_cards(
        slug,
        queue.comments.iter(),
        |c| format!(" on {}", target_label(&c.target)),
        "",
    ));
    html
}

#[derive(serde::Deserialize)]
struct CommentForm {
    target: String,
    text: String,
    #[serde(default)]
    quoted: Option<String>,
}

/// POST /sessions/{slug}/comments — append to the queue (the form's hidden
/// target is the block's wikitext anchor, verbatim from the artifact's
/// anchor table). Queue errors SURFACE: a malformed queue must never
/// silently become empty and re-allocate duplicate ids.
async fn comments_add(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    axum::extract::Query(q): Params,
    Form(form): Form<CommentForm>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let dir = session_dir(&slug);
    if form.text.trim().is_empty() {
        state.note_outcome(&slug, "comment failed: it was empty");
        return back_to(&slug, &q, Some("top")).into_response();
    }
    let added = match crate::comments::CommentQueue::load(&dir.join("comments.jsonl")) {
        Ok(mut queue) => {
            let comment = crate::comments::Comment::new(
                &form.target,
                &form.text,
                form.quoted.as_deref(),
                &crate::comments::now_iso(),
            );
            match queue.append(&dir.join("comments.jsonl"), comment) {
                Ok(id) => {
                    state.note_outcome(&slug, &format!("comment {id} added"));
                    true
                }
                Err(e) => {
                    state.note_outcome(&slug, &format!("comment queue write failed: {e}"));
                    false
                }
            }
        }
        Err(e) => {
            state.note_outcome(&slug, &format!("comment queue failed to load: {e}"));
            false
        }
    };
    back_to(&slug, &q, (!added).then_some("top")).into_response()
}

#[derive(serde::Deserialize)]
struct CommentResolveForm {
    id: String,
    note: String,
}

/// POST /sessions/{slug}/comments/resolve — manual resolution (the
/// evidence-card path and anything handled outside the driver step).
async fn comments_resolve(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    axum::extract::Query(q): Params,
    Form(form): Form<CommentResolveForm>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let dir = session_dir(&slug);
    let (resolved, outcome) = match crate::comments::CommentQueue::load(&dir.join("comments.jsonl"))
    {
        Ok(mut queue) => match queue.resolve(&dir.join("comments.jsonl"), &form.id, &form.note) {
            Ok(()) => (true, format!("comment {} resolved by hand", form.id)),
            Err(e) => (false, format!("comment resolve failed: {e}")),
        },
        Err(e) => (false, format!("comment queue failed to load: {e}")),
    };
    state.note_outcome(&slug, &outcome);
    back_to(&slug, &q, (!resolved).then_some("top")).into_response()
}

#[derive(serde::Deserialize)]
struct DisposeForm {
    source: String,
    disposition: String,
}

/// POST /sessions/{slug}/sweep-dispose — operator-signed disposition.
async fn sweep_dispose(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    Form(form): Form<DisposeForm>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let dir = session_dir(&slug);
    let disposition = form.disposition.trim();
    if disposition.is_empty() {
        // An empty disposition would resolve the sweep gate with no reason.
        state.note_outcome(
            &slug,
            &format!(
                "disposition for {} failed: say why the text cannot be had",
                form.source
            ),
        );
        return Redirect::to(&format!("/sessions/{slug}")).into_response();
    }
    let saved = Ledger::load(&dir.join("ledger.json")).and_then(|mut ledger| {
        ledger.set_disposition(&form.source, disposition)?;
        ledger.save(&dir.join("ledger.json"))
    });
    state.note_outcome(
        &slug,
        &match saved {
            Ok(()) => format!("disposition signed for {}: {disposition}", form.source),
            Err(e) => format!("disposition for {} failed: {e}", form.source),
        },
    );
    Redirect::to(&format!("/sessions/{slug}")).into_response()
}

/// POST /sessions/{slug}/sweep-fetch — batch fetch + classify (network).
async fn sweep_fetch_route(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    state.note_outcome(
        &slug,
        &match crate::cli::sweep_fetch(&slug).await {
            Ok(()) => "source fetch finished; statuses are in the table below".to_string(),
            Err(e) => format!("source fetch failed: {e}"),
        },
    );
    Redirect::to(&format!("/sessions/{slug}")).into_response()
}

#[derive(serde::Deserialize)]
struct AttachForm {
    source: String,
    text: String,
}

/// POST /sessions/{slug}/attach — ingest pasted operator-captured text
/// for one source (same ingestion as `wa ledger attach`).
async fn attach(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    Form(form): Form<AttachForm>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let dir = session_dir(&slug);
    let tmp = dir.join("attach-upload.txt");
    let attached = std::fs::write(&tmp, &form.text)
        .map_err(anyhow::Error::from)
        .and_then(|()| crate::cli::ledger_attach(&slug, &form.source, &tmp.to_string_lossy()));
    let _ = std::fs::remove_file(&tmp);
    state.note_outcome(
        &slug,
        &match attached {
            Ok(()) => format!("text attached to {}", form.source),
            Err(e) => format!("attaching text to {} failed: {e}", form.source),
        },
    );
    Redirect::to(&format!("/sessions/{slug}")).into_response()
}

#[derive(serde::Deserialize)]
struct PublishForm {
    summary: String,
}

/// POST /sessions/{slug}/driver/findings — judgment point 1: the model
/// authors findings from the swept ledger; output goes through the SAME
/// schema-validated admission as `wa findings add` (the step validates
/// before this handler appends).
async fn driver_findings(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
) -> axum::response::Response {
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let outcome = run_driver_findings(&state, &slug).await;
    state.note_outcome(&slug, &outcome);
    Redirect::to(&format!("/sessions/{slug}")).into_response()
}

async fn run_driver_findings(state: &Arc<ServeState>, slug: &str) -> String {
    use crate::driver::steps::FindingsContext;
    use crate::driver::steps::SourceDigest;
    let dir = session_dir(slug);
    let Ok(ledger) = Ledger::load(&dir.join("ledger.json")) else {
        return "driver findings: no ledger".into();
    };
    let Ok(base) = std::fs::read_to_string(dir.join("base.wikitext")) else {
        return "driver findings: no base wikitext".into();
    };
    let Ok(meta) = serde_json::from_str::<SessionMeta>(
        &std::fs::read_to_string(dir.join("session.json")).unwrap_or_default(),
    ) else {
        return "driver findings: no session meta".into();
    };
    let ctx = FindingsContext {
        article: meta.article,
        base_wikitext: base,
        sources: ledger
            .sources
            .iter()
            .filter(|s| s.fetched_text.is_some())
            .map(|s| SourceDigest {
                id: s.id.clone(),
                title: s
                    .metadata
                    .as_ref()
                    .and_then(|m| m.title.clone())
                    .unwrap_or_else(|| s.url.clone()),
                text: s.fetched_text.clone().unwrap_or_default(),
                quotes: ledger
                    .quotes
                    .iter()
                    .filter(|q| q.source_id == s.id)
                    .map(|q| (q.id.clone(), q.text.clone()))
                    .collect(),
            })
            .collect(),
        quote_ids: ledger.quotes.iter().map(|q| q.id.clone()).collect(),
        entry_loop: meta.entry_loop,
        max_findings: 3,
    };
    let zai = match state.zai_client() {
        Ok(z) => z,
        Err(e) => return format!("driver findings: {e}"),
    };
    match crate::driver::steps::author_findings(&zai, &ctx).await {
        Ok(findings) => {
            // Same admission as `wa findings add`: validate (the step
            // already did) and append without duplicate ids.
            let path = dir.join("findings.json");
            let mut file = crate::session::FindingsFile::load(&path).unwrap_or_default();
            for f in findings {
                if file.findings.iter().any(|e| e.id == f.id) {
                    continue;
                }
                file.findings.push(f);
            }
            match file.save(&path) {
                Ok(()) => format!("driver findings: {} in the session", file.findings.len()),
                Err(e) => format!("driver findings: save failed: {e}"),
            }
        }
        Err(e) => format!("driver findings: {e}"),
    }
}

/// POST /sessions/{slug}/driver/propose — judgment point 2: draft the
/// scoped edit for the session's first finding and write
/// `proposed.wikitext` (the gate runs at render/publish as always).
async fn driver_propose(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
) -> axum::response::Response {
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let outcome = run_driver_propose(&state, &slug).await;
    state.note_outcome(&slug, &outcome);
    Redirect::to(&format!("/sessions/{slug}")).into_response()
}

async fn run_driver_propose(state: &Arc<ServeState>, slug: &str) -> String {
    let dir = session_dir(slug);
    let Ok(file) = crate::session::FindingsFile::load(&dir.join("findings.json")) else {
        return "driver propose: no findings".into();
    };
    let Some(finding) = file.findings.first() else {
        return "driver propose: no findings".into();
    };
    let Ok(base) = std::fs::read_to_string(dir.join("base.wikitext")) else {
        return "driver propose: no base wikitext".into();
    };
    // The named refs on the page (offer them to the model).
    let named: Vec<String> = base
        .match_indices("<ref name=")
        .filter_map(|(i, _)| base[i..].split('"').nth(1).map(str::to_string))
        .collect();
    let zai = match state.zai_client() {
        Ok(z) => z,
        Err(e) => return format!("driver propose: {e}"),
    };
    // The block the finding anchors — the WHOLE line range (an anchor may
    // span several lines) — checked before the paid model call.
    let base_lines: Vec<&str> = base.lines().collect();
    let Some((start, end, base_block)) = anchor_line_range(&finding.wikitext_anchor)
        .and_then(|(s, e)| slice_lines(&base_lines, s, e).map(|block| (s, e, block)))
    else {
        return "driver propose: anchor line out of range".into();
    };
    // The evidence as the model must see it: verbatim quote + its source.
    let ledger = Ledger::load(&dir.join("ledger.json")).unwrap_or_default();
    let evidence: Vec<String> = finding
        .evidence
        .iter()
        .filter_map(|qid| ledger.quote(qid))
        .map(|q| {
            let source = ledger
                .sources
                .iter()
                .find(|s| s.id == q.source_id)
                .map(|s| format!("{} <{}>", crate::render::source_link_text(s), s.url))
                .unwrap_or_default();
            format!("- \"{}\" — {source}", q.text)
        })
        .collect();
    if evidence.len() != finding.evidence.len() {
        return "driver propose: the finding cites a quote that is not in the ledger".into();
    }
    match crate::driver::steps::draft_proposal(&zai, finding, &evidence, &base_block, &named).await
    {
        Ok(proposal) => {
            let proposed = apply_splices(
                &base,
                &[(start, end, proposal.proposed_wikitext_block.clone(), false)],
            );
            match std::fs::write(dir.join("proposed.wikitext"), proposed) {
                Ok(()) => format!(
                    "driver propose: block drafted ({}); render it, then review + publish",
                    proposal.edit_summary
                ),
                Err(e) => format!("driver propose: write failed: {e}"),
            }
        }
        Err(e) => format!("driver propose: {e}"),
    }
}

/// POST /sessions/{slug}/driver/resolve — judgment point 3, in-app: the
/// queue's OPEN comments mapped through the B.2 `resolve_comments` step
/// (reused verbatim), grouped by the enclosing changed block, with
/// applied/rejected/reply written back per group.
async fn driver_resolve(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    axum::extract::Query(q): Params,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let outcome = run_driver_resolve(&state, &slug).await;
    state.note_outcome(&slug, &outcome);
    back_to(&slug, &q, Some("top")).into_response()
}

/// `L{s}:C{a}-L{e}:C{b}` (optionally `base:`-prefixed) → 1-based
/// inclusive line range.
fn anchor_line_range(anchor: &str) -> Option<(usize, usize)> {
    let rest = anchor.strip_prefix("base:").unwrap_or(anchor);
    let mut parts = rest.split('-');
    let start = parts
        .next()?
        .strip_prefix('L')?
        .split(':')
        .next()?
        .parse()
        .ok()?;
    let end = parts
        .next()?
        .strip_prefix('L')?
        .split(':')
        .next()?
        .parse()
        .ok()?;
    if end < start {
        return None;
    }
    Some((start, end))
}

/// One comment group = one changed block: a changed pair's old-side
/// (`base:`-anchored) and new-side (`wa-N`) comments MERGE into one group
/// — both sides of a pair share the same line, and splicing per
/// element-id would silently clobber one of the two.
struct CommentGroup {
    /// The block's new-side anchor (`None` for pure deletions).
    new_anchor: Option<String>,
    /// The pair's old-side `base:` anchor (`None` for pure insertions).
    old_anchor: Option<String>,
    /// Queue ids of this group's open comments.
    ids: Vec<String>,
}

/// The lines `s..=e` (1-based, inclusive) or `None` when out of range.
fn slice_lines(lines: &[&str], s: usize, e: usize) -> Option<String> {
    if s >= 1 && e >= s && e <= lines.len() {
        Some(lines[s - 1..e].join("\n"))
    } else {
        None
    }
}

#[allow(clippy::too_many_lines)]
async fn run_driver_resolve(state: &Arc<ServeState>, slug: &str) -> String {
    let dir = session_dir(slug);
    let Ok(artifact) = std::fs::read_to_string(dir.join("review.html")) else {
        return "driver resolve: no review artifact — render first".into();
    };
    let Ok(base) = std::fs::read_to_string(dir.join("base.wikitext")) else {
        return "driver resolve: no base wikitext".into();
    };
    let Ok(mut proposed) = std::fs::read_to_string(dir.join("proposed.wikitext")) else {
        return "driver resolve: no proposed wikitext".into();
    };
    let mut queue = match crate::comments::CommentQueue::load(&dir.join("comments.jsonl")) {
        Ok(q) => q,
        Err(e) => return format!("driver resolve: {e}"),
    };
    if queue.open().is_empty() {
        return "driver resolve: no open comments".into();
    }
    // Staleness guard: never splice revisions based on an artifact that
    // predates a publish, a comment round, or a hand edit — the anchor
    // table would not describe the current text (operator shakedown
    // catch: an old page-creation draft served as current).
    match artifact_state(&dir) {
        Some(ArtifactState::Stale { kind, .. }) => {
            return format!(
                "driver resolve: the review artifact is out of date ({}) — re-render \
                 before applying comments",
                kind.short()
            );
        }
        Some(ArtifactState::Current { .. }) => {}
        None => {
            return "driver resolve: no current review artifact — render first".into();
        }
    }

    let (groups, evidence, unknown, id_to_anchor) = bucket_groups(&artifact, &queue);

    let base_lines: Vec<&str> = base.lines().collect();
    let zai = match state.zai_client() {
        Ok(z) => z,
        Err(e) => return format!("driver resolve: {e}"),
    };
    let mut outcome = resolve_groups(
        &zai,
        &groups,
        &mut queue,
        &dir.join("comments.jsonl"),
        &proposed,
        &base_lines,
        &id_to_anchor,
    )
    .await;
    outcome.errors.extend(
        unknown
            .iter()
            .map(|u| format!("UNRESOLVED target {u} — left open")),
    );

    // Apply splices (one per group) and persist.
    if !outcome.splices.is_empty() {
        proposed = apply_splices(&proposed, &outcome.splices);
        if let Err(e) = std::fs::write(dir.join("proposed.wikitext"), &proposed) {
            return format!(
                "driver resolve: comments resolved but the proposed.wikitext write failed: {e}"
            );
        }
    }

    // Round log entry when anything resolved.
    if !outcome.resolved_ids.is_empty() {
        let entry = crate::session::RoundEntry {
            round: 0,
            timestamp: crate::comments::now_iso(),
            summary: format!("driver: {} comment(s) resolved", outcome.resolved_ids.len()),
            phase: "comments-resolved".into(),
            detail: outcome.resolved_ids.clone(),
        };
        if let Ok(json) = serde_json::to_string(&entry)
            && let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("rounds.jsonl"))
        {
            let _ = writeln!(file, "{json}");
        }
    }

    let mut out = if outcome.splices.is_empty() {
        format!(
            "driver resolve: {} comment(s) resolved ({})",
            outcome.resolved_ids.len(),
            outcome.resolved_ids.join(", ")
        )
    } else {
        format!(
            "driver resolve: {} comment(s) resolved ({}); proposed.wikitext updated — re-render to review",
            outcome.resolved_ids.len(),
            outcome.resolved_ids.join(", ")
        )
    };
    if !evidence.is_empty() {
        let _ = write!(
            out,
            "; evidence comments left for manual resolution: {}",
            evidence.join(", ")
        );
    }
    for e in &outcome.errors {
        let _ = write!(out, "; ERROR {e}");
    }
    out
}

/// Build the comment groups from the artifact and bucket the queue's OPEN
/// comments into them. Returns the groups, the evidence comment ids
/// (excluded — sources, not wikitext), the unknown-target comments (left
/// open), and the element-id → anchor map.
fn bucket_groups(
    artifact: &str,
    queue: &crate::comments::CommentQueue,
) -> (
    Vec<CommentGroup>,
    Vec<String>,
    Vec<String>,
    std::collections::HashMap<String, String>,
) {
    use crate::comments::CommentStatus;

    // The changed blocks as groups: a changed pair's old-side (`base:`)
    // and new-side (`wa-N`) anchors both belong to ONE group (the old id
    // rides INSIDE the new-side block); a pure deletion is its own group
    // with only a base anchor.
    let mut groups: Vec<CommentGroup> = crate::render::review_targets(artifact)
        .iter()
        .map(|b| {
            let is_deletion = b.wikitext_anchor.starts_with("base:");
            CommentGroup {
                new_anchor: (!is_deletion).then(|| b.wikitext_anchor.clone()),
                old_anchor: if is_deletion {
                    Some(b.wikitext_anchor.clone())
                } else {
                    b.old_sides.first().map(|o| o.wikitext_anchor.clone())
                },
                ids: Vec::new(),
            }
        })
        .collect();
    let id_to_anchor: std::collections::HashMap<String, String> =
        crate::render::read_anchor_table_str(artifact)
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.element_id, e.wikitext_anchor))
            .collect();
    let mut evidence = Vec::new();
    let mut unknown = Vec::new();
    for c in queue
        .comments
        .iter()
        .filter(|c| c.status == CommentStatus::Open)
    {
        if c.is_evidence() {
            evidence.push(c.id.clone());
            continue;
        }
        let anchor = id_to_anchor
            .get(&c.target)
            .cloned()
            .unwrap_or_else(|| c.target.clone());
        let group = groups.iter_mut().find(|g| {
            g.new_anchor.as_deref() == Some(anchor.as_str())
                || g.old_anchor.as_deref() == Some(anchor.as_str())
        });
        match group {
            Some(g) => g.ids.push(c.id.clone()),
            None => unknown.push(format!("{} (target {})", c.id, c.target)),
        }
    }
    (groups, evidence, unknown, id_to_anchor)
}

/// Do two planned splices clobber each other? Replacement ranges are
/// original line ranges `[s..=e]`; an insertion sits before original line
/// `s`. Two insertions at the same boundary apply in a deterministic
/// order (not a clobber); any other intersection is.
fn splices_overlap(
    (s, e, insert): (usize, usize, bool),
    (ps, pe, pinsert): (usize, usize, bool),
) -> bool {
    match (insert, pinsert) {
        (true, true) => false,
        (true, false) => ps <= s && s <= pe,
        (false, true) => s <= ps && ps <= e,
        (false, false) => s <= pe && ps <= e,
    }
}

/// One pass over the comment groups: per group with comments, validate the
/// splice plan (before the model call), reject overlaps with an already
/// accepted group, one `resolve_comments` model call, and the queue
/// writeback (that group's combined applied/rejected/reply note).
struct GroupsOutcome {
    /// `(start_line, end_line, replacement, insertion)` per resolved
    /// group, applied later in descending order.
    splices: Vec<(usize, usize, String, bool)>,
    resolved_ids: Vec<String>,
    errors: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
async fn resolve_groups(
    zai: &crate::driver::model::ZaiClient,
    groups: &[CommentGroup],
    queue: &mut crate::comments::CommentQueue,
    comments_path: &std::path::Path,
    proposed: &str,
    base_lines: &[&str],
    id_to_anchor: &std::collections::HashMap<String, String>,
) -> GroupsOutcome {
    use crate::driver::steps::DriverComment;

    let mut out = GroupsOutcome {
        splices: Vec::new(),
        resolved_ids: Vec::new(),
        errors: Vec::new(),
    };
    for group in groups {
        if group.ids.is_empty() {
            continue;
        }
        let group_label = group
            .new_anchor
            .clone()
            .or_else(|| group.old_anchor.clone())
            .unwrap_or_default();
        // Validate the splice plan BEFORE the model call: an unsplicable
        // group must not burn a paid call whose output is then discarded.
        let plan = match splice_plan(group, proposed, base_lines) {
            Ok(plan) => plan,
            Err(e) => {
                out.errors.push(format!(
                    "group {group_label} ({}): {e}",
                    group.ids.join(", ")
                ));
                continue;
            }
        };
        // Overlap guard: two groups can carry coincident line anchors
        // (shared opening phrases / fallback collisions); applying both
        // would let the second replacement silently clobber the first
        // while BOTH note "applied". First group in artifact order wins;
        // the overlapping group errors and its comments stay open.
        let (s, e, insert) = plan;
        if out
            .splices
            .iter()
            .any(|(ps, pe, _, pinsert)| splices_overlap((s, e, insert), (*ps, *pe, *pinsert)))
        {
            out.errors.push(format!(
                "group {group_label} ({}): line range overlaps another group's splice — \
                 resolve the blocks one at a time",
                group.ids.join(", ")
            ));
            continue;
        }
        let driver_comments: Vec<DriverComment> = group
            .ids
            .iter()
            .filter_map(|id| queue.comments.iter().find(|c| &c.id == id))
            .map(|c| {
                let prompt = match &c.quoted {
                    Some(q) => format!("{} (operator highlighted: {q})", c.text),
                    None => c.text.clone(),
                };
                let anchor = id_to_anchor
                    .get(&c.target)
                    .cloned()
                    .unwrap_or_else(|| c.target.clone());
                DriverComment { prompt, anchor }
            })
            .collect();
        // Blocks for the step: the new side's proposed text and the old
        // side's base text (a pure deletion has no proposed text — the
        // wording is gone; the step decides whether the removal changes).
        let proposed_block = group
            .new_anchor
            .as_deref()
            .and_then(anchor_line_range)
            .and_then(|(s, e)| slice_lines(&proposed.lines().collect::<Vec<_>>(), s, e))
            .unwrap_or_default();
        let base_block = group
            .old_anchor
            .as_deref()
            .and_then(anchor_line_range)
            .and_then(|(s, e)| slice_lines(base_lines, s, e))
            .unwrap_or_default();
        let resolution = match crate::driver::steps::resolve_comments(
            zai,
            &proposed_block,
            &base_block,
            &driver_comments,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                out.errors.push(format!(
                    "group {group_label} ({}): {e}",
                    group.ids.join(", ")
                ));
                continue;
            }
        };
        // Comments resolve BEFORE the spliced proposed.wikitext is
        // persisted (the caller writes it after this loop): a crash in
        // between leaves "applied" notes on an edit that never landed —
        // accepted for a local single-operator tool; the queue note names
        // the group so the skew is visible.
        out.splices
            .push((s, e, resolution.proposed_wikitext_block.clone(), insert));
        let note = resolution_note(&resolution);
        for id in &group.ids {
            if queue.resolve(comments_path, id, &note).is_ok() {
                out.resolved_ids.push(id.clone());
            }
        }
    }
    out
}

/// Where a group's revised block splices into `proposed.wikitext`:
/// `(start_line, end_line, insertion)` (1-based, inclusive; `insertion`
/// splices BEFORE the given 0-based index instead of replacing a range).
///
/// # Errors
/// A new-side anchor out of range (or unparsable), or — for pure
/// deletions — a line-alignment failure: base and proposed must agree on
/// every line before the deletion (hand edits and multi-line revisions
/// break the mapping; a blind splice would edit the wrong line).
fn splice_plan(
    group: &CommentGroup,
    proposed: &str,
    base_lines: &[&str],
) -> Result<(usize, usize, bool), String> {
    if let Some(a) = &group.new_anchor {
        let (s, e) =
            anchor_line_range(a).ok_or_else(|| format!("unparsable new-side anchor {a}"))?;
        let proposed_line_count = proposed.lines().count();
        if s >= 1 && e <= proposed_line_count && e >= s {
            Ok((s, e, false))
        } else {
            Err(format!(
                "new-side anchor {a} out of range for proposed.wikitext ({proposed_line_count} lines)"
            ))
        }
    } else if let Some(a) = &group.old_anchor {
        let (s, _e) = anchor_line_range(a).ok_or_else(|| format!("unparsable base anchor {a}"))?;
        let proposed_lines: Vec<&str> = proposed.lines().collect();
        if s >= 1
            && s - 1 <= proposed_lines.len()
            && base_lines.get(..s - 1) == proposed_lines.get(..s - 1)
        {
            Ok((s, s - 1, true)) // insert before old base line s
        } else {
            Err(format!(
                "base anchor {a} misaligned: base and proposed disagree before line {s} — resolve by hand"
            ))
        }
    } else {
        Err("group has no anchors".into())
    }
}

/// The combined note for one group's comments: applied + rejected lines
/// and the model's reply (the B.2 contract's uncorrelated one-liners;
/// every comment in the group gets the group's combined note).
fn resolution_note(resolution: &crate::driver::steps::Resolution) -> String {
    let mut note = String::new();
    for line in &resolution.applied {
        let _ = writeln!(note, "applied: {line}");
    }
    for line in &resolution.rejected {
        let _ = writeln!(note, "rejected: {line}");
    }
    let _ = write!(note, "reply: {}", resolution.reply);
    note
}

/// Apply the collected splices in DESCENDING start-line order (so earlier
/// ranges stay valid), one per group. Trailing newline preserved.
fn apply_splices(proposed: &str, splices: &[(usize, usize, String, bool)]) -> String {
    let mut ordered: Vec<&(usize, usize, String, bool)> = splices.iter().collect();
    ordered.sort_by_key(|(start, _, _, _)| std::cmp::Reverse(*start));
    let mut lines: Vec<String> = proposed.lines().map(str::to_string).collect();
    for (s, e, replacement, insert) in ordered {
        let repl_lines: Vec<String> = replacement.lines().map(str::to_string).collect();
        if *insert {
            // e == s-1 is the 0-based insertion index.
            lines.splice(e..e, repl_lines);
        } else {
            lines.splice(s - 1..*e, repl_lines);
        }
    }
    let mut out = lines.join("\n");
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[derive(serde::Deserialize)]
struct RenderForm {
    round: u32,
    summary: String,
    /// Offline/test hooks: fixture Parsoid HTML paths, passed through to
    /// `render_cmd`'s existing `--html-base`/`--html-proposed` flags (an
    /// empty value means live Parsoid).
    #[serde(default)]
    html_base: Option<String>,
    #[serde(default)]
    html_proposed: Option<String>,
}

/// POST /sessions/{slug}/render — gate + render the artifact, then show
/// it (or show why the gate refused). The web
/// path never opens or probes lavish (`Via::Web`): the review surface is
/// the in-app artifact + the comment forms (plan-004).
async fn render(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    axum::extract::Query(q): Params,
    Form(form): Form<RenderForm>,
) -> axum::response::Response {
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let pair = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
    };
    let rendered = crate::cli::render_cmd(
        &slug,
        form.round,
        pair(&form.html_base),
        pair(&form.html_proposed),
        &form.summary,
        true,
        false,
        crate::cli::Via::Web,
    )
    .await;
    // A successful render lands on the review it produced; a failure (a
    // blocked gate above all) must be READ, so it goes back with the
    // reasons shown.
    match rendered {
        Ok(()) => {
            state.note_outcome(&slug, &format!("rendered round {}", form.round));
            Redirect::to(&format!("/sessions/{slug}/review")).into_response()
        }
        Err(e) => {
            state.note_outcome(&slug, &format!("render failed: {e}"));
            back_to(&slug, &q, Some("top")).into_response()
        }
    }
}

/// POST /sessions/{slug}/publish — start the shared publish flow with a
/// [`WebConfirm`]; the edit posts only when the pending confirmation is
/// approved (no auto-publish).
async fn publish(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    axum::extract::Query(q): Params,
    Form(form): Form<PublishForm>,
) -> axum::response::Response {
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    // One publish flow per session at a time: a second start while an
    // approval is waiting would park a second, competing confirmation.
    if state.has_pending_for(&slug) {
        return back_to(&slug, &q, Some("publish")).into_response();
    }
    let runs_before = state.run_count(&slug);
    let state_for_task = Arc::clone(&state);
    let slug_for_task = slug.clone();
    let summary = form.summary;
    tokio::spawn(async move {
        let outcome = run_publish(&state_for_task, &slug_for_task, &summary).await;
        state_for_task.note_outcome(&slug_for_task, &outcome);
    });
    // B.6 operator catch ("doesn't seem to allow me to confirm"): the
    // pending confirmation registers a beat after the redirect — land on
    // a page that already shows it (or the failure outcome), never an
    // empty one. Bounded wait; the flow itself is unaffected.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if state.has_pending_for(&slug) || state.run_count(&slug) > runs_before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    back_to(&slug, &q, Some("publish")).into_response()
}

async fn run_publish(state: &Arc<ServeState>, slug: &str, summary: &str) -> String {
    let dir = session_dir(slug);
    // The gate's report belongs in the web client, not the server
    // terminal (B.6 operator catch). Run it first, visibly; publish_core
    // re-runs it identically as its own invariant.
    let proposed = std::fs::read_to_string(dir.join("proposed.wikitext")).unwrap_or_default();
    if proposed.trim().is_empty() {
        return "publish not started: proposed.wikitext is empty — stage an edit first \
                (driver: draft proposal, or edit the file)"
            .into();
    }
    if let Some(report) = gate_report(slug) {
        return format!("publish blocked by the gate:\n{report}");
    }
    let wiki = match state.connect_wiki().await {
        Ok(w) => w,
        Err(e) => return format!("publish failed: {e}"),
    };
    let mut confirm = WebConfirm {
        state: Arc::clone(state),
        slug: slug.to_string(),
        summary: summary.to_string(),
    };
    match crate::cli::publish_core(slug, summary, &wiki, &mut confirm, crate::cli::Via::Web).await {
        Ok(out) => {
            // Read-back result rides the message (rule-enforcement item 1):
            // the word "failed" gives the notice the error style.
            let verify = if !out.created_revision {
                String::new()
            } else if out.verification.is_empty() {
                " — read-back verified clean".into()
            } else {
                format!(" — read-back FAILED: {}", out.verification.join("; "))
            };
            if out.created_revision {
                format!(
                    "gate: PASS — published: {} (new revid {}){verify}",
                    out.diff_url, out.new_revid
                )
            } else {
                "gate: PASS — no change: the page already contains this text".into()
            }
        }
        Err(e) => format!("gate: PASS — publish failed: {e}"),
    }
}

/// Run the session's gate and return the structured report when blocked
/// (`None` = pass). Mirrors `publish_core`'s inputs exactly.
fn gate_report(slug: &str) -> Option<String> {
    use crate::checks::gate::GateInput;
    use crate::checks::gate::run_gate;
    let dir = session_dir(slug);
    let corpus = crate::rules::RulesCorpus::load(std::path::Path::new("rules")).ok()?;
    let base = std::fs::read_to_string(dir.join("base.wikitext")).ok()?;
    let proposed = std::fs::read_to_string(dir.join("proposed.wikitext")).ok()?;
    let findings = crate::session::FindingsFile::load(&dir.join("findings.json")).ok()?;
    let ledger = Ledger::load(&dir.join("ledger.json")).ok()?;
    let verdict = run_gate(&GateInput {
        ledger: &ledger,
        findings: &findings.findings,
        base_wikitext: &base,
        proposed_wikitext: &proposed,
        linter_config: &corpus.linter,
        paraphrase_config: &corpus.paraphrase,
    });
    verdict
        .blocked
        .then(|| crate::checks::gate::format_reasons(&verdict.reasons))
}

#[derive(serde::Deserialize)]
struct ConfirmForm {
    approve: Option<String>,
}

/// POST /confirmations/{id} — the operator's explicit decision on a
/// pending publish confirmation. Responds once the publish flow has
/// finished (bounded wait), so the page the operator lands on says what
/// happened: the session page after an approval, the review page after a
/// decline made there.
async fn confirm(
    State(state): State<Arc<ServeState>>,
    Path(id): Path<String>,
    axum::extract::Query(q): Params,
    Form(form): Form<ConfirmForm>,
) -> axum::response::Response {
    let approve = form.approve.as_deref().is_some_and(|v| v == "true");
    let slug = state
        .confirmations
        .lock()
        .expect("confirmations")
        .get(&id)
        .map(|p| p.slug.clone());
    let runs_before = slug.as_deref().map_or(0, |s| state.run_count(s));
    let (Some(slug), true) = (slug, state.resolve(&id, approve)) else {
        return Html(crate::ui::page(
            "Approval not found",
            &[("Sessions", "/")],
            "<h1>Approval not found</h1><p>This publish approval was already answered, or it \
             expired. Nothing was written. Start the publish again from the session.</p>",
        ))
        .into_response();
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline && state.run_count(&slug) == runs_before {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if approve {
        Redirect::to(&format!("/sessions/{slug}")).into_response()
    } else {
        back_to(&slug, &q, Some("publish")).into_response()
    }
}

/// GET /confirmations — every pending publish approval across sessions
/// (operator catch-all; the session and review pages embed the same
/// blocks).
async fn confirmations(State(state): State<Arc<ServeState>>) -> Html<String> {
    let mut pending: Vec<(String, (String, String, String))> = state
        .confirmations
        .lock()
        .expect("confirmations")
        .iter()
        .map(|(id, p)| {
            (
                p.slug.clone(),
                (id.clone(), p.summary.clone(), p.prompt.clone()),
            )
        })
        .collect();
    pending.sort();
    let mut body = String::from("<h1>Publish approvals</h1>\n");
    if pending.is_empty() {
        body.push_str("<p class=\"empty\">No publish is waiting for your approval.</p>");
    }
    for (slug, p) in &pending {
        let _ = writeln!(
            body,
            "<p>Session <a href=\"/sessions/{0}\"><code>{0}</code></a></p>{1}",
            esc(slug),
            approval_html(p, false)
        );
    }
    Html(crate::ui::page(
        "Publish approvals",
        &[("Sessions", "/")],
        &body,
    ))
}

/// The router (exported for tests).
pub fn router(state: Arc<ServeState>) -> Router {
    Router::new()
        .route("/", get(console))
        .route("/sessions/{slug}", get(session_page))
        .route("/sessions/{slug}/review", get(review_artifact))
        .route("/sessions/{slug}/sweep-dispose", post(sweep_dispose))
        .route("/sessions/{slug}/sweep-fetch", post(sweep_fetch_route))
        .route("/sessions/{slug}/attach", post(attach))
        .route("/sessions/{slug}/driver/findings", post(driver_findings))
        .route("/sessions/{slug}/driver/propose", post(driver_propose))
        .route("/sessions/{slug}/driver/resolve", post(driver_resolve))
        .route("/sessions/{slug}/comments", post(comments_add))
        .route("/sessions/{slug}/comments/resolve", post(comments_resolve))
        .route("/sessions/{slug}/render", post(render))
        .route("/sessions/{slug}/publish", post(publish))
        .route("/confirmations", get(confirmations))
        .route("/confirmations/{id}", post(confirm))
        .with_state(state)
}

/// Resolve this machine's Tailscale address (IPv4 + `MagicDNS` name) via
/// the tailscale CLI — for operators on thin clients whose browser
/// reaches this machine over the tailnet, not localhost.
fn tsnet_address() -> Option<(std::net::Ipv4Addr, String)> {
    let ip_out = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    let ip: std::net::Ipv4Addr = String::from_utf8_lossy(&ip_out.stdout)
        .trim()
        .parse()
        .ok()?;
    let name_out = std::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .ok()?;
    let name_json: serde_json::Value = serde_json::from_slice(&name_out.stdout).ok()?;
    let dns = name_json
        .pointer("/Self/DNSName")?
        .as_str()?
        .trim_end_matches('.')
        .to_string();
    Some((ip, dns))
}

/// Bind and serve. Default: loopback only. `tsnet` (explicit operator
/// opt-in for thin-client setups) binds the machine's TAILNET interface
/// only — the operator's private tailnet, never the LAN at large.
///
/// # Errors
/// Bind or serve failures.
///
/// # Panics
/// Never in practice: the loopback literal always parses (the expect is
/// on a compile-time constant).
pub async fn run(port: u16, tsnet: bool) -> anyhow::Result<()> {
    let ip = if tsnet {
        match tsnet_address() {
            Some((ip, dns)) => {
                eprintln!(
                    "wa serve: binding the TAILNET interface {dns} ({ip}) — operator opt-in (--tsnet); the default remains loopback-only"
                );
                ip
            }
            None => anyhow::bail!("--tsnet needs the tailscale CLI (`tailscale ip -4` must work)"),
        }
    } else {
        LOOPBACK_HOST
            .parse::<std::net::Ipv4Addr>()
            .expect("literal loopback parses")
    };
    let addr = std::net::SocketAddr::from((ip, port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    // Port 0 binds an ephemeral port; report the actual bound address.
    let bound = listener.local_addr()?;
    println!("wa serve listening on http://{bound}");
    let app = router(Arc::new(ServeState::new()));
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

use std::fmt::Write as _;
use std::io::Write as _;

#[cfg(test)]
mod tests {
    use super::{
        CommentGroup, anchor_line_range, block_insertions, console_rank, splice_plan,
        splices_overlap,
    };

    /// Plan-005 O.3: the console's attention-first ordering, pinned at the
    /// unit level (live+comments → live → everything else).
    #[test]
    fn console_rank_orders_attention_first() {
        assert_eq!(console_rank("current+comments"), 0);
        assert_eq!(console_rank("current"), 1);
        for history in ["stale", "none", "anything-else"] {
            assert_eq!(console_rank(history), 2, "{history}");
        }
        assert!(console_rank("current+comments") < console_rank("current"));
        assert!(console_rank("current") < console_rank("stale"));
    }

    /// Plan-005 O.3: on a multi-block artifact, every form lands directly
    /// after ITS OWN block's closing `</div>` — including the link-rich
    /// block whose old-side `<del>` follows a content-link `<span>` (the
    /// `</span>` truncation class) — and nothing else's anchor rides along.
    #[test]
    fn block_insertions_place_each_form_after_its_own_block_close() {
        let artifact = concat!(
            "<main><section class=\"diff-col\">",
            // Link-rich changed pair: a <span class="wl"> link BEFORE the
            // old-side <del> — historically the del could be cut off.
            "<div class=\"block change para\" id=\"wa-1\" data-wiki-anchor=\"L2:C0-L2:C20\">",
            "<span class=\"anchor-tag\">paragraph · line 2</span>",
            "Changed <span class=\"wl\">link text</span> ",
            "<del id=\"wa-2\" data-wiki-anchor=\"base:L2:C0-L2:C17\">old</del> words</div>",
            "<div class=\"block change para\" id=\"wa-3\" data-wiki-anchor=\"L4:C0-L4:C18\">",
            "<span class=\"anchor-tag\">paragraph · line 4</span>Second block</div>",
            "</section>",
            "<aside class=\"evidence-col\">",
            "<div class=\"evidence\" id=\"ev-1\" data-wiki-anchor=\"ledger:Q1\">",
            "<span class=\"anchor-tag\">evidence for this edit</span>card</div>",
            "</aside></main>",
        );
        let mk = |id: &str, target: &str| crate::comments::Comment {
            id: id.into(),
            target: target.into(),
            text: format!("comment {id}"),
            quoted: None,
            timestamp: String::new(),
            status: crate::comments::CommentStatus::Open,
            resolution: None,
        };
        let queue = crate::comments::CommentQueue {
            comments: vec![
                mk("K1", "L2:C0-L2:C20"),
                mk("K2", "base:L2:C0-L2:C17"),
                mk("K3", "ledger:Q1"),
            ],
        };

        let (insertions, placed) = block_insertions("test-slug", artifact, &queue);
        assert_eq!(insertions.len(), 3, "two blocks + one evidence card");

        // Every insertion sits exactly at its own `</div>` close.
        for (pos, _) in &insertions {
            assert_eq!(&artifact[pos - 6..*pos], "</div>", "pos {pos}");
        }
        // Document order, no overlaps.
        let mut positions: Vec<usize> = insertions.iter().map(|(p, _)| *p).collect();
        let mut sorted = positions.clone();
        sorted.sort_unstable();
        assert_eq!(positions, sorted, "insertions in document order");
        positions.dedup();
        assert_eq!(positions.len(), 3, "three distinct close positions");

        let html_at = |anchor_of_block: usize| {
            insertions
                .iter()
                .find(|(p, _)| *p == positions[anchor_of_block])
                .map(|(_, h)| h.as_str())
                .unwrap()
        };
        // Block 1 (link-rich pair): new-side form, the old-side toggle
        // under it, and both comments — the del after a link span is not
        // truncated away.
        let first = html_at(0);
        assert!(
            first.contains("name=target value=\"L2:C0-L2:C20\""),
            "{first}"
        );
        assert!(
            first.contains("name=target value=\"base:L2:C0-L2:C17\""),
            "{first}"
        );
        assert!(first.contains("comment K1"), "{first}");
        assert!(first.contains("comment K2"), "{first}");
        assert!(!first.contains("L4:C0-L4:C18"), "{first}");
        // Block 2: only its own form, no old side.
        let second = html_at(1);
        assert!(
            second.contains("name=target value=\"L4:C0-L4:C18\""),
            "{second}"
        );
        assert!(!second.contains("base:"), "{second}");
        assert!(!second.contains("L2:C0-L2:C20"), "{second}");
        // Evidence card: its own form + the ledger comment.
        let third = html_at(2);
        assert!(third.contains("name=target value=\"ledger:Q1\""), "{third}");
        assert!(third.contains("comment K3"), "{third}");

        assert_eq!(
            placed,
            vec![
                "base:L2:C0-L2:C17".to_string(),
                "L2:C0-L2:C20".to_string(),
                "L4:C0-L4:C18".to_string(),
                "ledger:Q1".to_string(),
            ]
        );
    }

    #[test]
    fn anchor_line_range_parses_both_families() {
        assert_eq!(anchor_line_range("L3:C0-L3:C120"), Some((3, 3)));
        assert_eq!(anchor_line_range("base:L2:C4-L5:C9"), Some((2, 5)));
        assert_eq!(anchor_line_range("garbage"), None);
        assert_eq!(anchor_line_range("L5:C0-L3:C9"), None, "end before start");
    }

    #[test]
    fn overlap_guard_catches_clobbers_not_adjacent_work() {
        // Two replacements over the same lines clobber.
        assert!(splices_overlap((3, 5, false), (5, 7, false)));
        assert!(splices_overlap((3, 3, false), (3, 3, false)));
        // Disjoint replacements are fine.
        assert!(!splices_overlap((3, 5, false), (6, 8, false)));
        // An insertion strictly inside a replaced range is a clobber…
        assert!(splices_overlap((4, 4, true), (3, 5, false)));
        assert!(splices_overlap((3, 5, false), (4, 4, true)));
        // …but at/around the boundaries it is deterministic, not a clobber.
        assert!(!splices_overlap((2, 2, true), (3, 5, false)));
        // Two insertions at the same boundary apply in order.
        assert!(!splices_overlap((4, 3, true), (4, 3, true)));
    }

    #[test]
    fn pure_deletion_splice_requires_prefix_alignment() {
        let group = CommentGroup {
            new_anchor: None,
            old_anchor: Some("base:L2:C0-L2:C16".into()),
            ids: vec!["K1".into()],
        };
        // Aligned: base and proposed agree before line 2.
        let aligned = splice_plan(
            &group,
            "line one\nline three\n",
            &["line one", "line two", "line three"],
        );
        assert_eq!(aligned, Ok((2, 1, true)));
        // Misaligned: the pair at line 1 breaks the mapping.
        let mis = splice_plan(
            &group,
            "changed one\nline three\n",
            &["line one", "line two", "line three"],
        );
        assert!(mis.unwrap_err().contains("misaligned"));
    }

    #[test]
    fn new_side_splice_bounds_are_checked() {
        let group = CommentGroup {
            new_anchor: Some("L2:C0-L2:C20".into()),
            old_anchor: Some("base:L2:C0-L2:C17".into()),
            ids: vec![],
        };
        assert_eq!(
            splice_plan(&group, "one\ntwo\n", &["one", "two"]),
            Ok((2, 2, false))
        );
        let out_of_range = CommentGroup {
            new_anchor: Some("L9:C0-L9:C9".into()),
            old_anchor: None,
            ids: vec![],
        };
        assert!(
            splice_plan(&out_of_range, "one\n", &["one"])
                .unwrap_err()
                .contains("out of range")
        );
    }
}
