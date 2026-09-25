//! Tier-3 wikitext linter — mechanical rules, no judgment.
//!
//! Two scopes (plan Phase 2):
//! - [`scan_whole_page`]: full-text scan used by `wa analyze` and replay to
//!   report pre-existing defects.
//! - [`gate`]: runs over a proposed diff — added-lines rules fire on the diff
//!   itself; whole-page rules fire only on findings the edit *introduces*
//!   (present in proposed, absent in base). Error-severity gate findings block
//!   render (see [`crate::checks::gate`]).
//!
//! Rules are declared in `rules/linter.toml`; the config is the single
//! enumeration source — `tests/linter.rs` derives its cases from it (AC.4),
//! so adding a rule without a checker (or a checker without a rule) fails
//! tests rather than silently drifting.
//!
//! Documented MVP limitations: regex-level scanning; no `<nowiki>` /
//! ref-body spanning beyond single matches; templates spanning many lines are
//! attributed to the line where the match starts.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use similar::TextDiff;

/// Severity carried from config. `Error` blocks render in gate mode;
/// `Warn` is reported only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
}

impl Severity {
    /// Lowercase label for reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
        }
    }
}

/// Scope declared per rule in `rules/linter.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    WholePage,
    AddedLines,
}

/// One declared linter rule (parsed from `rules/linter.toml`).
#[derive(Debug, Clone, Deserialize)]
pub struct LintRule {
    pub id: String,
    pub description: String,
    pub severity: Severity,
    pub applies: Scope,
    /// A snippet that MUST be flagged — config-derived tests (AC.4).
    pub sample_violation: String,
    /// A snippet that MUST pass — config-derived tests (AC.4).
    pub sample_clean: String,
}

/// Parsed `rules/linter.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct LinterConfig {
    pub rules: HashMap<String, LintRule>,
}

impl LinterConfig {
    /// Parse config from TOML text.
    ///
    /// # Errors
    /// Returns an error when the TOML is malformed or a rule entry is
    /// missing a required field.
    pub fn from_toml_str(text: &str) -> anyhow::Result<Self> {
        let cfg: Self = toml::from_str(text)?;
        if cfg.rules.is_empty() {
            anyhow::bail!("linter config declares no rules");
        }
        Ok(cfg)
    }

    /// Load config from disk.
    ///
    /// # Errors
    /// Returns an error when the file is missing or malformed.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        Self::from_toml_str(&std::fs::read_to_string(path)?)
    }

    /// Rule ids sorted for deterministic iteration.
    #[must_use]
    pub fn rule_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.rules.values().map(|r| r.id.as_str()).collect();
        ids.sort_unstable();
        ids
    }
}

/// One linter finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintFinding {
    /// Config rule id (e.g. `refname-autonumber`).
    pub rule: String,
    /// 1-based line in the scanned text (0 when whole-text, not line-based).
    pub line: usize,
    /// Human-readable detail, including the offending snippet.
    pub detail: String,
    pub severity: Severity,
}

// --- regexes ----------------------------------------------------------------

static REF_AUTONUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<ref\s+name\s*=\s*":\d+""#).expect("valid regex"));
static SFN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*[Ss]fn\b").expect("valid regex"));
static PAGE_PARAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\|\s*page\s*=").expect("valid regex"));
static PAGES_PARAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\|\s*pages\s*=").expect("valid regex"));
static NAMED_REF_PINPOINT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<ref\s+name\s*=\s*"[^"]*"[^>]*>\s*\{\{[^}]*?\|\s*pages?\s*="#)
        .expect("valid regex")
});
static HEADING_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(={2,6})(.*?)(={2,6})\s*$").expect("valid regex"));
static URL_OR_ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://\S*|&\w+;").expect("valid regex"));
static SEMICOLON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r";").expect("valid regex"));
static TENSE_DRIFT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:is|are)\s+(?:now|currently)\b").expect("valid regex"));
static WIKILINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]|]+)").expect("valid regex"));

/// `-ise`/`-ize` family matcher for twin-stem detection.
static ISE_IZE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([a-z]{3,12})(?:is|iz)(?:e|ed|es|ing|ation|ations)$").expect("valid regex")
});

/// `{{See also|X|Y}}` template form.
static SEE_ALSO_TPL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\{\{\s*see also\s*\|([^\}]+)\}\}").expect("valid regex"));

