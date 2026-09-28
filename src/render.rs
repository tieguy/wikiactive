//! Review artifact renderer — two-pane rendered diff with evidence rail.
//!
//! One artifact per article session, regenerated in place each round (live
//! reload; per-round revisions legend via `data-lavish-revisions`). Every
//! changed block carries `id="wa-N"` + `data-wiki-anchor` (a wikitext
//! `L<line>:C<col>` range); every evidence card carries `id="ev-N"` +
//! `data-wiki-anchor="ledger:Q<n>"` so a comment on a citation maps to the
//! ledger entry. An embedded JSON anchor table maps ids to ranges; comments
//! resolve through it (see [`crate::anchors`]).
//!
//! [`render`] runs [`crate::checks::gate`] as its **mandatory pre-flight**
//! (AC.11): a blocked gate writes no artifact — the error carries every
//! reason. Render itself is pure/offline: HTML arrives as strings (live
//! Parsoid via [`crate::wikipedia`], recorded fixtures in tests).

use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde::Serialize;

use crate::checks::gate::GateInput;
use crate::checks::gate::GateVerdict;
use crate::checks::linter::LinterConfig;
use crate::ledger::Ledger;
use crate::session::Finding;

/// One revisions-registry entry (lavish `data-lavish-revisions` shape:
/// `{id,label,timestamp,summary}`, oldest first; the browser legend lists at
/// most 6 and silently ignores malformed registries — rounds beyond 6 stay
/// recorded here but unlisted in the legend; noted in PLAYBOOK.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct RevisionEntry {
    pub id: String,
    pub label: String,
    pub timestamp: String,
    pub summary: String,
}

/// One anchor-table row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorEntry {
    /// Artifact element id (`wa-N` or `ev-N`).
    pub element_id: String,
    /// `data-wiki-anchor` value, one of three families:
    /// `L..:C..-L..:C..` — range in the PROPOSED wikitext (new-side blocks);
    /// `base:L..:C..-L..:C..` — range in the BASE wikitext (old-side blocks:
    /// changed-pair old text and pure deletions — deletion anchors);
    /// `ledger:Q<n>` — a ledger quote (evidence cards).
    pub wikitext_anchor: String,
}

/// Render inputs.
pub struct RenderInput<'a> {
    pub article: String,
    pub round: u32,
    pub base_wikitext: &'a str,
    pub proposed_wikitext: &'a str,
    /// Parsoid HTML of the base wikitext.
    pub base_html: &'a str,
    /// Parsoid HTML of the proposed wikitext.
    pub proposed_html: &'a str,
    pub findings: &'a [Finding],
    pub ledger: &'a Ledger,
    pub linter_config: &'a LinterConfig,
    /// Paraphrase-gate thresholds (`rules/paraphrase.toml`, MVP-2 A.2.3).
    pub paraphrase_config: &'a crate::checks::paraphrase::ParaphraseConfig,
    /// Cumulative revisions registry (previous rounds first, current last).
    pub revisions: Vec<RevisionEntry>,
}

/// Render outputs.
#[derive(Debug, Clone)]
pub struct RenderOutput {
    pub artifact_html: String,
    pub anchor_table: Vec<AnchorEntry>,
    /// Findings with `rendered_span_id` back-filled.
    pub updated_findings: Vec<Finding>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("gate blocked render (no artifact written): {0}")]
    GateBlocked(String),
    #[error("html parse: {0}")]
    Html(String),
}

