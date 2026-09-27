//! CLI wiring: `wa` — session init / analyze / findings / render / poll /
//! publish / ledger / lint.

use std::io::Read as _;
use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

use crate::checks::gate::GateInput;
use crate::checks::linter;
use crate::checks::linter::LinterConfig;
use crate::lavish;
use crate::ledger::Ledger;
use crate::ledger::SourceMetadata;
use crate::render::RenderInput;
use crate::render::registry_with_round;
use crate::render::render;
use crate::rules::ArticleState;
use crate::rules::RulesCorpus;
use crate::rules::build_context_bundle;
use crate::session::FindingsFile;
use crate::session::RoundEntry;
use crate::session::SessionMeta;
use crate::session::SessionPaths;
use crate::wikipedia::EditRequest;
use crate::wikipedia::TtyConfirm;
use crate::wikipedia::Wikipedia;
use crate::wikipedia::WikipediaError;

#[derive(Parser)]
#[command(
    name = "wa",
    version,
    about = "wikiloop — structured Wikipedia improvement loop"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Session management.
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// Build and print the context bundle (step 0 of every iteration).
    Analyze {
        slug: String,
        /// Prior-session base wikitext for L3 re-review drift context: a
        /// unified diff against the current base is embedded in the bundle.
        #[arg(long)]
        prior_base: Option<PathBuf>,
    },
    /// Findings authoring (schema-validated).
    Findings {
        #[command(subcommand)]
        cmd: FindingsCmd,
    },
    /// Gate + render the review artifact (writes review.html).
    Render {
        slug: String,
        /// Round number (revisions registry appends r<N>).
        #[arg(long)]
        round: u32,
        /// Offline: Parsoid HTML of the base wikitext (fixture path).
        #[arg(long)]
        html_base: Option<PathBuf>,
        /// Offline: Parsoid HTML of the proposed wikitext (fixture path).
        #[arg(long)]
        html_proposed: Option<PathBuf>,
        /// Round summary for the revisions registry.
        #[arg(long, default_value = "proposed edit")]
        summary: String,
        /// Do not open a lavish session after rendering.
        #[arg(long)]
        no_open: bool,
        /// Reopen a user-ended lavish session (explicit operator request).
        #[arg(long)]
        reopen: bool,
    },
    /// Long-poll the lavish session; on feedback, print resolved anchors.
    Poll {
        slug: String,
        /// Reply to show in the Lavish conversation panel before waiting.
        #[arg(long)]
        agent_reply: Option<String>,
    },
    /// Publish the proposed edit (gate re-run + /dev/tty confirm).
    Publish {
        slug: String,
        #[arg(long)]
        summary: String,
    },
    /// Source ledger operations.
    Ledger {
        #[command(subcommand)]
        cmd: LedgerCmd,
    },
    /// Whole-page lint scan (pre-existing defects; also used by replay).
    Lint { path: PathBuf },
    /// Standalone gate run against the session's proposed edit: structured,
    /// disposition-grouped output; writes NO artifact (fail-fast preflight
    /// before the render attempt).
    Check { slug: String },
    /// Append this session's entry to the disclosure page log (idempotent;
    /// uses house-rules disclosure page).
    DisclosureLog {
        slug: String,
        /// Entry body (article, date, model, diff links) as wikitext.
        #[arg(long)]
        entry: String,
        /// Unique marker for idempotency (e.g. wa-session:2026-09-25-tf).
        #[arg(long)]
        marker: String,
    },
}

#[derive(Subcommand)]
pub enum SessionCmd {
    /// Pin the current revid and store base wikitext.
    Init {
        #[arg(long)]
        article: String,
        /// Triage-selected entry loop (1–5).
        #[arg(long)]
        entry_loop: u8,
        /// Drift-review pin (MVP-2 A.2.2): record this user's last edit to
        /// the article; analyze embeds the drift diff since. Bare flag
        /// defaults to rules/house-rules.toml [operator] username; an
        /// explicit value overrides.
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        review_since_user: Option<String>,
    },
    /// Show session state.
    Show { slug: String },
}

