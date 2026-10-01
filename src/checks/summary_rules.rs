//! Edit-summary rule resolution (loop-mechanization Phase 5): every
//! shortcut-shaped token in an accepted edit summary (`MOS:…`, `WP:…`,
//! `H:…`, …) must resolve against the canonical policy snapshots
//! committed under `rules/canonical/`. A summary citing a shortcut the
//! corpus does not know is refused wherever a summary gains the power to
//! produce or send an edit (the audit's revisions-registry summary and
//! publish). The index is built offline from the committed snapshots —
//! no network at build or run.
//!
//! House rules carry no canonical shortcuts: a house-rule edit
//! described with an invented shortcut is refused as unresolvable (the
//! honest move is to describe the edit plainly). Whether a RESOLVABLE
//! shortcut genuinely covers the edit is judgment, not mechanism.

use std::collections::HashMap;
use std::path::Path;

/// Offline map of Wikipedia policy shortcut tokens (uppercased) to the
/// canonical page (snapshot file stem) they resolve to.
#[derive(Debug, Clone, Default)]
pub struct ShortcutIndex {
    map: HashMap<String, String>,
}

fn shortcut_template() -> regex::Regex {
    regex::Regex::new(r"(?i)\{\{\s*shortcut\s*((?:\|[^{}]*?)*)\}\}")
        .expect("static shortcut-template regex")
}

fn shortcut_param() -> regex::Regex {
    regex::Regex::new(r"(?i)\bshortcut\d*\s*=\s*([A-Za-z]+:[A-Za-z0-9][A-Za-z0-9 _-]*)")
        .expect("static shortcut-param regex")
}

fn shortcut_token() -> regex::Regex {
    regex::Regex::new(r"\b((?:WP|MOS|H|WT|CAT):[A-Za-z0-9][A-Za-z0-9_-]*)")
        .expect("static shortcut-token regex")
}

impl ShortcutIndex {
    /// Build the index from a `rules/canonical/` directory's committed
    /// wikitext snapshots (offline; the corpus is in-repo).
    ///
    /// # Errors
    /// The directory is unreadable or empty (a corpus that cannot back
    /// the check is a configuration failure, not a pass).
    pub fn build(canonical_dir: &Path) -> Result<Self, String> {
        let mut map = HashMap::new();
        let entries = std::fs::read_dir(canonical_dir)
            .map_err(|e| format!("read {}: {e}", canonical_dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "wikitext") {
                continue;
            }
            let page = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // {{shortcut|A|B|C}} templates (any case; aliases fan out to
            // the same page; template params with `=` are skipped).
            for caps in shortcut_template().captures_iter(&text) {
                for alias in caps.get(1).map_or("", |m| m.as_str()).split('|') {
                    let alias = alias.trim();
                    if alias.is_empty() || alias.contains('=') {
                        continue;
                    }
                    map.entry(alias.to_uppercase())
                        .or_insert_with(|| page.clone());
                }
            }
            // shortcut=WP:X / shortcut1=MOS:Y infobox params.
            for caps in shortcut_param().captures_iter(&text) {
                if let Some(token) = caps.get(1) {
                    map.entry(token.as_str().to_uppercase())
                        .or_insert_with(|| page.clone());
                }
            }
        }
        if map.is_empty() {
            return Err(format!(
                "no shortcut templates found under {} — the canonical corpus is missing",
                canonical_dir.display()
            ));
        }
        Ok(Self { map })
    }

    /// Build from the conventional repo location (`rules/canonical`).
    ///
    /// # Errors
    /// See [`Self::build`].
    pub fn load() -> Result<Self, String> {
        Self::build(Path::new("rules").join("canonical").as_path())
    }

    /// Does this token (any case) resolve?
    #[must_use]
    pub fn resolves(&self, token: &str) -> bool {
        self.map.contains_key(&token.to_uppercase())
    }

    /// Number of known tokens (diagnostics/tests).
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the index is empty (diagnostics/tests).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// One line per shortcut-shaped token in `summary` that does not resolve
/// against `index`; an empty vec passes. Tokens are matched
/// case-insensitively (`mos:var` == `MOS:VAR`).
#[must_use]
pub fn summary_rule_check(summary: &str, index: &ShortcutIndex) -> Vec<String> {
    let mut missing = Vec::new();
    for caps in shortcut_token().captures_iter(summary) {
        let token = caps.get(1).map_or("", |m| m.as_str());
        if !index.resolves(token) {
            missing.push(format!(
                "{token} does not resolve against the canonical policy snapshots \
                 (rules/canonical/) — cite a real shortcut or describe the edit plainly"
            ));
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::{ShortcutIndex, summary_rule_check};

    /// loopmech.AC5.5 — the index builds offline from the committed
    /// snapshots and knows the corpus's own shortcuts.
    #[test]
    fn index_builds_offline_from_the_committed_snapshots() {
        let index = ShortcutIndex::load().expect("repo corpus builds");
        assert!(index.len() > 50, "the corpus is rich: {}", index.len());
        // Spot checks across prefixes and case.
        for token in ["WP:CITESHORT", "MOS:VAR", "wp:paraphrase", "mos:inthelead"] {
            assert!(index.resolves(token), "{token} resolves");
        }
    }

    /// loopmech.AC5.1 / AC5.2 — a resolvable token passes; an
    /// unresolvable one is named; token-free summaries pass unchanged.
    #[test]
    fn check_names_unresolvable_tokens_only() {
        let index = ShortcutIndex::load().unwrap();
        assert!(summary_rule_check("fixed ref-name formats per MOS:VAR", &index).is_empty());
        assert!(
            summary_rule_check("Fixed the marriage date", &index).is_empty(),
            "no tokens, no check"
        );
        let missing = summary_rule_check("trim per MOS:NOTREAL and WP:ALSONOTREAL", &index);
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing[0].contains("MOS:NOTREAL"), "{missing:?}");
        assert!(missing[1].contains("WP:ALSONOTREAL"), "{missing:?}");
        // Case-normalized matching: lowercase resolvable passes.
        assert!(summary_rule_check("tightened per mos:var", &index).is_empty());
    }

    /// loopmech.AC5.4 — house rules have no canonical shortcuts: a
    /// made-up shortcut on any edit is unresolvable; the same edit
    /// described plainly passes.
    #[test]
    fn invented_shortcut_refused_plain_description_passes() {
        let index = ShortcutIndex::load().unwrap();
        // A house rule (no semicolons) dressed in an invented MOS alias.
        let dressed = "remove semicolons per MOS:NOSEMI";
        assert!(
            !summary_rule_check(dressed, &index).is_empty(),
            "an invented shortcut never resolves"
        );
        assert!(summary_rule_check("remove semicolons from drafted prose", &index).is_empty());
    }

    /// Prefix variants and a missing corpus are handled.
    #[test]
    fn prefix_variants_and_missing_corpus() {
        let index = ShortcutIndex::load().unwrap();
        assert!(
            !summary_rule_check("see H:HELP", &index).is_empty(),
            "H: variant is checked"
        );
        let err = ShortcutIndex::build(std::path::Path::new("rules/definitely-not-here"))
            .expect_err("missing corpus is a failure, not a silent pass");
        assert!(err.contains("definitely-not-here"), "names the path: {err}");
        // An existing but empty directory is a corpus failure too.
        let empty = std::env::temp_dir().join(format!("wa-empty-canonical-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let err = ShortcutIndex::build(&empty).expect_err("empty corpus fails loudly");
        assert!(err.contains("missing"), "{err}");
        let _ = std::fs::remove_dir_all(&empty);
    }
}
