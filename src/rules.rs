//! Rules corpus loading and the `wa analyze` context bundle (AC.1).
//!
//! The context bundle is the mechanical guarantee behind "Tier 1 always in
//! context": step 0 of every playbook iteration runs `wa analyze`, which
//! embeds the tier1-core clauses **verbatim** plus the triage-selected
//! trigger cards, the article state at the pinned revid, and a findings/
//! ledger summary. If the corpus is missing, bundle construction fails
//! loudly — it can never be silently dropped.

use std::fmt::Write as _;

use crate::checks::linter::LinterConfig;
use crate::ledger::net::RspSeedRow;
use crate::ledger::net::load_rsp_seed;
use crate::session::Assessment;

/// Parsed house rules (`rules/house-rules.toml`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HouseRules {
    pub disclosure: DisclosureRules,
    pub prose: ProseRules,
    /// Operator identity (default username for `--review-since-user`).
    #[serde(default)]
    pub operator: OperatorRules,
    /// Model driver endpoint config (`[zai]` in house-rules.toml).
    #[serde(default)]
    pub zai: Option<ZaiRules>,
    /// Audit defaults (loop-mechanization Phase 2): `[audit]` in
    /// house-rules.toml — whether `wa audit` runs the LLM diagnosis pass
    /// when the run does not pass `--llm`/`--no-llm`.
    #[serde(default)]
    pub audit: AuditRules,
    #[serde(default)]
    pub citevar: Option<serde_json::Value>,
    pub user_agent: UserAgentRules,
    #[serde(default)]
    pub etiquette: Option<serde_json::Value>,
}

/// `[zai]`: the model driver's endpoint — fork configuration, not shell
/// ritual (plan-003 B.6 operator catch: "why am I doing shell exports?
/// Those should be configuration"). Env still overrides for one-offs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, Default)]
pub struct ZaiRules {
    /// Base URL (e.g. the Coding-Plan endpoint this fork's key uses).
    pub base_url: String,
    /// Model id at the three judgment points.
    #[serde(default)]
    pub model: Option<String>,
}

/// `[audit]` in `rules/house-rules.toml` — defaults for `wa audit`
/// (loop-mechanization Phase 2). A fork flips `llm_pass` to turn the
/// paid diagnosis pass off by default; `--llm`/`--no-llm` override per
/// run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct AuditRules {
    /// Run the LLM diagnosis pass when the run passes neither flag.
    #[serde(default = "default_true")]
    pub llm_pass: bool,
}

impl Default for AuditRules {
    fn default() -> Self {
        Self {
            llm_pass: default_true(),
        }
    }
}

/// Serde default helper: `true`.
fn default_true() -> bool {
    true
}

/// `[operator]` in `rules/house-rules.toml` — the on-wiki operator
/// identity, a fork-edit point alongside `user_agent.string`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct OperatorRules {
    /// Default username for `wa session init --review-since-user`.
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DisclosureRules {
    pub suffix: String,
    pub page: String,
    /// Subpage carrying per-article-session log entries.
    #[serde(default = "default_log_page")]
    pub log_page: String,
    /// Drafting model recorded in session-log entries.
    #[serde(default)]
    pub drafting_model: String,
    #[serde(default)]
    pub require_suffix: bool,
}