#[derive(Subcommand)]
pub enum FindingsCmd {
    /// Append findings from a JSON file (array of findings) or '-' for stdin.
    Add {
        slug: String,
        /// Path to JSON ('-' = stdin). Content: a finding object or an
        /// array of findings.
        json: String,
    },
    /// List findings.
    List { slug: String },
}

#[derive(Subcommand)]
pub enum LedgerCmd {
    /// Register a source URL.
    Register {
        slug: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        work: Option<String>,
    },
    /// Fetch a registered source's text (stored in the ledger).
    Fetch {
        slug: String,
        #[arg(long)]
        source: String,
    },
    /// Request an archive.org snapshot (save-page-now) for a source.
    Archive {
        slug: String,
        #[arg(long)]
        source: String,
    },
    /// Add a verbatim quote (verified against the fetched text at add time).
    Quote {
        slug: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        text: String,
    },
    /// Bind prose to quote ids (the claim ↔ quotes record).
    Claim {
        slug: String,
        #[arg(long)]
        prose: String,
        #[arg(long, value_delimiter = ',')]
        quotes: Vec<String>,
    },
}

/// Run the CLI (async: session and publish commands hit the live API).
///
/// # Errors
/// Any command failure, with context.
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Session { cmd } => match cmd {
            SessionCmd::Init {
                article,
                entry_loop,
                review_since_user,
            } => session_init(&article, entry_loop, review_since_user).await,
            SessionCmd::Show { slug } => session_show(&slug),
        },
        Command::Analyze { slug, prior_base } => analyze(&slug, prior_base),
        Command::Findings { cmd } => match cmd {
            FindingsCmd::Add { slug, json } => findings_add(&slug, &json),
            FindingsCmd::List { slug } => findings_list(&slug),
        },
        Command::Render {
            slug,
            round,
            html_base,
            html_proposed,
            summary,
            no_open,
            reopen,
        } => {
            render_cmd(
                &slug,
                round,
                html_base,
                html_proposed,
                &summary,
                no_open,
                reopen,
            )
            .await
        }
        Command::Poll { slug, agent_reply } => poll_cmd(&slug, agent_reply.as_deref()),
        Command::Publish { slug, summary } => publish_cmd(&slug, &summary).await,
        Command::Ledger { cmd } => match cmd {
            LedgerCmd::Register {
                slug,
                url,
                title,
                work,
            } => ledger_register(&slug, &url, title.as_deref(), work.as_deref()),
            LedgerCmd::Fetch { slug, source } => ledger_fetch(&slug, &source).await,
            LedgerCmd::Archive { slug, source } => ledger_archive(&slug, &source).await,
            LedgerCmd::Quote { slug, source, text } => ledger_quote(&slug, &source, &text),
            LedgerCmd::Claim {
                slug,
                prose,
                quotes,
            } => ledger_claim(&slug, &prose, &quotes),
        },
        Command::Lint { path } => lint_cmd(&path),
        Command::Check { slug } => check_cmd(&slug),
        Command::DisclosureLog {
            slug,
            entry,
            marker,
        } => disclosure_log_cmd(&slug, &entry, &marker).await,
    }
}

