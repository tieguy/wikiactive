//! Checks: the deterministic verification layer.
//!
//! - [`quote_anchor`]: verbatim-with-artifact-folding quote locator
//!   (ported from SP42 `sp42-citation` with provenance)
//! - [`paraphrase`]: shingle-similarity gate (CLOP / no-support)
//! - [`linter`]: Tier-3 mechanical wikitext rules (whole-page + added-lines)
//! - [`gate`]: the mandatory render/publish pre-flight (AC.11)

pub mod gate;
pub mod linter;
pub mod paraphrase;
pub mod quote_anchor;
pub mod summary_rules;