/// British/American `-our`/`-or` pairs: both members present anywhere is a
/// variety mix (each pair is individually unambiguous).
const OUR_OR_PAIRS: &[(&str, &str)] = &[
    ("colour", "color"),
    ("colours", "colors"),
    ("behaviour", "behavior"),
    ("behaviours", "behaviors"),
    ("favour", "favor"),
    ("favours", "favors"),
    ("honour", "honor"),
    ("labour", "labor"),
    ("neighbour", "neighbor"),
    ("harbour", "harbor"),
    ("rumour", "rumor"),
    ("humour", "humor"),
    ("flavour", "flavor"),
    ("endeavour", "endeavor"),
    ("armour", "armor"),
    ("tumour", "tumor"),
    ("parlour", "parlor"),
    ("splendour", "splendor"),
    ("odour", "odor"),
];

/// Words that end in `-ise`/`-isation` in *both* national varieties: their
/// presence is not a British marker.
const NON_VARIANT_ISE_PREFIXES: &[&str] = &[
    "advise",
    "arise",
    "chastise",
    "compromise",
    "comprise",
    "concise",
    "devise",
    "disguise",
    "enterprise",
    "exercise",
    "excise",
    "franchise",
    "guise",
    "improvise",
    "merchandise",
    "noise",
    "otherwise",
    "paradise",
    "premise",
    "precise",
    "praise",
    "promise",
    "raise",
    "revise",
    "rise",
    "supervise",
    "surprise",
    "televise",
    "wise",
];

/// Word-run length whose sharing between a lead sentence and a body sentence
/// is lead/body duplication (mechanized MOS:LEAD).
pub const LEAD_BODY_NGRAM: usize = 8;

// --- diff plumbing -----------------------------------------------------------

/// Lines present in `proposed` that are new or changed relative to `base`,
/// with their 1-based line numbers in `proposed`.
#[must_use]
pub fn added_lines(base: &str, proposed: &str) -> Vec<(usize, String)> {
    // Normalize trailing newlines so the final line never merges into a
    // spurious block insert.
    let norm = |s: &str| {
        if s.ends_with('\n') {
            s.to_string()
        } else {
            format!("{s}\n")
        }
    };
    let base_norm = norm(base);
    let proposed_norm = norm(proposed);
    let diff = TextDiff::from_lines(&base_norm, &proposed_norm);
    let mut out = Vec::new();
    let mut new_line = 0usize;
    for change in diff.iter_all_changes() {
        let tag = change.tag();
        let text = change.value().trim_end_matches('\n').to_string();
        match tag {
            similar::ChangeTag::Equal => new_line += 1,
            similar::ChangeTag::Delete => {}
            similar::ChangeTag::Insert => {
                new_line += 1;
                out.push((new_line, text));
            }
        }
    }
    out
}

// --- scan entry points -------------------------------------------------------

/// Whole-page scan (analyze/replay mode): every declared rule runs over the
/// full text. Reports pre-existing defects regardless of severity.
#[must_use]
pub fn scan_whole_page(wikitext: &str, cfg: &LinterConfig) -> Vec<LintFinding> {
    let mut findings = Vec::new();
    for rule in cfg.rules.values() {
        check_rule(rule, wikitext, &mut findings);
    }
    findings.sort_by(|a, b| a.rule.cmp(&b.rule).then(a.line.cmp(&b.line)));
    findings
}

/// Gate scan (render/publish mode): added-lines rules run over the added
/// lines only; whole-page rules run over the full proposed text, and only
/// findings that are *new* relative to `base` are emitted.
#[must_use]
pub fn gate(base: &str, proposed: &str, cfg: &LinterConfig) -> Vec<LintFinding> {
    let mut findings = Vec::new();
    let added = added_lines(base, proposed);
    let base_findings: HashSet<(String, String)> = scan_whole_page(base, cfg)
        .into_iter()
        .map(|f| (f.rule, f.detail))
        .collect();

    for rule in cfg.rules.values() {
        match rule.applies {
            Scope::AddedLines => {
                for (line, text) in &added {
                    check_rule_on(rule, text, *line, &mut findings);
                }
            }
            Scope::WholePage => {
                let mut proposed_findings = Vec::new();
                check_rule(rule, proposed, &mut proposed_findings);
                for f in proposed_findings {
                    if base_findings.contains(&(f.rule.clone(), f.detail.clone())) {
                        continue;
                    }
                    findings.push(f);
                }
            }
        }
    }
    findings.sort_by(|a, b| a.rule.cmp(&b.rule).then(a.line.cmp(&b.line)));
    findings
}