async fn session_init(
    article: &str,
    entry_loop: u8,
    review_since_user: Option<String>,
) -> Result<()> {
    anyhow::ensure!((1..=5).contains(&entry_loop), "entry_loop must be 1-5");
    let slug = slugify(article);
    let paths = SessionPaths::new(&slug);
    std::fs::create_dir_all(&paths.dir).context("create session dir")?;

    // Drift-review default: the operator identity from house rules (single
    // fork-edit point) unless the flag names another user explicitly. A
    // bare flag (empty value) also selects the configured default.
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let review_user = match review_since_user {
        Some(user) if !user.is_empty() => Some(user),
        _ => corpus.house_rules.operator.username.clone(),
    };

    let wiki = Wikipedia::connect()
        .await
        .context("connect to en.wikipedia")?;
    let (revid, wikitext) = match wiki.current_revid(article).await {
        Ok(revid) => {
            let wikitext = wiki
                .wikitext_at_revid(article, revid)
                .await
                .context("fetch wikitext at pinned revid")?;
            (revid, wikitext)
        }
        Err(WikipediaError::PageMissing(_)) => {
            // New-page sessions (userspace smoke targets): base 0, empty
            // base text; the publish path creates the page.
            (0, String::new())
        }
        Err(other) => return Err(other.into()),
    };

    // Drift-review pin (MVP-2 A.2.2): the operator's last-edit revid + the
    // wikitext at it (stored beside the base; analyze diffs against it).
    let mut drift_note = String::new();
    let (review_since_revid, review_since_user) = match review_user {
        Some(user) => match wiki.last_edit_revid(&user, article).await {
            Ok(Some(rs_revid)) => {
                let prior = wiki
                    .wikitext_at_revid(article, rs_revid)
                    .await
                    .context("fetch wikitext at review-since revid")?;
                std::fs::write(paths.dir.join("review-since.wikitext"), &prior)?;
                drift_note = format!(", drift pin: {user}'s last edit @ {rs_revid}");
                (Some(rs_revid), Some(user))
            }
            Ok(None) => {
                println!(
                    "--review-since-user: {user} has never edited {article}; no drift pin recorded"
                );
                (None, None)
            }
            Err(e) => return Err(e.into()),
        },
        None => (None, None),
    };

    let meta = SessionMeta {
        article: article.to_string(),
        base_revid: revid,
        started: now_iso(),
        entry_loop,
        last_published_diff_url: None,
        review_since_revid,
        review_since_user,
    };
    std::fs::write(paths.meta(), serde_json::to_string_pretty(&meta)?)?;
    std::fs::write(paths.base(), &wikitext)?;
    std::fs::write(paths.proposed(), &wikitext)?;
    Ledger::default().save(&paths.ledger())?;
    FindingsFile::default()
        .save(&paths.findings())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!(
        "session initialized: sessions/{slug} (base revid {revid}, entry loop L{entry_loop}{drift_note})"
    );
    Ok(())
}

/// A small line-based unified diff (drift context for L3 re-reviews).
fn unified_diff(old: &str, new: &str) -> String {
    use similar::ChangeTag;
    use similar::TextDiff;
    let diff = TextDiff::from_lines(old, new);
    let mut out = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => '-',
            ChangeTag::Insert => '+',
            ChangeTag::Equal => ' ',
        };
        out.push(sign);
        out.push_str(change.value());
    }
    out
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn session_show(slug: &str) -> Result<()> {
    let (paths, meta) = load_session(slug)?;
    let _ = paths;
    println!("{}", serde_json::to_string_pretty(&meta)?);
    Ok(())
}

