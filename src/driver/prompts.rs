//! Versioned prompt templates for the driver's three judgment points
//! (`prompts/*.md`), plus the tiny named-slot renderer the pipeline steps
//! use to fill them.
//!
//! The templates live in the repo — not in code — so the disclosure page's
//! "exact code including model prompts" promise is mechanical: a prompt IS
//! a file, checksum-pinned by test (AC.9). Changing a prompt is a
//! deliberate act: edit the file, then update the pin in the test with
//! intent.

use std::path::Path;

/// Prompt file names under `prompts/`.
pub const AUTHOR_FINDINGS: &str = "author-findings.md";
pub const PROPOSE: &str = "propose.md";
pub const RESOLVE: &str = "resolve.md";

/// Errors loading or rendering a prompt template.
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    /// The template file is missing or unreadable.
    #[error("prompt template {name}: {source}")]
    Missing {
        /// Template file name.
        name: String,
        /// Underlying IO error.
        source: std::io::Error,
    },
}

/// Load a prompt template by file name from `prompts/`.
///
/// # Errors
/// Unreadable file (see [`PromptError::Missing`]).
pub fn load(name: &str) -> Result<String, PromptError> {
    std::fs::read_to_string(Path::new("prompts").join(name)).map_err(|source| {
        PromptError::Missing {
            name: name.to_string(),
            source,
        }
    })
}

/// Render a template by replacing `{{slot}}` markers with values.
/// Unknown markers are left verbatim (a pipeline bug should be visible in
/// the sent prompt, not silently dropped).
#[must_use]
pub fn render(template: &str, slots: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in slots {
        let marker = format!("{{{{{key}}}}}");
        out = out.replace(&marker, value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{AUTHOR_FINDINGS, PROPOSE, RESOLVE, load, render};
    use sha2::{Digest, Sha256};

    fn sha256_hex(text: &str) -> String {
        use std::fmt::Write as _;
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let mut out = String::with_capacity(64);
        for byte in hasher.finalize() {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    /// AC.9 — prompt checksum pins: drift fails the test. To change a
    /// prompt deliberately: edit the file, recompute, update the pin, and
    /// say why in the commit message.
    #[test]
    fn prompt_checksums_are_pinned() {
        for (name, pinned) in [
            (
                AUTHOR_FINDINGS,
                // rule-enforcement item 3: gained the {{guidance}} rules
                // section (tier-1 verbatim + this loop's cards).
                "c660c41cf0cad8c2439b4f6cb8bb2f88c8596a769bcb3b4f2ee751feb3e7f83d",
            ),
            (
                PROPOSE,
                // rule-enforcement item 3: gained the {{guidance}} rules
                // section.
                "793cfc487c4909a8577140606bcb429e55e8e8acbc9776151d6d407d85da2db0",
            ),
            (
                RESOLVE,
                // rule-enforcement item 3: gained the {{guidance}} rules
                // section.
                "0126cd0f87bb21015b8a349d6c06f93bf028f2f2f0161a7388ae32c42f9a2439",
            ),
        ] {
            let text = load(name).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(
                sha256_hex(&text),
                pinned,
                "{name} drifted; update the pin deliberately"
            );
        }
    }

    #[test]
    fn render_replaces_known_slots_only() {
        let out = render(
            "loop {{entry_loop}} keep {{unknown}}",
            &[("entry_loop", "2")],
        );
        assert_eq!(out, "loop 2 keep {{unknown}}");
    }
}
