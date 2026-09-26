//! AC.6 — anchor resolution round-trips with synthetic selector and
//! text-range payloads in lavish's documented format (vendored source,
//! `selector(el)` + `rangeBoundary` shapes). The one-time manual lavish
//! round-trip (Phase 0 gate) is logged in `docs/spike-notes.md` —
//! annotating needs a human hand (accepted limitation).

use wikiloop::anchors::CommentPrompt;
use wikiloop::anchors::CommentTarget;
use wikiloop::anchors::RangeBoundary;
use wikiloop::anchors::id_from_selector;
use wikiloop::anchors::resolve_comment;

fn table() -> Vec<(String, String)> {
    vec![
        ("wa-1".into(), "L3:C0-L3:C410".into()),
        ("wa-2".into(), "L3:C0-L3:C433".into()),
        ("ev-1".into(), "ledger:Q1".into()),
    ]
}

#[test]
fn ac6_selector_payload_resolves_to_data_wiki_anchor() {
    // lavish selector() stops at the first id: `#wa-2` for an element with
    // an id; `pane > div:nth-of-type(2)` when no id exists (must fail).
    let payload = CommentPrompt {
        prompt: "Tighten the lead.".into(),
        selector: "#wa-2".into(),
        tag: "div".into(),
        text: "I contribute mainly to…".into(),
        target: None,
    };
    let resolved = resolve_comment(&payload, &table()).expect("resolves");
    assert_eq!(resolved.element_id, "wa-2");
    assert_eq!(resolved.wikitext_anchor, "L3:C0-L3:C433");

    assert_eq!(id_from_selector("#wa-1"), Some("wa-1".into()));
    assert_eq!(
        id_from_selector("#pane-new > div:nth-of-type(2)"),
        Some("pane-new".into())
    );
    assert_eq!(id_from_selector("div > p"), None);
}

#[test]
fn ac6_text_range_payload_resolves_with_selection_context() {
    // target = {type: "text-range", text, selector, start: {selector, path,
    // offset}, end: {...}} — path navigates childNodes; our artifact ids
    // make the common-ancestor selector id-rooted.
    let payload = CommentPrompt {
        prompt: "Check this figure.".into(),
        selector: "#wa-1".into(),
        tag: "text".into(),
        text: "several hundred grammar".into(),
        target: Some(CommentTarget::TextRange {
            text: "several hundred grammar".into(),
            selector: "#wa-1".into(),
            start: RangeBoundary {
                selector: "#wa-1".into(),
                path: vec![2],
                offset: 14,
            },
            end: RangeBoundary {
                selector: "#wa-1".into(),
                path: vec![2],
                offset: 36,
            },
        }),
    };
    let resolved = resolve_comment(&payload, &table()).expect("resolves");
    assert_eq!(resolved.element_id, "wa-1");
    assert_eq!(resolved.wikitext_anchor, "L3:C0-L3:C410");
    assert_eq!(
        resolved.selected_text.as_deref(),
        Some("several hundred grammar")
    );
}

#[test]
fn ac6_evidence_rail_comment_maps_to_ledger_quote_id() {
    let payload = CommentPrompt {
        prompt: "Quote looks trimmed.".into(),
        selector: "#ev-1".into(),
        tag: "div".into(),
        text: "…".into(),
        target: None,
    };
    let resolved = resolve_comment(&payload, &table()).expect("resolves");
    assert_eq!(resolved.wikitext_anchor, "ledger:Q1");
}

/// MVP-2 A.1.1 — deletion anchors: comments on old-pane blocks (removed
/// content, including pure deletions) resolve to `base:`-prefixed ranges
/// into the BASE wikitext, for element and text-range payloads alike.
#[test]
fn ac6_old_pane_comments_resolve_to_base_wikitext_anchors() {
    let table = vec![
        ("wa-1".into(), "base:L12:C0-L12:C310".into()),
        ("wa-2".into(), "L12:C0-L12:C298".into()),
    ];
    let element = CommentPrompt {
        prompt: "This removal drops the attribution.".into(),
        selector: "div#wa-1".into(),
        tag: "div".into(),
        text: "The keep was removed in 1970…".into(),
        target: None,
    };
    let resolved = resolve_comment(&element, &table).expect("resolves");
    assert_eq!(resolved.element_id, "wa-1");
    assert_eq!(resolved.wikitext_anchor, "base:L12:C0-L12:C310");

    let text_range = CommentPrompt {
        prompt: "This clause is sourced — keep it.".into(),
        selector: "#wa-1".into(),
        tag: "text".into(),
        text: "removed in 1970".into(),
        target: Some(CommentTarget::TextRange {
            text: "removed in 1970".into(),
            selector: "#wa-1".into(),
            start: RangeBoundary {
                selector: "#wa-1".into(),
                path: vec![1],
                offset: 8,
            },
            end: RangeBoundary {
                selector: "#wa-1".into(),
                path: vec![1],
                offset: 23,
            },
        }),
    };
    let resolved = resolve_comment(&text_range, &table).expect("resolves");
    assert_eq!(resolved.wikitext_anchor, "base:L12:C0-L12:C310");
    assert_eq!(resolved.selected_text.as_deref(), Some("removed in 1970"));
}

#[test]
fn ac6_unknown_id_and_idless_selector_fail_loudly() {
    let bad_id = CommentPrompt {
        prompt: "x".into(),
        selector: "#wa-99".into(),
        tag: "div".into(),
        text: String::new(),
        target: None,
    };
    assert!(resolve_comment(&bad_id, &table()).is_err());

    let idless = CommentPrompt {
        prompt: "x".into(),
        selector: "div > p:nth-of-type(3)".into(),
        tag: "p".into(),
        text: String::new(),
        target: None,
    };
    assert!(resolve_comment(&idless, &table()).is_err());
}
