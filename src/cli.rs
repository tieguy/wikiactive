//! CLI wiring: `wa` — session init / analyze / assess / render / poll /
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
use crate::session::AssessmentsFile;
use crate::session::RoundEntry;
use crate::session::SessionMeta;
use crate::session::SessionPaths;
use crate::wikipedia::ConfirmSource;
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
    /// Assessment authoring (schema-validated).
    Assess {
        #[command(subcommand)]
        cmd: AssessCmd,
    },
    /// Retired name (loop-mechanization Phase 1): assessments. Always
    /// fails with a pointer; never dispatches.
    #[command(hide = true)]
    Findings {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _rest: Vec<String>,
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
    /// (Legacy tty path — the default loop reviews in-app via `wa serve`;
    /// see `wa comments`.)
    Poll {
        slug: String,
        /// Reply to show in the Lavish conversation panel before waiting.
        #[arg(long)]
        agent_reply: Option<String>,
    },
    /// The review comment queue (plan-004): block-anchored comments, the
    /// single reviewer↔loop interface (the session page's forms write
    /// here; the driver's resolve step consumes the open entries).
    Comments {
        #[command(subcommand)]
        cmd: CommentsCmd,
    },
    /// Rule review (rule-enforcement item 5): a model pass reads the
    /// drafted text against the tier-1 rules and this loop's cards —
    /// clause-by-clause advice, on demand, never a gate.
    Review { slug: String },
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
    /// Source sweep (plan-003 B.3): fetch-or-dispose every cited source
    /// before textual analysis.
    Sweep {
        #[command(subcommand)]
        cmd: SweepCmd,
    },
    /// Local web console (plan-003 B.4): session console, sweep manifest,
    /// publish confirmation. Loopback-only bind by default; `--tsnet`
    /// (thin-client setups) binds the machine's TAILNET interface only.
    Serve {
        /// Port (default 7427).
        #[arg(long)]
        port: Option<u16>,
        /// Bind the Tailscale interface instead of loopback (operator
        /// opt-in; the browser reaches this machine via the tailnet).
        #[arg(long)]
        tsnet: bool,
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
pub enum SweepCmd {
    /// Inventory the base wikitext's citation apparatus into ledger
    /// candidates (pending). URL-less books/ISBNs are auto-dispositioned
    /// `print: no web text`.
    Inventory {
        slug: String,
        /// Offline/tests: parse this wikitext file instead of the
        /// session's base.
        #[arg(long)]
        wikitext: Option<PathBuf>,
    },
    /// Batch fetch + classify pending sources (`fetched` / `needs_operator`
    /// / `snapshot_available` / `no_text`). Dead links get a Wayback CDX check.
    Fetch { slug: String },
    /// Record an operator-signed disposition on a source (resolves the
    /// sweep gate).
    Dispose {
        slug: String,
        /// Source id (S3, …).
        #[arg(long)]
        source: String,
        /// Free-form operator disposition, e.g. "attested-unreachable",
        /// "dropped: paywall".
        #[arg(long)]
        disposition: String,
    },
    /// Show the sweep manifest (per-source status + disposition).
    Status { slug: String },
}

#[derive(Subcommand)]
pub enum CommentsCmd {
    /// List the queue: open comments first, then resolved with their notes.
    List { slug: String },
    /// Add a comment from the tty (the `ev-N` path and anything the web
    /// forms make awkward).
    Add {
        slug: String,
        /// Target: the block's wikitext anchor from the artifact's anchor
        /// table (`L..:C..-L..:C..`, `base:`-prefixed, `ledger:Q<n>`) or
        /// the artifact element id (`wa-2`, `ev-1`).
        #[arg(long)]
        target: String,
        /// The comment text.
        #[arg(long)]
        text: String,
        /// Operator-highlighted words (free text).
        #[arg(long)]
        quoted: Option<String>,
    },
    /// Resolve a comment with a manual note (the evidence-card path and
    /// anything handled outside the driver step).
    Resolve {
        slug: String,
        /// Comment id (K1, …).
        #[arg(long)]
        id: String,
        /// What was done.
        #[arg(long)]
        note: String,
    },
}

#[derive(Subcommand)]
pub enum AssessCmd {
    /// Append assessments from a JSON file (array of assessments) or '-' for stdin.
    Add {
        slug: String,
        /// Path to JSON ('-' = stdin). Content: an assessment object or an
        /// array of assessments.
        json: String,
    },
    /// List assessments.
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
    /// Attach operator-provided text for a source the fetcher cannot reach
    /// (paywalled, bot-protected, or lending-gated). Quotes against it are
    /// verbatim-verified exactly like auto-fetched text — operator fetches
    /// in their browser, the tool keeps the guarantee.
    Attach {
        slug: String,
        #[arg(long)]
        source: String,
        /// Path to the saved text ('-' = stdin).
        #[arg(long)]
        file: String,
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
        Command::Assess { cmd } => match cmd {
            AssessCmd::Add { slug, json } => assess_add(&slug, &json),
            AssessCmd::List { slug } => assess_list(&slug),
        },
        Command::Findings { .. } => anyhow::bail!(
            "`wa findings` is now `wa assess` — try `wa assess add <slug> <json|->` \
             (schema-validated admission) or `wa assess list <slug>`"
        ),
        Command::Sweep { cmd } => match cmd {
            SweepCmd::Inventory { slug, wikitext } => sweep_inventory(&slug, wikitext.as_deref()),
            SweepCmd::Fetch { slug } => sweep_fetch(&slug).await,
            SweepCmd::Dispose {
                slug,
                source,
                disposition,
            } => sweep_dispose(&slug, &source, &disposition),
            SweepCmd::Status { slug } => sweep_status(&slug),
        },
        Command::Serve { port, tsnet } => crate::serve::run(port.unwrap_or(7427), tsnet).await,
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
                Via::Tty,
            )
            .await
        }
        Command::Poll { slug, agent_reply } => poll_cmd(&slug, agent_reply.as_deref()),
        Command::Comments { cmd } => match cmd {
            CommentsCmd::List { slug } => comments_list(&slug),
            CommentsCmd::Add {
                slug,
                target,
                text,
                quoted,
            } => comments_add(&slug, &target, &text, quoted.as_deref()),
            CommentsCmd::Resolve { slug, id, note } => comments_resolve(&slug, &id, &note),
        },
        Command::Publish { slug, summary } => publish_cmd(&slug, &summary).await,
        Command::Review { slug } => review_cmd(&slug).await,
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
            LedgerCmd::Attach { slug, source, file } => ledger_attach(&slug, &source, &file),
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
    // Init writes a fresh ledger, assessments and proposed text: on an
    // existing session that would erase the work in it.
    anyhow::ensure!(
        !paths.meta().exists(),
        "session sessions/{slug} already exists — init would erase its ledger, assessments \
         and draft; remove the directory first to start over"
    );
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
    AssessmentsFile::default()
        .save(&paths.assessments())
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
        if change.missing_newline() {
            out.push('\n');
        }
    }
    out
}