/// Run the gate then render the artifact. On gate block, nothing is
/// produced (the caller writes no file).
///
/// # Errors
/// [`RenderError::GateBlocked`] with every reason, or an HTML parse failure.
#[allow(clippy::too_many_lines)]
pub fn render(input: &RenderInput) -> Result<RenderOutput, RenderError> {
    // 1. Mandatory pre-flight (AC.11).
    let verdict = crate::checks::gate::run_gate(&GateInput {
        ledger: input.ledger,
        findings: input.findings,
        base_wikitext: input.base_wikitext,
        proposed_wikitext: input.proposed_wikitext,
        linter_config: input.linter_config,
        paraphrase_config: input.paraphrase_config,
    });
    let GateVerdict {
        blocked, reasons, ..
    } = verdict;
    if blocked {
        return Err(RenderError::GateBlocked(
            crate::checks::gate::format_reasons(&reasons),
        ));
    }

    // 2. Extract content blocks from both HTML sides.
    let base_blocks = extract_blocks(input.base_html).map_err(RenderError::Html)?;
    let proposed_blocks = extract_blocks(input.proposed_html).map_err(RenderError::Html)?;

    // 3. Block-level diff.
    let base_texts: Vec<&str> = base_blocks.iter().map(|b| b.text.as_str()).collect();
    let prop_texts: Vec<&str> = proposed_blocks.iter().map(|b| b.text.as_str()).collect();
    let diff = similar::TextDiff::from_slices(&base_texts, &prop_texts);

    // 4. Walk the diff; assign wa-N ids + anchors. NEW-side blocks (pure
    //    insertions, changed pairs' new half) anchor in the PROPOSED
    //    wikitext; OLD-side blocks (changed pairs' old half, pure
    //    deletions) anchor in the BASE wikitext with a `base:` prefix, so
    //    comments on removed content resolve (deletion anchors).
    //    Consecutive Delete+Insert runs pair into ONE changed block whose
    //    inside is a WORD-LEVEL diff (<del>/<ins> runs), so a one-word
    //    change is visible inside a long paragraph.
    let round_id = format!("r{}", input.round);
    let mut anchor_table: Vec<AnchorEntry> = Vec::new();
    // Single-column diff (operator round-5 layout request: Word-style
    // inline diff, evidence rail as a right-hand column).
    let mut diff_html = String::new();
    let mut counter = 0usize;
    let mut prop_idx = 0usize;
    let mut pending_deletes: Vec<PendingDelete<'_>> = Vec::new();
    let mut skipped_equal = 0usize;

    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Equal => {
                // Review surface shows ONLY changed blocks; unchanged runs
                // collapse to a count separator (operator request: "show
                // only the changed paragraphs").
                flush_pure_deletes(
                    &mut pending_deletes,
                    &mut diff_html,
                    input.base_wikitext,
                    &round_id,
                    &mut counter,
                    &mut anchor_table,
                );
                skipped_equal += 1;
                prop_idx += 1;
            }
            similar::ChangeTag::Delete => {
                emit_ctx_sep(&mut diff_html, &mut skipped_equal);
                let idx = change.old_index().unwrap_or(0);
                let kind = base_blocks
                    .get(idx)
                    .map_or(BlockKind::Paragraph, |b| b.kind);
                pending_deletes.push(PendingDelete {
                    text: change.value(),
                    kind,
                    block_idx: idx,
                });
            }
            similar::ChangeTag::Insert => {
                emit_ctx_sep(&mut diff_html, &mut skipped_equal);
                let new_text = change.value();
                let new_kind = change
                    .new_index()
                    .map_or(BlockKind::Paragraph, |i| proposed_blocks[i].kind);
                let (old_text, _old_kind, old_block_idx) = if pending_deletes.is_empty() {
                    (String::new(), BlockKind::Paragraph, 0)
                } else {
                    let texts: Vec<&str> = pending_deletes.iter().map(|d| d.text).collect();
                    let first = pending_deletes[0];
                    pending_deletes.clear();
                    (texts.join("\n\n"), first.kind, first.block_idx)
                };
                prop_idx += 1;
                if old_text.is_empty() {
                    // Pure insertion: one id, anchored in the proposed wikitext.
                    counter += 1;
                    let id = format!("wa-{counter}");
                    let anchor = locate_block_anchor(input.proposed_wikitext, new_text)
                        .unwrap_or_else(|| next_line_anchor(input.proposed_wikitext, prop_idx - 1));
                    anchor_table.push(AnchorEntry {
                        element_id: id.clone(),
                        wikitext_anchor: anchor.clone(),
                    });
                    diff_html.push_str(&block_html(
                        Some(&id),
                        Some(&anchor),
                        new_text,
                        new_kind,
                        "add",
                        &round_id,
                    ));
                } else {
                    // Changed pair → ONE combined block (Word-style inline
                    // diff). The new side's id anchors the block; the old
                    // side's id rides the first <del> run (or an empty
                    // marker span) so comments on removed wording resolve.
                    let combined = inline_word_diff(&old_text, new_text);
                    counter += 1;
                    let old_id = format!("wa-{counter}");
                    let old_anchor = format!(
                        "base:{}",
                        locate_block_anchor(input.base_wikitext, &old_text).unwrap_or_else(|| {
                            next_line_anchor(input.base_wikitext, old_block_idx)
                        })
                    );
                    anchor_table.push(AnchorEntry {
                        element_id: old_id.clone(),
                        wikitext_anchor: old_anchor.clone(),
                    });
                    counter += 1;
                    let new_id = format!("wa-{counter}");
                    let new_anchor = locate_block_anchor(input.proposed_wikitext, new_text)
                        .unwrap_or_else(|| next_line_anchor(input.proposed_wikitext, prop_idx - 1));
                    anchor_table.push(AnchorEntry {
                        element_id: new_id.clone(),
                        wikitext_anchor: new_anchor.clone(),
                    });
                    let combined = if combined.contains("<del>") {
                        combined.replacen(
                            "<del>",
                            &format!("<del id=\"{old_id}\" data-wiki-anchor=\"{old_anchor}\">"),
                            1,
                        )
                    } else {
                        format!(
                            "<span id=\"{old_id}\" data-wiki-anchor=\"{old_anchor}\"></span>\
                             {combined}"
                        )
                    };
                    let (tag, tag_title) = block_tag(Some(&new_id), Some(&new_anchor), new_kind);
                    let _ = writeln!(
                        diff_html,
                        "<div class=\"block change {}\" id=\"{new_id}\" \
                         data-wiki-anchor=\"{new_anchor}\" \
                         data-lavish-revision=\"{round_id}\">\
                         <span class=\"anchor-tag\"{tag_title}>{tag}</span>{combined}</div>",
                        new_kind.css(),
                    );
                }
            }
        }
    }
    if !pending_deletes.is_empty() {
        emit_ctx_sep(&mut diff_html, &mut skipped_equal);
    }
    flush_pure_deletes(
        &mut pending_deletes,
        &mut diff_html,
        input.base_wikitext,
        &round_id,
        &mut counter,
        &mut anchor_table,
    );

    // 5. Evidence rail: one card per finding (gate guarantees quotes
    //    resolve into the ledger).
    let mut evidence_html = String::new();
    let mut updated_findings = input.findings.to_vec();
    for (idx, finding) in input.findings.iter().enumerate() {
        let ev_id = format!("ev-{}", idx + 1);
        let primary_quote_id = finding.evidence[0].clone();
        evidence_html.push_str(&evidence_card(&ev_id, finding, input.ledger));
        anchor_table.push(AnchorEntry {
            element_id: ev_id.clone(),
            wikitext_anchor: format!("ledger:{primary_quote_id}"),
        });
        updated_findings[idx].rendered_span_id = Some(ev_id);
    }

    // 6. Assemble artifact.
    let anchor_json = serde_json::to_string_pretty(&anchor_table).unwrap_or_default();
    let revisions_json =
        serde_json::to_string(&input.revisions).unwrap_or_else(|_| "[]".to_string());
    let artifact = assemble_artifact(&AssembleArgs {
        article: &input.article,
        round: input.round,
        diff_html: &diff_html,
        evidence_html: &evidence_html,
        anchor_json: &anchor_json,
        revisions_json: &revisions_json,
        round_id: &round_id,
        wiki_links: include_str!("../vendor/enwiki-link-affordances.css"),
    });

    Ok(RenderOutput {
        artifact_html: artifact,
        anchor_table,
        updated_findings,
    })
}

/// One content block: its text plus its structural kind, so the review
/// panes can SHOW formatting (a heading renders as a heading, a bullet as
/// a bullet) instead of flattening the page to prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub text: String,
    pub kind: BlockKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    ListItem,
    Quote,
    Pre,
    Def,
}

impl BlockKind {
    fn of(element: scraper::ElementRef<'_>) -> Self {
        match element.value().name() {
            "h2" => Self::Heading(2),
            "h3" => Self::Heading(3),
            "h4" => Self::Heading(4),
            "p" => Self::Paragraph,
            "li" => Self::ListItem,
            "blockquote" => Self::Quote,
            "pre" => Self::Pre,
            _ => Self::Def,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Heading(2) => "h2 heading",
            Self::Heading(3) => "h3 heading",
            Self::Heading(_) => "heading",
            Self::Paragraph => "paragraph",
            Self::ListItem => "list item",
            Self::Quote => "quote",
            Self::Pre => "pre",
            Self::Def => "definition",
        }
    }

