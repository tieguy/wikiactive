//! Anchor resolution — lavish comment payloads → wikitext anchors / ledger
//! quote ids.
//!
//! lavish (vendored normative reference: `vendor/lavish-axi-0.1.78.tgz`,
//! `dist/cli.mjs`) emits, per comment:
//!
//! - **element comments**: `{prompt, selector, tag, text}` where `selector`
//!   is id-rooted CSS (`#wa-2`, `#ev-1`, or `tag#id > …`); `selector(el)`
//!   stops at the first element with an `id`.
//! - **text-range comments**: additionally `target = {type: "text-range",
//!   text, selector, start: {selector, path, offset},
//!   end: {selector, path, offset}}` where `path` navigates `childNodes`
//!   indices from the selector's element and `offset` is a text-node
//!   character offset.
//!
//! wikiactive artifacts put `data-wiki-anchor` on every annotatable block
//! and embed `#wa-anchor-table` (id → anchor), so resolution is:
//! id from selector → anchor table → wikitext range or `ledger:Q<n>`.
//! The selected text travels with the resolution so the agent's reply can
//! quote the exact words (mis-maps become visible, per the plan's risk
//! mitigation).

use serde::Deserialize;
use serde::Serialize;

/// A lavish text-range boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeBoundary {
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub path: Vec<usize>,
    #[serde(default)]
    pub offset: usize,
}

/// A lavish comment target (subset actually emitted for artifacts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CommentTarget {
    TextRange {
        text: String,
        #[serde(default)]
        selector: String,
        start: RangeBoundary,
        end: RangeBoundary,
    },
    #[serde(other)]
    Other,
}

/// One parsed comment prompt from a poll response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentPrompt {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub target: Option<CommentTarget>,
}

/// A resolved comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedComment {
    /// The artifact element id the selector rooted at (`wa-N` / `ev-N`).
    pub element_id: String,
    /// `L..:C..-L..:C..` wikitext range or `ledger:Q<n>`.
    pub wikitext_anchor: String,
    /// For text-range comments: the exact selected words.
    pub selected_text: Option<String>,
    /// The comment text.
    pub comment: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AnchorError {
    #[error("selector {0:?} does not root at an artifact element id")]
    NoId(String),
    #[error("element id {0:?} not in anchor table")]
    UnknownId(String),
}

/// Extract the id segment from a lavish selector: the first `#name` token,
/// trimmed of any further combinator (e.g. `#wa-2 > span` → `wa-2`,
/// `div#ev-1:nth-of-type(2)` → `ev-1`).
#[must_use]
pub fn id_from_selector(selector: &str) -> Option<String> {
    let mut current = String::new();
    let mut in_id = false;
    let mut id_seen = false;
    for ch in selector.chars() {
        match ch {
            '#' if !id_seen => {
                in_id = true;
                id_seen = true;
            }
            _ if in_id => {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    current.push(ch);
                } else {
                    in_id = false;
                }
            }
            _ => {}
        }
    }
    if current.is_empty() {
        None
    } else {
        Some(current)
    }
}

/// Resolve one comment prompt against an anchor table.
///
/// # Errors
/// [`AnchorError::NoId`] when the selector has no id root (the artifact
/// author must give annotatable blocks ids), or [`AnchorError::UnknownId`]
/// when the id is absent from the table.
pub fn resolve_comment(
    comment: &CommentPrompt,
    anchor_table: &[(String, String)],
) -> Result<ResolvedComment, AnchorError> {
    let selector = comment
        .target
        .as_ref()
        .and_then(|t| match t {
            CommentTarget::TextRange { selector, .. } if !selector.is_empty() => {
                Some(selector.clone())
            }
            CommentTarget::TextRange { .. } | CommentTarget::Other => None,
        })
        .unwrap_or_else(|| comment.selector.clone());
    let id = id_from_selector(&selector).ok_or_else(|| AnchorError::NoId(selector.clone()))?;
    let anchor = anchor_table
        .iter()
        .find(|(eid, _)| *eid == id)
        .map(|(_, a)| a.clone())
        .ok_or_else(|| AnchorError::UnknownId(id.clone()))?;
    let selected_text = comment.target.as_ref().and_then(|t| match t {
        CommentTarget::TextRange { text, .. } => Some(text.clone()),
        CommentTarget::Other => None,
    });
    Ok(ResolvedComment {
        element_id: id,
        wikitext_anchor: anchor,
        selected_text,
        comment: comment.prompt.clone(),
    })
}