fn default_log_page() -> String {
    "User:LuisVilla/wikiactive/log".to_string()
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProseRules {
    #[serde(default)]
    pub ban_semicolons: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct UserAgentRules {
    pub string: String,
}

/// The loaded rules corpus.
#[derive(Debug, Clone)]
pub struct RulesCorpus {
    /// tier1-core.md contents (verbatim; embedded verbatim in bundles).
    pub tier1_core: String,
    /// Card slugs (file stems under `rules/cards/`).
    pub card_slugs: Vec<String>,
    /// Cards by slug.
    pub cards: std::collections::HashMap<String, String>,
    pub linter: LinterConfig,
    /// Paraphrase-gate thresholds (`rules/paraphrase.toml`, MVP-2 A.2.3).
    pub paraphrase: crate::checks::paraphrase::ParaphraseConfig,
    pub house_rules: HouseRules,
    pub rsp_seed: Vec<RspSeedRow>,
}

impl RulesCorpus {
    /// Load the corpus from the repo's `rules/` directory.
    ///
    /// # Errors
    /// Missing files or malformed configs (with the offending path).
    pub fn load(rules_dir: &std::path::Path) -> Result<Self, String> {
        let tier1_path = rules_dir.join("tier1-core.md");
        let tier1_core = std::fs::read_to_string(&tier1_path).map_err(|e| {
            format!(
                "tier1-core.md missing/unreadable ({}): {e}",
                tier1_path.display()
            )
        })?;
        if !tier1_core.contains("Cluster A")
            || !tier1_core.contains("Cluster B")
            || !tier1_core.contains("Cluster C")
        {
            return Err("tier1-core.md is missing one of the three clusters (A/B/C)".into());
        }

        let cards_dir = rules_dir.join("cards");
        let mut card_slugs = Vec::new();
        let mut cards = std::collections::HashMap::new();
        let entries = std::fs::read_dir(&cards_dir)
            .map_err(|e| format!("rules/cards/ unreadable ({}): {e}", cards_dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("reading rules/cards entry: {e}"))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let slug = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or("non-utf8 card filename")?
                    .to_string();
                let body = std::fs::read_to_string(&path)
                    .map_err(|e| format!("card {slug} unreadable: {e}"))?;
                card_slugs.push(slug.clone());
                cards.insert(slug, body);
            }
        }
        card_slugs.sort();

        let linter = LinterConfig::load(&rules_dir.join("linter.toml"))
            .map_err(|e| format!("linter.toml: {e}"))?;
        let paraphrase =
            crate::checks::paraphrase::ParaphraseConfig::load(&rules_dir.join("paraphrase.toml"))
                .map_err(|e| format!("paraphrase.toml: {e}"))?;
        let house_text = std::fs::read_to_string(rules_dir.join("house-rules.toml"))
            .map_err(|e| format!("house-rules.toml unreadable: {e}"))?;
        let house_rules: HouseRules =
            toml::from_str(&house_text).map_err(|e| format!("house-rules.toml malformed: {e}"))?;
        let rsp_seed = load_rsp_seed(&rules_dir.join("sources").join("rsp-seed.tsv"))
            .map_err(|e| format!("rsp-seed.tsv: {e}"))?;

        Ok(Self {
            tier1_core,
            card_slugs,
            cards,
            linter,
            paraphrase,
            house_rules,
            rsp_seed,
        })
    }

    /// The RSP tier for a source id (seed table).
    #[must_use]
    pub fn rsp_tier(&self, source_id: &str) -> Option<&str> {
        self.rsp_seed
            .iter()
            .find(|r| r.source_id == source_id)
            .map(|r| r.tier.as_str())
    }
}

/// Article state block of the context bundle.
#[derive(Debug, Clone)]
pub struct ArticleState {
    pub title: String,
    pub base_revid: u64,
    pub wikitext: String,
    /// Triage-selected entry loop (1–5).
    pub entry_loop: u8,
    /// Diff vs the operator's prior-session base, when re-reviewing (drift
    /// context for the triage ladder).
    pub prior_session_diff: Option<String>,
}

/// The `wa analyze` context bundle.
#[derive(Debug, Clone)]
pub struct ContextBundle {
    pub text: String,
}

/// Build the context bundle: article state + tier1 verbatim + selected cards
/// + findings/ledger summary.
///
/// Card selection by loop (triage): each loop loads its standard card set;
/// Loop 4 adds CLOP; every loop with new sources loads RS-TIERS and
/// PRIMARY-CARVEOUTS.
#[must_use]
pub fn cards_for_loop(loop_id: u8) -> Vec<String> {
    match loop_id {
        1 => vec!["leadcite"],
        2 => vec!["rs-tiers", "clop", "primary-carveouts"],
        3 => vec![
            "undue",
            "notlitreview",
            "proseline",
            "recentism",
            "leadcite",
            "summary-spinoff",
            "efn-conflicts",
        ],
        4 => vec![
            "rs-tiers",
            "primary-carveouts",
            "clop",
            "efn-conflicts",
            "notlitreview",
        ],
        _ => vec![],
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// The rules guidance every model step must apply for `loop_id`: tier-1
/// verbatim, then the triage-selected trigger cards (rule-enforcement
/// item 3). ONE source for this text — the `wa analyze` bundle and the
/// driver prompts embed the same bytes, so the model behind "Write
/// findings" / "Draft the edit" / "Apply comments" sees exactly what
/// analyze prints.
///
/// # Errors
/// When the corpus is missing a card the triage selected (never silently
/// dropped — the step fails before any model call).
pub fn guidance_for_loop(corpus: &RulesCorpus, loop_id: u8) -> Result<String, String> {
    let selected = cards_for_loop(loop_id);
    for slug in &selected {
        if !corpus.cards.contains_key(slug) {
            return Err(format!(
                "triage selected card '{slug}' but it is missing from the corpus"
            ));
        }
    }
    let mut text = String::new();
    text.push_str("## Tier 1 — judgment core (verbatim)\n\n");
    text.push_str(&corpus.tier1_core);
    text.push_str("\n\n## Trigger cards (triage-selected)\n\n");
    for slug in &selected {
        let card = corpus
            .cards
            .get(slug)
            .ok_or_else(|| format!("card {slug} missing"))?;
        text.push_str(card);
        text.push_str("\n\n---\n\n");
    }
    Ok(text)
}

/// The clause ids the rule-review step may cite (rule-enforcement item
/// 5): tier-1 paragraph ids (the `**A1.` … markers) plus the selected
/// cards' slugs — built from the corpus, never by hand, so a renamed or
/// removed clause cannot slip through validation.
#[must_use]
pub fn guidance_clauses(corpus: &RulesCorpus, loop_id: u8) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for line in corpus.tier1_core.lines() {
        if let Some(rest) = line.strip_prefix("**")
            && let Some((id, _)) = rest.split_once('.')
            && id.starts_with(['A', 'B', 'C'])
            && id.len() > 1
            && id[1..].bytes().all(|b| b.is_ascii_digit())
        {
            ids.push(id.to_string());
        }
    }
    ids.extend(cards_for_loop(loop_id));
    ids
}

/// Assemble the bundle text.
///
/// # Errors
/// When the corpus is missing a card the triage selected (never silently
/// dropped).
pub fn build_context_bundle(
    corpus: &RulesCorpus,
    article: &ArticleState,
    assessments: &[Assessment],
    ledger: &crate::ledger::Ledger,
) -> Result<ContextBundle, String> {
    // The rules guidance is shared verbatim with the driver prompts
    // (guidance_for_loop) — one source, both surfaces.
    let guidance = guidance_for_loop(corpus, article.entry_loop)?;

    let mut text = String::new();
    text.push_str("# wa analyze — context bundle\n\n");
    let _ = write!(
        text,
        "## Article state\n- title: {}\n- base revid: {}\n- entry loop: L{}\n- wikitext bytes: {}\n",
        article.title,
        article.base_revid,
        article.entry_loop,
        article.wikitext.len()
    );
    if let Some(diff) = &article.prior_session_diff {
        let _ = write!(
            text,
            "- drift (prior session or operator's last edit):\n{diff}\n"
        );
    }
    text.push('\n');
    text.push_str(&guidance);
    text.push_str("## Session summary\n\n");
    let _ = write!(
        text,
        "- ledger sources: {}\n- ledger quotes: {}\n- ledger claims: {}\n- assessments: {}\n",
        ledger.sources.len(),
        ledger.quotes.len(),
        ledger.claims.len(),
        assessments.len()
    );
    for assessment in assessments {
        let _ = writeln!(
            text,
            "- {} [{}] loop {} — {}",
            assessment.id,
            assessment.rules.join(","),
            assessment.loop_id,
            assessment.factual_note
        );
    }
    // Base-article defect candidates (loop-mechanization Phase 6): the
    // linter corpus run over the BASE text — detection only. These are
    // candidate assessments for the assess step to judge; nothing
    // auto-applies, and no gate reason can originate from this list.
    let defects = crate::checks::linter::scan_whole_page(&article.wikitext, &corpus.linter);
    if !defects.is_empty() {
        text.push_str(
            "\n## Base-article defect candidates (detection only — nothing auto-applies)\n\n",
        );
        for d in &defects {
            let description = corpus
                .linter
                .rules
                .values()
                .find(|r| r.id == d.rule)
                .map_or("", |r| r.description.as_str());
            let _ = writeln!(
                text,
                "- [{}] {} (line {}): {} — {}",
                d.severity.label(),
                d.rule,
                d.line,
                d.detail,
                description
            );
        }
        text.push_str(
            "\nThese are MECHANICAL observations about the base article, offered as \
             candidate assessments — the assess step decides what, if anything, to do \
             with them. Detection is not enforcement: the gate's drafted-lines rules \
             apply only to lines this tool drafts.\n",
        );
    }
    // Sweep manifest (plan-003 B.3): informational in the bundle — the
    // mechanical enforcement lives in the gate (SweepSourceUnresolved).
    if ledger.has_sweep_state() {
        let unresolved = ledger.sweep_unresolved();
        let _ = write!(
            text,
            "\n## Source sweep manifest\n\n- warning: {} unresolved source(s) — the gate blocks render/publish until each is fetched, captured, or dispositioned (`wa fetch <slug>` / `wa ledger attach` / `wa fetch dispose`)\n",
            unresolved.len()
        );
        for s in &ledger.sources {
            let status = s.sweep_status.as_deref().unwrap_or("—");
            let disposition = s.disposition.as_deref().unwrap_or("—");
            let text_state = if s.fetched_text.is_some() {
                "text✓"
            } else {
                "no text"
            };
            let _ = writeln!(
                text,
                "- {} [{status}] ({text_state}, disposition: {disposition}) {}",
                s.id, s.url
            );
        }
    }
    Ok(ContextBundle { text })
}

#[cfg(test)]
mod tests {
    use super::{AuditRules, HouseRules, RulesCorpus, build_context_bundle, cards_for_loop};

    fn corpus() -> RulesCorpus {
        RulesCorpus::load(std::path::Path::new("rules")).expect("repo corpus loads")
    }

    /// loopmech.AC8.1 — the audit LLM-pass default is fork config:
    /// the repo ships it ON; the section is optional and defaults ON;
    /// an explicit false flips it.
    #[test]
    fn audit_llm_pass_default_is_fork_config() {
        // The shipped fork config has it on.
        let c = corpus();
        assert!(c.house_rules.audit.llm_pass, "repo default is on");

        // Section absent entirely => on (the shipped file minus [audit]).
        let raw = std::fs::read_to_string("rules/house-rules.toml").unwrap();
        let without_audit = raw
            .lines()
            .filter(|l| !l.contains("llm_pass") && !l.trim().starts_with("[audit]"))
            .collect::<Vec<_>>()
            .join("\n");
        let minimal: HouseRules = toml::from_str(&without_audit).expect("parses without [audit]");
        assert!(minimal.audit.llm_pass);

        // Explicit false => off.
        let off: AuditRules = toml::from_str("llm_pass = false").unwrap();
        assert!(!off.llm_pass);
    }

    #[test]
    fn corpus_loads_with_three_clusters_and_ten_cards() {
        let c = corpus();
        assert!(c.tier1_core.contains("Cluster A"));
        assert!(c.tier1_core.contains("Cluster B"));
        assert!(c.tier1_core.contains("Cluster C"));
        assert!(c.card_slugs.len() >= 10, "{} cards", c.card_slugs.len());
        assert!(c.house_rules.prose.ban_semicolons);
        assert!(!c.house_rules.disclosure.suffix.is_empty());
        assert!(c.rsp_seed.iter().any(|r| r.tier == "deny"));
    }

    #[test]
    fn bundle_embeds_tier1_verbatim_and_selected_cards() {
        let c = corpus();
        let article = super::ArticleState {
            title: "Test".into(),
            base_revid: 1,
            wikitext: "x".into(),
            entry_loop: 3,
            prior_session_diff: None,
        };
        let bundle =
            build_context_bundle(&c, &article, &[], &crate::ledger::Ledger::default()).unwrap();
        // Verbatim tier1: a distinctive clause from the corpus appears.
        assert!(bundle.text.contains("Attribution over wikivoice"));
        // Selected cards present for loop 3.
        for slug in cards_for_loop(3) {
            assert!(
                bundle
                    .text
                    .contains(&format!("Card: {}", slug_upper(&slug))),
                "card {slug} missing — cards are embedded with their Card header"
            );
        }
        // Loop 3 does NOT load clop (loop 2/4 own it).
        assert!(!bundle.text.contains("Card: CLOP"));
    }

    fn slug_upper(slug: &str) -> String {
        let map = [
            ("undue", "UNDUE"),
            ("notlitreview", "NOTLITREVIEW"),
            ("proseline", "PROSELINE"),
            ("recentism", "RECENTISM"),
            ("leadcite", "LEADCITE"),
            ("summary-spinoff", "SUMMARY-SPINOFF"),
            ("efn-conflicts", "EFN-CONFLICTS"),
            ("clop", "CLOP"),
            ("rs-tiers", "RS-TIERS"),
            ("primary-carveouts", "PRIMARY-CARVEOUTS"),
        ];
        for (from, to) in map {
            if from == slug {
                return to.to_string();
            }
        }
        slug.to_uppercase()
    }

    #[test]
    fn missing_card_fails_bundle_loudly() {
        let mut c = corpus();
        c.cards.remove("undue");
        let article = super::ArticleState {
            title: "T".into(),
            base_revid: 1,
            wikitext: String::new(),
            entry_loop: 3,
            prior_session_diff: None,
        };
        let err =
            build_context_bundle(&c, &article, &[], &crate::ledger::Ledger::default()).unwrap_err();
        assert!(err.contains("undue"), "{err}");
    }

    /// Rule-enforcement item 3: `guidance_for_loop` is the one source the
    /// driver prompts embed — a corpus missing a triage-selected card
    /// errors (the handler fails before any model call), and the built
    /// guidance carries tier-1 verbatim plus the loop's cards.
    #[test]
    fn guidance_for_loop_carries_tier1_and_cards_and_fails_on_missing_card() {
        let c = corpus();
        let g = super::guidance_for_loop(&c, 2).unwrap();
        assert!(g.contains("Cluster A") && g.contains("Cluster B") && g.contains("Cluster C"));
        for slug in cards_for_loop(2) {
            assert!(g.contains(&slug_upper(&slug)), "card {slug} missing");
        }
        // (No negative card assertion: loop-2 cards legitimately mention
        // other cards' rule names in their text.)

        let mut c = corpus();
        c.cards.remove("clop");
        let err = super::guidance_for_loop(&c, 2).unwrap_err();
        assert!(err.contains("clop"), "{err}");
    }

    /// Rule-enforcement item 5: the clause allowlist is corpus-built —
    /// tier-1 paragraph ids plus the loop's card slugs.
    #[test]
    fn guidance_clauses_lists_tier1_ids_and_card_slugs() {
        let c = corpus();
        let ids = super::guidance_clauses(&c, 2);
        assert!(ids.contains(&"A1".to_string()), "{ids:?}");
        assert!(ids.contains(&"A3".to_string()), "{ids:?}");
        assert!(ids.contains(&"C1".to_string()), "{ids:?}");
        assert!(ids.contains(&"rs-tiers".to_string()), "{ids:?}");
        assert!(ids.contains(&"clop".to_string()), "{ids:?}");
        assert!(!ids.contains(&"undue".to_string()), "loop 2 cards only");
    }
}
