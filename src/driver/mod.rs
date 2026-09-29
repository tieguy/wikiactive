//! The Phase-B model driver: a pipeline, not a free-form agent. The model
//! is called only at the three judgment points (findings authoring,
//! proposal drafting, comment resolution); loop control, gates, the
//! ledger, and the human publish gate stay in deterministic Rust.
//!
//! - [`model`] — the z.ai (GLM) chat client (OpenAI-compatible transport).
//! - [`prompts`] — versioned prompt templates, checksum-pinned (AC.9).

pub mod model;
pub mod prompts;
pub mod steps;