/// Whether the gate findings contain any error-severity finding.
#[must_use]
pub fn has_errors(findings: &[LintFinding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Error)
}

// --- per-rule checkers --------------------------------------------------------

/// Run one rule over a whole text (line attribution where meaningful).
fn check_rule(rule: &LintRule, text: &str, findings: &mut Vec<LintFinding>) {
    match rule.id.as_str() {
        "refname-autonumber" => line_scan(rule, text, &REF_AUTONUMBER, "auto ref name", findings),
        "sfn-usage" => line_scan(rule, text, &SFN, "{{sfn}}", findings),
        "named-ref-with-pinpoint" => {
            line_scan(
                rule,
                text,
                &NAMED_REF_PINPOINT,
                "named ref carries |page=",
                findings,
            );
        }
        "page-pages-consistency" => {
            // Mix = both parameter forms present among citations on the page.
            if PAGE_PARAM.is_match(text) && PAGES_PARAM.is_match(text) {
                findings.push(LintFinding {
                    rule: rule.id.clone(),
                    line: 0,
                    detail: "both |page= and |pages= appear in citations; pick one".into(),
                    severity: rule.severity,
                });
            }
        }
        "heading-spacing" => {
            for (idx, line) in text.lines().enumerate() {
                if let Some(caps) = HEADING_LINE.captures(line) {
                    let mid = &caps[2];
                    let spaced =
                        mid.starts_with(' ') && (mid.ends_with(' ') || mid.trim().is_empty());
                    if !spaced {
                        findings.push(LintFinding {
                            rule: rule.id.clone(),
                            line: idx + 1,
                            detail: format!("unspaced heading: {line}"),
                            severity: rule.severity,
                        });
                    }
                }
            }
        }
        "semicolon-prose" => line_scan_stripped(rule, text, findings),
        "tense-drift" => line_scan(rule, text, &TENSE_DRIFT, "tense drift marker", findings),
        "national-variety-mix" => {
            if let Some(detail) = variety_mix(text) {
                findings.push(LintFinding {
                    rule: rule.id.clone(),
                    line: 0,
                    detail,
                    severity: rule.severity,
                });
            }
        }
        "italic-mismatch" => {
            if let Some(detail) = italic_mismatch(text) {
                findings.push(LintFinding {
                    rule: rule.id.clone(),
                    line: 0,
                    detail,
                    severity: rule.severity,
                });
            }
        }
        "see-also-duplication" => {
            for detail in see_also_duplications(text) {
                findings.push(LintFinding {
                    rule: rule.id.clone(),
                    line: 0,
                    detail,
                    severity: rule.severity,
                });
            }
        }
        "lead-body-duplication" => {
            if let Some(detail) = lead_body_duplication(text) {
                findings.push(LintFinding {
                    rule: rule.id.clone(),
                    line: 0,
                    detail,
                    severity: rule.severity,
                });
            }
        }
        other => {
            // Unknown rule id in config: surface loudly rather than drift.
            findings.push(LintFinding {
                rule: other.to_string(),
                line: 0,
                detail: "NO CHECKER IMPLEMENTED for declared rule".into(),
                severity: Severity::Error,
            });
        }
    }
}

/// Run one rule over a single line (added-lines mode).
fn check_rule_on(rule: &LintRule, text: &str, line: usize, findings: &mut Vec<LintFinding>) {
    let mut batch = Vec::new();
    check_rule(rule, text, &mut batch);
    for mut f in batch {
        f.line = line;
        findings.push(f);
    }
}

fn line_scan(rule: &LintRule, text: &str, re: &Regex, what: &str, findings: &mut Vec<LintFinding>) {
    let matches: Vec<&str> = re.find_iter(text).map(|m| m.as_str()).collect();
    if let Some(first) = matches.first() {
        let count = matches.len();
        let suffix = if count > 1 {
            format!(" (+{} more)", count - 1)
        } else {
            String::new()
        };
        findings.push(LintFinding {
            rule: rule.id.clone(),
            line: 0,
            detail: format!("{what}: {first}{suffix}"),
            severity: rule.severity,
        });
    }
}

/// Semicolon scan with URLs and HTML entities masked first.
fn line_scan_stripped(rule: &LintRule, text: &str, findings: &mut Vec<LintFinding>) {
    let prose = mask_templates_and_refs(text);
    let masked = URL_OR_ENTITY.replace_all(&prose, "");
    if let Some(found) = SEMICOLON.find(&masked) {
        let pos = found.start();
        let start = masked[..pos]
            .rfind(char::is_whitespace)
            .map_or(0, |p| p + 1);
        let end = masked[pos..]
            .find(char::is_whitespace)
            .map_or(masked.len(), |r| pos + r);
        findings.push(LintFinding {
            rule: rule.id.clone(),
            line: 0,
            detail: format!("semicolon in prose: {}", &masked[start..end]),
            severity: rule.severity,
        });
    }
}

