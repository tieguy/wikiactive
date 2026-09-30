//! en.wikipedia client on `mwapi` — `OAuth2` owner-only, etiquette-
//! internalized (AC.13), edit path with the publish gate invariants
//! (AC.7/AC.10).
//!
//! Every API call carries: the identifying User-Agent
//! (`crate::USER_AGENT`, hardcoded — a forked repo must edit it once, here),
//! `maxlag=5`, and `assert=user` (via the mwapi builder). The client never
//! reads any external skill file; the codified rules live in
//! `docs/api-etiquette.md` and `rules/house-rules.toml`.
//!
//! Auth: `OAuth2` **owner-only** consumer — a token issued at consumer
//! registration, supplied via `WIKIACTIVE_OAUTH2_TOKEN`; no authorize/token
//! exchange is performed. (SP42 implements the authorization-code flow;
//! wikiactive deliberately does not.)

use mwapi::Assert;
use mwapi::Client as ApiClient;
use serde_json::Value;

use crate::DISCLOSURE_SUFFIX;

/// Action API endpoint for en.wikipedia.
pub const ENWIKI_API: &str = "https://en.wikipedia.org/w/api.php";

/// Env var carrying the `OAuth2` owner-only token.
pub const OAUTH2_TOKEN_ENV: &str = "WIKIACTIVE_OAUTH2_TOKEN";

/// Env var for the `BotPasswords` smoke-test fallback:
/// `WIKIACTIVE_BOTPASSWORD="SomeUser@botname:password"`. Plan policy:
/// `BotPasswords` is for the userspace smoke test ONLY, never mainspace —
/// mainspace requires the OAuth owner-only consumer (descriptive tool tag,
/// revocable grant).
pub const BOTPASSWORD_ENV: &str = "WIKIACTIVE_BOTPASSWORD";

/// Confirmation source for the publish gate: the model can never
/// self-publish — an interactive read through this trait is required.
/// The decision may resolve asynchronously (the `wa serve` console's
/// pending-confirmation click), so the method returns a boxed future.
pub trait ConfirmSource: Send {
    /// Show `prompt`, return the human's decision.
    fn confirm<'a>(
        &'a mut self,
        prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>>;
}

fn ready_confirm(value: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> {
    Box::pin(std::future::ready(value))
}

/// Reads confirmation from `/dev/tty` so piped stdin/stdout cannot fake it.
/// `action` names the write in the trailing instruction — one phrase per
/// write kind ("publish", "write to Wikidata item Q…"): the yes is always
/// worded for exactly the write that follows, never a generic one.
#[derive(Default)]
pub struct TtyConfirm {
    /// The phrase after "[type yes to …]" (default: "publish").
    pub action: String,
}

impl TtyConfirm {
    /// A confirm source with a dedicated action phrase.
    #[must_use]
    pub fn for_action(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
        }
    }
}

impl ConfirmSource for TtyConfirm {
    fn confirm<'a>(
        &'a mut self,
        prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        let prompt = format!("{prompt} [type yes to {}] ", self.action);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || read_tty_confirm(&prompt))
                .await
                .unwrap_or(false)
        })
    }
}

fn read_tty_confirm(prompt: &str) -> bool {
    use std::io::BufRead;
    use std::io::Write;
    let Ok(tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    else {
        eprintln!("no controlling terminal; this write requires one");
        return false;
    };
    let mut reader = std::io::BufReader::new(tty);
    let mut handle = reader.get_ref().try_clone().expect("tty clone");
    let _ = writeln!(handle, "{prompt}");
    let _ = handle.flush();
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return false;
    }
    line.trim().eq_ignore_ascii_case("yes")
}

/// A confirmation source that always answers no (tests; absent tty).
#[derive(Default)]
pub struct DenyConfirm;

impl ConfirmSource for DenyConfirm {
    fn confirm<'a>(
        &'a mut self,
        _prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        ready_confirm(false)
    }
}

/// A confirmation source for writes whose consent was BUNDLED into the
/// publish prompt (the prompt states that confirming also updates the
/// disclosure session log). Never usable for the article edit itself —
/// `wa serve`'s publish action backs a dedicated [`WebConfirm`], and the
/// article edit's yes is always its own explicit operator action.
pub struct BundledConsent;