    fn css(self) -> &'static str {
        match self {
            Self::Heading(_) => "heading",
            Self::ListItem => "listitem",
            Self::Quote => "quote",
            Self::Pre => "pre",
            Self::Paragraph | Self::Def => "para",
        }
    }
}

/// Extract top-level content blocks (text + kind) from Parsoid HTML:
/// headings, paragraphs, list items, in document order. `<style>`/`<script>`
/// descendant text (e.g. the `.mw-parser-output` CSS Parsoid embeds in the
/// References section) is excluded — it is not content.
fn extract_blocks(html: &str) -> Result<Vec<Block>, String> {
    let doc = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("h2, h3, h4, p, li, blockquote, pre, dd")
        .map_err(|e| format!("selector: {e:?}"))?;
    let mut blocks = Vec::new();
    for element in doc.select(&selector) {
        let text = collapse_ws(&text_of(element));
        if !text.is_empty() {
            blocks.push(Block {
                text,
                kind: BlockKind::of(element),
            });
        }
    }
    Ok(blocks)
}

/// Sentinels wrapping anchor text so the panes can SHOW what is linked
/// (Parsoid wraps internal links in `<a rel="mw:WikiLink">`; plain
/// `element.text()` drops the wrapper — operator review catch: "no visual
/// indication of the link").
const LINK_START: char = '\u{1}';
const LINK_END: char = '\u{2}';
const EM_START: char = '\u{3}';
const EM_END: char = '\u{4}';
const STRONG_START: char = '\u{5}';
const STRONG_END: char = '\u{6}';

/// Strip link sentinels (for anchor matching, which wants plain text).
fn strip_link_marks(text: &str) -> String {
    const SENTINELS: [char; 6] = [
        LINK_START,
        LINK_END,
        EM_START,
        EM_END,
        STRONG_START,
        STRONG_END,
    ];
    text.chars().filter(|c| !SENTINELS.contains(c)).collect()
}

/// Visible text of an element, skipping style/script subtrees. Anchor text
/// is wrapped in [`LINK_START`]/[`LINK_END`] sentinels for link-aware
/// rendering.
fn text_of(element: scraper::ElementRef<'_>) -> String {
    use scraper::node::Node;
    let mut out = String::new();
    for child in element.children() {
        match child.value() {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) if e.name() == "a" => {
                // Only true content links: mw:WikiLink (internal) and
                // mw:ExtLink (external). Citation backlinks (<a href="#cite_note…">)
                // are navigation, not links — marking them glues a dotted
                // underline onto the preceding word (operator catch:
                // "restaurants" looked linked).
                let is_content_link = e
                    .attr("rel")
                    .is_some_and(|r| r.contains("mw:WikiLink") || r.contains("mw:ExtLink"));
                if let Some(inner) = scraper::ElementRef::wrap(child) {
                    if is_content_link {
                        out.push(LINK_START);
                        out.push_str(&text_of(inner));
                        out.push(LINK_END);
                    } else {
                        out.push_str(&text_of(inner));
                    }
                }
            }
            Node::Element(e) if matches!(e.name(), "i" | "em") => {
                if let Some(inner) = scraper::ElementRef::wrap(child) {
                    out.push(EM_START);
                    out.push_str(&text_of(inner));
                    out.push(EM_END);
                }
            }
            Node::Element(e) if matches!(e.name(), "b" | "strong") => {
                if let Some(inner) = scraper::ElementRef::wrap(child) {
                    out.push(STRONG_START);
                    out.push_str(&text_of(inner));
                    out.push(STRONG_END);
                }
            }
            Node::Element(e) if e.name() != "style" && e.name() != "script" => {
                if let Some(inner) = scraper::ElementRef::wrap(child) {
                    out.push_str(&text_of(inner));
                }
            }
            _ => {}
        }
    }
    out
}

/// Render sentinel-marked text to pane HTML: plain text escaped, marked
/// spans wrapped in `<span class="wl">` (dotted underline in CSS).
fn marked_text_to_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            LINK_START => out.push_str("<span class=\"wl\">"),
            LINK_END => out.push_str("</span>"),
            EM_START => out.push_str("<em>"),
            EM_END => out.push_str("</em>"),
            STRONG_START => out.push_str("<strong>"),
            STRONG_END => out.push_str("</strong>"),
            other => out.push_str(&esc(&other.to_string())),
        }
    }
    out
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out.trim().to_string()
}

/// One-line collapsed-context separator between change groups: keeps the
/// reviewer oriented without rendering unchanged blocks.
fn emit_ctx_sep(out: &mut String, skipped: &mut usize) {
    if *skipped > 0 {
        let _ = writeln!(
            out,
            "<div class=\"ctx-sep\">⋯ {} unchanged block{} ⋯</div>",
            skipped,
            if *skipped == 1 { "" } else { "s" }
        );
        *skipped = 0;
    }
}

/// A buffered deletion awaiting its fate: paired into a changed block, or
/// flushed as a pure deletion.
#[derive(Clone, Copy)]
struct PendingDelete<'a> {
    text: &'a str,
    kind: BlockKind,
    /// Position among the base blocks (fallback anchor line).
    block_idx: usize,
}