/// Resolve a batch of comments; a failed resolution stops the batch (the
/// model must see the failure, never skip silently).
///
/// # Errors
/// The first resolution failure.
pub fn resolve_comments(
    comments: &[CommentPrompt],
    anchor_table: &[(String, String)],
) -> Result<Vec<ResolvedComment>, AnchorError> {
    comments
        .iter()
        .map(|c| resolve_comment(c, anchor_table))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        AnchorError, CommentPrompt, CommentTarget, RangeBoundary, id_from_selector,
        resolve_comment, resolve_comments,
    };

    fn table() -> Vec<(String, String)> {
        vec![
            ("wa-1".into(), "L3:C0-L3:C410".into()),
            ("wa-2".into(), "L3:C0-L3:C433".into()),
            ("ev-1".into(), "ledger:Q1".into()),
        ]
    }

    #[test]
    fn id_extraction_matches_lavish_selector_shapes() {
        assert_eq!(id_from_selector("#wa-2"), Some("wa-2".into()));
        assert_eq!(
            id_from_selector("#pane-new > div:nth-of-type(2)"),
            Some("pane-new".into())
        );
        assert_eq!(id_from_selector("div#ev-1"), Some("ev-1".into()));
        assert_eq!(id_from_selector("div > p"), None);
        assert_eq!(id_from_selector(""), None);
    }

    #[test]
    fn element_comment_resolves_to_wikitext_range() {
        let c = CommentPrompt {
            prompt: "tighten this".into(),
            selector: "#wa-2".into(),
            tag: "div".into(),
            text: "I contribute mainly to the English-language Wikipedia…".into(),
            target: None,
        };
        let r = resolve_comment(&c, &table()).unwrap();
        assert_eq!(r.element_id, "wa-2");
        assert_eq!(r.wikitext_anchor, "L3:C0-L3:C433");
        assert!(r.selected_text.is_none());
    }

    #[test]
    fn text_range_comment_resolves_to_block_anchor_with_selection() {
        let c = CommentPrompt {
            prompt: "check this number".into(),
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
        let r = resolve_comment(&c, &table()).unwrap();
        assert_eq!(r.element_id, "wa-1");
        assert_eq!(r.selected_text.as_deref(), Some("several hundred grammar"));
    }

    #[test]
    fn evidence_comment_maps_to_ledger_quote() {
        let c = CommentPrompt {
            prompt: "quote looks trimmed".into(),
            selector: "#ev-1".into(),
            tag: "div".into(),
            text: "…".into(),
            target: None,
        };
        let r = resolve_comment(&c, &table()).unwrap();
        assert_eq!(r.wikitext_anchor, "ledger:Q1");
    }

    #[test]
    fn unknown_id_fails_loudly() {
        let c = CommentPrompt {
            prompt: "x".into(),
            selector: "#wa-99".into(),
            tag: "div".into(),
            text: String::new(),
            target: None,
        };
        assert!(matches!(
            resolve_comment(&c, &table()),
            Err(AnchorError::UnknownId(_))
        ));
    }

    #[test]
    fn batch_resolution_all_or_error() {
        let comments = vec![
            CommentPrompt {
                prompt: "a".into(),
                selector: "#wa-1".into(),
                tag: "div".into(),
                text: String::new(),
                target: None,
            },
            CommentPrompt {
                prompt: "b".into(),
                selector: "div > p".into(),
                tag: "p".into(),
                text: String::new(),
                target: None,
            },
        ];
        assert!(resolve_comments(&comments, &table()).is_err());
    }
}
