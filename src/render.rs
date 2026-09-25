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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnchorEntry {
    /// Artifact element id (`wa-N` or `ev-N`).
    pub element_id: String,
    /// `data-wiki-anchor` value: wikitext `L..:C..-L..:C..` range or
    /// `ledger:Q<n>`.
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
    });
    let GateVerdict {
        blocked, reasons, ..
    } = verdict;
    if blocked {
        let joined = reasons
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  ");
        return Err(RenderError::GateBlocked(joined));
    }

    // 2. Extract content blocks from both HTML sides.
    let base_blocks = extract_blocks(input.base_html).map_err(RenderError::Html)?;
    let proposed_blocks = extract_blocks(input.proposed_html).map_err(RenderError::Html)?;

    // 3. Block-level diff.
    let base_texts: Vec<&str> = base_blocks.iter().map(|b| b.text.as_str()).collect();
    let prop_texts: Vec<&str> = proposed_blocks.iter().map(|b| b.text.as_str()).collect();
    let diff = similar::TextDiff::from_slices(&base_texts, &prop_texts);

    // 4. Walk the diff; assign wa-N ids + anchors (all anchored in the
    //    PROPOSED wikitext; a pure deletion anchors to the nearest
    //    surviving proposed line). Consecutive Delete+Insert runs pair into
    //    ONE changed block whose inside is a WORD-LEVEL diff (<del>/<ins>
    //    runs), so a one-word change is visible inside a long paragraph.
    let round_id = format!("r{}", input.round);
    let mut anchor_table: Vec<AnchorEntry> = Vec::new();
    let mut old_pane = String::new();
    let mut new_pane = String::new();
    let mut counter = 0usize;
    let mut prop_idx = 0usize;
    let mut pending_deletes: Vec<(&str, BlockKind)> = Vec::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Equal => {
                flush_pure_deletes(&mut pending_deletes, &mut old_pane, &round_id);
                prop_idx += 1;
                let kind = change
                    .new_index()
                    .map_or(BlockKind::Paragraph, |i| proposed_blocks[i].kind);
                old_pane.push_str(&block_html(
                    None,
                    None,
                    change.value(),
                    kind,
                    "equal",
                    &round_id,
                ));
                new_pane.push_str(&block_html(
                    None,
                    None,
                    change.value(),
                    kind,
                    "equal",
                    &round_id,
                ));
            }
            similar::ChangeTag::Delete => {
                let kind = change
                    .old_index()
                    .map_or(BlockKind::Paragraph, |i| base_blocks[i].kind);
                pending_deletes.push((change.value(), kind));
            }
            similar::ChangeTag::Insert => {
                let new_text = change.value();
                let new_kind = change
                    .new_index()
                    .map_or(BlockKind::Paragraph, |i| proposed_blocks[i].kind);
                let (old_text, old_kind) = if pending_deletes.is_empty() {
                    (String::new(), new_kind)
                } else {
                    let texts: Vec<&str> = pending_deletes.iter().map(|(t, _)| *t).collect();
                    let kind = pending_deletes[0].1;
                    pending_deletes.clear();
                    (texts.join("\n\n"), kind)
                };
                counter += 1;
                let id = format!("wa-{counter}");
                // Anchor: the proposed wikitext range of this block.
                let anchor = locate_block_anchor(input.proposed_wikitext, new_text)
                    .unwrap_or_else(|| next_line_anchor(input.proposed_wikitext, prop_idx));
                prop_idx += 1;
                anchor_table.push(AnchorEntry {
                    element_id: id.clone(),
                    wikitext_anchor: anchor.clone(),
                });
                if old_text.is_empty() {
                    // Pure insertion: no old counterpart to pair.
                    new_pane.push_str(&block_html(
                        Some(&id),
                        Some(&anchor),
                        new_text,
                        new_kind,
                        "add",
                        &round_id,
                    ));
                } else {
                    let (old_inline, new_inline) = inline_word_diff(&old_text, new_text);
                    old_pane.push_str(&block_html_inline(
                        None,
                        None,
                        &old_inline,
                        old_kind,
                        "del",
                        &round_id,
                    ));
                    new_pane.push_str(&block_html_inline(
                        Some(&id),
                        Some(&anchor),
                        &new_inline,
                        new_kind,
                        "add",
                        &round_id,
                    ));
                }
            }
        }
    }
    flush_pure_deletes(&mut pending_deletes, &mut old_pane, &round_id);

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
        old_pane: &old_pane,
        new_pane: &new_pane,
        evidence_html: &evidence_html,
        anchor_json: &anchor_json,
        revisions_json: &revisions_json,
        round_id: &round_id,
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