fn slugify(title: &str) -> String {
    title
        .trim()
        .replace(['/', ':', '?'], "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

fn load_session(slug: &str) -> Result<(SessionPaths, SessionMeta)> {
    let paths = SessionPaths::new(slug);
    let meta: SessionMeta = serde_json::from_str(
        &std::fs::read_to_string(paths.meta())
            .with_context(|| format!("session {slug} not initialized"))?,
    )?;
    Ok((paths, meta))
}

fn analyze(slug: &str, prior_base: Option<PathBuf>) -> Result<()> {
    let (paths, meta) = load_session(slug)?;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let wikitext = std::fs::read_to_string(paths.proposed()).with_context(|| {
        format!("sessions/{slug}/proposed.wikitext missing; nothing to analyze")
    })?;
    let base_wikitext = std::fs::read_to_string(paths.base())
        .with_context(|| format!("sessions/{slug}/base.wikitext missing"))?;
    let findings = FindingsFile::load(&paths.findings()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let ledger = Ledger::load(&paths.ledger())?;
    // Drift context (MVP-2 A.2.2): explicit --prior-base wins; otherwise a
    // session initialized with --review-since-user diffs against the
    // operator's last-edit wikitext (review-since.wikitext).
    let prior_and_label: Option<(String, String)> = if let Some(path) = prior_base {
        let prior = std::fs::read_to_string(&path)
            .with_context(|| format!("--prior-base {} unreadable", path.display()))?;
        Some((prior, "prior base".to_string()))
    } else {
        let rs_path = paths.dir.join("review-since.wikitext");
        if meta.review_since_revid.is_some() && rs_path.exists() {
            let prior =
                std::fs::read_to_string(&rs_path).context("review-since.wikitext unreadable")?;
            let user = meta.review_since_user.clone().unwrap_or_default();
            let revid = meta.review_since_revid.unwrap_or_default();
            Some((prior, format!("{user}'s last edit (revid {revid})")))
        } else {
            None
        }
    };
    let prior_session_diff = prior_and_label
        .map(|(prior, label)| format!("since {label}:\n{}", unified_diff(&prior, &base_wikitext)));
    let article = ArticleState {
        title: meta.article.clone(),
        base_revid: meta.base_revid,
        wikitext,
        entry_loop: meta.entry_loop,
        prior_session_diff,
    };
    let bundle = build_context_bundle(&corpus, &article, &findings.findings, &ledger)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::write(paths.dir.join("context.md"), &bundle.text)?;
    println!("{}", bundle.text);
    Ok(())
}

fn findings_add(slug: &str, json_source: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let raw = if json_source == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(json_source).context("read findings json")?
    };
    let trimmed = raw.trim();
    // Accept a single object or an array.
    let incoming: Vec<crate::session::Finding> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).context("parse findings array")?
    } else {
        vec![serde_json::from_str(trimmed).context("parse finding object")?]
    };
    // Schema-validate every finding (all problems at once).
    for finding in &incoming {
        finding.validate().map_err(|problems| {
            anyhow::anyhow!("finding {} invalid: {}", finding.id, problems.join("; "))
        })?;
    }
    let mut file = if paths.findings().exists() {
        FindingsFile::load(&paths.findings()).map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        FindingsFile::default()
    };
    for finding in incoming {
        anyhow::ensure!(
            !file.findings.iter().any(|f| f.id == finding.id),
            "duplicate finding id {}",
            finding.id
        );
        println!("accepted finding {}", finding.id);
        file.findings.push(finding);
    }
    file.save(&paths.findings())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

fn findings_list(slug: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let file = FindingsFile::load(&paths.findings()).map_err(|e| anyhow::anyhow!("{e}"))?;
    for f in &file.findings {
        println!(
            "{} [{}] loop {} — {}",
            f.id,
            f.rules.join(","),
            f.loop_id,
            f.proposed_fix
        );
    }
    Ok(())
}

async fn render_cmd(
    slug: &str,
    round: u32,
    html_base: Option<PathBuf>,
    html_proposed: Option<PathBuf>,
    summary: &str,
    no_open: bool,
    reopen: bool,
) -> Result<()> {
    let (paths, meta) = load_session(slug)?;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let base_wikitext = std::fs::read_to_string(paths.base())?;
    let proposed_wikitext = std::fs::read_to_string(paths.proposed())?;
    let findings = FindingsFile::load(&paths.findings()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let ledger = Ledger::load(&paths.ledger())?;

    // HTML sides: offline fixtures when provided; otherwise live Parsoid.
    let (base_html, proposed_html) = match (html_base, html_proposed) {
        (Some(b), Some(p)) => (std::fs::read_to_string(b)?, std::fs::read_to_string(p)?),
        (None, None) => {
            let client =
                parsoid::Client::new("https://en.wikipedia.org/w/rest.php", crate::USER_AGENT)
                    .map_err(|e| anyhow::anyhow!("parsoid client: {e}"))?;
            let b = client
                .transform_to_html_raw(&base_wikitext)
                .await
                .map_err(|e| anyhow::anyhow!("parsoid base transform: {e}"))?;
            let p = client
                .transform_to_html_raw(&proposed_wikitext)
                .await
                .map_err(|e| anyhow::anyhow!("parsoid proposed transform: {e}"))?;
            (b, p)
        }
        _ => anyhow::bail!("--html-base and --html-proposed must be given together"),
    };

    // Cumulative revisions registry from the previous artifact, if any.
    let previous = read_registry(&paths.review_html());
    let revisions = registry_with_round(&previous, round, summary, &now_iso());

    let output = render(&RenderInput {
        article: meta.article.clone(),
        round,
        base_wikitext: &base_wikitext,
        proposed_wikitext: &proposed_wikitext,
        base_html: &base_html,
        proposed_html: &proposed_html,
        findings: &findings.findings,
        ledger: &ledger,
        linter_config: &corpus.linter,
        paraphrase_config: &corpus.paraphrase,
        revisions,
    })
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    std::fs::write(paths.review_html(), &output.artifact_html)?;
    // Back-filled rendered_span_ids persist.
    let mut updated = findings;
    updated.findings = output.updated_findings;
    updated
        .save(&paths.findings())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let entry = RoundEntry {
        round,
        timestamp: now_iso(),
        summary: summary.to_string(),
        phase: "rendered".into(),
        detail: vec![],
    };
    append_round(&paths, &entry)?;
    println!(
        "rendered sessions/{slug}/review.html ({} anchor entries)",
        output.anchor_table.len()
    );
    if !no_open {
        let out = lavish::open_session(&paths.review_html(), false, reopen)?;
        print_lavish_output(&out)?;
    }
    if let Some(url) = lavish::session_url(&paths.review_html()) {
        println!(
            "review: {}  (or: {url})",
            lavish::terminal_link(&url, "open the review session")
        );
    }
    Ok(())
}

fn read_registry(review_html: &std::path::Path) -> Vec<crate::render::RevisionEntry> {
    let Ok(html) = std::fs::read_to_string(review_html) else {
        return Vec::new();
    };
    let Some(i) = html.find("data-lavish-revisions") else {
        return Vec::new();
    };
    let Some(start) = html[i..].find('[').map(|j| i + j) else {
        return Vec::new();
    };
    let Some(end) = html[start..].find("</script>").map(|j| start + j) else {
        return Vec::new();
    };
    serde_json::from_str(html[start..end].trim()).unwrap_or_default()
}

fn append_round(paths: &SessionPaths, entry: &RoundEntry) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.rounds())?;
    writeln!(file, "{}", serde_json::to_string(entry)?)?;
    Ok(())
}

