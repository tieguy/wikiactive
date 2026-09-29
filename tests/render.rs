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

/// Validate one changed-block anchor: a plain `L..:C..-L..:C..` range must
/// fall inside the proposed wikitext, a `base:`-prefixed range (deletion
/// anchor) inside the base wikitext; the covered slice must be non-empty.
fn assert_valid_range(anchor: &str, base_lines: &[&str], proposed_lines: &[&str]) {
    let (in_base, range) = match anchor.strip_prefix("base:") {
        Some(rest) => (true, rest),
        None => (false, anchor),
    };
    let lines = if in_base { base_lines } else { proposed_lines };
    // Anchor is L<s>:C<a>-L<e>:C<b>; the covered wikitext slice must be
    // non-empty (b > a on the start line, or e > s).
    let parts: Vec<&str> = range.split(&['-', ':'][..]).collect();
    assert!(
        parts.len() == 4
            && parts[0].starts_with('L')
            && parts[2].starts_with('L')
            && parts[0][1..].parse::<usize>().is_ok()
            && col_of(parts[1]).parse::<usize>().is_ok()
            && parts[2][1..].parse::<usize>().is_ok()
            && col_of(parts[3]).parse::<usize>().is_ok(),
        "malformed anchor {anchor}"
    );
    let line_start: usize = parts[0][1..].parse().unwrap();
    let col_start: usize = col_of(parts[1]).parse().unwrap();
    let line_end: usize = parts[2][1..].parse().unwrap();
    let col_end: usize = col_of(parts[3]).parse().unwrap();
    assert!(line_start >= 1 && line_start <= lines.len(), "{anchor}");
    assert!(
        line_end > line_start || col_end > col_start,
        "empty range {anchor}"
    );
}

fn linter() -> LinterConfig {
    LinterConfig::load(std::path::Path::new("rules/linter.toml")).unwrap()
}

fn paraphrase() -> wikiloop::checks::paraphrase::ParaphraseConfig {
    wikiloop::checks::paraphrase::ParaphraseConfig::load(std::path::Path::new(
        "rules/paraphrase.toml",
    ))
    .unwrap()
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
        paraphrase_config: &paraphrase(),
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

    // Every changed-block id maps to a non-empty wikitext range; new-side
    // blocks anchor in the proposed wikitext, old-side blocks (deletion
    // anchors) carry a `base:` prefix and anchor in the base wikitext.
    let proposed_lines: Vec<&str> = proposed_wt.lines().collect();
    let base_lines: Vec<&str> = base_wt.lines().collect();
    let mut old_side_anchors = 0usize;
    for entry in &out.anchor_table {
        if entry.element_id.starts_with("ev-") {
            continue;
        }
        if entry.wikitext_anchor.starts_with("base:") {
            old_side_anchors += 1;
        }
        assert_valid_range(&entry.wikitext_anchor, &base_lines, &proposed_lines);
    }
    // The fixture edit is a changed pair: its old half carries a `base:`
    // deletion anchor into the recorded base wikitext.
    assert!(
        old_side_anchors >= 1,
        "old-side (base:) anchors missing — deletion anchors"
    );

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
        paraphrase_config: &paraphrase(),
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
        paraphrase_config: &paraphrase(),
        revisions: revisions(),
    })
    .unwrap_err();

    assert!(err.to_string().contains("gate blocked"), "{err}");
    assert!(err.to_string().contains("does not re-locate"), "{err}");
    // No artifact: the error carries no HTML.
    let rendered = err.to_string();
    assert!(!rendered.contains("<!doctype html>"));
}