/// Whether `word` belongs to a `-ise` family that is NOT a British marker
/// (both-variant words like "promise", "advise", "exercise"). Matches the
/// exclusion base or its `e`-less stem so inflected forms
/// ("promised", "comprised") stay excluded.
fn non_variant_ise(word: &str) -> bool {
    NON_VARIANT_ISE_PREFIXES.iter().any(|p| {
        let trimmed = p.strip_suffix('e').unwrap_or(p);
        word.starts_with(p) || word.starts_with(trimmed)
    })
}

/// Replace `{{...}}` template bodies (brace-balanced) and `<ref ...>...</ref>`
/// / `<ref ... />` spans with spaces, so citation/footnote content is not
/// scanned as drafted prose (a semicolon inside an `{{efn|…}}` note or a
/// `|magazine=Time` cite param is not prose). Newlines are preserved.
fn mask_templates_and_refs(text: &str) -> String {
    fn push_masked(out: &mut String, slice: &str) {
        for ch in slice.chars() {
            out.push(if ch == '\n' { '\n' } else { ' ' });
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut template_depth = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        if template_depth == 0 && text[i..].starts_with("<ref") {
            let self_close = text[i..].find("/>");
            let close_tag = text[i..].find("</ref>");
            let (rel, tail) = match (self_close, close_tag) {
                (Some(a), Some(b)) if a <= b => (a, 2),
                (Some(a), None) => (a, 2),
                (_, Some(b)) => (b, 6),
                (None, None) => (text.len() - i, 0),
            };
            let stop = (i + rel + tail).min(text.len());
            push_masked(&mut out, &text[i..stop]);
            i = stop;
            continue;
        }
        if text[i..].starts_with("{{") {
            template_depth += 1;
            out.push_str("  ");
            i += 2;
            continue;
        }
        if text[i..].starts_with("}}") {
            template_depth = template_depth.saturating_sub(1);
            out.push_str("  ");
            i += 2;
            continue;
        }
        let ch = text[i..].chars().next().unwrap_or(' ');
        if template_depth > 0 {
            push_masked(&mut out, &ch.to_string());
        } else {
            out.push(ch);
        }
        i += ch.len_utf8();
    }
    out
}

/// British/American marker suffix families for the cross-word check.
const UK_FAMILIES: &[&str] = &["ise", "ised", "ises", "ising", "isation", "isations"];
const US_FAMILIES: &[&str] = &["ize", "ized", "izes", "izing", "ization", "izations"];

/// Detect a national-variety mix: an `-our`/`-or` twin pair, an `-ise`/`-ize`
/// twin stem, or co-occurrence of a British `-our`/`-ise` marker with an
/// American `-or`/`-ize` marker.
fn variety_mix(text: &str) -> Option<String> {
    let words: HashSet<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3)
        .map(str::to_string)
        .collect();

    // 1. Unambiguous twin pairs (colour+color).
    for (uk, us) in OUR_OR_PAIRS {
        if words.contains(*uk) && words.contains(*us) {
            return Some(format!(
                "national variety mix: '{uk}' and '{us}' both appear"
            ));
        }
    }

    // 2. -ise/-ize twin stems (organised+organized).
    let mut stems: HashMap<String, HashSet<&str>> = HashMap::new();
    for word in &words {
        if non_variant_ise(word) {
            continue;
        }
        if let Some(caps) = ISE_IZE.captures(word) {
            let family = if word.contains("is") { "is" } else { "iz" };
            stems.entry(caps[1].to_string()).or_default().insert(family);
        }
    }
    for (stem, families) in &stems {
        if families.len() > 1 {
            return Some(format!(
                "national variety mix: '-is-' and '-iz-' forms of '{stem}' both appear"
            ));
        }
    }

    // 3. Cross-word mix: at least one marker of each variety.
    let british_marker = words.iter().any(|w| {
        OUR_OR_PAIRS.iter().any(|(uk, _)| w == uk)
            || (UK_FAMILIES.iter().any(|s| w.ends_with(s)) && !non_variant_ise(w))
    });
    let american_marker = words.iter().any(|w| {
        OUR_OR_PAIRS.iter().any(|(_, us)| w == us) || US_FAMILIES.iter().any(|s| w.ends_with(s))
    });
    if british_marker && american_marker {
        return Some(
            "national variety mix: British (-our/-ise) and American (-or/-ize) spellings \
             co-occur"
                .into(),
        );
    }
    None
}