fn poll_cmd(slug: &str, agent_reply_msg: Option<&str>) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let artifact = paths.review_html();
    let output = if let Some(msg) = agent_reply_msg {
        lavish::agent_reply(&artifact, msg)?
    } else {
        let file = artifact.to_string_lossy().to_string();
        lavish::lavish_command(&["poll", &file])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()?
            .wait_with_output()?
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let tree = lavish::parse_toon(&stdout);
    let comments = lavish::comments_from_poll(&tree);
    if comments.is_empty() {
        println!("{stdout}");
        return Ok(());
    }
    // Resolve against the artifact's embedded anchor table.
    let anchor_table = read_anchor_table(&paths.review_html());
    println!("feedback: {} comment(s)", comments.len());
    for comment in &comments {
        match crate::anchors::resolve_comment(comment, &anchor_table) {
            Ok(resolved) => {
                println!("  [{}] {}", resolved.element_id, resolved.wikitext_anchor);
                if let Some(text) = &resolved.selected_text {
                    println!("      selection: {text:?}");
                }
                println!("      comment: {}", resolved.comment);
            }
            Err(e) => println!("  UNRESOLVED ({e}): {}", comment.prompt),
        }
    }
    println!("next_step: apply the requested changes, re-render, and re-poll with --agent-reply");
    Ok(())
}