/// plan-004 AC.2 (golden): the in-app comment forms' targets carry the
/// artifact's `wikitext_anchor`s VERBATIM from the embedded anchor table,
/// across all three families — `wa-N` new-side (plain range), `base:`
/// old-side (the changed pair's removed wording, riding inside the
/// new-side block), and `ev-N` (ledger). The form target IS the anchor,
/// so comment anchoring is exact by construction.
#[test]
fn comment_form_targets_match_the_anchor_table_verbatim_all_three_families() {
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
        wikitext_anchor: "L1:C0-L1:C80".into(),
        rendered_span_id: None,
        rules: vec!["WP:V".into()],
        evidence: vec![qid.clone()],
        factual_note: "Scope verified against the ledger quote.".into(),
        proposed_fix: "Restore the full qualifier.".into(),
        loop_id: 2,
    };

    let base_wt = "The guide sold copies.\n";
    let prop_wt = "The guide sold three million copies in Japan by 1986.\n";
    let base_html = "<html><body><p>The guide sold copies.</p></body></html>";
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
        paraphrase_config: &paraphrase(),
        revisions: revisions(),
    })
    .expect("render");

    // The embedded anchor table is the oracle.
    let table = wikiloop::render::read_anchor_table_str(&out.artifact_html).unwrap();
    let lookup = |id: &str| {
        table
            .iter()
            .find(|e| e.element_id == id)
            .unwrap_or_else(|| panic!("anchor {id} in table: {table:?}"))
            .wikitext_anchor
            .clone()
    };

    // Family 1: the changed pair's new-side block — a plain L..C..-L..C..
    // range into the proposed wikitext.
    let targets = wikiloop::render::review_targets(&out.artifact_html);
    let new_side = targets
        .iter()
        .find(|t| t.element_id.starts_with("wa-") && !t.wikitext_anchor.starts_with("base:"))
        .expect("new-side block target");
    assert!(
        new_side.wikitext_anchor.starts_with("L1:C0-L1:"),
        "new-side anchor is a proposed-wikitext range: {:?}",
        new_side.wikitext_anchor
    );
    assert_eq!(
        new_side.wikitext_anchor,
        lookup(&new_side.element_id),
        "form target must be the anchor-table anchor VERBATIM"
    );
    // Labels come from the block's leading words (server-side).
    assert!(
        new_side.label.to_lowercase().starts_with("the guide sold"),
        "label from leading words: {:?}",
        new_side.label
    );

    // Family 2: the pair's old side (`base:`-prefixed) rides inside the
    // new-side block — either a <del> run (label = its words) or an empty
    // marker span (label falls back to the line).
    let old = new_side
        .old_sides
        .first()
        .expect("changed pair carries an old-side target");
    assert!(old.wikitext_anchor.starts_with("base:L1:"));
    assert_eq!(
        old.wikitext_anchor,
        lookup(&old.element_id),
        "old-side form target must be the anchor-table anchor VERBATIM"
    );

    // Family 3: the evidence card — `ledger:Q<n>`, labeled by the source's
    // citation text, never the Q-id.
    let evidence = wikiloop::render::evidence_targets(&out.artifact_html);
    assert_eq!(evidence.len(), 1, "one evidence card");
    assert_eq!(
        evidence[0].wikitext_anchor,
        lookup(&evidence[0].element_id),
        "evidence form target must be the anchor-table anchor VERBATIM"
    );
    assert_eq!(evidence[0].wikitext_anchor, format!("ledger:{qid}"));
    assert_eq!(
        evidence[0].label, "example.com",
        "evidence label is the source link text (host fallback), not the Q-id and not \
         the literal 'Source: ' prefix"
    );

    // Anchors contain no HTML-escapable characters, so the form's escaped
    // value round-trips verbatim through the POST back into the queue.
    for anchor in [
        &new_side.wikitext_anchor,
        &old.wikitext_anchor,
        &evidence[0].wikitext_anchor,
    ] {
        assert!(!anchor.contains(['&', '<', '>', '"']), "{anchor}");
    }
}

/// Plan-004 review finding (major 2), pinned: a changed pair whose equal
/// prefix contains a content link (`<span class="wl">…</span>` in the
/// rendered diff) still yields its old-side "removed wording" form — the
/// old-side `<del id=…>` run comes AFTER the link's `</span>`, and the
/// target parser must not cut the block body at that span.
#[test]
fn old_side_forms_survive_link_containing_blocks() {
    let base_wt = "See the keep which is old.\n";
    let prop_wt = "See the keep which is ancient.\n";
    let base_html = "<html><body><p>See the <a rel=\"mw:WikiLink\" href=\"./Keep\">keep</a> which is old.</p></body></html>";
    let prop_html = "<html><body><p>See the <a rel=\"mw:WikiLink\" href=\"./Keep\">keep</a> which is ancient.</p></body></html>";

    let out = render(&RenderInput {
        article: "T".into(),
        round: 1,
        base_wikitext: base_wt,
        proposed_wikitext: prop_wt,
        base_html,
        proposed_html: prop_html,
        findings: &[],
        ledger: &Ledger::default(),
        linter_config: &linter(),
        paraphrase_config: &paraphrase(),
        revisions: revisions(),
    })
    .expect("render");

    let table = wikiloop::render::read_anchor_table_str(&out.artifact_html).unwrap();
    let targets = wikiloop::render::review_targets(&out.artifact_html);
    let new_side = targets
        .iter()
        .find(|t| !t.wikitext_anchor.starts_with("base:"))
        .expect("new-side block");
    // The rendered combined diff carries the link span; the old-side id
    // must still be found after it.
    assert!(
        out.artifact_html.contains("<span class=\"wl\">"),
        "fixture must exercise a content-link span"
    );
    let old = new_side
        .old_sides
        .first()
        .expect("old-side target survives the link span");
    assert!(old.wikitext_anchor.starts_with("base:L1:"));
    assert_eq!(
        old.wikitext_anchor,
        table
            .iter()
            .find(|e| e.element_id == old.element_id)
            .expect("old side in anchor table")
            .wikitext_anchor
    );
    // The old-side label comes from the <del> run's words.
    assert_eq!(old.label.as_deref(), Some("old."));
}