use crate::comments::now_iso;

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
    let assessments =
        AssessmentsFile::load(&paths.assessments()).map_err(|e| anyhow::anyhow!("{e}"))?;
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
    let bundle = build_context_bundle(&corpus, &article, &assessments.assessments, &ledger)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::write(paths.dir.join("context.md"), &bundle.text)?;
    println!("{}", bundle.text);
    Ok(())
}

fn assess_add(slug: &str, json_source: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let raw = if json_source == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(json_source).context("read assessments json")?
    };
    let trimmed = raw.trim();
    // Accept a single object or an array.
    let incoming: Vec<crate::session::Assessment> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).context("parse assessments array")?
    } else {
        vec![serde_json::from_str(trimmed).context("parse assessment object")?]
    };
    // Schema-validate every assessment (all problems at once).
    for assessment in &incoming {
        assessment.validate().map_err(|problems| {
            anyhow::anyhow!(
                "assessment {} invalid: {}",
                assessment.id,
                problems.join("; ")
            )
        })?;
    }
    let mut file = if paths.assessments().exists() {
        AssessmentsFile::load(&paths.assessments()).map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        AssessmentsFile::default()
    };
    let mut accepted = Vec::new();
    for assessment in incoming {
        anyhow::ensure!(
            !file.assessments.iter().any(|a| a.id == assessment.id),
            "duplicate assessment id {}",
            assessment.id
        );
        accepted.push(assessment.id.clone());
        file.assessments.push(assessment);
    }
    file.save(&paths.assessments())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    // Announce only what is now on disk (a rejected batch saves nothing).
    for id in accepted {
        println!("accepted assessment {id}");
    }
    Ok(())
}

