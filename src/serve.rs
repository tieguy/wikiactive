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
                id,
                PendingConfirm {
                    slug,
                    summary,
                    prompt,
                    responder: tx,
                },
            );
            // Timeout declines: no click, no write.
            matches!(
                tokio::time::timeout(state.timeout(), rx).await,
                Ok(Ok(true))
            )
        })
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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

/// GET / — the console: every session with its state and links.
async fn console(State(state): State<Arc<ServeState>>) -> Html<String> {
    let mut body = String::from("<h1>wa serve — session console</h1><ul>");
    for slug in list_sessions() {
        let meta = read_meta(&slug);
        let outcome = state
            .outcomes
            .lock()
            .expect("outcomes")
            .get(&slug)
            .cloned()
            .unwrap_or_default();
        let _ = writeln!(
            body,
            "<li><a href=\"/sessions/{slug}\">{slug}</a> — {} (base revid {}){}</li>",
            esc(meta
                .as_ref()
                .map_or("?".into(), |m| m.article.clone())
                .as_str()),
            meta.as_ref().map_or(0, |m| m.base_revid),
            if outcome.is_empty() {
                String::new()
            } else {
                format!(" — <strong>{}</strong>", esc(&outcome))
            }
        );
    }
    body.push_str("</ul>");
    Html(body)
}

/// GET /sessions/{slug} — one session: state, review link, sweep
/// manifest with disposition/attach forms, publish form. Server-rendered
/// string HTML (no templating dependency); splitting the page into
/// helpers would obscure the one-glance operator layout.
#[allow(clippy::too_many_lines)]
async fn session_page(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
) -> Html<String> {
    let Some(meta) = read_meta(&slug) else {
        return Html(format!("<h1>unknown session {}</h1>", esc(&slug)));
    };
    let dir = session_dir(&slug);
    let ledger = Ledger::load(&dir.join("ledger.json")).ok();

    let mut page = format!(
        "<h1>{}</h1><p>slug {} · base revid {} · entry loop L{}</p>",
        esc(&meta.article),
        esc(&slug),
        meta.base_revid,
        meta.entry_loop
    );
    // The review surface is the in-app artifact + this page's comment
    // forms — no external review server, no lifecycle (plan-004).
    let _ = writeln!(
        page,
        "<p><a href=\"/sessions/{slug}/review\">open the review artifact (in-app)</a> · \
         leave comments below (one per block)</p>"
    );
    page.push_str(&review_comments_section(&slug));
    if let Some(last) = meta.last_published_diff_url {
        let _ = writeln!(
            page,
            "<p>last published: <a href=\"{}\">{}</a></p>",
            esc(&last),
            esc(&last)
        );
    }

    // Sweep manifest with operator resolution forms.
    if let Some(ledger) = ledger.as_ref().filter(|l| l.has_sweep_state()) {
        let unresolved = ledger.sweep_unresolved().len();
        let _ = writeln!(
            page,
            "<h2>Source sweep ({unresolved} unresolved)</h2><table border=1><tr><th>id</th><th>status</th><th>text</th><th>disposition</th><th>source</th><th>resolve</th></tr>"
        );
        for s in &ledger.sources {
            let status = s.sweep_status.as_deref().unwrap_or("—");
            let disp = s.disposition.as_deref().unwrap_or("—");
            let text = if s.fetched_text.is_some() {
                "✓"
            } else {
                "—"
            };
            let _ = writeln!(
                page,
                "<tr><td>{}</td><td>{}</td><td>{text}</td><td>{}</td><td>{}</td>\
                 <td><form method=post action=\"/sessions/{slug}/sweep-dispose\">\
                 <input type=hidden name=source value=\"{}\">\
                 <input name=disposition placeholder=\"attested-unreachable\">\
                 <button>sign disposition</button></form></td></tr>",
                esc(&s.id),
                esc(status),
                esc(disp),
                esc(&s.url),
                esc(&s.id)
            );
        }
        page.push_str("</table>");
        let _ = writeln!(
            page,
            "<form method=post action=\"/sessions/{slug}/sweep-fetch\"><button>run sweep fetch</button></form>"
        );
        let _ = writeln!(
            page,
            "<h3>attach operator capture</h3>\
             <form method=post action=\"/sessions/{slug}/attach\">\
             <input name=source placeholder=\"S3\">\
             <textarea name=text rows=6 cols=80 placeholder=\"paste the captured page text\"></textarea>\
             <button>attach</button></form>"
        );
    }

    // Pending confirmations for this session (the full prompt text is
    // shown — the operator confirms exactly what the flow would write).
    let pendings: Vec<(String, String, String)> = state
        .confirmations
        .lock()
        .expect("confirmations")
        .iter()
        .filter(|(_, p)| p.slug == slug)
        .map(|(id, p)| (id.clone(), p.summary.clone(), p.prompt.clone()))
        .collect();
    for (id, summary, prompt) in pendings {
        let _ = writeln!(
            page,
            "<h2>PENDING PUBLISH CONFIRMATION</h2><p>summary: {}</p><pre>{}</pre>\
             <form method=post action=\"/confirmations/{id}\"><button name=approve value=true>approve — publish</button>\
             <button name=approve value=false>decline</button></form>",
            esc(&summary),
            esc(&prompt)
        );
    }

    let outcome = state
        .outcomes
        .lock()
        .expect("outcomes")
        .get(&slug)
        .cloned()
        .unwrap_or_default();
    if !outcome.is_empty() {
        let _ = writeln!(page, "<h2>Last run</h2><pre>{}</pre>", esc(&outcome));
    }

    let _ = writeln!(
        page,
        "<h2>Publish</h2>\
         <form method=post action=\"/sessions/{slug}/driver/findings\"><button>driver: author findings (model)</button></form>\
         <form method=post action=\"/sessions/{slug}/driver/propose\"><button>driver: draft proposal (model)</button></form>\
         <form method=post action=\"/sessions/{slug}/render\">\
         <input name=round value=1 size=3><input name=summary size=40 placeholder=\"round summary\">\
         <button>render review artifact</button></form>\
         <form method=post action=\"/sessions/{slug}/publish\">\
         <input name=summary size=60 placeholder=\"scoped edit summary\">\
         <button>start publish (gate → confirmation)</button></form>"
    );
    Html(page)
}

