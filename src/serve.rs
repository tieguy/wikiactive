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

    /// The z.ai client for the driver judgment points (env credentials;
    /// the test hook points at a mock).
    fn zai_client(
        &self,
    ) -> Result<crate::driver::model::ZaiClient, crate::driver::model::ZaiError> {
        match &self.zai_override {
            Some(base) => Ok(crate::driver::model::ZaiClient::with_base(
                base, "glm-5.3", "test-key",
            )),
            None => crate::driver::model::ZaiClient::from_env("glm-5.3"),
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
    let review_url = crate::lavish::session_url(&dir.join("review.html"));

    let mut page = format!(
        "<h1>{}</h1><p>slug {} · base revid {} · entry loop L{}</p>",
        esc(&meta.article),
        esc(&slug),
        meta.base_revid,
        meta.entry_loop
    );
    if let Some(url) = review_url {
        let _ = writeln!(
            page,
            "<p><a href=\"{}\">open the live review session</a></p>",
            esc(&url)
        );
    }
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
        let _ = writeln!(page, "<p><strong>{}</strong></p>", esc(&outcome));
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

#[derive(serde::Deserialize)]
struct DisposeForm {
    source: String,
    disposition: String,
}

/// POST /sessions/{slug}/sweep-dispose — operator-signed disposition.
async fn sweep_dispose(Path(slug): Path<String>, Form(form): Form<DisposeForm>) -> Redirect {
    let dir = session_dir(&slug);
    if let Ok(mut ledger) = Ledger::load(&dir.join("ledger.json"))
        && ledger
            .set_disposition(&form.source, &form.disposition)
            .is_ok()
    {
        let _ = ledger.save(&dir.join("ledger.json"));
    }
    Redirect::to(&format!("/sessions/{slug}"))
}

/// POST /sessions/{slug}/sweep-fetch — batch fetch + classify (network).
async fn sweep_fetch_route(Path(slug): Path<String>) -> Redirect {
    let _ = crate::cli::sweep_fetch(&slug).await;
    Redirect::to(&format!("/sessions/{slug}"))
}

#[derive(serde::Deserialize)]
struct AttachForm {
    source: String,
    text: String,
}

/// POST /sessions/{slug}/attach — ingest pasted operator-captured text
/// for one source (same ingestion as `wa ledger attach`).
async fn attach(Path(slug): Path<String>, Form(form): Form<AttachForm>) -> Redirect {
    let dir = session_dir(&slug);
    let tmp = dir.join("attach-upload.txt");
    if std::fs::write(&tmp, &form.text).is_ok() {
        let _ = crate::cli::ledger_attach(&slug, &form.source, &tmp.to_string_lossy());
        let _ = std::fs::remove_file(&tmp);
    }
    Redirect::to(&format!("/sessions/{slug}"))
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

#[derive(serde::Deserialize)]
struct RenderForm {
    round: u32,
    summary: String,
}

/// POST /sessions/{slug}/render — gate + render the artifact (the
/// session page links the live lavish review URL).
async fn render(Path(slug): Path<String>, Form(form): Form<RenderForm>) -> Redirect {
    let _ = crate::cli::render_cmd(&slug, form.round, None, None, &form.summary, true, false).await;
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
    let state_for_task = Arc::clone(&state);
    let slug_for_task = slug.clone();
    let summary = form.summary;
    tokio::spawn(async move {
        let outcome = run_publish(&state_for_task, &slug_for_task, &summary).await;
        state_for_task.note_outcome(&slug_for_task, &outcome);
    });
    Redirect::to(&format!("/sessions/{slug}"))
}

async fn run_publish(state: &Arc<ServeState>, slug: &str, summary: &str) -> String {
    let wiki = match state.connect_wiki().await {
        Ok(w) => w,
        Err(e) => return format!("publish failed: {e}"),
    };
    let mut confirm = WebConfirm {
        state: Arc::clone(state),
        slug: slug.to_string(),
        summary: summary.to_string(),
    };
    match crate::cli::publish_core(slug, summary, &wiki, &mut confirm).await {
        Ok(out) => {
            if out.created_revision {
                format!("published: {} (new revid {})", out.diff_url, out.new_revid)
            } else {
                "no change: the page already contains this text".into()
            }
        }
        Err(e) => format!("publish failed: {e}"),
    }
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
        .route("/sessions/{slug}/sweep-dispose", post(sweep_dispose))
        .route("/sessions/{slug}/sweep-fetch", post(sweep_fetch_route))
        .route("/sessions/{slug}/attach", post(attach))
        .route("/sessions/{slug}/driver/findings", post(driver_findings))
        .route("/sessions/{slug}/driver/propose", post(driver_propose))
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
