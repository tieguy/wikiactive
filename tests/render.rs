//! AC.5 — renderer emits resolvable artifacts (offline golden test against
//! recorded Parsoid fixtures) and AC.11's render-side enforcement.

use wikiloop::checks::linter::LinterConfig;
use wikiloop::ledger::Ledger;
use wikiloop::render::RenderInput;
use wikiloop::render::RevisionEntry;
use wikiloop::render::render;
use wikiloop::session::Finding;

/// Strip the `C` prefix from an anchor column part ("C447" → "447").
fn col_of(p: &str) -> &str {
    p.strip_prefix('C').unwrap_or(p)
}

fn linter() -> LinterConfig {
    LinterConfig::load(std::path::Path::new("rules/linter.toml")).unwrap()
}

fn revisions() -> Vec<RevisionEntry> {
    vec![RevisionEntry {
        id: "r1".into(),
        label: "Round 1".into(),
        timestamp: "2026-09-24T18:00:00Z".into(),
        summary: "L2 fix: heading spacing".into(),
    }]
}

/// Offline render over the recorded Temple Fielding Parsoid fixture: the
/// proposed edit adds a clause to the OSS paragraph (a realistic L2 edit),
/// with the proposed wikitext and proposed HTML derived offline from the
/// recorded fixtures (no network).
#[test]
fn ac5_golden_render_against_recorded_parsoid() {
    let base_wt =
        std::fs::read_to_string("fixtures/replay/temple-fielding@1372827284.wikitext").unwrap();
    let base_html =
        std::fs::read_to_string("fixtures/parsoid/temple-fielding@1372827284.html").unwrap();
    // One logical edit: a scoped copy edit on the clean 1945-Warner line
    // (the article's other lines carry pre-existing lint defects, which the
    // gate rightly blocks edits onto — this line is clean).
    let (needle, replacement) = (
        "public relations director for TWA International Division",
        "public relations director for TWA's International Division",
    );
    assert!(
        base_wt.contains(needle),
        "fixture wikitext changed upstream"
    );
    assert!(base_html.contains(needle), "fixture HTML changed upstream");
    let proposed_wt = base_wt.replacen(needle, replacement, 1);
    let proposed_html = base_html.replacen(needle, replacement, 1);
    let ledger = Ledger::default();

    let out = render(&RenderInput {
        article: "Temple Fielding".into(),
        round: 1,
        base_wikitext: &base_wt,
        proposed_wikitext: &proposed_wt,
        base_html: &base_html,
        proposed_html: &proposed_html,
        findings: &[],
        ledger: &ledger,
        linter_config: &linter(),
        revisions: revisions(),
    })
    .expect("render");

    // Unique ids.
    let ids: std::collections::HashSet<_> = out
        .anchor_table
        .iter()
        .map(|a| a.element_id.clone())
        .collect();
    assert_eq!(ids.len(), out.anchor_table.len(), "ids must be unique");

    // Every changed-block id maps to a non-empty wikitext range in the
    // proposed wikitext.
    let lines: Vec<&str> = proposed_wt.lines().collect();
    for entry in &out.anchor_table {
        if entry.element_id.starts_with("ev-") {
            continue;
        }
        // Anchor is L<s>:C<a>-L<e>:C<b>; the covered wikitext slice must be
        // non-empty (b > a on the start line, or e > s).
        let parts: Vec<&str> = entry.wikitext_anchor.split(&['-', ':'][..]).collect();
        assert!(
            parts.len() == 4
                && parts[0].starts_with('L')
                && parts[2].starts_with('L')
                && parts[0][1..].parse::<usize>().is_ok()
                && col_of(parts[1]).parse::<usize>().is_ok()
                && parts[2][1..].parse::<usize>().is_ok()
                && col_of(parts[3]).parse::<usize>().is_ok(),
            "{entry:?} malformed anchor {}",
            entry.wikitext_anchor
        );
        let line_start: usize = parts[0][1..].parse().unwrap();
        let col_start: usize = col_of(parts[1]).parse().unwrap();
        let line_end: usize = parts[2][1..].parse().unwrap();
        let col_end: usize = col_of(parts[3]).parse().unwrap();
        assert!(line_start >= 1 && line_start <= lines.len(), "{entry:?}");
        assert!(
            line_end > line_start || col_end > col_start,
            "{entry:?} empty range {}",
            entry.wikitext_anchor
        );
    }

    // Revisions registry matches the documented shape and appears in the
    // artifact.
    let registry_start = out
        .artifact_html
        .find("data-lavish-revisions")
        .expect("registry present");
    let json_start = out.artifact_html[registry_start..]
        .find('[')
        .map(|i| registry_start + i)
        .unwrap();
    let json_end = out.artifact_html[json_start..]
        .find("</script>")
        .map(|i| json_start + i)
        .unwrap();
    let registry: Vec<serde_json::Value> =
        serde_json::from_str(out.artifact_html[json_start..json_end].trim()).unwrap();
    assert_eq!(registry.len(), 1);
    for key in ["id", "label", "timestamp", "summary"] {
        assert!(registry[0].get(key).is_some(), "missing {key}");
    }
    // Changed blocks carry the revision marker.
    assert!(out.artifact_html.contains("data-lavish-revision=\"r1\""));

    // Anchor table embedded.
    assert!(out.artifact_html.contains("wa-anchor-table"));
}