/// Italic mismatch: a `''span''` whose text also appears outside italics.
fn italic_mismatch(raw: &str) -> Option<String> {
    // Scan only drafted prose: cite-template params (|magazine=Time)
    // italicize via the template and are not mismatches.
    let text = mask_templates_and_refs(raw);
    let parts: Vec<&str> = text.split("''").collect();
    if parts.len() < 3 {
        return None;
    }
    let plain: String = parts
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(_, s)| *s)
        .collect::<Vec<_>>()
        .join(" ");
    let fold = |s: &str| s.replace("[[", " ").replace("]]", " ").replace('|', " ");
    for (i, part) in parts.iter().enumerate() {
        if i % 2 == 1 && part.trim().len() >= 3 {
            let needle = fold(part.trim());
            if plain.contains(&needle) {
                return Some(format!(
                    "italic mismatch: '{}' appears both italicized and plain",
                    part.trim()
                ));
            }
        }
    }
    None
}

/// See-also entries whose target is already wikilinked in the body.
///
/// The See also section is its contiguous list of `*`/`#`/`{{…}}` entries
/// after the heading; the first blank or non-list line ends the section
/// (real sections are lists; prose after them is body).
fn see_also_duplications(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut see_also = Vec::new();
    let mut body = String::new();
    let mut in_section = false;
    for line in &lines {
        let trimmed = line.trim();
        let is_l2 =
            trimmed.starts_with("==") && !trimmed.starts_with("===") && trimmed.ends_with("==");
        if is_l2 {
            in_section = trimmed.to_lowercase().contains("see also");
            continue;
        }
        let is_list_entry =
            trimmed.starts_with('*') || trimmed.starts_with('#') || trimmed.starts_with("{{");
        if in_section && !trimmed.is_empty() && is_list_entry {
            see_also.push((*line).to_string());
        } else {
            if in_section {
                in_section = false;
            }
            body.push_str(line);
            body.push('\n');
        }
    }
    // {{See also|X}} template form anywhere.
    let mut targets: Vec<String> = Vec::new();
    for cap in SEE_ALSO_TPL.captures_iter(text) {
        for part in cap[1].split('|') {
            targets.push(part.trim().to_lowercase());
        }
    }
    for line in &see_also {
        if let Some(cap) = WIKILINK.captures(line) {
            targets.push(cap[1].trim().to_lowercase());
        }
    }
    targets.retain(|t| !t.is_empty());

    let body_links: Vec<String> = WIKILINK
        .captures_iter(&body)
        .map(|c| c[1].trim().to_lowercase())
        .collect();
    targets
        .into_iter()
        .filter(|t| body_links.iter().any(|b| b.starts_with(t.as_str())))
        .map(|t| format!("see-also duplication: [[{t}]] is already linked in the body"))
        .collect()
}

