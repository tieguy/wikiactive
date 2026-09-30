//! z.ai (GLM) chat client — the Phase-B model driver's transport.
//!
//! One client for the three judgment points (findings authoring, proposal
//! drafting, comment resolution): OpenAI-compatible, non-streaming chat
//! completions over the shared etiquette client (reqwest/rustls,
//! identifying UA, guarded redirects), bearer auth from the environment.
//!
//! B.0 contract facts (`fixtures/zai/README.md`): the operator's key is a
//! Coding-Plan key whose quota lives on a dedicated base URL — the default
//! below is the STANDARD endpoint; `ZAI_BASE_URL` is the switch. A 429
//! carrying error code 1113 ("no resource package") is terminal and never
//! retried. Observed wire shape: `choices[0].message` carries `content`
//! (the answer — raw or json-code-fenced JSON) plus a separate
//! `reasoning_content` field (chain-of-thought, never parsed).

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default base: the standard v4 endpoint. Coding-Plan keys set
/// `ZAI_BASE_URL=https://api.z.ai/api/coding/paas/v4` (B.0).
pub const ZAI_DEFAULT_BASE: &str = "https://api.z.ai/api/paas/v4";

/// Env var holding the API key (never read into any stored artifact).
pub const ZAI_API_KEY_ENV: &str = "ZAI_API_KEY";

/// Env var switching the base URL (the Coding-Plan endpoint switch).
pub const ZAI_BASE_URL_ENV: &str = "ZAI_BASE_URL";

/// z.ai terminal error code: authenticated, but no balance or resource
/// package on this endpoint (a Coding-Plan key hitting the standard base
/// returns exactly this). Not retryable.
pub const ZAI_ERR_NO_PACKAGE: &str = "1113";

/// One chat message. The driver only ever needs system and user roles.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    /// `system` or `user`.
    pub role: String,
    /// Message body.
    pub content: String,
}

impl ChatMessage {
    /// A system message (the rendered prompt template).
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    /// A user message (the context bundle).
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    /// An assistant message (a prior model turn — the corrective retry
    /// includes the rejected output so the model can actually see it).
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// A parsed completion: the answer content plus what the driver logs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatResponse {
    /// The model's answer (`choices[0].message.content`).
    pub content: String,
    /// Chain-of-thought (`reasoning_content`), kept for the session log.
    pub reasoning: Option<String>,
    /// `usage.total_tokens` when present.
    pub total_tokens: Option<u64>,
}

/// Client errors. Transport-level retrying is internal (bounded, honoring
/// `Retry-After`); what reaches the caller is final.
#[derive(Debug, thiserror::Error)]
pub enum ZaiError {
    /// `ZAI_API_KEY` is not set in the environment.
    #[error("{ZAI_API_KEY_ENV} is not set — export it (README: Phase-B env vars)")]
    MissingApiKey,
    /// Network/transport failure after all attempts.
    #[error("z.ai transport: {0}")]
    Transport(String),
    /// Non-retryable HTTP status with the response body for diagnosis.
    #[error("z.ai http {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Response body (truncated by the caller).
        body: String,
    },
    /// 429 with terminal code 1113: no balance/resource package on this
    /// endpoint. Retrying cannot help; for a Coding-Plan key, set
    /// `ZAI_BASE_URL`.
    #[error(
        "z.ai rejected the key on this endpoint (code 1113, no resource package); a Coding-Plan key needs ZAI_BASE_URL=https://api.z.ai/api/coding/paas/v4 — {message}"
    )]
    NoPackage {
        /// The API's message.
        message: String,
    },
    /// Rate-limited (429 without 1113) after exhausting retries.
    #[error("z.ai rate-limited after {} attempts", attempts)]
    RateLimited {
        /// Attempts made.
        attempts: u8,
    },
    /// Response body was not the expected chat-completions shape.
    #[error("z.ai response malformed: {0}")]
    Malformed(String),
}

/// The z.ai chat client.
pub struct ZaiClient {
    http: reqwest::Client,
    base: String,
    model: String,
    api_key: String,
    /// Sleep between retry attempts (default: MW-etiquette-style bounded
    /// exponential: 1s, 2s, 4s). Injectable so contract tests run fast.
    retry_delays: Vec<Duration>,
}

/// Per-request timeout for a chat completion.
const CHAT_TIMEOUT: Duration = Duration::from_mins(5);