/// Emit buffered deletes that never paired with an insert (pure
/// deletions). Each gets its own `wa-N` id anchored in the BASE wikitext
/// (`base:` prefix) so operator comments on removed content resolve —
/// deletion anchors.
fn flush_pure_deletes(
    pending: &mut Vec<PendingDelete<'_>>,
    out: &mut String,
    base_wikitext: &str,
    round_id: &str,
    counter: &mut usize,
    anchor_table: &mut Vec<AnchorEntry>,
) {
    for d in pending.drain(..) {
        let n = *counter + 1;
        *counter = n;
        let id = format!("wa-{n}");
        let anchor = format!(
            "base:{}",
            locate_block_anchor(base_wikitext, d.text)
                .unwrap_or_else(|| next_line_anchor(base_wikitext, d.block_idx))
        );
        anchor_table.push(AnchorEntry {
            element_id: id.clone(),
            wikitext_anchor: anchor.clone(),
        });
        out.push_str(&block_html(
            Some(&id),
            Some(&anchor),
            d.text,
            d.kind,
            "del",
            round_id,
        ));
    }
}
/// Split sentinel-marked text into whitespace-separated words carrying a
/// linked flag.
fn marked_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Word-level inline diff between the old and new sides of a changed block
/// pair, in ONE combined flow (Word-style "all markup"): deletions render
/// as `<del>` runs and insertions as `<ins>` runs inline, equal words once.
/// (Operator round-5 layout request: single-column diff, not two panes.)
fn inline_word_diff(old: &str, new: &str) -> String {
    let old_words = marked_words(old);
    let new_words = marked_words(new);
    let old_plain: Vec<String> = old_words.iter().map(|w| strip_link_marks(w)).collect();
    let new_plain: Vec<String> = new_words.iter().map(|w| strip_link_marks(w)).collect();
    let old_texts: Vec<&str> = old_plain.iter().map(String::as_str).collect();
    let new_texts: Vec<&str> = new_plain.iter().map(String::as_str).collect();
    let diff = similar::TextDiff::from_slices(&old_texts, &new_texts);
    let render = |word: &str| -> String { marked_text_to_html(word) };
    let mut html = String::new();
    let mut del_run = String::new();
    let mut ins_run = String::new();
    for change in diff.iter_all_changes() {
        // Formatting sentinels render PER SIDE so an edit that only removes
        // formatting still shows the old formatting in the deleted run
        // (operator catch: "no visible change").
        let piece_old = match change.old_index() {
            Some(i) => render(&old_words[i]),
            None => match change.new_index() {
                Some(j) => render(&new_words[j]),
                None => esc(change.value()),
            },
        };
        let glue = " ";
        match change.tag() {
            similar::ChangeTag::Delete => {
                del_run.push_str(&piece_old);
                del_run.push_str(glue);
            }
            similar::ChangeTag::Insert => {
                let piece_new = match change.new_index() {
                    Some(j) => render(&new_words[j]),
                    None => piece_old.clone(),
                };
                ins_run.push_str(&piece_new);
                ins_run.push_str(glue);
            }
            similar::ChangeTag::Equal => {
                if !del_run.is_empty() {
                    let trimmed = del_run.trim_end().to_string();
                    let _ = write!(html, "<del>{trimmed}</del> ");
                    del_run.clear();
                }
                if !ins_run.is_empty() {
                    let trimmed = ins_run.trim_end().to_string();
                    let _ = write!(html, "<ins>{trimmed}</ins> ");
                    ins_run.clear();
                }
                html.push_str(&piece_old);
                html.push(' ');
            }
        }
    }
    if !del_run.is_empty() {
        let trimmed = del_run.trim_end().to_string();
        let _ = write!(html, "<del>{trimmed}</del>");
    }
    if !ins_run.is_empty() {
        let trimmed = ins_run.trim_end().to_string();
        let _ = write!(html, "<ins>{trimmed}</ins>");
    }
    html
}

/// Block anchor tag in reviewer language: kind plus wikitext line
/// ("paragraph · line 11"; "original line 11" for old-side base anchors).
/// The technical wa-N + range string becomes a hover tooltip only — raw
/// anchor text in the visible UI is cruft (operator round-4 catch).
fn block_tag(id: Option<&str>, anchor: Option<&str>, kind: BlockKind) -> (String, String) {
    let label = kind.label();
    let visible = match anchor.and_then(human_line) {
        Some(line) => format!("{label} · {line}"),
        None => label.to_string(),
    };
    let title = match (id, anchor) {
        (Some(i), Some(a)) => format!(" title=\"{i} · {a}\""),
        _ => String::new(),
    };
    (visible, title)
}

/// "L11:…" / "base:L11:…" anchor → "line 11" / "original line 11".
fn human_line(anchor: &str) -> Option<String> {
    let (base, rest) = match anchor.strip_prefix("base:") {
        Some(r) => (true, r),
        None => (false, anchor),
    };
    let line = rest
        .split(&[':', '-'][..])
        .next()?
        .strip_prefix('L')?
        .parse::<usize>()
        .ok()?;
    Some(if base {
        format!("original line {line}")
    } else {
        format!("line {line}")
    })
}

/// HTML-escape text.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Wrap one block for a pane.
fn block_html(
    id: Option<&str>,
    anchor: Option<&str>,
    text: &str,
    kind: BlockKind,
    class: &str,
    round_id: &str,
) -> String {
    let id_attr = id.map_or_else(String::new, |i| format!(" id=\"{i}\""));
    let anchor_attr = anchor.map_or_else(String::new, |a| format!(" data-wiki-anchor=\"{a}\""));
    let rev_attr = id.map_or_else(String::new, |_| {
        format!(" data-lavish-revision=\"{round_id}\"")
    });
    let (tag, tag_title) = block_tag(id, anchor, kind);
    format!(
        "<div class=\"block {class} {}\"{id_attr}{anchor_attr}>{rev_attr}<span class=\"anchor-tag\"{tag_title}>{tag}</span>{}</div>\n",
        kind.css(),
        marked_text_to_html(text),
    )
}
/// Locate a block's wikitext anchor: the line range (1-based, char cols)
/// whose content matches the block text's opening words. Returns
/// `L<s>:C<col>-L<e>:C<col2>`. Also used by the gate to span-locate claim
/// prose in the proposed wikitext (MVP-2 A.2.1).
///
/// Parsoid renders maintenance templates and ref backlinks as bracketed
/// annotations glued to words ("Ohio[citation needed]", "1870.[7]"): strip
/// them before matching, and try progressively shorter prefixes (6→4
/// words) — one line still containing a 4-word run is strong evidence
/// (live L2 round-1 catch: the moved {{Cn}} poisoned the 6-word prefix and
/// the block mis-located to its fallback line).
pub(crate) fn locate_block_anchor(wikitext: &str, block_text: &str) -> Option<String> {
    static ANNOTATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[[^\]]*\]").expect("valid regex"));
    let norm = |s: &str| {
        s.to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let block_norm = norm(&ANNOTATION.replace_all(&strip_link_marks(block_text), " "));
    if block_norm.is_empty() {
        return None;
    }
    let words: Vec<&str> = block_norm.split(' ').collect();
    let lines: Vec<String> = wikitext.split('\n').map(norm).collect();
    for take in (4..=6).rev() {
        if words.len() < take {
            continue;
        }
        let prefix = words[..take].join(" ");
        if prefix.is_empty() {
            continue;
        }
        for (line_no, line_norm) in lines.iter().enumerate() {
            if line_norm.contains(&prefix) {
                let col_end = wikitext
                    .split('\n')
                    .nth(line_no)
                    .map_or(0, |l| l.chars().count());
                return Some(format!("L{}:C0-L{}:C{}", line_no + 1, line_no + 1, col_end));
            }
        }
    }
    None
}