impl ConfirmSource for BundledConsent {
    fn confirm<'a>(
        &'a mut self,
        _prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        ready_confirm(true)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WikipediaError {
    #[error("mwapi: {0}")]
    Mwapi(#[from] mwapi::Error),
    #[error("api error {code}: {info}")]
    Api { code: String, info: String },
    #[error("page {0} not found")]
    PageMissing(String),
    #[error("edit conflict: base revid {base} is not current ({current})")]
    EditConflict { base: u64, current: u64 },
    #[error("operator declined to publish")]
    Declined,
    #[error("refusing bare/empty summary")]
    BareSummary,
    #[error("assert=user failed: {0}")]
    AssertFailed(String),
    #[error("json shape unexpected: {0}")]
    BadShape(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// The publish request (one logical edit).
#[derive(Debug, Clone)]
pub struct EditRequest<'a> {
    pub title: &'a str,
    /// The pinned revision this edit was prepared against. The edit posts
    /// with `baserevid` and the client pre-checks currency: a moved base
    /// aborts without writing (AC.10).
    pub base_revid: u64,
    pub wikitext: &'a str,
    /// Operator-approved scoped summary, WITHOUT the disclosure suffix
    /// (appended here, idempotently).
    pub summary: &'a str,
    /// Path to the rendered review artifact, shown in the confirmation
    /// prompt so the human reviews the visual diff before confirming.
    pub review_artifact: Option<&'a str>,
    /// Validate only: run every pre-flight step, do not post the edit.
    pub dry_run: bool,
}

/// Result of a successful (non-dry-run) edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOutcome {
    pub new_revid: u64,
    pub diff_url: String,
}

impl EditOutcome {
    /// Permanent link to the saved revision (empty `diff_url` = null edit; the
    /// revid then names the existing revision that already holds the text).
    #[must_use]
    pub fn permalink(&self) -> String {
        format!(
            "https://en.wikipedia.org/wiki/Special:PermaLink/{}",
            self.new_revid
        )
    }

    /// Whether this outcome created a revision (null edits did not).
    #[must_use]
    pub fn created_revision(&self) -> bool {
        !self.diff_url.is_empty()
    }
}

/// What the read-back (`Wikipedia::verify_revision`) expects of a
/// just-saved revision — everything the edit path promised, so a
/// presence-flag or config regression on the server side is seen, not
/// silently absorbed (rule-enforcement plan item 1).
#[derive(Debug, Clone, Copy)]
pub struct RevisionExpectation<'a> {
    /// The base revid the edit was pinned to; 0 = page creation (the
    /// parent check is skipped — a creation has no parent).
    pub base_revid: u64,
    /// The full expected summary, disclosure suffix included.
    pub summary: &'a str,
    /// House-rules operator username; `None` skips the user check.
    pub operator: Option<&'a str>,
    /// The proposed text the edit carried. The text compare is advisory:
    /// pre-save transforms may legitimately alter it.
    pub text: &'a str,
}

/// Build the final edit summary: refuse a bare summary, append the
/// disclosure suffix idempotently (AC.7).
///
/// # Errors
/// [`WikipediaError::BareSummary`] when the operator summary is empty.
pub fn summary_with_disclosure(summary: &str) -> Result<String, WikipediaError> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return Err(WikipediaError::BareSummary);
    }
    if trimmed.contains(DISCLOSURE_SUFFIX) {
        return Ok(trimmed.to_string());
    }
    Ok(format!("{trimmed} ({DISCLOSURE_SUFFIX})"))
}

/// Resolve the owner-only `OAuth2` token: [`OAUTH2_TOKEN_ENV`] first, then
/// the Bitwarden Secrets fallback. Shared by every wiki client (enwiki,
/// Wikidata) so the secret id lives in exactly one place. Returns None
/// when both are absent — callers proceed unauthenticated (read-only).
pub(crate) fn resolve_oauth2_token() -> Option<String> {
    std::env::var(OAUTH2_TOKEN_ENV)
        .ok()
        .filter(|t| !t.trim().is_empty())
        .or_else(resolve_oauth2_token_from_bws)
}