impl ZaiClient {
    /// Build from the environment + fork configuration: `ZAI_API_KEY`
    /// required (env only — keys never live in config), base URL resolved
    /// as `ZAI_BASE_URL` (one-off override) → `rules/house-rules.toml`
    /// `[zai] base_url` (the fork's configured endpoint — no shell ritual)
    /// → the standard v4 default. The model id comes from the caller (the
    /// corpus's `[zai] model` / `disclosure.drafting_model`).
    ///
    /// # Errors
    /// [`ZaiError::MissingApiKey`] or client construction failure.
    pub fn from_env(model: &str) -> Result<Self, ZaiError> {
        let api_key = std::env::var(ZAI_API_KEY_ENV)
            .ok()
            .filter(|k| !k.trim().is_empty())
            .ok_or(ZaiError::MissingApiKey)?;
        let base = std::env::var(ZAI_BASE_URL_ENV)
            .ok()
            .filter(|b| !b.trim().is_empty())
            .or_else(|| {
                crate::rules::RulesCorpus::load(std::path::Path::new("rules"))
                    .ok()
                    .and_then(|c| c.house_rules.zai.map(|z| z.base_url))
            })
            .unwrap_or_else(|| ZAI_DEFAULT_BASE.to_string());
        Ok(Self::with_base(&base, model, &api_key))
    }

    /// Build against an explicit base URL (tests and the B.0 spike
    /// replays; production uses [`ZaiClient::from_env`]).
    ///
    /// # Panics
    /// If the shared etiquette client cannot be constructed (same
    /// precedent as the Earwig/SPN clients).
    #[must_use]
    pub fn with_base(base: &str, model: &str, api_key: &str) -> Self {
        let http = crate::ledger::net::http_client().expect("reqwest client builds");
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
            retry_delays: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ],
        }
    }

    /// Override the retry schedule (contract tests use zero-length
    /// delays).
    #[must_use]
    pub fn with_retry_delays(mut self, delays: Vec<Duration>) -> Self {
        self.retry_delays = delays;
        self
    }

    /// The chat-completions endpoint URL.
    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base)
    }

    /// One non-streaming chat completion. Retries rate-limit (429) and
    /// 5xx responses per the injected schedule (honoring `Retry-After`
    /// when larger); 429-with-1113 returns [`ZaiError::NoPackage`] without
    /// retrying; other transport errors surface immediately (the MW fetch
    /// client's bounded backoff is the precedent for the scope here).
    ///
    /// # Errors
    /// See [`ZaiError`]; the response is parsed with
    /// [`parse_chat_response`].
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        temperature: f64,
    ) -> Result<ChatResponse, ZaiError> {
        let mut attempt: u8 = 1;
        loop {
            let resp = self
                .http
                .post(self.endpoint())
                .bearer_auth(&self.api_key)
                // A non-streaming completion over a whole article plus its
                // sources outlasts the shared client's source-fetch timeout.
                .timeout(CHAT_TIMEOUT)
                .json(&serde_json::json!({
                    "model": self.model,
                    "messages": messages,
                    "temperature": temperature,
                }))
                .send()
                .await
                .map_err(|e| ZaiError::Transport(e.to_string()))?;

            let status = resp.status();
            if status.is_success() {
                let body = resp
                    .text()
                    .await
                    .map_err(|e| ZaiError::Transport(e.to_string()))?;
                return parse_chat_response(&body);
            }

            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            let body = resp.text().await.unwrap_or_default();

            // Terminal 429: authenticated but no package on this endpoint.
            if status.as_u16() == 429 && error_code(&body).as_deref() == Some(ZAI_ERR_NO_PACKAGE) {
                let message = error_message(&body).unwrap_or_default();
                return Err(ZaiError::NoPackage { message });
            }

            let retryable = status.as_u16() == 429 || status.is_server_error();
            if retryable {
                if let Some(delay) = self.retry_delays.get(usize::from(attempt - 1)) {
                    let delay = retry_after
                        .map_or(*delay, |ra| ra.max(*delay))
                        .min(Duration::from_secs(30));
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    continue;
                }
                // Out of retries: a 429 is rate limiting; a 5xx is an
                // upstream failure and reports as one (status + body).
                if status.as_u16() == 429 {
                    return Err(ZaiError::RateLimited { attempts: attempt });
                }
            }
            return Err(ZaiError::Http {
                status: status.as_u16(),
                body: truncate(&body, 500),
            });
        }
    }
}

