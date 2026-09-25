//! wikiloop — structured Wikipedia improvement loop.
//!
//! One logical edit at a time, every content change grounded in a quoted
//! source from the session source ledger, reviewed by the operator as a
//! visual diff, published only on explicit human confirmation.
//!
//! Modules land incrementally per the MVP-1 plan
//! (`docs/design-plans/2026-09-24-mvp1-structured-loop.md`):
//! `ledger`, `checks` (`quote_anchor`/`paraphrase`/`linter`/`gate`),
//! `render`, `anchors`, `lavish`, `wikipedia`, `rules`, `session`, `cli`.

pub mod checks;
pub mod ledger;
pub mod session;
pub mod wikipedia;

/// User-Agent used for every outbound HTTP request to Wikimedia (and, with
/// the same identity, to source/archive/Earwig hosts). Product-internalized
/// per docs/api-etiquette.md: it must identify the operator.
pub const USER_AGENT: &str = "wikiactive/0.1 (en.wikipedia User:LuisVilla; luis@lu.is)";

/// Disclosure suffix appended to every published edit summary. Mechanically
/// enforced by the publish path; configured in rules/house-rules.toml.
pub const DISCLOSURE_SUFFIX: &str = "LLM-Disclosure: U:LuisVilla/wikiactive";