/// Evidence rail: every gated finding shows at least one quote whose id
/// resolves into the session ledger.
#[test]
fn ac5_evidence_rail_quote_resolves_into_ledger() {
    let mut ledger = Ledger::default();
    let sid = ledger.register_source("https://example.com/fielding", "2026-09-24", None);
    ledger
        .attach_fetched_text(
            &sid,
            "Fielding's guide sold three million copies in Japan by 1986.",
        )
        .unwrap();
    let qid = ledger
        .add_quote(&sid, "sold three million copies in Japan by 1986")
        .unwrap();
    let finding = Finding {
        id: "F1".into(),
        wikitext_anchor: "L3:C0-L3:C80".into(),
        rendered_span_id: None,
        rules: vec!["WP:V".into()],
        evidence: vec![qid.clone()],
        factual_note: "Scope verified against the ledger quote.".into(),
        proposed_fix: "Restore the full qualifier.".into(),
        loop_id: 2,
    };

    let base_wt = "Old text.\n";
    let prop_wt = "The guide sold three million copies in Japan by 1986.\n";
    let base_html = "<html><body><p>Old text.</p></body></html>";
    let prop_html =
        "<html><body><p>The guide sold three million copies in Japan by 1986.</p></body></html>";

    let out = render(&RenderInput {
        article: "Temple Fielding".into(),
        round: 1,
        base_wikitext: base_wt,
        proposed_wikitext: prop_wt,
        base_html,
        proposed_html: prop_html,
        findings: std::slice::from_ref(&finding),
        ledger: &ledger,
        linter_config: &linter(),
        revisions: revisions(),
    })
    .expect("render with evidence");

    assert!(out.artifact_html.contains("id=\"ev-1\""));
    // The evidence card's anchor maps to the ledger quote id, and the quote
    // id resolves in the ledger.
    let anchor = out
        .anchor_table
        .iter()
        .find(|a| a.element_id == "ev-1")
        .expect("ev-1 in anchor table");
    assert_eq!(anchor.wikitext_anchor, format!("ledger:{qid}"));
    let qid_str = anchor
        .wikitext_anchor
        .strip_prefix("ledger:")
        .expect("ledger-prefixed");
    assert!(ledger.quote(qid_str).is_some(), "quote resolves");
    // The rendered_span_id was back-filled.
    assert_eq!(
        out.updated_findings[0].rendered_span_id.as_deref(),
        Some("ev-1")
    );
}

/// AC.11 render side: a finding whose evidence fails validation blocks
/// render — no artifact is produced.
#[test]
fn ac11_failing_evidence_blocks_render_no_artifact() {
    let mut ledger = Ledger::default();
    let sid = ledger.register_source("https://example.com/s", "2026-09-24", None);
    ledger
        .attach_fetched_text(&sid, "Real source text.")
        .unwrap();
    let _qid = ledger.add_quote(&sid, "Real source text").unwrap();
    // Tamper: the stored quote no longer re-locates.
    ledger.quotes[0].text = "Fabricated quote that is nowhere in the source".into();
    let finding = Finding {
        id: "F1".into(),
        wikitext_anchor: "L1:C0-L1:C10".into(),
        rendered_span_id: None,
        rules: vec!["WP:V".into()],
        evidence: vec!["Q1".into()],
        factual_note: "n".into(),
        proposed_fix: "f".into(),
        loop_id: 2,
    };

    let err = render(&RenderInput {
        article: "T".into(),
        round: 1,
        base_wikitext: "old",
        proposed_wikitext: "new",
        base_html: "<html><body><p>old</p></body></html>",
        proposed_html: "<html><body><p>new</p></body></html>",
        findings: std::slice::from_ref(&finding),
        ledger: &ledger,
        linter_config: &linter(),
        revisions: revisions(),
    })
    .unwrap_err();

    assert!(err.to_string().contains("gate blocked"), "{err}");
    assert!(err.to_string().contains("does not re-locate"), "{err}");
    // No artifact: the error carries no HTML.
    let rendered = err.to_string();
    assert!(!rendered.contains("<!doctype html>"));
}