/// Parse a chat-completions response body into a [`ChatResponse`].
///
/// # Errors
/// [`ZaiError::Malformed`] when the expected `choices[0].message` shape
/// is absent.
pub fn parse_chat_response(body: &str) -> Result<ChatResponse, ZaiError> {
    #[derive(Deserialize)]
    struct Wire {
        #[serde(default)]
        choices: Vec<Choice>,
        #[serde(default)]
        usage: Option<Usage>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Message,
    }
    #[derive(Deserialize)]
    struct Message {
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        reasoning_content: Option<String>,
    }
    #[derive(Deserialize)]
    struct Usage {
        #[serde(default)]
        total_tokens: Option<u64>,
    }
    let wire: Wire =
        serde_json::from_str(body).map_err(|e| ZaiError::Malformed(format!("not JSON: {e}")))?;
    let choice = wire
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ZaiError::Malformed("no choices[0]".into()))?;
    Ok(ChatResponse {
        content: choice.message.content.unwrap_or_default(),
        reasoning: choice.message.reasoning_content,
        total_tokens: wire.usage.and_then(|u| u.total_tokens),
    })
}

/// Strip markdown code fencing from model output: returns the first
/// fenced block when one appears ANYWHERE in the content (observed live:
/// fenced and raw JSON, sometimes after a one-line preamble), else the
/// trimmed content.
#[must_use]
pub fn strip_code_fence(content: &str) -> &str {
    let trimmed = content.trim();
    let Some(open) = trimmed.find("```") else {
        return trimmed;
    };
    // Skip the fence and its info string (e.g. "json") up to the newline.
    let after_open = &trimmed[open + 3..];
    let Some(newline) = after_open.find('\n') else {
        // Degenerate: a fence with no content line.
        return "";
    };
    // The block ends at the first fence that starts a line (a second
    // fenced block may follow it); failing that, at the last fence.
    let body = &after_open[newline + 1..];
    let close = if body.starts_with("```") {
        Some(0)
    } else {
        body.find("\n```").or_else(|| body.rfind("```"))
    };
    close.map_or(body, |c| &body[..c]).trim()
}

fn error_code(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .pointer("/error/code")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn error_message(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .pointer("/error/message")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &s[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatResponse, ZaiError, parse_chat_response, strip_code_fence};

    fn fixture(path: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/fixtures/zai/{path}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_else(|e| panic!("fixture {path}: {e}"))
    }

    /// B.0 wire pin: the findings capture (fenced JSON content, separate
    /// reasoning field, usage totals).
    #[test]
    fn parses_live_findings_fixture() {
        let resp = parse_chat_response(&fixture("findings.response.json")).expect("parses");
        assert!(
            resp.content.starts_with("```json"),
            "fenced content observed live"
        );
        assert!(resp.reasoning.is_some(), "reasoning_content observed live");
        assert_eq!(resp.total_tokens, Some(3071));
        let stripped = strip_code_fence(&resp.content);
        let parsed: serde_json::Value =
            serde_json::from_str(stripped).expect("fence-stripped content is JSON");
        assert!(
            parsed.as_array().is_some(),
            "findings output is a JSON array"
        );
    }

    /// B.0 wire pin: the propose capture (raw JSON content).
    #[test]
    fn parses_live_propose_fixture() {
        let resp = parse_chat_response(&fixture("propose.response.json")).expect("parses");
        let parsed: serde_json::Value =
            serde_json::from_str(strip_code_fence(&resp.content)).expect("raw JSON content");
        assert!(parsed.get("proposed_wikitext_block").is_some());
    }

    /// B.0 wire pin: the standard-endpoint 1113 rejection shape.
    #[test]
    fn recognizes_no_package_body() {
        let body = fixture("standard-endpoint.response.json");
        assert_eq!(
            super::error_code(&body).as_deref(),
            Some(super::ZAI_ERR_NO_PACKAGE)
        );
        assert!(
            super::error_message(&body)
                .unwrap()
                .contains("resource package")
        );
    }

    #[test]
    fn malformed_bodies_error_loudly() {
        assert!(matches!(
            parse_chat_response("{}"),
            Err(ZaiError::Malformed(_))
        ));
        assert!(matches!(
            parse_chat_response("<html>gateway</html>"),
            Err(ZaiError::Malformed(_))
        ));
    }

    #[test]
    fn fence_stripping_variants() {
        assert_eq!(strip_code_fence("```json\n[1]\n```"), "[1]");
        assert_eq!(strip_code_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("  {\"raw\":true}  "), "{\"raw\":true}");
        assert_eq!(
            strip_code_fence("```"),
            "",
            "degenerate fence is empty content"
        );
        // Preamble before the fence (review finding: offset-0-only fences
        // used to defeat extraction).
        assert_eq!(
            strip_code_fence("Here is the JSON you asked for:\n```json\n[2]\n```"),
            "[2]"
        );
        let _ = ChatResponse::default();
    }
}