/// Anchor for the n-th surviving proposed line (fallback for unmatched
/// blocks). Always non-empty per AC.5: a blank line would yield a
/// zero-width range, so step to the nearest non-blank line (live L2
/// round-1 catch: the reflist pair anchored L16:C0-L16:C0 on a blank).
fn next_line_anchor(wikitext: &str, prop_idx: usize) -> String {
    let lines: Vec<&str> = wikitext.lines().collect();
    let total = lines.len().max(1);
    let start = (prop_idx + 1).clamp(1, total);
    let non_blank = |i: usize| lines.get(i - 1).is_some_and(|l| !l.trim().is_empty());
    let line = (start..=total)
        .find(|i| non_blank(*i))
        .or_else(|| (1..start).rev().find(|i| non_blank(*i)))
        .unwrap_or(start);
    let col_end = lines.get(line - 1).map_or(0, |l| l.chars().count());
    format!("L{line}:C0-L{line}:C{col_end}")
}

/// One evidence card, in reviewer language: the verbatim excerpt first
/// (the reviewer verifies quote→prose mapping), the source under its real
/// title with clickable url + archive links, then the finding note and fix.
/// Internal ids (Q/S/F, ledger refs) live only in element ids and
/// data-wiki-anchor attributes — never in visible text (operator L2
/// round-3 catch: "Q1/F1/S1/locator-verified are internal jargon").
fn evidence_card(ev_id: &str, finding: &Finding, ledger: &Ledger) -> String {
    let mut quotes_html = String::new();
    let mut sources_html = String::new();
    let mut consulted_html = String::new();
    let evidence_source_ids: Vec<&str> = finding
        .evidence
        .iter()
        .filter_map(|qid| ledger.quote(qid).map(|q| q.source_id.as_str()))
        .collect();
    for source in &ledger.sources {
        // Every consulted source appears with a link and fetch status —
        // the reviewer must be able to chase paywalled/403 sources from
        // the UI (operator round-6 catch: "not enough detail to google
        // them, no link to the 403").
        if evidence_source_ids.contains(&source.id.as_str()) {
            continue;
        }
        let status = match (&source.fetched_text, source.fetched_via.as_deref()) {
            (Some(t), Some(via)) if !t.is_empty() && via.starts_with("operator") => {
                "fetched (operator-provided)"
            }
            (Some(t), _) if !t.is_empty() => "fetched",
            _ => "not fetched (access failed)",
        };
        let _ = writeln!(
            consulted_html,
            "<p class=\"src\"><a href=\"{url}\">{text}</a> — {status}</p>",
            url = esc(&source.url),
            text = esc(&source_link_text(source)),
            status = status,
        );
    }
    for qid in &finding.evidence {
        if let Some(quote) = ledger.quote(qid) {
            if let Some(source) = ledger.sources.iter().find(|s| s.id == quote.source_id) {
                let is_archive_snapshot = source.url.contains("web.archive.org/");
                let archive_html = match source.archive_url.as_deref() {
                    Some(a) if !a.is_empty() => format!("<a href=\"{}\">{}</a>", esc(a), esc(a)),
                    _ if is_archive_snapshot => {
                        // The source URL is itself an archived snapshot (a
                        // dead live page registered from Wayback) — no
                        // separate archive exists or is needed (operator
                        // catch: "the archive is there and has been since
                        // 2013").
                        "this link is the archived snapshot".to_string()
                    }
                    _ => "(archive pending)".to_string(),
                };
                let _ = writeln!(
                    sources_html,
                    "<p class=\"src\">Source: <a href=\"{url}\">{text}</a><br>archive: {archive} · accessed {accessed}</p>",
                    url = esc(&source.url),
                    text = esc(&source_link_text(source)),
                    archive = archive_html,
                    accessed = esc(&source.access_date),
                );
            }
            let _ = writeln!(
                quotes_html,
                "<p class=\"quote-head\">Verbatim excerpt from the source (re-checked automatically against the fetched text):</p>\n<blockquote data-wiki-anchor=\"ledger:{qid}\">{text}</blockquote>",
                text = esc(&quote.text),
            );
        }
    }
    let rules_html = if finding.rules.is_empty() {
        String::new()
    } else {
        let links: Vec<String> = finding
            .rules
            .iter()
            .map(|r| {
                format!(
                    "<a href=\"https://en.wikipedia.org/wiki/{}\">{}</a>",
                    esc(&r.replace(' ', "_")),
                    esc(r)
                )
            })
            .collect();
        format!(
            "<p class=\"rules\">Relevant guidance: {}</p>",
            links.join(" · ")
        )
    };
    let consulted = if consulted_html.is_empty() {
        String::new()
    } else {
        format!("<p class=\"quote-head\">Also consulted:</p>\n{consulted_html}")
    };
    format!(
        "<div class=\"evidence\" id=\"{ev_id}\" data-wiki-anchor=\"ledger:{primary}\">\n\
         <span class=\"anchor-tag\">evidence for this edit</span>\n\
         <p class=\"finding\">{note}</p>\n\
         <p class=\"rules\">{rules}</p>\n\
         {quotes}\n{sources}\n{consulted}\
         <p class=\"fix\">Proposed fix: {fix}</p>\n</div>\n",
        primary = finding.evidence[0],
        note = esc(&finding.factual_note),
        rules = rules_html,
        fix = esc(&finding.proposed_fix),
        sources = sources_html,
        quotes = quotes_html,
    )
}

/// Link text for a source: its citation if we have one, else the URL's
/// host — the raw URL stays in the href only (compactness, operator
/// round-7 request).
fn source_link_text(source: &crate::ledger::SourceEntry) -> String {
    let cite = source_cite(source);
    if !cite.is_empty() {
        return cite;
    }
    url::Url::parse(&source.url).map_or_else(
        |_| "(link)".to_string(),
        |u| u.host_str().unwrap_or("(link)").to_string(),
    )
}

/// " — Work: “Title”" citation fragment from a source's metadata.
fn source_cite(source: &crate::ledger::SourceEntry) -> String {
    let meta = source.metadata.as_ref();
    let title = meta.and_then(|m| m.title.as_deref()).unwrap_or("");
    let work = meta.and_then(|m| m.work.as_deref()).unwrap_or("");
    match (title.is_empty(), work.is_empty()) {
        (false, false) => format!("{work}: “{title}”"),
        (false, true) => format!("“{title}”"),
        (true, false) => work.to_string(),
        (true, true) => String::new(),
    }
}