fn assess_list(slug: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let file = AssessmentsFile::load(&paths.assessments()).map_err(|e| anyhow::anyhow!("{e}"))?;
    for f in &file.assessments {
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

fn sweep_inventory(slug: &str, wikitext_path: Option<&std::path::Path>) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let wikitext = match wikitext_path {
        Some(p) => std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?,
        None => std::fs::read_to_string(paths.base())?,
    };
    let candidates = crate::sweep::parse_citations(&wikitext);
    let mut ledger = Ledger::load(&paths.ledger())?;
    println!(
        "sweep inventory: {} distinct cited sources",
        candidates.len()
    );
    for c in &candidates {
        let key = c
            .ledger_url()
            .ok_or_else(|| anyhow::anyhow!("candidate without url or isbn"))?;
        let metadata = if c.title.is_some() || c.work.is_some() {
            Some(crate::ledger::SourceMetadata {
                title: c.title.clone(),
                work: c.work.clone(),
                ..Default::default()
            })
        } else {
            None
        };
        let (id, added) = ledger.register_sweep_source(&key, metadata);
        let disp = ledger
            .sources
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.disposition.clone())
            .unwrap_or_default();
        println!(
            "  {id} {}{key}{} {disp}",
            if added { "registered " } else { "already     " },
            if c.dead_original.is_some() {
                " (dead original; archive is the fetch target)"
            } else {
                ""
            }
        );
    }
    ledger.save(&paths.ledger())?;
    println!(
        "next: `wa sweep fetch {slug}` (network) then resolve the rest by capture or disposition"
    );
    Ok(())
}

pub(crate) async fn sweep_fetch(slug: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let config = crate::sweep::SweepConfig::load(std::path::Path::new("rules/sweep.toml"))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let fetcher = crate::ledger::SourceFetcher::new()?;
    let cdx = crate::ledger::net::CdxClient::default();
    let mut ledger = Ledger::load(&paths.ledger())?;
    let pending: Vec<String> = ledger
        .sources
        .iter()
        .filter(|s| {
            // A source that already has text (an operator capture above
            // all) is never refetched over.
            s.fetched_text.as_deref().is_none_or(str::is_empty)
                && matches!(
                    s.sweep_status.as_deref(),
                    Some(crate::sweep::status::PENDING | crate::sweep::status::SNAPSHOT_AVAILABLE)
                )
        })
        .map(|s| s.id.clone())
        .collect();
    if pending.is_empty() {
        println!("nothing pending — run `wa sweep inventory {slug}` first");
        return Ok(());
    }
    println!("sweeping {} pending sources…", pending.len());
    for id in pending {
        let url = ledger
            .sources
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.url.clone())
            .unwrap_or_default();
        match crate::sweep::sweep_fetch_one(&fetcher, &cdx, &mut ledger, &id, &config).await {
            Ok(outcome) => {
                println!("  {id} -> {} {url}", outcome.status);
                if let Some(note) = outcome.note {
                    println!("        {note}");
                }
            }
            Err(e) => println!("  {id} -> ERROR ({e}) — left pending"),
        }
        // Saved per source: an interrupted sweep keeps what it fetched.
        ledger.save(&paths.ledger())?;
    }
    println!("manifest: `wa sweep status {slug}`");
    Ok(())
}

fn sweep_dispose(slug: &str, source: &str, disposition: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let mut ledger = Ledger::load(&paths.ledger())?;
    ledger.set_disposition(source, disposition)?;
    ledger.save(&paths.ledger())?;
    println!("disposition recorded on {source}: {disposition}");
    Ok(())
}

fn sweep_status(slug: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let ledger = Ledger::load(&paths.ledger())?;
    if !ledger.has_sweep_state() {
        println!("no sweep state (run `wa sweep inventory {slug}`)");
        return Ok(());
    }
    let unresolved = ledger.sweep_unresolved().len();
    println!("sweep manifest ({unresolved} unresolved):");
    for s in &ledger.sources {
        let status = s.sweep_status.clone().unwrap_or_else(|| "—".into());
        let disp = s.disposition.clone().unwrap_or_else(|| "—".into());
        let text = if s.fetched_text.is_some() {
            "text✓"
        } else {
            "    "
        };
        println!("  {} [{status}] {text} disp: {disp} — {}", s.id, s.url);
    }
    Ok(())
}

