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
pub trait ConfirmSource {
    /// Show `prompt`, return the human's decision.
    fn confirm(&mut self, prompt: &str) -> bool;
}

/// Reads confirmation from `/dev/tty` so piped stdin/stdout cannot fake it.
pub struct TtyConfirm;

impl ConfirmSource for TtyConfirm {
    fn confirm(&mut self, prompt: &str) -> bool {
        use std::io::BufRead;
        use std::io::Write;
        let Ok(tty) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
        else {
            eprintln!("no controlling terminal; publish requires one");
            return false;
        };
        let mut reader = std::io::BufReader::new(tty);
        let mut handle = reader.get_ref().try_clone().expect("tty clone");
        let _ = writeln!(handle, "{prompt} [type yes to publish] ");
        let _ = handle.flush();
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return false;
        }
        line.trim().eq_ignore_ascii_case("yes")
    }
}

/// A confirmation source that always answers no (tests; absent tty).
#[derive(Default)]
pub struct DenyConfirm;

impl ConfirmSource for DenyConfirm {
    fn confirm(&mut self, _prompt: &str) -> bool {
        false
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
    /// Validate only: run every pre-flight step, do not post the edit.
    pub dry_run: bool,
}

/// Result of a successful (non-dry-run) edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOutcome {
    pub new_revid: u64,
    pub diff_url: String,
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
        let oauth2_token = std::env::var(OAUTH2_TOKEN_ENV).ok();
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
        let resp: Value = self
            .api
            .get_value([
                ("action", "query"),
                ("prop", "revisions"),
                ("titles", title),
                ("rvprop", "ids|content"),
                ("rvslots", "main"),
                ("revids", revid_str.as_str()),
            ])
            .await?;
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

        let prompt = format!(
            "Publish one edit to {}?\n  summary: {summary}\n  base revid: {}\nThis is the \
             only gate before the wiki history changes.",
            req.title, req.base_revid
        );
        if !confirm.confirm(&prompt) {
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
            ("minor", "0"),
        ];
        let base_str;
        if req.base_revid > 0 {
            base_str = req.base_revid.to_string();
            params.push(("baserevid", base_str.as_str()));
            params.push(("nocreate", "1"));
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
        let new_revid = resp
            .pointer("/edit/newrevid")
            .and_then(Value::as_u64)
            .ok_or_else(|| WikipediaError::BadShape("edit.newrevid".into()))?;
        Ok(EditOutcome {
            new_revid,
            diff_url: format!(
                "https://en.wikipedia.org/w/index.php?diff={new_revid}&oldid={}",
                req.base_revid
            ),
        })
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
    ) -> Result<bool, WikipediaError> {
        let current = self.current_revid(page).await;
        let existing = match current {
            Ok(revid) => self.wikitext_at_revid(page, revid).await?,
            Err(WikipediaError::PageMissing(_)) => String::new(),
            Err(other) => return Err(other),
        };
        if existing.contains(marker) {
            tracing::info!("disclosure log already contains {marker}; skipping");
            return Ok(false);
        }
        let updated = format!("{existing}\n{entry}\n");
        let revid = match current {
            Ok(revid) => revid,
            Err(WikipediaError::PageMissing(_)) => 0,
            Err(other) => return Err(other),
        };
        self.edit(
            EditRequest {
                title: page,
                base_revid: revid,
                wikitext: &updated,
                summary: "Append wikiactive session log entry",
                dry_run: false,
            },
            confirm,
        )
        .await?;
        Ok(true)
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

    #[test]
    fn declined_confirmation_is_an_error_not_a_write() {
        // The confirm gate refusing is a Declined error; exercised end-to-end
        // against a mock server in tests/publish.rs.
        use super::ConfirmSource as _;
        let mut deny = DenyConfirm;
        assert!(!deny.confirm("anything"));
    }

    #[test]
    fn edit_request_shape() {
        let req = EditRequest {
            title: "Test page",
            base_revid: 123,
            wikitext: "new text",
            summary: "s",
            dry_run: true,
        };
        assert_eq!(req.base_revid, 123);
        assert!(req.dry_run);
    }
}