/// Bitwarden Secrets secret id holding the owner-only `OAuth2` token — an
/// identifier, not a secret (pinned like the UA constant; the operator's
/// secrets live in bws, operator direction 2026-09-27: "we should be
/// getting that from bws"). Env var always wins.
const BWS_SECRET_ID: &str = "1f6f7860-1ae7-4a31-a6d1-b4d0003768dc";

/// Fallback token source: `bws secret get` (Bitwarden Secrets CLI) when
/// the env var is unset. Returns None when bws is absent or fails — the
/// caller then proceeds unauthenticated (read-only) exactly as before.
fn resolve_oauth2_token_from_bws() -> Option<String> {
    let out = std::process::Command::new("bws")
        .args(["secret", "get", BWS_SECRET_ID, "--output", "json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value: String = serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .ok()?
        .get("value")?
        .as_str()?
        .to_string();
    (!value.trim().is_empty()).then_some(value)
}

/// The en.wikipedia API client.
pub struct Wikipedia {
    api: ApiClient,
}

impl Wikipedia {
    /// Connect to en.wikipedia with etiquette defaults and credentials from
    /// the environment: `OAuth2` owner-only token from [`OAUTH2_TOKEN_ENV`]
    /// if set, else the `BotPasswords` fallback from [`BOTPASSWORD_ENV`]
    /// (smoke-test only), else unauthenticated read-only. `assert=user` is
    /// set client-wide only when authenticated — an anonymous client must
    /// be able to read; the edit path passes `assert=user` explicitly per
    /// AC.7 either way.
    ///
    /// # Errors
    /// Client construction failure.
    pub async fn connect() -> Result<Self, WikipediaError> {
        let oauth2_token = resolve_oauth2_token();
        let botpassword = std::env::var(BOTPASSWORD_ENV).ok();
        let mut builder = ApiClient::builder(ENWIKI_API)
            .set_user_agent(crate::USER_AGENT)
            .set_maxlag(5)
            .set_concurrency(1);
        if let Some(token) = oauth2_token.as_deref() {
            builder = builder.set_oauth2_token(token).set_assert(Assert::User);
        } else if let Some(cred) = botpassword.as_deref() {
            let (user, pass) = cred.split_once(':').ok_or_else(|| {
                WikipediaError::BadShape(format!(
                    "{BOTPASSWORD_ENV} must be 'User@botname:password'"
                ))
            })?;
            builder = builder.set_botpassword(user, pass).set_assert(Assert::User);
        }
        Ok(Self {
            api: builder.build().await?,
        })
    }

    /// Connect to an explicit API URL (tests point this at a mock server).
    /// Client-wide `assert=user` is applied only when a token is given
    /// (mirrors [`Wikipedia::connect`]); the edit path always carries an
    /// explicit `assert=user` regardless.
    ///
    /// # Errors
    /// Client construction failure.
    pub async fn connect_with_api_url(
        api_url: &str,
        oauth2_token: Option<&str>,
    ) -> Result<Self, WikipediaError> {
        let mut builder = ApiClient::builder(api_url)
            .set_user_agent(crate::USER_AGENT)
            .set_maxlag(5)
            .set_concurrency(1);
        if let Some(token) = oauth2_token {
            builder = builder.set_oauth2_token(token).set_assert(Assert::User);
        }
        Ok(Self {
            api: builder.build().await?,
        })
    }

    /// The underlying mwapi client (for REST/parsoid and tests).
    #[must_use]
    pub fn raw(&self) -> &ApiClient {
        &self.api
    }

    /// Current revision id of a page (0-length sentinel: missing pages error).
    ///
    /// # Errors
    /// Network/API error, or the page does not exist.
    pub async fn current_revid(&self, title: &str) -> Result<u64, WikipediaError> {
        let resp: Value = self
            .api
            .get_value([
                ("action", "query"),
                ("prop", "revisions"),
                ("titles", title),
                ("rvprop", "ids"),
                ("rvslots", "main"),
            ])
            .await?;
        let pages = query_pages(&resp);
        if pages.is_empty() {
            return Err(WikipediaError::BadShape("query.pages".into()));
        }
        for page in pages {
            if page.get("missing").is_some() {
                return Err(WikipediaError::PageMissing(title.to_string()));
            }
            if let Some(revid) = page.pointer("/revisions/0/revid").and_then(Value::as_u64) {
                return Ok(revid);
            }
        }
        Err(WikipediaError::BadShape("no revid in response".into()))
    }

    /// Most recent revision id of `user`'s edits to `title` (None when the
    /// user never edited it) — the drift-review pin for
    /// `--review-since-user` (MVP-2 A.2.2).
    ///
    /// The Action API has no `uctitle` filter for `list=usercontribs`, so
    /// contributions are paged newest-first and filtered client-side
    /// (bounded: 10 pages × 500 = 5000 contributions).
    ///
    /// # Errors
    /// Network/API error or malformed response.
    pub async fn last_edit_revid(
        &self,
        user: &str,
        title: &str,
    ) -> Result<Option<u64>, WikipediaError> {
        let mut continue_from: Option<String> = None;
        for _ in 0..10 {
            let mut params: Vec<(&str, &str)> = vec![
                ("action", "query"),
                ("list", "usercontribs"),
                ("ucuser", user),
                ("ucprop", "ids|title"),
                ("uclimit", "500"),
            ];
            if let Some(cursor) = &continue_from {
                params.push(("uccontinue", cursor));
            }
            let resp: Value = self.api.get_value(params).await?;
            let contribs = resp
                .pointer("/query/usercontribs")
                .and_then(Value::as_array)
                .ok_or_else(|| WikipediaError::BadShape("query.usercontribs".into()))?;
            // Newest-first: the first matching entry is the last edit.
            if let Some(revid) = contribs
                .iter()
                .find(|c| c.get("title").and_then(Value::as_str) == Some(title))
                .and_then(|c| c.get("revid"))
                .and_then(Value::as_u64)
            {
                return Ok(Some(revid));
            }
            continue_from = resp
                .pointer("/continue/uccontinue")
                .and_then(Value::as_str)
                .map(str::to_string);
            if continue_from.is_none() {
                return Ok(None);
            }
        }
        Ok(None)
    }

    /// Fetch wikitext at a pinned revid (session base).
    ///
    /// # Errors
    /// Network/API error, page missing, or the revid is absent.
    pub async fn wikitext_at_revid(
        &self,
        title: &str,
        revid: u64,
    ) -> Result<String, WikipediaError> {
        let revid_str = revid.to_string();
        // NB: the API forbids revids+titles together; revids alone addresses
        // the revision directly.
        let resp: Value = self
            .api
            .get_value([
                ("action", "query"),
                ("prop", "revisions"),
                ("rvprop", "ids|content"),
                ("rvslots", "main"),
                ("revids", revid_str.as_str()),
            ])
            .await?;
        let _ = title;
        let pages = query_pages(&resp);
        if pages.is_empty() {
            return Err(WikipediaError::BadShape("query.pages".into()));
        }
        for page in pages {
            if let Some(content) = page
                .pointer("/revisions/0/slots/main/content")
                .and_then(Value::as_str)
            {
                return Ok(content.to_string());
            }
        }
        Err(WikipediaError::BadShape("no content in response".into()))
    }

    /// One logical edit with every publish-gate invariant:
    /// 1. summary validated (non-bare, disclosure appended);
    /// 2. base revid pre-check (current != base aborts, nothing written);
    /// 3. interactive confirmation via [`ConfirmSource`] (absent/declined
    ///    refuses);
    /// 4. `action=edit` with `baserevid` (client-wide `assert=user`,
    ///    `maxlag=5`, UA from [`crate::USER_AGENT`] are set at connect).
    ///
    /// `dry_run` performs 1–3 and returns without posting.
    ///
    /// # Errors
    /// Any invariant failure or API error.
    pub async fn edit(
        &self,
        req: EditRequest<'_>,
        confirm: &mut dyn ConfirmSource,
    ) -> Result<EditOutcome, WikipediaError> {
        let summary = summary_with_disclosure(req.summary)?;

        // Pre-check currency only when editing an existing page. A base of
        // 0 means "create" (userspace smoke targets): nothing to check, and
        // baserevid/nocreate are not sent.
        let current = if req.base_revid > 0 {
            let current = self.current_revid(req.title).await?;
            if current != req.base_revid {
                return Err(WikipediaError::EditConflict {
                    base: req.base_revid,
                    current,
                });
            }
            current
        } else {
            0
        };

        let review_line = req
            .review_artifact
            .map(|path| {
                format!("\n  review artifact: {path} (review the rendered diff there first)")
            })
            .unwrap_or_default();
        let prompt = format!(
            "Publish one edit to {}?\n  summary: {summary}\n  base revid: {}{review_line}\n\nThis confirmation is the FINAL gate: the anchor/linter gate re-ran before this \
             prompt, and the wiki history changes when you approve.",
            req.title, req.base_revid
        );
        if !confirm.confirm(&prompt).await {
            return Err(WikipediaError::Declined);
        }

        if req.dry_run {
            tracing::info!(title = req.title, "dry run: edit validated, not posted");
            return Ok(EditOutcome {
                new_revid: 0,
                diff_url: String::new(),
            });
        }

        // Fixed params for both create and update.
        let mut params: Vec<(&str, &str)> = vec![
            ("action", "edit"),
            ("title", req.title),
            ("text", req.wikitext),
            ("summary", summary.as_str()),
            // Explicit on the edit itself (the client-wide builder assert
            // covers reads; AC.7 wants it on the edit).
            ("assert", "user"),
            // Never minor. `minor` is a presence-flag (any value, "0"
            // included, marks the edit minor); `notminor` is the explicit
            // opposite and also overrides a "mark all edits minor"
            // account preference.
            ("notminor", "1"),
        ];
        let base_str;
        if req.base_revid > 0 {
            base_str = req.base_revid.to_string();
            params.push(("baserevid", base_str.as_str()));
            params.push(("nocreate", "1"));
        } else {
            // A create must not overwrite a page that appeared since the
            // session (or the log read) saw it missing.
            params.push(("createonly", "1"));
        }
        let resp: Value = self.api.post_with_token("csrf", params).await?;

        if let Some((code, info)) = error_code_and_info(&resp) {
            if code == "editconflict" || code == "articleexists" {
                return Err(WikipediaError::EditConflict {
                    base: req.base_revid,
                    current,
                });
            }
            return Err(WikipediaError::Api { code, info });
        }
        // A successful save of identical content is a null edit: the API
        // returns result=Success with NO newrevid. Report it as a no-change
        // outcome (the page already holds this text), not a shape error.
        let Some(new_revid) = resp.pointer("/edit/newrevid").and_then(Value::as_u64) else {
            if resp.pointer("/edit/result").and_then(Value::as_str) == Some("Success") {
                tracing::info!("null edit: page already contains this text (nochange)");
                return Ok(EditOutcome {
                    new_revid: req.base_revid,
                    diff_url: String::new(), // no revision was created
                });
            }
            return Err(WikipediaError::BadShape("edit.newrevid".into()));
        };
        Ok(EditOutcome {
            new_revid,
            diff_url: format!(
                "https://en.wikipedia.org/w/index.php?diff={new_revid}&oldid={}",
                req.base_revid
            ),
        })
    }

    /// Read a just-published revision back and compare what the wiki
    /// recorded with what the edit path intended (rule-enforcement plan
    /// item 1: verify outcomes, not only inputs). One query for the
    /// flags/parent/comment/user, one for the text. Returns one line per
    /// mismatch — a mismatch NEVER errors (the edit is already live); a
    /// failed read-back itself is reported as a line.
    pub async fn verify_revision(
        &self,
        revid: u64,
        expected: &RevisionExpectation<'_>,
    ) -> Vec<String> {
        let revid_str = revid.to_string();
        let resp: Value = match self
            .api
            .get_value([
                ("action", "query"),
                ("prop", "revisions"),
                ("revids", revid_str.as_str()),
                ("rvprop", "ids|flags|comment|user"),
                ("rvslots", "main"),
            ])
            .await
        {
            Ok(v) => v,
            Err(e) => return vec![format!("could not verify the saved revision: {e}")],
        };
        let Some(rev) = query_pages(&resp)
            .into_iter()
            .next()
            .and_then(|page| page.get("revisions")?.as_array()?.first().cloned())
        else {
            return vec!["could not verify the saved revision: no such revid".into()];
        };
        let mut lines = Vec::new();
        // `minor` is a presence flag on the saved revision; no edit this
        // tool makes may carry it (the edit posts notminor=1).
        if rev
            .get("flags")
            .and_then(Value::as_array)
            .is_some_and(|flags| flags.iter().any(|f| f.as_str() == Some("minor")))
        {
            lines.push(
                "saved as a MINOR edit — edits post notminor=1, so this must not happen".into(),
            );
        }
        // The parent must be the base the edit was pinned to (skipped for a
        // page creation: there is no parent).
        if expected.base_revid > 0 {
            let parent = rev.get("parentid").and_then(Value::as_u64).unwrap_or(0);
            if parent != expected.base_revid {
                lines.push(format!(
                    "saved revision's parent is {parent}, not the pinned base {} — the base \
                     moved under the edit",
                    expected.base_revid
                ));
            }
        }
        let comment = rev.get("comment").and_then(Value::as_str).unwrap_or("");
        // NB: the suffix rides wrapped in parens ("… (SUFFIX)"), and a
        // summary that already carried it passes through verbatim — so
        // the invariant is that the saved comment CONTAINS it, not that
        // it ends with the bare suffix.
        if !comment.contains(crate::DISCLOSURE_SUFFIX) {
            lines.push(format!(
                "saved summary does not carry the disclosure suffix: {comment:?}"
            ));
        }
        if let Some(operator) = expected.operator {
            let user = rev.get("user").and_then(Value::as_str).unwrap_or("");
            if user != operator {
                lines.push(format!(
                    "saved by {user:?}, not the operator account {operator:?}"
                ));
            }
        }
        // Text: advisory compare — MediaWiki pre-save transforms may
        // legitimately alter the text, so a difference is reported, not
        // claimed as a violation. (Title unused: the revid addresses the
        // revision directly.)
        match self.wikitext_at_revid("", revid).await {
            Ok(saved) => {
                if saved.trim_end() != expected.text.trim_end() {
                    lines.push(
                        "saved text differs from the proposed text (pre-save transforms are \
                         possible — inspect the diff)"
                            .into(),
                    );
                }
            }
            Err(e) => lines.push(format!("could not verify the saved revision text: {e}")),
        }
        lines
    }

    /// Append one entry to the disclosure page's session log, idempotently:
    /// if `marker` (the session id line) already exists on the page, do
    /// nothing and return `false`.
    ///
    /// # Errors
    /// API errors from fetch or edit.
    pub async fn append_disclosure_log(
        &self,
        page: &str,
        entry: &str,
        marker: &str,
        confirm: &mut dyn ConfirmSource,
    ) -> Result<Option<EditOutcome>, WikipediaError> {
        let current = self.current_revid(page).await;
        let existing = match current {
            Ok(revid) => self.wikitext_at_revid(page, revid).await?,
            Err(WikipediaError::PageMissing(_)) => String::new(),
            Err(other) => return Err(other),
        };
        if existing.contains(marker) {
            tracing::info!("disclosure log already contains {marker}; skipping");
            return Ok(None);
        }
        // The marker rides IN the entry as an invisible comment so
        // idempotency survives hand-edited log pages (re-runs detect the
        // on-page marker, and tool-appended entries carry it too).
        let marked_entry = format!("<!-- {marker} -->\n{entry}");
        let updated = format!("{existing}\n{marked_entry}\n");
        let revid = match current {
            Ok(revid) => revid,
            Err(WikipediaError::PageMissing(_)) => 0,
            Err(other) => return Err(other),
        };
        let outcome = self
            .edit(
                EditRequest {
                    title: page,
                    base_revid: revid,
                    wikitext: &updated,
                    summary: "Append wikiactive session log entry",
                    review_artifact: None,
                    dry_run: false,
                },
                confirm,
            )
            .await?;
        Ok(Some(outcome))
    }
    /// Upsert one session entry on the disclosure log page: if the page
    /// already contains `<!-- {marker} -->`, the entry block that follows it
    /// is REPLACED with `entry` (so a session's entry grows as its edits
    /// accumulate); otherwise the marked entry is appended. Returns the
    /// edit outcome, or `None` when the page already holds exactly this
    /// entry text (no-op).
    ///
    /// # Errors
    /// API errors from fetch or edit.
    pub async fn upsert_disclosure_log(
        &self,
        page: &str,
        marker: &str,
        entry: &str,
        confirm: &mut dyn ConfirmSource,
    ) -> Result<Option<EditOutcome>, WikipediaError> {
        let marker_comment = format!("<!-- {marker} -->");
        let block = format!("{marker_comment}\n{entry}");
        let current = self.current_revid(page).await;
        let existing = match current {
            Ok(revid) => self.wikitext_at_revid(page, revid).await?,
            Err(WikipediaError::PageMissing(_)) => String::new(),
            Err(other) => return Err(other),
        };
        let updated = if let Some(start) = existing.find(&marker_comment) {
            if existing.contains(&block) {
                return Ok(None); // already up to date
            }
            // Replace from the marker to the end of this entry block: the
            // entry runs to the next marker comment or the next blank line
            // (whichever comes first) — text a person added below the last
            // entry is not part of it.
            let after = &existing[start..];
            let end = [after.find("\n<!-- "), after.find("\n\n")]
                .into_iter()
                .flatten()
                .min()
                .map_or(existing.len(), |e| start + e);
            format!("{}{}{}", &existing[..start], block, &existing[end..])
        } else {
            format!("{existing}\n{block}\n")
        };
        if updated == existing {
            return Ok(None);
        }
        let revid = match current {
            Ok(revid) => revid,
            Err(WikipediaError::PageMissing(_)) => 0,
            Err(other) => return Err(other),
        };
        let outcome = self
            .edit(
                EditRequest {
                    title: page,
                    base_revid: revid,
                    wikitext: &updated,
                    summary: "Update wikiactive session log",
                    review_artifact: None,
                    dry_run: false,
                },
                confirm,
            )
            .await?;
        Ok(Some(outcome))
    }
}

/// Pages from a `query.pages` value, accepting both formatversion shapes
/// (v1: object keyed by pageid; v2: array — mwapi requests formatversion=2).
fn query_pages(resp: &Value) -> Vec<&Value> {
    match resp.pointer("/query/pages") {
        Some(Value::Object(pages)) => pages.values().collect(),
        Some(Value::Array(pages)) => pages.iter().collect(),
        _ => Vec::new(),
    }
}

/// Extract `(errorcode, errorinfo)` from an API response if present.
fn error_code_and_info(resp: &Value) -> Option<(String, String)> {
    let err = resp.get("error")?;
    Some((
        err.get("code")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        err.get("info")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{DenyConfirm, EditRequest, WikipediaError, summary_with_disclosure};

    #[test]
    fn summary_appends_disclosure_suffix() {
        let s = summary_with_disclosure("Fix scope of sales figure").unwrap();
        assert!(s.starts_with("Fix scope of sales figure"));
        assert!(s.ends_with(')'));
        assert!(s.contains(super::DISCLOSURE_SUFFIX));
    }

    #[test]
    fn summary_suffix_is_idempotent() {
        let once = summary_with_disclosure("Fix scope").unwrap();
        let twice = summary_with_disclosure(&once).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn bare_summary_refused() {
        assert!(matches!(
            summary_with_disclosure("   "),
            Err(WikipediaError::BareSummary)
        ));
    }

    #[tokio::test]
    async fn declined_confirmation_is_an_error_not_a_write() {
        // The confirm gate refusing is a Declined error; exercised end-to-end
        // against a mock server in tests/publish.rs.
        use super::ConfirmSource as _;
        let mut deny = DenyConfirm;
        assert!(!deny.confirm("anything").await);
    }

    #[test]
    fn edit_request_shape() {
        let req = EditRequest {
            title: "Test page",
            base_revid: 123,
            wikitext: "new text",
            summary: "s",
            review_artifact: None,
            dry_run: true,
        };
        assert_eq!(req.base_revid, 123);
        assert!(req.dry_run);
    }
}