/// GET /sessions/{slug}/review — the review artifact, served in-app.
/// The artifact is self-contained HTML; reading it never needs the lavish
/// server (B.6 operator catch: reading the diff was chained through
/// lavish's lifecycle — "session is already over" — for no reason).
async fn review_artifact(Path(slug): Path<String>) -> axum::response::Response {
    let path = session_dir(&slug).join("review.html");
    match std::fs::read_to_string(&path) {
        Ok(html) => Html(html).into_response(),
        Err(_) => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// The "Review comments" section (plan-004 P.2): one comment form per
/// changed block (built from the artifact's embedded anchor table — the
/// hidden `target` is the block's `wikitext_anchor` VERBATIM, so comment
/// anchoring is exact by construction), old-side forms for removed
/// wording, evidence-card forms labeled by the source's citation text,
/// and the queue itself — open first (highlighted), then resolved with
/// their notes. Built as its own helper: `session_page` is at the
/// `too_many_lines` ceiling.
fn review_comments_section(slug: &str) -> String {
    let dir = session_dir(slug);
    let mut html = String::from("\n<h2>Review comments</h2>\n");
    match std::fs::read_to_string(dir.join("review.html")) {
        Ok(text) => html.push_str(&comment_forms(slug, &text)),
        Err(_) => html.push_str("<p>no review artifact yet — render one first</p>\n"),
    }
    html.push_str(&queue_html(slug, &dir));
    html
}

/// One comment form group: per-changed-block forms (plus old-side forms
/// for removed wording) and evidence-card forms labeled by the source's
/// citation text — never the Q-id (evidence comments are about sources).
fn comment_forms(slug: &str, artifact: &str) -> String {
    let blocks = crate::render::review_targets(artifact);
    let evidence = crate::render::evidence_targets(artifact);
    if blocks.is_empty() && evidence.is_empty() {
        return "<p>no changed blocks in this artifact — nothing to comment on</p>\n".into();
    }
    let mut html = String::new();
    for block in &blocks {
        let _ = writeln!(
            html,
            "<div class=\"cform\">\
             <form method=post action=\"/sessions/{slug}/comments\">\
             <input type=hidden name=target value=\"{}\">\
             <span class=\"cform-label\">change · {}</span>\
             <textarea name=text rows=2 cols=70 placeholder=\"comment on this block\"></textarea>\
             <input name=quoted size=30 placeholder=\"words you mean (optional)\">\
             <button>comment</button></form></div>",
            esc(&block.wikitext_anchor),
            esc(&block.label)
        );
        for old in &block.old_sides {
            let label = old.label.clone().unwrap_or_else(|| {
                crate::render::human_line_label(&old.wikitext_anchor)
                    .unwrap_or_else(|| old.element_id.clone())
            });
            let _ = writeln!(
                html,
                "<div class=\"cform cform-old\">\
                 <form method=post action=\"/sessions/{slug}/comments\">\
                 <input type=hidden name=target value=\"{}\">\
                 <span class=\"cform-label\">removed wording · {}</span>\
                 <textarea name=text rows=2 cols=70 placeholder=\"comment on the removed wording\"></textarea>\
                 <input name=quoted size=30 placeholder=\"words you mean (optional)\">\
                 <button>comment</button></form></div>",
                esc(&old.wikitext_anchor),
                esc(&label)
            );
        }
    }
    if !evidence.is_empty() {
        html.push_str("<h3>On the sources (evidence)</h3>\n");
        for ev in &evidence {
            let _ = writeln!(
                html,
                "<div class=\"cform\">\
                 <form method=post action=\"/sessions/{slug}/comments\">\
                 <input type=hidden name=target value=\"{}\">\
                 <span class=\"cform-label\">source · {}</span>\
                 <textarea name=text rows=2 cols=70 placeholder=\"about this source or its quotes\"></textarea>\
                 <input name=quoted size=30 placeholder=\"words you mean (optional)\">\
                 <button>comment</button></form></div>",
                esc(&ev.wikitext_anchor),
                esc(&ev.label)
            );
        }
    }
    html
}

/// The queue: open comments highlighted (each with its resolve form and
/// the shared driver button), resolved ones with their notes.
fn queue_html(slug: &str, dir: &std::path::Path) -> String {
    let mut html = String::new();
    let queue = match crate::comments::CommentQueue::load(&dir.join("comments.jsonl")) {
        Ok(queue) if queue.comments.is_empty() => {
            return "<p>queue empty</p>\n".into();
        }
        Ok(queue) => queue,
        Err(e) => return format!("<p class=error>comment queue: {e}</p>\n"),
    };
    let open = queue.open();
    if !open.is_empty() {
        let _ = writeln!(
            html,
            "<h3>Open ({})</h3><form method=post action=\"/sessions/{slug}/driver/resolve\">\
             <button>driver: apply review comments (model)</button></form>",
            open.len()
        );
        for c in open {
            let _ = writeln!(
                html,
                "<div class=\"comment comment-open\"><strong>{} OPEN</strong> → <code>{}</code><br>{}{}\
                 <form method=post action=\"/sessions/{slug}/comments/resolve\" class=inline>\
                 <input type=hidden name=id value=\"{}\">\
                 <input name=note size=50 placeholder=\"resolution note\">\
                 <button>resolve manually</button></form></div>",
                esc(&c.id),
                esc(&c.target),
                esc(&c.text),
                c.quoted
                    .as_ref()
                    .map(|q| format!("<br><em>highlighted:</em> {}", esc(q)))
                    .unwrap_or_default(),
                esc(&c.id)
            );
        }
    }
    let resolved: Vec<_> = queue
        .comments
        .iter()
        .filter(|c| c.status == crate::comments::CommentStatus::Resolved)
        .collect();
    if !resolved.is_empty() {
        html.push_str("<h3>Resolved</h3>\n");
        for c in resolved {
            let _ = writeln!(
                html,
                "<div class=\"comment comment-resolved\"><strong>{}</strong> → <code>{}</code><br>{}\
                 <br><em>resolution:</em> {}</div>",
                esc(&c.id),
                esc(&c.target),
                esc(&c.text),
                esc(c.resolution.as_deref().unwrap_or("(none recorded)"))
            );
        }
    }
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
    Form(form): Form<CommentForm>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let dir = session_dir(&slug);
    match crate::comments::CommentQueue::load(&dir.join("comments.jsonl")) {
        Ok(mut queue) => {
            let comment = crate::comments::Comment::new(
                &form.target,
                &form.text,
                form.quoted.as_deref(),
                &crate::comments::now_iso(),
            );
            match queue.append(&dir.join("comments.jsonl"), comment) {
                Ok(id) => {
                    state.note_outcome(&slug, &format!("comment {id} queued → {}", form.target));
                }
                Err(e) => state.note_outcome(&slug, &format!("comment queue write failed: {e}")),
            }
        }
        Err(e) => state.note_outcome(&slug, &format!("comment queue: {e}")),
    }
    Redirect::to(&format!("/sessions/{slug}")).into_response()
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
    Form(form): Form<CommentResolveForm>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let dir = session_dir(&slug);
    let outcome = match crate::comments::CommentQueue::load(&dir.join("comments.jsonl")) {
        Ok(mut queue) => match queue.resolve(&dir.join("comments.jsonl"), &form.id, &form.note) {
            Ok(()) => format!("comment {} resolved manually", form.id),
            Err(e) => format!("comment resolve failed: {e}"),
        },
        Err(e) => format!("comment queue: {e}"),
    };
    state.note_outcome(&slug, &outcome);
    Redirect::to(&format!("/sessions/{slug}")).into_response()
}

#[derive(serde::Deserialize)]
struct DisposeForm {
    source: String,
    disposition: String,
}

/// POST /sessions/{slug}/sweep-dispose — operator-signed disposition.
async fn sweep_dispose(
    Path(slug): Path<String>,
    Form(form): Form<DisposeForm>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let dir = session_dir(&slug);
    if let Ok(mut ledger) = Ledger::load(&dir.join("ledger.json"))
        && ledger
            .set_disposition(&form.source, &form.disposition)
            .is_ok()
    {
        let _ = ledger.save(&dir.join("ledger.json"));
    }
    Redirect::to(&format!("/sessions/{slug}")).into_response()
}

/// POST /sessions/{slug}/sweep-fetch — batch fetch + classify (network).
async fn sweep_fetch_route(Path(slug): Path<String>) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let _ = crate::cli::sweep_fetch(&slug).await;
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
    Path(slug): Path<String>,
    Form(form): Form<AttachForm>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let dir = session_dir(&slug);
    let tmp = dir.join("attach-upload.txt");
    if std::fs::write(&tmp, &form.text).is_ok() {
        let _ = crate::cli::ledger_attach(&slug, &form.source, &tmp.to_string_lossy());
        let _ = std::fs::remove_file(&tmp);
    }
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
) -> Redirect {
    let outcome = run_driver_findings(&state, &slug).await;
    state.note_outcome(&slug, &outcome);
    Redirect::to(&format!("/sessions/{slug}"))
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
) -> Redirect {
    let outcome = run_driver_propose(&state, &slug).await;
    state.note_outcome(&slug, &outcome);
    Redirect::to(&format!("/sessions/{slug}"))
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
    match crate::driver::steps::draft_proposal(&zai, finding, &base, &named).await {
        Ok(proposal) => {
            let anchor_line = finding
                .wikitext_anchor
                .split([':', '-'])
                .next()
                .unwrap_or("L1")
                .trim_start_matches('L');
            let line_no: usize = anchor_line.parse().unwrap_or(1);
            let lines: Vec<&str> = base.lines().collect();
            if line_no == 0 || line_no > lines.len() {
                return "driver propose: anchor line out of range".into();
            }
            let mut out = lines.clone();
            out[line_no - 1] = &proposal.proposed_wikitext_block;
            let proposed = out.join("\n");
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
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !known_session(&slug) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    let outcome = run_driver_resolve(&state, &slug).await;
    state.note_outcome(&slug, &outcome);
    Redirect::to(&format!("/sessions/{slug}")).into_response()
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

/// POST /sessions/{slug}/render — gate + render the artifact. The web
/// path never opens or probes lavish (`Via::Web`): the review surface is
/// the in-app artifact + the comment forms (plan-004).
async fn render(Path(slug): Path<String>, Form(form): Form<RenderForm>) -> Redirect {
    let pair = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
    };
    let _ = crate::cli::render_cmd(
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
    Redirect::to(&format!("/sessions/{slug}"))
}

/// POST /sessions/{slug}/publish — start the shared publish flow with a
/// [`WebConfirm`]; the edit posts only when the pending confirmation is
/// approved (no auto-publish).
async fn publish(
    State(state): State<Arc<ServeState>>,
    Path(slug): Path<String>,
    Form(form): Form<PublishForm>,
) -> Redirect {
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
    Redirect::to(&format!("/sessions/{slug}"))
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
            if out.created_revision {
                format!(
                    "gate: PASS — published: {} (new revid {})",
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
/// pending publish confirmation.
async fn confirm(
    State(state): State<Arc<ServeState>>,
    Path(id): Path<String>,
    Form(form): Form<ConfirmForm>,
) -> Html<String> {
    let approve = form.approve.as_deref().is_some_and(|v| v == "true");
    if state.resolve(&id, approve) {
        Html(format!(
            "<p>confirmation {id} resolved: {}.</p><p><a href=\"/\">console</a></p>",
            if approve {
                "APPROVED — publishing"
            } else {
                "declined — nothing will be written"
            }
        ))
    } else {
        Html(format!(
            "<p>confirmation {id} not found (already resolved or expired).</p><p><a href=\"/\">console</a></p>"
        ))
    }
}

/// GET /confirmations — the pending list (operator catch-all; the session
/// page embeds the same forms).
async fn confirmations(State(state): State<Arc<ServeState>>) -> Html<String> {
    let mut body = String::from("<h1>pending confirmations</h1>");
    let pending: Vec<(String, String, String)> = state
        .confirmations
        .lock()
        .expect("confirmations")
        .iter()
        .map(|(id, p)| (id.clone(), p.slug.clone(), p.summary.clone()))
        .collect();
    if pending.is_empty() {
        body.push_str("<p>none</p>");
    }
    for (id, slug, summary) in pending {
        let _ = writeln!(
            body,
            "<p><strong>{slug}</strong>: {}<br>\
             <form method=post action=\"/confirmations/{id}\"><button name=approve value=true>approve — publish</button>\
             <button name=approve value=false>decline</button></form></p>",
            esc(&summary)
        );
    }
    Html(body)
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
    use super::{CommentGroup, anchor_line_range, splice_plan, splices_overlap};

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