/// Visible text of an element, skipping style/script subtrees.
fn text_of(element: scraper::ElementRef<'_>) -> String {
    use scraper::node::Node;
    let mut out = String::new();
    for child in element.children() {
        match child.value() {
            Node::Text(t) => out.push_str(t),
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

/// Emit buffered deletes that never paired with an insert (pure deletions).
fn flush_pure_deletes(pending: &mut Vec<(&str, BlockKind)>, old_pane: &mut String, round_id: &str) {
    for (text, kind) in pending.drain(..) {
        old_pane.push_str(&block_html(None, None, text, kind, "del", round_id));
    }
}
/// Word-level inline diff between the old and new sides of a changed block
/// pair: returns (`old_html`, `new_html`) with `<del>`/`<ins>` runs around the
/// changed words (equal words render plain in both).
fn inline_word_diff(old: &str, new: &str) -> (String, String) {
    let diff = similar::TextDiff::from_words(old, new);
    let mut old_html = String::new();
    let mut new_html = String::new();
    let mut del_run = String::new();
    let mut ins_run = String::new();
    for change in diff.iter_all_changes() {
        let piece = esc(change.value());
        match change.tag() {
            similar::ChangeTag::Delete => del_run.push_str(&piece),
            similar::ChangeTag::Insert => ins_run.push_str(&piece),
            similar::ChangeTag::Equal => {
                if !del_run.is_empty() {
                    let _ = write!(old_html, "<del>{del_run}</del>");
                    del_run.clear();
                }
                if !ins_run.is_empty() {
                    let _ = write!(new_html, "<ins>{ins_run}</ins>");
                    ins_run.clear();
                }
                old_html.push_str(&piece);
                new_html.push_str(&piece);
            }
        }
    }
    if !del_run.is_empty() {
        let _ = write!(old_html, "<del>{del_run}</del>");
    }
    if !ins_run.is_empty() {
        let _ = write!(new_html, "<ins>{ins_run}</ins>");
    }
    (old_html, new_html)
}

/// Wrap one inline-diffed block for a pane (content is pre-escaped HTML with
/// `<del>`/`<ins>` runs).
fn block_html_inline(
    id: Option<&str>,
    anchor: Option<&str>,
    inline_html: &str,
    kind: BlockKind,
    class: &str,
    round_id: &str,
) -> String {
    let id_attr = id.map_or_else(String::new, |i| format!(" id=\"{i}\""));
    let anchor_attr = anchor.map_or_else(String::new, |a| format!(" data-wiki-anchor=\"{a}\""));
    let rev_attr = id.map_or_else(String::new, |_| {
        format!(" data-lavish-revision=\"{round_id}\"")
    });
    let tag = id.map_or_else(
        || format!("[{}]", kind.label()),
        |i| format!("{i} · {} · {}", anchor.unwrap_or("-"), kind.label()),
    );
    format!(
        "<div class=\"block {class} {}\"{id_attr}{anchor_attr}>{rev_attr}<span class=\"anchor-tag\">{tag}</span>{inline_html}</div>\n",
        kind.css(),
    )
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
    let tag = id.map_or_else(
        || format!("[{}]", kind.label()),
        |i| format!("{i} · {} · {}", anchor.unwrap_or("-"), kind.label()),
    );
    format!(
        "<div class=\"block {class} {}\"{id_attr}{anchor_attr}>{rev_attr}<span class=\"anchor-tag\">{tag}</span>{}</div>\n",
        kind.css(),
        esc(text),
    )
}
/// Locate a block's wikitext anchor: the line range (1-based, char cols)
/// whose content matches the block text's opening words. Returns
/// `L<s>:C<col>-L<e>:C<col2>`.
fn locate_block_anchor(wikitext: &str, block_text: &str) -> Option<String> {
    let norm = |s: &str| {
        s.to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let block_norm = norm(block_text);
    if block_norm.is_empty() {
        return None;
    }
    let prefix: String = block_norm.split(' ').take(6).collect::<Vec<_>>().join(" ");
    if prefix.is_empty() {
        return None;
    }
    // Search per line: a normalized-line containment is strong evidence.
    let mut lines_and_starts = Vec::new();
    let mut byte = 0usize;
    for line in wikitext.split('\n') {
        lines_and_starts.push((byte, line));
        byte += line.len() + 1;
    }
    for (line_no, (_start, line)) in lines_and_starts.iter().enumerate() {
        let line_norm = norm(line);
        if line_norm.contains(&prefix) {
            let col_end = line.chars().count();
            return Some(format!("L{}:C0-L{}:C{}", line_no + 1, line_no + 1, col_end));
        }
    }
    None
}

/// Anchor for the n-th surviving proposed line (fallback for unmatched
/// blocks). Always non-empty per AC.5.
fn next_line_anchor(wikitext: &str, prop_idx: usize) -> String {
    let total = wikitext.lines().count().max(1);
    let line = (prop_idx + 1).clamp(1, total);
    let col_end = wikitext
        .lines()
        .nth(line - 1)
        .map_or(0, |l| l.chars().count());
    format!("L{line}:C0-L{line}:C{col_end}")
}

/// One evidence card: full source citation, verbatim quotes, archive URL,
/// attached finding.
fn evidence_card(ev_id: &str, finding: &Finding, ledger: &Ledger) -> String {
    let mut quotes_html = String::new();
    let mut sources_html = String::new();
    for qid in &finding.evidence {
        if let Some(quote) = ledger.quote(qid) {
            if let Some(source) = ledger.sources.iter().find(|s| s.id == quote.source_id) {
                let archive = source.archive_url.as_deref().unwrap_or("(archive pending)");
                let _ = writeln!(
                    sources_html,
                    "<p class=\"src\"><strong>{}</strong> {}<br>archive: {} · accessed {}</p>",
                    esc(&source.id),
                    esc(&source.url),
                    esc(archive),
                    esc(&source.access_date),
                );
            }
            let _ = writeln!(
                quotes_html,
                "<blockquote data-wiki-anchor=\"ledger:{qid}\">{}</blockquote>",
                esc(&quote.text)
            );
        }
    }
    format!(
        "<div class=\"evidence\" id=\"{ev_id}\" data-wiki-anchor=\"ledger:{primary}\">\n\
         <span class=\"anchor-tag\">{ev_id} · ledger:{primary}</span>\n\
         <p class=\"finding\"><strong>{fid}</strong> [{rules}] · loop {loop_id}</p>\n\
         <p class=\"finding\">{note}</p>\n\
         <p class=\"fix\">Fix: {fix}</p>\n\
         {sources}\n{quotes}\n</div>\n",
        primary = finding.evidence[0],
        fid = esc(&finding.id),
        rules = esc(&finding.rules.join(", ")),
        loop_id = finding.loop_id,
        note = esc(&finding.factual_note),
        fix = esc(&finding.proposed_fix),
        sources = sources_html,
        quotes = quotes_html,
    )
}

struct AssembleArgs<'a> {
    article: &'a str,
    round: u32,
    old_pane: &'a str,
    new_pane: &'a str,
    evidence_html: &'a str,
    anchor_json: &'a str,
    revisions_json: &'a str,
    round_id: &'a str,
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
  main {{ max-width: 1200px; margin: 0 auto; }}
  h1 {{ font-size: 1.4rem; }} h2 {{ font-size: 1.1rem; border-bottom: 1px solid var(--line); padding-bottom: .3rem; }}
  .meta {{ color: var(--muted); font-family: ui-monospace, monospace; font-size: .8rem; }}
  .pane-pair {{ display: grid; grid-template-columns: minmax(0,1fr) minmax(0,1fr); gap: 1rem; }}
  .pane {{ border: 1px solid var(--line); border-radius: 8px; background: #fff; padding: 1rem; overflow: auto; max-height: 65vh; }}
  .pane h3 {{ margin: 0 0 .5rem; font-size: .8rem; font-family: ui-monospace, monospace; color: var(--muted); font-weight: 600; }}
  .block {{ border: 1px solid var(--line); border-radius: 6px; padding: .75rem; margin: .5rem 0; position: relative; overflow: auto; }}
  .block .anchor-tag {{ display: block; font-family: ui-monospace, monospace; font-size: .7rem; color: var(--muted); margin-bottom: .4rem; }}
  .block.del {{ background: var(--del); }} .block.add {{ background: var(--add); }} .block.equal {{ opacity: .8; }}
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
  .evidence .fix {{ color: var(--muted); }}
</style>
</head>
<body>
<main>
  <h1>wikiloop review — {article}</h1>
  <p class="meta">round {round} · one logical edit per round · comments anchor to ledger quotes and wikitext ranges</p>
  <h2>Proposed edit (old → new)</h2>
  <div class="pane-pair">
    <section class="pane" id="pane-old"><h3>old</h3>
{old}
    </section>
    <section class="pane" id="pane-new"><h3>new (proposed)</h3>
{new}
    </section>
  </div>
  <h2>Evidence rail</h2>
{evidence}
  <p class="meta">current round marker: {round_marker} · anchor table embedded as #wa-anchor-table</p>
</main>
</body>
</html>
"#,
        article = esc(a.article),
        round = a.round,
        revisions = a.revisions_json,
        anchors = a.anchor_json,
        old = a.old_pane,
        new = a.new_pane,
        evidence = a.evidence_html,
        round_marker = a.round_id,
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
        // Every changed-block anchor resolves to a non-empty range in the
        // proposed wikitext (AC.5).
        for entry in &out.anchor_table {
            if entry.element_id.starts_with("wa-") {
                assert!(
                    entry.wikitext_anchor.starts_with('L'),
                    "{} -> {}",
                    entry.element_id,
                    entry.wikitext_anchor
                );
            }
        }
        let anchor = out
            .anchor_table
            .iter()
            .find(|a| a.element_id == "wa-1")
            .unwrap();
        assert!(
            anchor.wikitext_anchor.starts_with("L4:C0-L4:"),
            "{}",
            anchor.wikitext_anchor
        );
    }

    /// Review feedback (TF round 1): a one-word change inside a long
    /// paragraph must show word-level <del>/<ins> runs, not a whole-block
    /// swap.
    #[test]
    fn changed_pairs_render_word_level_inline_diff() {
        let ledger = crate::ledger::Ledger::default();
        let out = render(&input(BASE_WT, PROP_WT, BASE_HTML, PROP_HTML, &ledger, &[])).unwrap();
        assert!(
            out.artifact_html.contains("<del>1937.</del>"),
            "old side must mark the removed words; got artifact lacking del run"
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