/// Lead/body duplication: a shared word-run of [`LEAD_BODY_NGRAM`] tokens
/// between the lead (text before the first level-2 heading) and the body.
fn lead_body_duplication(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let lead_end = lines
        .iter()
        .position(|l| {
            let t = l.trim();
            t.starts_with("==") && !t.starts_with("===") && t.ends_with("==")
        })
        .unwrap_or(lines.len());
    if lead_end == 0 || lead_end == lines.len() {
        return None; // no lead+body structure to compare
    }
    let lead = lines[..lead_end].join(" ");
    let body = lines[lead_end..].join(" ");

    let fold = crate::checks::quote_anchor::normalize_for_match;
    let lead_tokens: Vec<String> = fold(&lead)
        .split(' ')
        .map(crate::checks::quote_anchor::clean_token_pub)
        .filter(|t| !t.is_empty())
        .collect();
    let body_tokens: Vec<String> = fold(&body)
        .split(' ')
        .map(crate::checks::quote_anchor::clean_token_pub)
        .filter(|t| !t.is_empty())
        .collect();
    if lead_tokens.len() < LEAD_BODY_NGRAM || body_tokens.len() < LEAD_BODY_NGRAM {
        return None;
    }
    let body_shingles: HashSet<&[String]> = body_tokens.windows(LEAD_BODY_NGRAM).collect();
    for window in lead_tokens.windows(LEAD_BODY_NGRAM) {
        if body_shingles.contains(window) {
            let run = window.join(" ");
            return Some(format!(
                "lead/body duplication: shared {LEAD_BODY_NGRAM}-word run \"{run}\""
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{LinterConfig, Scope, gate, scan_whole_page};

    fn cfg() -> LinterConfig {
        LinterConfig::from_toml_str(include_str!("../../rules/linter.toml"))
            .expect("bundled linter.toml parses")
    }

    #[test]
    fn added_lines_diff_reports_new_and_changed() {
        let base = "alpha\nbeta\ngamma";
        let proposed = "alpha\nBETA!\ngamma\ndelta";
        let added = super::added_lines(base, proposed);
        let texts: Vec<&str> = added.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(texts, vec!["BETA!", "delta"]);
    }

    #[test]
    fn heading_spacing_flags_unspaced_only() {
        let c = cfg();
        let bad = scan_whole_page("==Life==\nHe was born.", &c);
        assert!(bad.iter().any(|f| f.rule == "heading-spacing"));
        let good = scan_whole_page("== Life ==\nHe was born.", &c);
        assert!(!good.iter().any(|f| f.rule == "heading-spacing"));
    }

    #[test]
    fn lead_body_duplication_catches_verbatim_repeat() {
        let c = cfg();
        let text = "The commitment device is a psychological strategy studied in behavioral economics.\n\n\
             == Concept ==\nScholars describe how the commitment device is a psychological \
             strategy studied in behavioral economics, beginning in 1985.";
        let findings = scan_whole_page(text, &c);
        assert!(
            findings.iter().any(|f| f.rule == "lead-body-duplication"),
            "{findings:?}"
        );
        let clean = "The commitment device is a psychological strategy studied in behavioral economics.\n\n\
             == Concept ==\nBehavioral economists analyze how people bind their own future \
             choices, a pattern first named in 1985.";
        assert!(
            !scan_whole_page(clean, &c)
                .iter()
                .any(|f| f.rule == "lead-body-duplication")
        );
    }

    #[test]
    fn semicolon_inside_efn_or_ref_is_not_prose() {
        let c = cfg();
        let text = "He was born in 1913{{efn|The obituary gives 1912; see the note.}} in the Bronx.<ref name=\"npg\">{{Cite web |title=X |page=2}}</ref>";
        let findings = super::scan_whole_page(text, &c);
        assert!(
            !findings.iter().any(|f| f.rule == "semicolon-prose"),
            "{findings:?}"
        );
    }

    #[test]
    fn italic_work_inside_cite_param_is_not_a_mismatch() {
        let c = cfg();
        let text = "The piece in ''Time'' praised it.<ref name=\"t\">{{Cite magazine |magazine=Time |title=Modern Living |date=1969}}</ref>";
        let findings = super::scan_whole_page(text, &c);
        assert!(
            !findings.iter().any(|f| f.rule == "italic-mismatch"),
            "{findings:?}"
        );
    }

    #[test]
    fn four_and_raised_are_not_british_markers() {
        let c = cfg();
        let text = "They raised four children and organized the funds.";
        let findings = super::scan_whole_page(text, &c);
        assert!(
            !findings.iter().any(|f| f.rule == "national-variety-mix"),
            "{findings:?}"
        );
    }

    #[test]
    fn gate_introduces_only_new_whole_page_findings() {
        let c = cfg();
        // Base already mixes varieties; proposed keeps it — not a new finding.
        let base = "The harbour trust met. The organizer spoke.";
        let proposed = "The harbour trust met. The organizer spoke. New sentence here.";
        let findings = gate(base, proposed, &c);
        assert!(
            !findings.iter().any(|f| f.rule == "national-variety-mix"),
            "{findings:?}"
        );
        // Now the edit introduces the mix.
        let clean_base = "The harbor trust met.";
        let mixed = "The harbor trust met. The harbour commission disagreed.";
        let findings = gate(clean_base, mixed, &c);
        assert!(
            findings.iter().any(|f| f.rule == "national-variety-mix"),
            "{findings:?}"
        );
    }

    #[test]
    fn config_scope_fields_parse() {
        let c = cfg();
        let semicolon = c
            .rules
            .values()
            .find(|r| r.id == "semicolon-prose")
            .expect("present");
        assert_eq!(semicolon.applies, Scope::AddedLines);
        let heading = c
            .rules
            .values()
            .find(|r| r.id == "heading-spacing")
            .expect("present");
        assert_eq!(heading.applies, Scope::WholePage);
    }
}