fn read_anchor_table(review_html: &std::path::Path) -> Vec<(String, String)> {
    let Ok(html) = std::fs::read_to_string(review_html) else {
        return Vec::new();
    };
    let Some(i) = html.find("wa-anchor-table") else {
        return Vec::new();
    };
    let Some(start) = html[i..].find('[').map(|j| i + j) else {
        return Vec::new();
    };
    let Some(end) = html[start..].find("</script>").map(|j| start + j) else {
        return Vec::new();
    };
    // The table is a JSON array of AnchorEntry objects (element_id +
    // wikitext_anchor). Parse loudly: a silent empty table made every poll
    // comment UNRESOLVED (live L2 round-1 catch).
    let text = html[start..end].trim();
    match serde_json::from_str::<Vec<crate::render::AnchorEntry>>(text) {
        Ok(entries) => entries
            .into_iter()
            .map(|e| (e.element_id, e.wikitext_anchor))
            .collect(),
        Err(e) => {
            eprintln!(
                "warning: anchor table unparsable in {}: {e}",
                review_html.display()
            );
            Vec::new()
        }
    }
}

fn print_lavish_output(output: &std::process::Output) -> Result<()> {
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stderr().write_all(&output.stderr)?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn publish_cmd(slug: &str, summary: &str) -> Result<()> {
    let (paths, meta) = load_session(slug)?;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let base_wikitext = std::fs::read_to_string(paths.base())?;
    let proposed_wikitext = std::fs::read_to_string(paths.proposed())?;
    let findings = FindingsFile::load(&paths.findings()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let ledger = Ledger::load(&paths.ledger())?;

    // Publish gate: the SAME gate as render's pre-flight, re-run now.
    let verdict = crate::checks::gate::run_gate(&GateInput {
        ledger: &ledger,
        findings: &findings.findings,
        base_wikitext: &base_wikitext,
        proposed_wikitext: &proposed_wikitext,
        linter_config: &corpus.linter,
        paraphrase_config: &corpus.paraphrase,
    });
    anyhow::ensure!(
        !verdict.blocked,
        "gate blocked publish:\n{}",
        crate::checks::gate::format_reasons(&verdict.reasons)
    );

    // Reviews should be absurdly easy: a clickable (OSC 8) link to the live
    // review session above the confirmation prompt, with a copyable raw URL
    // fallback for terminals without hyperlink support.
    if let Some(url) = lavish::session_url(&paths.review_html()) {
        println!(
            "review it: {}  (or: {url})",
            lavish::terminal_link(&url, "open the live review session")
        );
    }
    // Guarantee: confirming publishes the article edit AND upserts this
    // session's entry on the disclosure log (one yes, both writes).
    println!(
        "confirming also updates the session log on {}",
        corpus.house_rules.disclosure.log_page
    );

    let wiki = Wikipedia::connect().await?;
    let mut confirm = TtyConfirm;
    let outcome = wiki
        .edit(
            EditRequest {
                title: &meta.article,
                base_revid: meta.base_revid,
                wikitext: &proposed_wikitext,
                summary,
                review_artifact: Some(&format!("sessions/{slug}/review.html")),
                dry_run: false,
            },
            &mut confirm,
        )
        .await?;

    // Re-pin: base becomes the published state.
    let mut meta = meta;
    meta.base_revid = outcome.new_revid;
    meta.last_published_diff_url = Some(outcome.diff_url.clone());
    std::fs::write(paths.meta(), serde_json::to_string_pretty(&meta)?)?;
    std::fs::write(paths.base(), &proposed_wikitext)?;
    let entry = RoundEntry {
        round: 0,
        timestamp: now_iso(),
        summary: format!("published: {summary}"),
        phase: "published".into(),
        detail: vec![outcome.diff_url.clone()],
    };
    append_round(&paths, &entry)?;
    if outcome.created_revision() {
        println!(
            "published: {} (new revid {})",
            outcome.diff_url, outcome.new_revid
        );
    } else {
        println!(
            "no change: the page already contains this text (revid {})",
            outcome.new_revid
        );
    }
    println!(
        "check it: {}  (or: {})",
        lavish::terminal_link(&outcome.permalink(), "open the saved revision"),
        outcome.permalink()
    );
    // Automatic disclosure-log upsert (bundled consent: the prompt stated
    // confirming covers this). One entry per article session, growing with
    // each published diff.
    if outcome.created_revision() {
        let diffs: Vec<String> = std::fs::read_to_string(paths.rounds())
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str::<RoundEntry>(line).ok())
                    .filter(|e| e.phase == "published")
                    .flat_map(|e| e.detail)
                    .map(|d| {
                        let label = d
                            .rsplit("diff=")
                            .next()
                            .unwrap_or("diff")
                            .split('&')
                            .next()
                            .unwrap_or("diff")
                            .to_string();
                        format!("[{d} {label}]")
                    })
                    .collect()
            })
            .unwrap_or_default();
        let model = &corpus.house_rules.disclosure.drafting_model;
        let diffs_text = if diffs.is_empty() {
            "(none)".to_string()
        } else {
            diffs.join(" ")
        };
        let entry = format!(
            "* '''{date}''' — [[{article}]] (assisted editing session). AI assistance: {model} (initial drafting and tooling implementation; every edit human-reviewed and confirmed). Diffs: {diffs_text}",
            date = chrono::Utc::now().date_naive(),
            article = meta.article,
            model = model,
            diffs_text = diffs_text,
        );
        let marker = format!("wa-session:{slug}");
        let mut bundled = crate::wikipedia::BundledConsent;
        match wiki
            .upsert_disclosure_log(
                &corpus.house_rules.disclosure.log_page,
                &marker,
                &entry,
                &mut bundled,
            )
            .await
        {
            Ok(Some(log_outcome)) => println!(
                "session log updated: {}  (or: {})",
                lavish::terminal_link(&log_outcome.permalink(), "open the log entry"),
                log_outcome.permalink()
            ),
            Ok(None) => println!("session log already current"),
            Err(e) => {
                println!(
                    "WARNING: session log upsert failed ({e}); run `wa disclosure-log` to retry"
                );
            }
        }
    }
    println!("post-publish: run Earwig compare per new web source");
    Ok(())
}