struct AssembleArgs<'a> {
    article: &'a str,
    round: u32,
    /// Single-column inline diff (Word-style), changed blocks only.
    diff_html: &'a str,
    evidence_html: &'a str,
    anchor_json: &'a str,
    revisions_json: &'a str,
    round_id: &'a str,
    /// Enwiki link affordances (vendored snapshot,
    /// `vendor/enwiki-link-affordances.css` — checksum-pinned by test).
    wiki_links: &'a str,
}

fn assemble_artifact(a: &AssembleArgs<'_>) -> String {
    format!(
        r#"<!doctype html>
<html lang="en" data-theme="light">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>wikiloop review — {article} — round {round}</title>
<script type="application/json" data-lavish-revisions>
{revisions}
</script>
<script type="application/json" id="wa-anchor-table">
{anchors}
</script>
<style>
  :root {{ --ink:#1a1a1a; --muted:#667; --paper:#faf9f7; --line:#ddd8d0; --add:#e6f4e6; --del:#fde8e8; --accent:#7c5cbf; }}
  * {{ box-sizing: border-box; }}
  body {{ font: 15px/1.6 Georgia, serif; color: var(--ink); background: var(--paper); margin: 0; padding: 2rem; }}
  main {{ max-width: 1400px; margin: 0 auto; }}
  h1 {{ font-size: 1.4rem; }} h2 {{ font-size: 1.1rem; border-bottom: 1px solid var(--line); padding-bottom: .3rem; }}
  .meta {{ color: var(--muted); font-family: ui-monospace, monospace; font-size: .8rem; }}
  /* Word-style single-column diff + right-hand evidence rail (operator
     round-5 layout request); stacks on narrow viewports. */
  .layout {{ display: grid; grid-template-columns: minmax(0, 1.7fr) minmax(300px, 1fr); gap: 1.25rem; align-items: start; }}
  @media (max-width: 980px) {{ .layout {{ grid-template-columns: 1fr; }} }}
  .diff-col {{ border: 1px solid var(--line); border-radius: 8px; background: #fff; padding: 1rem; }}
  .evidence-col h2 {{ margin-top: 0; }}
  .block {{ border: 1px solid var(--line); border-radius: 6px; padding: .75rem; margin: .5rem 0; position: relative; overflow: auto; }}
  .block.change {{ border-left: 3px solid var(--accent); }}
  .block .anchor-tag {{ display: block; font-family: ui-monospace, monospace; font-size: .7rem; color: var(--muted); margin-bottom: .4rem; }}
  .block.del {{ background: var(--del); }} .block.add {{ background: var(--add); }} .block.equal {{ opacity: .8; }}
  .ctx-sep {{ color: var(--muted); font-family: ui-monospace, monospace; font-size: .75rem; text-align: center; padding: .15rem 0; }}
  /* enwiki link affordances: vendored snapshot (see vendor/PROVENANCE.md) */
{wiki_links}
  .block.heading {{ font-weight: 700; font-family: system-ui, sans-serif; font-size: 1.05em; }}
  .block.listitem {{ padding-left: 1.5rem; }}
  .block.listitem::before {{ content: "\2022  "; color: var(--muted); }}
  .block.quote {{ font-style: italic; border-left: 3px solid var(--line); }}
  .block.pre {{ font-family: ui-monospace, monospace; font-size: .85em; }}
  .block del {{ background: #f3b8b8; text-decoration: line-through; }}
  .block ins {{ background: #a8d8a8; text-decoration: none; }}
  .evidence {{ border: 1px solid var(--accent); border-radius: 6px; margin: 1rem 0; padding: .75rem; background: #f6f2fc; overflow: auto; }}
  .evidence blockquote {{ margin: .5rem 0; padding: .5rem .75rem; border-left: 3px solid var(--accent); background: #fff; }}
  .evidence .src, .evidence .finding, .evidence .fix {{ font-size: .85rem; }}
  .evidence .quote-head {{ font-size: .8rem; color: var(--muted); font-family: ui-monospace, monospace; margin: .4rem 0 .1rem; }}
  .evidence a {{ color: #36c; word-break: break-all; overflow-wrap: anywhere; }}
  .evidence .src {{ overflow-wrap: anywhere; }}
  .evidence .fix {{ color: var(--muted); }}
</style>
</head>
<body>
<main>
  <h1>wikiloop review — {article}</h1>
  <p class="meta">round {round} · one logical edit per round · deletions struck, insertions highlighted · comments anchor to the excerpt or the wikitext</p>
  <div class="layout">
    <section class="diff-col">
      <h2>Proposed edit</h2>
{diff}
    </section>
    <aside class="evidence-col">
      <h2>Evidence</h2>
{evidence}
    </aside>
  </div>
  <p class="meta">current round marker: {round_marker} · anchor table embedded as #wa-anchor-table</p>
</main>
</body>
</html>
"#,
        article = esc(a.article),
        round = a.round,
        revisions = a.revisions_json,
        anchors = a.anchor_json,
        diff = a.diff_html,
        evidence = a.evidence_html,
        round_marker = a.round_id,
        wiki_links = a.wiki_links,
    )
}

/// Convenience: revisions registry with the current round appended.
#[must_use]
pub fn registry_with_round(
    existing: &[RevisionEntry],
    round: u32,
    summary: &str,
    timestamp: &str,
) -> Vec<RevisionEntry> {
    let mut next = existing.to_vec();
    next.push(RevisionEntry {
        id: format!("r{round}"),
        label: format!("Round {round}"),
        timestamp: timestamp.to_string(),
        summary: summary.to_string(),
    });
    next
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::{RenderInput, RevisionEntry, render};

    static LINTER: LazyLock<crate::checks::linter::LinterConfig> = LazyLock::new(|| {
        crate::checks::linter::LinterConfig::from_toml_str(include_str!("../rules/linter.toml"))
            .unwrap()
    });

    static PARA: LazyLock<crate::checks::paraphrase::ParaphraseConfig> = LazyLock::new(|| {
        crate::checks::paraphrase::ParaphraseConfig::from_toml_str(include_str!(
            "../rules/paraphrase.toml"
        ))
        .unwrap()
    });

    fn input<'a>(
        base: &'a str,
        proposed: &'a str,
        base_html: &'a str,
        prop_html: &'a str,
        ledger: &'a crate::ledger::Ledger,
        findings: &'a [crate::session::Finding],
    ) -> RenderInput<'a> {
        RenderInput {
            article: "Test article".into(),
            round: 1,
            base_wikitext: base,
            proposed_wikitext: proposed,
            base_html,
            proposed_html: prop_html,
            findings,
            ledger,
            linter_config: &LINTER,
            paraphrase_config: &PARA,
            revisions: vec![RevisionEntry {
                id: "r1".into(),
                label: "Round 1".into(),
                timestamp: "2026-09-24T00:00:00Z".into(),
                summary: "first".into(),
            }],
        }
    }

    const BASE_WT: &str = "The tower is old.\n\n== History ==\nIt was built in 1937.\n";
    const PROP_WT: &str =
        "The tower is old.\n\n== History ==\nIt was built in 1937 and remains in use.\n";
    const BASE_HTML: &str = "<html><body><p>The tower is old.</p><h2>History</h2><p>It was built in 1937.</p></body></html>";
    const PROP_HTML: &str = "<html><body><p>The tower is old.</p><h2>History</h2><p>It was built in 1937 and remains in use.</p></body></html>";

    #[test]
    fn renders_two_panes_with_unique_ids_and_anchors() {
        let ledger = crate::ledger::Ledger::default();
        let out = render(&input(BASE_WT, PROP_WT, BASE_HTML, PROP_HTML, &ledger, &[])).unwrap();
        assert!(
            out.artifact_html.contains("id=\"wa-1\""),
            "changed block id"
        );
        assert!(out.artifact_html.contains("data-wiki-anchor="));
        assert!(out.artifact_html.contains("data-lavish-revisions"));
        assert!(out.artifact_html.contains("wa-anchor-table"));
        // Unique ids.
        let ids: std::collections::HashSet<_> = out
            .anchor_table
            .iter()
            .map(|a| a.element_id.clone())
            .collect();
        assert_eq!(ids.len(), out.anchor_table.len());
        // Every wa- anchor is a non-empty range in ONE of the two
        // wikitexts: old-side blocks (deletion anchors) carry `base:`
        // ranges into the base wikitext; new-side blocks plain ranges into
        // the proposed wikitext.
        for entry in &out.anchor_table {
            if entry.element_id.starts_with("wa-") {
                assert!(
                    entry.wikitext_anchor.starts_with('L')
                        || entry.wikitext_anchor.starts_with("base:L"),
                    "{} -> {}",
                    entry.element_id,
                    entry.wikitext_anchor
                );
            }
        }
        // The changed pair yields TWO ids: old side (base anchor) first,
        // new side (proposed anchor) second.
        let old_side = out
            .anchor_table
            .iter()
            .find(|a| a.element_id == "wa-1")
            .unwrap();
        assert!(
            old_side.wikitext_anchor.starts_with("base:L4:C0-L4:"),
            "{}",
            old_side.wikitext_anchor
        );
        let new_side = out
            .anchor_table
            .iter()
            .find(|a| a.element_id == "wa-2")
            .unwrap();
        assert!(
            new_side.wikitext_anchor.starts_with("L4:C0-L4:"),
            "{}",
            new_side.wikitext_anchor
        );
    }

    /// Deletion anchors: a block removed entirely still gets its own
    /// `wa-N` id anchored in the BASE wikitext (`base:` prefix), so
    /// operator comments on removed content resolve (MVP-2 A.1.1).
    #[test]
    fn pure_deletion_gets_anchored_id_in_base_wikitext() {
        let ledger = crate::ledger::Ledger::default();
        let base_wt = "The tower is old.\n\n== History ==\nIt was built in 1937.\n\nThe keep was removed in 1970.\n";
        let prop_wt = "The tower is old.\n\n== History ==\nIt was built in 1937.\n";
        let base_html = "<html><body><p>The tower is old.</p><h2>History</h2><p>It was built in 1937.</p><p>The keep was removed in 1970.</p></body></html>";
        let prop_html = "<html><body><p>The tower is old.</p><h2>History</h2><p>It was built in 1937.</p></body></html>";
        let out = render(&input(base_wt, prop_wt, base_html, prop_html, &ledger, &[])).unwrap();
        assert!(
            out.artifact_html.contains("The keep was removed in 1970."),
            "deleted text renders in the old pane"
        );
        let entry = out
            .anchor_table
            .iter()
            .find(|a| a.element_id == "wa-1")
            .expect("pure deletion gets a wa id");
        assert!(
            entry.wikitext_anchor.starts_with("base:L6:C0-L6:"),
            "deletion anchor points into the base wikitext: {}",
            entry.wikitext_anchor
        );
        assert!(
            out.artifact_html
                .contains(&format!("data-wiki-anchor=\"{}\"", entry.wikitext_anchor)),
            "the deleted block carries its anchor attribute"
        );
    }

    /// MVP-2 A.1.2 — the enwiki link-affordance snapshot is checksum-pinned:
    /// editing the vendored file without recording it in
    /// `vendor/PROVENANCE.md` fails here.
    #[test]
    fn enwiki_link_affordance_snapshot_is_checksum_pinned() {
        use sha2::Digest as _;
        use std::fmt::Write as _;
        let css = include_str!("../vendor/enwiki-link-affordances.css");
        let digest = sha2::Sha256::digest(css.as_bytes());
        let mut hex = String::with_capacity(digest.len() * 2);
        for b in digest {
            let _ = write!(hex, "{b:02x}");
        }
        assert_eq!(
            hex, "2e77dd0634ddd9fd781a0d2dfaffba0d68aae4a9fe58c39213b93dc7ba5a6a50",
            "vendor/enwiki-link-affordances.css changed — update this pin AND vendor/PROVENANCE.md"
        );
    }

    /// MVP-2 A.1.2 — wikilinks carry the wiki's own link affordances, not
    /// the MVP expedient dotted-underline that read like a misspelling
    /// mark (operator backlog note, 2026-09-25).
    #[test]
    fn wikilinks_use_enwiki_link_affordances() {
        let ledger = crate::ledger::Ledger::default();
        let out = render(&input(BASE_WT, PROP_WT, BASE_HTML, PROP_HTML, &ledger, &[])).unwrap();
        assert!(
            out.artifact_html.contains(".wl {"),
            "vendored enwiki link styles inlined into the artifact"
        );
        assert!(
            out.artifact_html.contains("color: #36c;"),
            "enwiki progressive link color present"
        );
        assert!(
            !out.artifact_html.contains("underline dotted"),
            "the dotted-underline misspelling-mark styling is gone"
        );
    }

    /// Live L2 round-1 catch (Sarah Kidder): Parsoid glues bracketed
    /// annotations to words ("Ohio[citation needed]", "1870.[7]"); the
    /// match prefix must ignore them or the block mis-locates to its
    /// fallback line.
    #[test]
    fn locate_ignores_bracketed_annotations() {
        let wt = "Lead sentence.\n\n== History ==\nBorn Sarah A. Clark in Ohio, Kidder married in 1870.\n";
        let anchor = super::locate_block_anchor(
            wt,
            "Born Sarah A. Clark in Ohio[citation needed], Kidder married in 1870.[7]",
        )
        .expect("locates despite annotations");
        assert!(anchor.starts_with("L4:"), "{anchor}");
    }

    /// Live L2 round-1 catch: the fallback anchor must never land on a
    /// blank line (zero-width range).
    #[test]
    fn fallback_anchor_never_lands_on_blank_line() {
        let wt = "a\n\n\nb\n";
        let anchor = super::next_line_anchor(wt, 1);
        assert!(
            anchor.starts_with("L4:"),
            "steps to the non-blank line: {anchor}"
        );
        assert!(!anchor.ends_with("C0"), "non-empty range: {anchor}");
    }

    /// Review feedback (TF round 1): a one-word change inside a long
    /// paragraph must show word-level <del>/<ins> runs, not a whole-block
    /// swap.
    #[test]
    fn changed_pairs_render_word_level_inline_diff() {
        let ledger = crate::ledger::Ledger::default();
        let out = render(&input(BASE_WT, PROP_WT, BASE_HTML, PROP_HTML, &ledger, &[])).unwrap();
        assert!(
            out.artifact_html.contains("1937.</del>"),
            "removed words must render as an inline del run (Word-style combined block)"
        );
        assert!(
            out.artifact_html
                .contains("<ins>1937 and remains in use.</ins>"),
            "new side must mark the added words"
        );
    }

    /// Review feedback (TF round 1): Parsoid's embedded <style> text (the
    /// `.mw-parser-output` CSS in the References section) must not leak
    /// into block text.
    #[test]
    fn style_and_script_text_never_leak_into_blocks() {
        let ledger = crate::ledger::Ledger::default();
        let base_html = "<html><body><p>Old text.</p></body></html>";
        let prop_html = concat!(
            "<html><body>",
            "<style>.mw-parser-output cite.citation{font-size:1px}</style>",
            "<p>New text.</p>",
            "<li>1 <style>.x{}</style>cited claim</li>",
            "</body></html>"
        );
        let out = render(&input(
            "Old text.\n",
            "New text.\n",
            base_html,
            prop_html,
            &ledger,
            &[],
        ))
        .unwrap();
        assert!(!out.artifact_html.contains("mw-parser-output"));
        // The fixture's distinctive CSS strings must not appear (the
        // artifact's own stylesheet legitimately mentions font sizes).
        assert!(!out.artifact_html.contains("cite.citation"));
        assert!(!out.artifact_html.contains(".x{}"));
        assert!(out.artifact_html.contains("cited claim"));
    }

    /// Review surface shows ONLY changed blocks: unchanged runs collapse
    /// to a count separator instead of rendering (operator request).
    #[test]
    fn unchanged_blocks_collapse_to_separator() {
        let ledger = crate::ledger::Ledger::default();
        let out = render(&input(BASE_WT, PROP_WT, BASE_HTML, PROP_HTML, &ledger, &[])).unwrap();
        // The unchanged lead paragraph must NOT render as a pane block.
        assert!(
            !out.artifact_html.contains("The tower is old."),
            "unchanged blocks must not render"
        );
        // A collapsed-context separator reports the skipped run.
        assert!(
            out.artifact_html.contains("unchanged block"),
            "separator missing"
        );
        // The changed block still renders with its anchor.
        assert!(out.artifact_html.contains("id=\"wa-1\""));
    }

    #[test]
    fn gate_block_writes_nothing() {
        // Semicolon on the added line blocks via the linter gate.
        let bad_prop = "The tower is old; it is tall.\n";
        let ledger = crate::ledger::Ledger::default();
        let err = render(&input(
            BASE_WT,
            bad_prop,
            BASE_HTML,
            BASE_HTML,
            &ledger,
            &[],
        ))
        .unwrap_err();
        assert!(err.to_string().contains("gate blocked"), "{err}");
    }

    #[test]
    fn revisions_registry_shape() {
        let ledger = crate::ledger::Ledger::default();
        let out = render(&input(BASE_WT, PROP_WT, BASE_HTML, PROP_HTML, &ledger, &[])).unwrap();
        let start = out
            .artifact_html
            .find("data-lavish-revisions")
            .and_then(|i| out.artifact_html[i..].find('[').map(|j| i + j))
            .expect("registry present");
        let end = out.artifact_html[start..]
            .find("</script>")
            .map(|i| &out.artifact_html[start..start + i])
            .expect("closed");
        let parsed: serde_json::Value = serde_json::from_str(end.trim()).expect("valid json");
        let entry = &parsed[0];
        for key in ["id", "label", "timestamp", "summary"] {
            assert!(entry.get(key).is_some(), "registry entry missing {key}");
        }
    }

    #[test]
    fn evidence_rail_renders_for_every_gated_finding() {
        let mut ledger = crate::ledger::Ledger::default();
        let sid = ledger.register_source("https://example.com/src", "2026-09-24", None);
        ledger
            .attach_fetched_text(&sid, "The tower was completed in 1937 and painted red.")
            .unwrap();
        let qid = ledger.add_quote(&sid, "completed in 1937").unwrap();
        let finding = crate::session::Finding {
            id: "F1".into(),
            wikitext_anchor: "L4:C0-L4:C40".into(),
            rendered_span_id: None,
            rules: vec!["WP:V".into()],
            evidence: vec![qid.clone()],
            factual_note: "Date verified.".into(),
            proposed_fix: "Add the completion date.".into(),
            loop_id: 2,
        };
        let inp = input(
            BASE_WT,
            PROP_WT,
            BASE_HTML,
            PROP_HTML,
            &ledger,
            std::slice::from_ref(&finding),
        );
        let out = render(&inp).unwrap();
        assert!(out.artifact_html.contains("id=\"ev-1\""));
        assert!(
            out.artifact_html
                .contains(&format!("data-wiki-anchor=\"ledger:{qid}\""))
        );
        assert!(out.artifact_html.contains("completed in 1937"));
        assert_eq!(
            out.updated_findings[0].rendered_span_id.as_deref(),
            Some("ev-1")
        );
    }
}