/// Who is driving the command — selects the review-link surface
/// (plan-004 P.4): the web path prints the in-app artifact path and never
/// touches lavish state; the tty path keeps the lavish session link (it
/// still uses `wa poll` legitimately).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// The CLI (lavish link + `wa poll` remain available).
    Tty,
    /// `wa serve` (in-app artifact link; no lavish probes).
    Web,
}

/// Gate + render the review artifact (driver-facing surface: the e2e
/// session runner and `wa serve` call the same path as the CLI).
///
/// # Errors
/// Session/rules IO, a blocked gate (no artifact is written), or lavish
/// open failures.
#[allow(clippy::too_many_arguments)]
pub async fn render_cmd(
    slug: &str,
    round: u32,
    html_base: Option<PathBuf>,
    html_proposed: Option<PathBuf>,
    summary: &str,
    no_open: bool,
    reopen: bool,
    via: Via,
) -> Result<()> {
    let (paths, meta) = load_session(slug)?;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let base_wikitext = std::fs::read_to_string(paths.base())?;
    let proposed_wikitext = std::fs::read_to_string(paths.proposed())?;
    let assessments =
        AssessmentsFile::load(&paths.assessments()).map_err(|e| anyhow::anyhow!("{e}"))?;
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
        assessments: &assessments.assessments,
        ledger: &ledger,
        linter_config: &corpus.linter,
        paraphrase_config: &corpus.paraphrase,
        revisions,
    })
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    std::fs::write(paths.review_html(), &output.artifact_html)?;
    // Back-filled rendered_span_ids persist.
    let mut updated = assessments;
    updated.assessments = output.updated_assessments;
    updated
        .save(&paths.assessments())
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
    if !no_open && via == Via::Tty {
        let out = lavish::open_session(&paths.review_html(), false, reopen)?;
        print_lavish_output(&out)?;
    }
    match via {
        Via::Tty => {
            if let Some(url) = lavish::session_url(&paths.review_html()) {
                println!(
                    "review: {}  (or: {url})",
                    lavish::terminal_link(&url, "open the review session")
                );
            }
        }
        Via::Web => {
            // The web loop's review surface is the in-app artifact; no
            // lavish state is read, no lavish URL printed (AC.1).
            println!("review: /sessions/{slug}/review (in-app)");
        }
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
    serde_json::from_str(html[start..end].trim()).unwrap_or_else(|e| {
        eprintln!(
            "warning: revisions registry unparsable in {} ({e}) — earlier rounds drop out of \
             the legend",
            review_html.display()
        );
        Vec::new()
    })
}