fn ledger_register(slug: &str, url: &str, title: Option<&str>, work: Option<&str>) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let mut ledger = Ledger::load(&paths.ledger())?;
    let id = ledger.register_source(
        url,
        chrono::Utc::now().date_naive().to_string(),
        Some(SourceMetadata {
            title: title.map(str::to_string),
            work: work.map(str::to_string),
            ..Default::default()
        }),
    );
    ledger.save(&paths.ledger())?;
    println!("registered {id}: {url}");
    Ok(())
}

async fn ledger_fetch(slug: &str, source: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let mut ledger = Ledger::load(&paths.ledger())?;
    let url = ledger
        .sources
        .iter()
        .find(|s| s.id == source)
        .map(|s| s.url.clone())
        .ok_or_else(|| anyhow::anyhow!("unknown source {source}"))?;
    let fetcher = crate::ledger::SourceFetcher::new()?;
    let body = fetcher
        .fetch_text(&url)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let looks_html = body.trim_start().starts_with('<');
    let text = if looks_html {
        crate::ledger::net::html_to_text(&body)
    } else {
        body
    };
    ledger.attach_fetched_text(source, &text)?;
    ledger.save(&paths.ledger())?;
    println!("fetched {source}: {} chars stored", text.len());
    Ok(())
}

async fn ledger_archive(slug: &str, source: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let mut ledger = Ledger::load(&paths.ledger())?;
    let url = ledger
        .sources
        .iter()
        .find(|s| s.id == source)
        .map(|s| s.url.clone())
        .ok_or_else(|| anyhow::anyhow!("unknown source {source}"))?;
    let spn = crate::ledger::SavePageNow::default();
    let archive_url = spn.save(&url).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    ledger.attach_archive_url(source, &archive_url)?;
    ledger.save(&paths.ledger())?;
    println!("archived {source}: {archive_url}");
    Ok(())
}

fn ledger_quote(slug: &str, source: &str, text: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let mut ledger = Ledger::load(&paths.ledger())?;
    let id = ledger
        .add_quote(source, text)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ledger.save(&paths.ledger())?;
    println!("quote {id} verified and stored");
    Ok(())
}

fn ledger_claim(slug: &str, prose: &str, quotes: &[String]) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let mut ledger = Ledger::load(&paths.ledger())?;
    let id = ledger
        .add_claim(prose, quotes.to_vec())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ledger.save(&paths.ledger())?;
    println!("claim {id} recorded");
    Ok(())
}

async fn disclosure_log_cmd(slug: &str, entry: &str, marker: &str) -> Result<()> {
    let _ = slug;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let wiki = Wikipedia::connect().await?;
    let mut confirm = TtyConfirm;
    match wiki
        .append_disclosure_log(
            &corpus.house_rules.disclosure.log_page,
            entry,
            marker,
            &mut confirm,
        )
        .await?
    {
        Some(outcome) => {
            println!(
                "disclosure log appended to {}",
                corpus.house_rules.disclosure.log_page
            );
            println!(
                "check it: {}  (or: {})",
                lavish::terminal_link(&outcome.permalink(), "open the log entry"),
                outcome.permalink()
            );
        }
        None => println!(
            "disclosure log already contains this entry (no-op): {}",
            corpus.house_rules.disclosure.log_page
        ),
    }
    Ok(())
}

/// `wa check <slug>` — standalone gate run (fail-fast preflight): the same
/// gate as render's mandatory pre-flight, run without any artifact attempt
/// and without opening a session. Output is the structured, disposition-
/// grouped report; exits non-zero when blocked so the driver can iterate
/// cheaply before rendering.
fn check_cmd(slug: &str) -> Result<()> {
    let (paths, _meta) = load_session(slug)?;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let base_wikitext = std::fs::read_to_string(paths.base())?;
    let proposed_wikitext = std::fs::read_to_string(paths.proposed())?;
    let findings = FindingsFile::load(&paths.findings()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let ledger = Ledger::load(&paths.ledger())?;

    let verdict = crate::checks::gate::run_gate(&GateInput {
        ledger: &ledger,
        findings: &findings.findings,
        base_wikitext: &base_wikitext,
        proposed_wikitext: &proposed_wikitext,
        linter_config: &corpus.linter,
        paraphrase_config: &corpus.paraphrase,
    });
    if verdict.blocked {
        print!("{}", crate::checks::gate::format_reasons(&verdict.reasons));
        println!("\nno artifact written (wa check never renders)");
        anyhow::bail!("gate blocked");
    }
    println!("gate: PASS — the proposal is renderable (no artifact written by check)");
    Ok(())
}

fn lint_cmd(path: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    let config = LinterConfig::load(std::path::Path::new("rules/linter.toml"))?;
    let findings = linter::scan_whole_page(&text, &config);
    if findings.is_empty() {
        println!("clean");
    }
    for f in &findings {
        println!(
            "[{}] {} (line {}): {}",
            f.severity.label(),
            f.rule,
            f.line,
            f.detail
        );
    }
    Ok(())
}