/// Archive the session's findings to `findings-archive.jsonl` (one line per
/// finding, annotated with the published diff url) and reset `assessments.json`
/// for the next edit.
fn archive_assessments(paths: &SessionPaths, diff_url: &str) -> Result<()> {
    let assessments =
        AssessmentsFile::load(&paths.assessments()).map_err(|e| anyhow::anyhow!("{e}"))?;
    if !assessments.assessments.is_empty() {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(paths.dir.join("assessments-archive.jsonl"))
            .context("open assessments archive")?;
        for f in &assessments.assessments {
            let mut archived = serde_json::to_value(f)?;
            archived["published_diff"] = serde_json::Value::String(diff_url.to_string());
            writeln!(file, "{archived}")?;
        }
    }
    AssessmentsFile::default()
        .save(&paths.assessments())
        .map_err(|e| anyhow::anyhow!("{e}"))
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
    if !output.status.success() && stdout.trim().is_empty() {
        anyhow::bail!(
            "lavish poll failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
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
    crate::render::read_anchor_table(review_html)
        .into_iter()
        .map(|e| (e.element_id, e.wikitext_anchor))
        .collect()
}

fn print_lavish_output(output: &std::process::Output) -> Result<()> {
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stderr().write_all(&output.stderr)?;
    Ok(())
}

/// `wa comments list` — the queue, open first.
fn comments_list(slug: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let queue = crate::comments::CommentQueue::load(&paths.comments())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let open: Vec<_> = queue
        .comments
        .iter()
        .filter(|c| c.status == crate::comments::CommentStatus::Open)
        .collect();
    println!("{} open / {} total", open.len(), queue.comments.len());
    for c in &open {
        print_comment(c);
    }
    for c in &queue.comments {
        if c.status == crate::comments::CommentStatus::Resolved {
            print_comment(c);
        }
    }
    Ok(())
}

fn print_comment(c: &crate::comments::Comment) {
    let status = match c.status {
        crate::comments::CommentStatus::Open => "OPEN",
        crate::comments::CommentStatus::Resolved => "resolved",
    };
    println!("[{status}] {} → {}", c.id, c.target);
    if let Some(q) = &c.quoted {
        println!("      highlighted: {q:?}");
    }
    println!("      {}", c.text);
    if let Some(note) = &c.resolution {
        println!("      resolution: {note}");
    }
}

/// `wa comments add` — append one comment (tty / `ev-N` path).
fn comments_add(slug: &str, target: &str, text: &str, quoted: Option<&str>) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let mut queue = crate::comments::CommentQueue::load(&paths.comments())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let comment = crate::comments::Comment::new(target, text, quoted, &crate::comments::now_iso());
    let id = queue
        .append(&paths.comments(), comment)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("queued {id} → {target}");
    Ok(())
}

/// `wa comments resolve` — manual resolution with a note.
fn comments_resolve(slug: &str, id: &str, note: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let mut queue = crate::comments::CommentQueue::load(&paths.comments())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    queue
        .resolve(&paths.comments(), id, note)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("resolved {id}");
    Ok(())
}

/// Disclosure-log entry for one article session. The publish-time upsert
/// REGENERATES this text on every publish (the marker-delimited block is
/// replaced wholesale), so this template IS the wording that ends up
/// on-wiki — a hand edit to the entry is clobbered at the next publish.
/// Change the wording here (and in the pin test), never on-wiki.
fn disclosure_entry(date: &str, article: &str, model: &str, diffs_text: &str) -> String {
    format!(
        "* '''{date}''' — [[{article}]]. AI assistance: {model} (initial drafting and tooling implementation; every edit human-reviewed and confirmed). Diffs: {diffs_text}"
    )
}

/// What a completed publish did (shared by the CLI prints and the
/// `wa serve` session state).
#[derive(Debug)]
pub struct PublishOutcome {
    /// Diff URL of the published revision.
    pub diff_url: String,
    /// The new revid (0 when the edit was a no-change).
    pub new_revid: u64,
    /// Whether a revision was actually created.
    pub created_revision: bool,
    /// Read-back lines (rule-enforcement item 1): empty = the saved
    /// revision matched; each line names one mismatch, or reports that
    /// the read-back itself failed.
    pub verification: Vec<String>,
}

/// The shared publish path (plan-003 B.4): gate → confirm → edit → re-pin
/// → disclosure-log upsert. The tty CLI and `wa serve`'s web action run
/// the SAME flow with the confirm source injected. `BundledConsent` backs
/// only the disclosure-log upsert bundled into the one yes — never the
/// article edit. There is no auto-publish path: every caller requires an
/// explicit operator action through its `ConfirmSource`.
// One linear flow by design (gate → confirm → edit → re-pin → disclosure);
// splitting it would hide the ordering guarantees.
#[allow(clippy::too_many_lines)]
///
/// # Errors
/// Gate blocked, wiki/edit errors (including [`crate::wikipedia::WikipediaError::Declined`]),
/// or session IO failures.
pub async fn publish_core(
    slug: &str,
    summary: &str,
    wiki: &Wikipedia,
    confirm: &mut dyn ConfirmSource,
    via: Via,
) -> Result<PublishOutcome> {
    let (paths, meta) = load_session(slug)?;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let base_wikitext = std::fs::read_to_string(paths.base())?;
    let proposed_wikitext = std::fs::read_to_string(paths.proposed())?;
    let assessments =
        AssessmentsFile::load(&paths.assessments()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let ledger = Ledger::load(&paths.ledger())?;

    // Publish gate: the SAME gate as render's pre-flight, re-run now.
    let verdict = crate::checks::gate::run_gate(&GateInput {
        ledger: &ledger,
        assessments: &assessments.assessments,
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

    // Reviews should be absurdly easy: the tty path gets a clickable
    // (OSC 8) link to the live lavish review session above the
    // confirmation prompt; the web path links the in-app artifact (no
    // lavish state read — AC.1).
    match via {
        Via::Tty => {
            if let Some(url) = lavish::session_url(&paths.review_html()) {
                println!(
                    "review it: {}  (or: {url})",
                    lavish::terminal_link(&url, "open the live review session")
                );
            }
        }
        Via::Web => println!("review it: /sessions/{slug}/review (in-app artifact)"),
    }
    // Guarantee: confirming publishes the article edit AND upserts this
    // session's entry on the disclosure log (one yes, both writes).
    println!(
        "confirming also updates the session log on {}",
        corpus.house_rules.disclosure.log_page
    );

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
            confirm,
        )
        .await?;

    // Re-pin: base becomes the published state.
    let base_before_publish = meta.base_revid;
    let mut meta = meta;
    meta.base_revid = outcome.new_revid;
    // A null edit (the page already held this text) created no revision:
    // it has no diff to record, here or in the round log — an empty URL
    // would otherwise reach the on-wiki disclosure entry as a dead link.
    let published_diff: Vec<String> = outcome
        .created_revision()
        .then(|| outcome.diff_url.clone())
        .into_iter()
        .collect();
    if let Some(diff) = published_diff.first() {
        meta.last_published_diff_url = Some(diff.clone());
    }
    std::fs::write(paths.meta(), serde_json::to_string_pretty(&meta)?)?;
    std::fs::write(paths.base(), &proposed_wikitext)?;
    // Read-back (rule-enforcement item 1): compare what the wiki recorded
    // with what this path intended. The edit is already live, so a
    // mismatch warns and records — it can never turn the publish into an
    // error. Null edits created no revision: nothing to read back.
    let verification = if outcome.created_revision() {
        let expected_summary = crate::wikipedia::summary_with_disclosure(summary)
            .unwrap_or_else(|_| summary.to_string());
        wiki.verify_revision(
            outcome.new_revid,
            &crate::wikipedia::RevisionExpectation {
                base_revid: base_before_publish,
                summary: &expected_summary,
                operator: corpus.house_rules.operator.username.as_deref(),
                text: &proposed_wikitext,
            },
        )
        .await
    } else {
        Vec::new()
    };
    // Publishing consumes the session's findings: archive them with the
    // diff link (audit trail) and reset — the next edit's artifact must
    // show only ITS evidence, not stale cards from published edits
    // (operator catch: "evidence for this edit seems cached").
    archive_assessments(&paths, &outcome.diff_url)?;
    // The round entry records the read-back lines after the diff URL —
    // the audit trail carries what the wiki actually saved.
    let mut round_detail = published_diff;
    round_detail.extend(verification.iter().cloned());
    let entry = RoundEntry {
        round: 0,
        timestamp: now_iso(),
        summary: format!("published: {summary}"),
        phase: "published".into(),
        detail: round_detail,
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
    // Read-back result (rule-enforcement item 1): every line under a
    // VERIFY heading; a clean read-back is stated too, so its absence is
    // visible.
    if outcome.created_revision() {
        if verification.is_empty() {
            println!("VERIFY: read-back clean — the saved revision matches what was sent");
        } else {
            println!("VERIFY: read-back FAILED — the saved revision differs from what was sent:");
            for line in &verification {
                println!("  {line}");
            }
        }
    }
    // Tty gets a clickable (OSC 8) link; the web path logs a plain URL —
    // the serve log is not a terminal (plan-005 O.2: raw OSC-8 escapes
    // were leaking into `wa serve`'s log from this shared path).
    match via {
        Via::Tty => println!(
            "check it: {}  (or: {})",
            lavish::terminal_link(&outcome.permalink(), "open the saved revision"),
            outcome.permalink()
        ),
        Via::Web => println!("check it: {}", outcome.permalink()),
    }
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
        let entry = disclosure_entry(
            &chrono::Utc::now().date_naive().to_string(),
            &meta.article,
            model,
            &diffs_text,
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
            Ok(Some(log_outcome)) => {
                // Same Via split as the diff link above (plan-005 O.2).
                match via {
                    Via::Tty => println!(
                        "session log updated: {}  (or: {})",
                        lavish::terminal_link(&log_outcome.permalink(), "open the log entry"),
                        log_outcome.permalink()
                    ),
                    Via::Web => println!("session log updated: {}", log_outcome.permalink()),
                }
            }
            Ok(None) => println!("session log already current"),
            Err(e) => {
                println!(
                    "WARNING: session log upsert failed ({e}); run `wa disclosure-log` to retry"
                );
            }
        }
    }
    Ok(PublishOutcome {
        created_revision: outcome.created_revision(),
        diff_url: outcome.diff_url,
        new_revid: outcome.new_revid,
        verification,
    })
}

/// `wa review <slug>` — the rule-review judgment point, tty path
/// (rule-enforcement item 5): advice printed here and stored beside the
/// session (`rule-review.json`, keyed by round) for the review page to
/// show. Never a gate, on demand only.
async fn review_cmd(slug: &str) -> Result<()> {
    use crate::driver::steps::ConcernVerdict;
    let (paths, meta) = load_session(slug)?;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let artifact = std::fs::read_to_string(paths.review_html())
        .context("no review artifact — render first")?;
    // Only the CURRENT artifact: a stale anchor table must not be
    // reviewed (same refusal as the serve path).
    let round = match crate::serve::artifact_state(&paths.dir) {
        Some(crate::serve::ArtifactState::Current { round }) => round,
        Some(crate::serve::ArtifactState::Stale { .. }) => {
            anyhow::bail!("the review artifact is out of date — re-render first")
        }
        None => anyhow::bail!("no current review artifact — render first"),
    };
    let base = std::fs::read_to_string(paths.base())?;
    let proposed = std::fs::read_to_string(paths.proposed())?;
    let ledger = Ledger::load(&paths.ledger())?;
    let blocks = crate::serve::rule_review_blocks(&artifact, &base, &proposed, &ledger);
    anyhow::ensure!(!blocks.is_empty(), "no changed blocks to review");
    let guidance = crate::rules::guidance_for_loop(&corpus, meta.entry_loop)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let clauses = crate::rules::guidance_clauses(&corpus, meta.entry_loop);
    let model = corpus
        .house_rules
        .zai
        .as_ref()
        .and_then(|z| z.model.clone())
        .unwrap_or_else(|| "glm-5.3".to_string());
    let zai = crate::driver::model::ZaiClient::from_env(&model)
        .map_err(|e| anyhow::anyhow!("model client: {e}"))?;
    let concerns = crate::driver::steps::review_draft(&zai, &guidance, &clauses, &blocks).await?;
    // Store + print: concerns only (ok verdicts are discarded), each
    // under the block whose proposed text contains the span.
    let stored: Vec<(String, String, String, String)> = concerns
        .into_iter()
        .filter(|c| c.verdict == ConcernVerdict::Concern)
        .map(|c| {
            let element_id = blocks
                .iter()
                .find(|b| b.proposed.contains(c.span.trim()))
                .map_or_else(String::new, |b| b.element_id.clone());
            (c.clause, c.span, c.note, element_id)
        })
        .collect();
    let file = serde_json::json!({
        "round": round,
        "timestamp": crate::comments::now_iso(),
        "concerns": stored
            .iter()
            .map(|(clause, span, note, element_id)| {
                serde_json::json!({
                    "clause": clause, "span": span, "note": note, "element_id": element_id
                })
            })
            .collect::<Vec<_>>(),
    });
    std::fs::write(paths.rule_review(), serde_json::to_string_pretty(&file)?)?;
    if stored.is_empty() {
        println!("rule review: no concerns raised");
    } else {
        println!("rule review: {} concern(s)", stored.len());
        for (clause, span, note, _) in &stored {
            println!("  [{clause}] {note}\n      about: \"{}\"", span.trim());
        }
    }
    println!("stored: sessions/{slug}/rule-review.json (shown on the review page)");
    let entry = RoundEntry {
        round: 0,
        timestamp: now_iso(),
        summary: format!("rule review: {} concern(s)", stored.len()),
        phase: "rule-reviewed".into(),
        detail: Vec::new(),
    };
    append_round(&paths, &entry)?;
    Ok(())
}

async fn publish_cmd(slug: &str, summary: &str) -> Result<()> {
    let wiki = Wikipedia::connect().await?;
    let mut confirm = TtyConfirm::default();
    publish_core(slug, summary, &wiki, &mut confirm, Via::Tty).await?;
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
    // Prefer the sweep-found Wayback snapshot when the original is dead
    // (`snapshot_available` sources otherwise re-fetch the dead URL).
    let url = ledger
        .sources
        .iter()
        .find(|s| s.id == source)
        .map(|s| {
            s.metadata
                .as_ref()
                .and_then(|m| m.snapshot_url.clone())
                .unwrap_or_else(|| s.url.clone())
        })
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

/// `wa ledger attach` — ingest operator-fetched text for an unreachable
/// source; quotes against it verify verbatim exactly like auto-fetches
/// (driver-facing surface).
///
/// # Errors
/// Unknown session/source, unreadable capture, or unknown format.
pub fn ledger_attach(slug: &str, source: &str, file: &str) -> Result<()> {
    let (paths, _) = load_session(slug)?;
    let text = if file == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(file).with_context(|| format!("--file {file} unreadable"))?
    };
    anyhow::ensure!(!text.trim().is_empty(), "attached capture is empty");
    let (extracted, format) = Ledger::text_from_capture(file, &text);
    anyhow::ensure!(
        !extracted.trim().is_empty(),
        "no extractable text in {file} (format {format})"
    );
    let mut ledger = Ledger::load(&paths.ledger())?;
    let via = format!("operator:{format}");
    ledger
        .attach_operator_text(source, &extracted, &via)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ledger.save(&paths.ledger())?;
    println!(
        "attached {format} capture to {source} ({} chars extracted) — quotes verify against it as usual",
        extracted.chars().count()
    );
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
    let mut confirm = TtyConfirm::default();
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
    use std::fmt::Write as _;
    let (paths, _meta) = load_session(slug)?;
    let corpus =
        RulesCorpus::load(std::path::Path::new("rules")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let base_wikitext = std::fs::read_to_string(paths.base())?;
    let proposed_wikitext = std::fs::read_to_string(paths.proposed())?;
    let assessments =
        AssessmentsFile::load(&paths.assessments()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let ledger = Ledger::load(&paths.ledger())?;

    let verdict = crate::checks::gate::run_gate(&GateInput {
        ledger: &ledger,
        assessments: &assessments.assessments,
        base_wikitext: &base_wikitext,
        proposed_wikitext: &proposed_wikitext,
        linter_config: &corpus.linter,
        paraphrase_config: &corpus.paraphrase,
    });
    // Warn-severity lint findings never block, but the reviewer must see
    // them (rule-enforcement item 4) — after the gate report, either way.
    let warnings =
        crate::checks::gate::lint_warnings(&base_wikitext, &proposed_wikitext, &corpus.linter);
    let print_warnings = |out: &mut String| {
        if warnings.is_empty() {
            return;
        }
        let _ = writeln!(out, "WARNINGS (advisory — they do not block):");
        for w in &warnings {
            // NB: the config's map keys are snake_case; findings carry the
            // kebab-case id — look up by id.
            let description = corpus
                .linter
                .rules
                .values()
                .find(|r| r.id == w.rule)
                .map_or("", |r| r.description.as_str());
            let _ = writeln!(
                out,
                "  [{}] {} (line {}): {}\n      {}",
                w.severity.label(),
                w.rule,
                w.line,
                w.detail,
                description
            );
        }
    };
    if verdict.blocked {
        let mut report = crate::checks::gate::format_reasons(&verdict.reasons);
        print_warnings(&mut report);
        print!("{report}");
        println!("\nno artifact written (wa check never renders)");
        anyhow::bail!("gate blocked");
    }
    println!("gate: PASS — the proposal is renderable (no artifact written by check)");
    let mut out = String::new();
    print_warnings(&mut out);
    print!("{out}");
    Ok(())
}

fn lint_cmd(path: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    let config = LinterConfig::load(std::path::Path::new("rules/linter.toml"))?;
    let lint_hits = linter::scan_whole_page(&text, &config);
    if lint_hits.is_empty() {
        println!("clean");
    }
    for f in &lint_hits {
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

#[cfg(test)]
mod tests {
    /// Pins the disclosure entry template: the publish-time upsert
    /// regenerates the entry on every publish, so on-wiki wording always
    /// equals this string. "(assisted editing session)" was dropped as
    /// redundant (the whole tool is that; operator catch on the first live
    /// L2 publish) — it must not come back.
    #[test]
    fn disclosure_entry_template_is_pinned() {
        let entry = super::disclosure_entry(
            "2026-09-28",
            "Sarah Kidder",
            "glm-5.3",
            "[https://en.wikipedia.org/w/index.php?diff=1377121505&oldid=1370213000 1377121505]",
        );
        assert_eq!(
            entry,
            "* '''2026-09-28''' — [[Sarah Kidder]]. AI assistance: glm-5.3 (initial drafting and tooling implementation; every edit human-reviewed and confirmed). Diffs: [https://en.wikipedia.org/w/index.php?diff=1377121505&oldid=1370213000 1377121505]"
        );
        assert!(!entry.contains("assisted editing session"));
    }
}
