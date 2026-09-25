//! lavish-axi session management and TOON parsing.
//!
//! lavish-axi is pinned (`npx -y lavish-axi@0.1.78`, Node ≥ 22 required only
//! for lavish) and its CLI output is TOON — an indentation-structured text
//! format. The normative reference for both payload shapes and TOON is the
//! vendored source (`vendor/lavish-axi-0.1.78.tgz`) plus captured real
//! output under `fixtures/lavish/`. The parser below is written against the
//! observed shapes:
//!
//! - `key: value` (value optionally `"quoted"`; arrays as `"a","b",…` on one
//!   line),
//! - nested maps by indentation (`key:` then deeper lines),
//! - table headers `key[n]{cols}:` (line ends with `:`) with one row per
//!   following line at deeper indentation.
//!
//! Parsing is tolerant by design: unknown constructs degrade to scalars and
//! `parse_toon` never panics — it pins observed formats, not a spec.

use std::process::Command;
use std::process::Output;

use crate::anchors::CommentPrompt;

/// The pinned lavish-axi version (README documents the pin).
pub const LAVISH_VERSION: &str = "0.1.78";

/// One parsed TOON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toon {
    /// A scalar (unquoted or quoted single string).
    Str(String),
    /// An array of scalars or table rows.
    List(Vec<Toon>),
    /// A map (key order preserved).
    Map(Vec<(String, Toon)>),
}

impl Toon {
    /// Map lookup.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Toon> {
        match self {
            Toon::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Scalar value.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Toon::Str(s) => Some(s),
            _ => None,
        }
    }

    /// List items.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Toon]> {
        match self {
            Toon::List(items) => Some(items),
            _ => None,
        }
    }

    /// All map pairs.
    #[must_use]
    pub fn as_map(&self) -> Option<&[(String, Toon)]> {
        match self {
            Toon::Map(pairs) => Some(pairs),
            _ => None,
        }
    }
}

/// Parse TOON text.
#[must_use]
pub fn parse_toon(text: &str) -> Toon {
    let lines: Vec<(usize, String)> = text
        .lines()
        .filter_map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with("[lavish-axi]") {
                // Poll banner chatter, not TOON (real capture:
                // fixtures/lavish/poll-feedback-real.toon).
                return None;
            }
            let indent = raw.len() - raw.trim_start().len();
            Some((indent, trimmed.to_string()))
        })
        .collect();
    let mut pos = 0usize;
    Toon::Map(parse_block(&lines, &mut pos, 0))
}

/// Parse consecutive lines at exactly `indent` (deeper lines nest).
fn parse_block(lines: &[(usize, String)], pos: &mut usize, indent: usize) -> Vec<(String, Toon)> {
    let mut out = Vec::new();
    while *pos < lines.len() {
        let (ind, line) = &lines[*pos];
        if *ind < indent {
            break;
        }
        if *ind > indent {
            // Unexpected deeper line without an opener: attach as scalar.
            out.push((
                "__line".to_string(),
                Toon::Str(strip_quotes(line).to_string()),
            ));
            *pos += 1;
            continue;
        }
        // Table header: `key[n]{cols}:` — the line ends with ':'.
        if line.ends_with(':') {
            if let Some(key) = table_key(line) {
                *pos += 1;
                let start = *pos;
                while *pos < lines.len() && lines[*pos].0 > indent {
                    *pos += 1;
                }
                let deeper = &lines[start..*pos];
                if deeper.is_empty() {
                    out.push((key, Toon::List(Vec::new())));
                    continue;
                }
                // Observed row shapes (fixtures/lavish/poll-feedback-real.toon):
                //  (a) `- key: value` list items — one object per `- ` start,
                //      fields on the following deeper lines;
                //  (b) bare `key: value` lines — a single object;
                //  (c) plain scalars — string table rows.
                if deeper.iter().any(|(_, l)| l.starts_with("- ")) {
                    *pos = start;
                    let items = parse_list_items(lines, pos, indent);
                    out.push((key, Toon::List(items)));
                } else if deeper
                    .iter()
                    .any(|(_, l)| l.contains(": ") || (l.ends_with(':') && table_key(l).is_none()))
                {
                    let mut p = start;
                    let child_indent = deeper[0].0;
                    let children = parse_block(lines, &mut p, child_indent);
                    out.push((key, Toon::List(vec![Toon::Map(children)])));
                } else {
                    let rows = deeper
                        .iter()
                        .map(|(_, l)| Toon::Str(strip_quotes(l).to_string()))
                        .collect();
                    out.push((key, Toon::List(rows)));
                }
                continue;
            }
            // Nested map: `key:`
            let key = strip_count(line[..line.len() - 1].trim());
            *pos += 1;
            let child_indent = lines
                .get(*pos)
                .map_or(indent + 1, |(i, _)| *i)
                .max(indent + 1);
            let children = parse_block(lines, pos, child_indent);
            out.push((key, Toon::Map(children)));
            continue;
        }
        // `key: value`
        if let Some(idx) = line.find(": ") {
            let key = strip_count(line[..idx].trim());
            let value = line[idx + 2..].trim();
            out.push((key, parse_value(value)));
            *pos += 1;
            continue;
        }
        // Bare scalar: when deeper lines follow, it opens an implicit map
        // (observed: the leading file-path line with `status:` beneath it).
        if lines.get(*pos + 1).is_some_and(|(i, _)| *i > indent) {
            let key = strip_quotes(line).to_string();
            *pos += 1;
            let child_indent = lines[*pos].0.max(indent + 1);
            let children = parse_block(lines, pos, child_indent);
            out.push((key, Toon::Map(children)));
            continue;
        }
        out.push((
            "__line".to_string(),
            Toon::Str(strip_quotes(line).to_string()),
        ));
        *pos += 1;
    }
    out
}

/// Parse `- key: value` list items (one [`Toon::Map`] per item) under a table
/// header at `header_indent`. Item fields continue on deeper lines until the
/// next `- ` at the item indent or a dedent to the header level.
fn parse_list_items(lines: &[(usize, String)], pos: &mut usize, header_indent: usize) -> Vec<Toon> {
    let mut items = Vec::new();
    while *pos < lines.len() {
        let (ind, line) = &lines[*pos];
        if *ind <= header_indent {
            break;
        }
        if !line.starts_with("- ") {
            // Non-dash line inside a dash region: skip as scalar.
            *pos += 1;
            continue;
        }
        let item_indent = *ind;
        let mut fields = Vec::new();
        let first = line[2..].to_string();
        *pos += 1;
        if let Some((k, v)) = split_kv_inline(&first) {
            if v.is_empty() {
                let child_indent = lines
                    .get(*pos)
                    .map_or(item_indent + 1, |(i, _)| *i)
                    .max(item_indent + 2);
                let children = parse_block(lines, pos, child_indent);
                fields.push((k, Toon::Map(children)));
            } else {
                fields.push((k, parse_value(&v)));
            }
        }
        while *pos < lines.len() && lines[*pos].0 > item_indent {
            let (ci, cline) = &lines[*pos];
            if cline.starts_with("- ") {
                fields.push((
                    "__line".to_string(),
                    Toon::Str(strip_quotes(cline).to_string()),
                ));
                *pos += 1;
                continue;
            }
            if cline.ends_with(':') && !cline.starts_with("- ") {
                // Bare `key:` — nested map opener (e.g. `target:`).
                let k = strip_count(cline[..cline.len() - 1].trim());
                let child_indent = lines
                    .get(*pos + 1)
                    .map_or(*ci + 1, |(i, _)| *i)
                    .max(*ci + 1);
                *pos += 1;
                let children = parse_block(lines, pos, child_indent);
                fields.push((k, Toon::Map(children)));
            } else if let Some((k, v)) = split_kv_inline(cline) {
                if v.is_empty() {
                    let child_indent = lines
                        .get(*pos + 1)
                        .map_or(*ci + 1, |(i, _)| *i)
                        .max(*ci + 1);
                    *pos += 1;
                    let children = parse_block(lines, pos, child_indent);
                    fields.push((k, Toon::Map(children)));
                } else {
                    fields.push((k, parse_value(&v)));
                    *pos += 1;
                }
            } else {
                fields.push((
                    "__line".to_string(),
                    Toon::Str(strip_quotes(cline).to_string()),
                ));
                *pos += 1;
            }
        }
        items.push(Toon::Map(fields));
    }
    items
}

/// Split `key: value`; `None` when the line is not a KV line.
fn split_kv_inline(line: &str) -> Option<(String, String)> {
    let idx = line.find(": ")?;
    Some((
        strip_count(line[..idx].trim()),
        line[idx + 2..].trim().to_string(),
    ))
}

/// Key of a table header `key[n]{cols}:` / `key[n]:` (line already ends with
/// ':'). Returns `None` for plain `key:` (no bracket).
fn table_key(line: &str) -> Option<String> {
    let head = line.strip_suffix(':')?;
    let bracket = head.find('[')?;
    if !head[bracket..].contains(']') {
        return None;
    }
    let key = head[..bracket].trim();
    (!key.is_empty()).then(|| key.to_string())
}

/// Strip a `[n]` / `[n]{cols}` count suffix from a key (`help[3]` → `help`).
fn strip_count(key: &str) -> String {
    match key.find('[') {
        Some(idx) if key[idx..].contains(']') => key[..idx].trim().to_string(),
        _ => key.to_string(),
    }
}

/// Strip surrounding double quotes.
fn strip_quotes(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

/// Parse a value: `"a","b",…` → List of Str; single quoted → Str; else Str.
fn parse_value(value: &str) -> Toon {
    let v = value.trim();
    if v.starts_with('"') {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        for ch in v.chars() {
            match ch {
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => {
                    parts.push(Toon::Str(current.trim().to_string()));
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
        if !current.trim().is_empty() {
            parts.push(Toon::Str(current.trim().to_string()));
        }
        if parts.len() > 1 {
            return Toon::List(parts);
        }
        if let Some(first) = parts.into_iter().next() {
            return first;
        }
    }
    Toon::Str(v.to_string())
}

/// Extract comment prompts from a parsed poll response: the `prompts` list
/// (each item a map with prompt/selector/tag/text/target), tolerating shape
/// drift by skipping non-map entries.
#[must_use]
pub fn comments_from_poll(tree: &Toon) -> Vec<CommentPrompt> {
    let Some(prompts) = tree.get("prompts").and_then(Toon::as_list) else {
        return Vec::new();
    };
    prompts
        .iter()
        .filter_map(|item| {
            let map = item.as_map()?;
            let get_str = |k: &str| {
                map.iter()
                    .find(|(key, _)| key == k)
                    .and_then(|(_, v)| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let prompt = get_str("prompt");
            let selector = get_str("selector");
            let tag = get_str("tag");
            let text = get_str("text");
            let target = map
                .iter()
                .find(|(key, _)| key == "target")
                .and_then(|(_, v)| v.as_map())
                .and_then(|tmap| {
                    let ttype = tmap
                        .iter()
                        .find(|(k, _)| k == "type")
                        .and_then(|(_, v)| v.as_str())
                        .unwrap_or_default();
                    if ttype != "text-range" {
                        return None;
                    }
                    let ts = |k: &str| {
                        tmap.iter()
                            .find(|(key, _)| key == k)
                            .and_then(|(_, v)| v.as_str())
                            .unwrap_or_default()
                            .to_string()
                    };
                    let boundary = |name: &str| {
                        tmap.iter()
                            .find(|(key, _)| key == name)
                            .and_then(|(_, v)| v.as_map())
                            .map(|b| {
                                let get = |k: &str| {
                                    b.iter()
                                        .find(|(key, _)| key == k)
                                        .and_then(|(_, v)| v.as_str())
                                        .unwrap_or_default()
                                        .to_string()
                                };
                                // `path[2]: 3 5` parses to [3, 5];
                                // `path[1]: 4` to [4].
                                let path = get("path")
                                    .split_whitespace()
                                    .filter_map(|p| p.parse().ok())
                                    .collect();
                                crate::anchors::RangeBoundary {
                                    selector: get("selector"),
                                    path,
                                    offset: get("offset").parse().unwrap_or(0),
                                }
                            })
                            .unwrap_or_default()
                    };
                    Some(crate::anchors::CommentTarget::TextRange {
                        text: ts("text"),
                        selector: ts("selector"),
                        start: boundary("start"),
                        end: boundary("end"),
                    })
                });
            if prompt.is_empty() && selector.is_empty() && target.is_none() {
                return None;
            }
            Some(CommentPrompt {
                prompt,
                selector,
                tag,
                text,
                target,
            })
        })
        .collect()
}

/// Build the pinned lavish-axi command.
#[must_use]
pub fn lavish_command(args: &[&str]) -> Command {
    let mut cmd = Command::new("npx");
    cmd.arg("-y").arg(format!("lavish-axi@{LAVISH_VERSION}"));
    for arg in args {
        cmd.arg(arg);
    }
    cmd
}

/// Run `lavish-axi <file>` (open or resume). Set `no_open` to keep the
/// browser from launching (headless/CI).
///
/// # Errors
/// Spawn or non-zero exit.
pub fn open_session(
    artifact: &std::path::Path,
    no_open: bool,
    reopen: bool,
) -> std::io::Result<Output> {
    let file = artifact.to_string_lossy().to_string();
    let mut args: Vec<&str> = Vec::new();
    if reopen {
        args.push("--reopen");
    }
    args.push(&file);
    let mut cmd = lavish_command(&args);
    if no_open {
        cmd.env("LAVISH_AXI_NO_OPEN", "1");
    }
    cmd.output()
}

/// Run `lavish-axi end <file>`.
///
/// # Errors
/// Spawn or non-zero exit.
pub fn end_session(artifact: &std::path::Path) -> std::io::Result<Output> {
    let file = artifact.to_string_lossy().to_string();
    lavish_command(&["end", &file]).output()
}

/// Run `lavish-axi poll <file> --agent-reply <msg>` (after applying
/// feedback).
///
/// # Errors
/// Spawn or non-zero exit.
pub fn agent_reply(artifact: &std::path::Path, message: &str) -> std::io::Result<Output> {
    let file = artifact.to_string_lossy().to_string();
    lavish_command(&["poll", &file, "--agent-reply", message]).output()
}

#[cfg(test)]
mod tests {
    use super::{Toon, comments_from_poll, parse_toon};

    const OPEN_OUTPUT: &str = "session:\n  file: /tmp/x.html\n  url: \"http://127.0.0.1:4000/session/abc\"\n  status: opened\nnext_step: \"Now you must run poll.\"\n";

    #[test]
    fn parses_nested_maps_and_scalars() {
        let tree = parse_toon(OPEN_OUTPUT);
        let session = tree
            .get("session")
            .and_then(Toon::as_map)
            .expect("session map");
        assert!(
            session
                .iter()
                .any(|(k, v)| k == "status" && v.as_str() == Some("opened"))
        );
        assert_eq!(
            tree.get("next_step").and_then(Toon::as_str),
            Some("Now you must run poll.")
        );
    }

    #[test]
    fn parses_quoted_array_values() {
        let text = "help[3]: \"one\",\"two\",\"three\"\n";
        let tree = parse_toon(text);
        let help = tree.get("help").and_then(Toon::as_list).expect("list");
        assert_eq!(help.len(), 3);
        assert_eq!(help[2].as_str(), Some("three"));
    }

    #[test]
    fn parses_table_rows() {
        let text = "playbooks[2]{id,use_when}:\n  diagram,\"Explain relationships\"\n  table,\"Turn dense records\"\nnext: 1\n";
        let tree = parse_toon(text);
        let rows = tree.get("playbooks").and_then(Toon::as_list).expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].as_str(), Some("diagram,\"Explain relationships\""));
        assert_eq!(tree.get("next").and_then(Toon::as_str), Some("1"));
    }

    #[test]
    fn parses_real_captured_help_fixture() {
        let captured =
            std::fs::read_to_string("fixtures/lavish/help-output.toon").expect("captured fixture");
        let tree = parse_toon(&captured);
        assert!(tree.get("bin").is_some(), "bin key");
        assert!(tree.get("playbooks").is_some(), "playbooks table");
        let playbooks = tree.get("playbooks").and_then(Toon::as_list).expect("list");
        assert!(playbooks.len() >= 8, "{} rows", playbooks.len());
    }

    #[test]
    fn comments_from_poll_synthetic() {
        let poll = "status: feedback\ndom_snapshot: \"<html>…</html>\"\nprompts[1]:\n  prompt: tighten\n  selector: \"#wa-2\"\n  tag: div\n  text: \"proposed text\"\nnext_step: \"apply changes.\"\n";
        let tree = parse_toon(poll);
        let comments = comments_from_poll(&tree);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].selector, "#wa-2");
        assert_eq!(comments[0].prompt, "tighten");
    }

    #[test]
    fn comments_with_text_range_target() {
        let poll = "status: feedback\nprompts[1]:\n  prompt: check this\n  selector: \"#wa-1\"\n  tag: text\n  text: \"several hundred grammar\"\n  target:\n    type: text-range\n    text: \"several hundred grammar\"\n    selector: \"#wa-1\"\n    start:\n      selector: \"#wa-1\"\n      offset: 14\n    end:\n      selector: \"#wa-1\"\n      offset: 36\n";
        let tree = parse_toon(poll);
        let comments = comments_from_poll(&tree);
        assert_eq!(comments.len(), 1);
        let target = comments[0].target.as_ref().expect("target");
        match target {
            crate::anchors::CommentTarget::TextRange { text, .. } => {
                assert_eq!(text, "several hundred grammar");
            }
            crate::anchors::CommentTarget::Other => panic!("expected text-range"),
        }
    }

    /// The REAL captured round-trip payload (Phase 0 gate): three operator
    /// annotations — pane element comment, text-range on wa-1, evidence
    /// comment on ev-1. The parser must reproduce all three, and the
    /// anchors resolver must map them exactly as the operator intended.
    #[test]
    fn parses_real_captured_feedback_fixture() {
        let captured =
            std::fs::read_to_string("fixtures/lavish/poll-feedback-real.toon").expect("fixture");
        let tree = parse_toon(&captured);

        // Session status under the leading file-path line.
        // The session block is a root map (file/status/…); the poll banner
        // line before it is skipped by the parser.
        let session = tree.get("session").expect("session map");
        assert_eq!(
            session.get("status").and_then(Toon::as_str),
            Some("feedback")
        );
        assert_eq!(session.get("ended_by").and_then(Toon::as_str), Some("user"));
        assert!(
            session
                .get("file")
                .and_then(Toon::as_str)
                .is_some_and(|f| f.ends_with("spike-roundtrip.html"))
        );

        let comments = comments_from_poll(&tree);
        assert_eq!(comments.len(), 3, "{comments:?}");

        // (1) element comment rooted at the pane section.
        assert_eq!(comments[0].selector, "section#pane-new");
        assert!(comments[0].prompt.contains("whole element"));

        // (2) text-range comment on wa-1 with real path/offset values.
        assert_eq!(comments[1].selector, "div#wa-1");
        let target = comments[1].target.as_ref().expect("target");
        match target {
            crate::anchors::CommentTarget::TextRange {
                text, start, end, ..
            } => {
                assert_eq!(text, "several hundred grammar/nitpicking");
                assert_eq!(start.path, vec![4]);
                assert_eq!(start.offset, 43);
                assert_eq!(end.path, vec![4]);
                assert_eq!(end.offset, 77);
            }
            crate::anchors::CommentTarget::Other => panic!("expected text-range"),
        }

        // (3) evidence comment on ev-1.
        assert_eq!(comments[2].selector, "div#ev-1");
        assert_eq!(comments[2].prompt, "hrrrrrrm");
    }

    /// End-to-end: the real payload resolves through the anchor table the
    /// same way `wa poll` does.
    #[test]
    fn real_feedback_resolves_through_anchor_table() {
        let captured = std::fs::read_to_string("fixtures/lavish/poll-feedback-real.toon").unwrap();
        let comments = comments_from_poll(&parse_toon(&captured));
        let table: Vec<(String, String)> = vec![
            ("wa-1".into(), "L3:C0-L3:C410".into()),
            ("wa-2".into(), "L3:C0-L3:C433".into()),
            ("ev-1".into(), "ledger:Q1".into()),
        ];
        // Pane-level comment: not in the table (the operator commented on
        // the section, not a changed block) — fails loudly, never silently.
        assert!(crate::anchors::resolve_comment(&comments[0], &table).is_err());
        let text_range = crate::anchors::resolve_comment(&comments[1], &table).unwrap();
        assert_eq!(text_range.element_id, "wa-1");
        assert_eq!(text_range.wikitext_anchor, "L3:C0-L3:C410");
        assert_eq!(
            text_range.selected_text.as_deref(),
            Some("several hundred grammar/nitpicking")
        );
        let evidence = crate::anchors::resolve_comment(&comments[2], &table).unwrap();
        assert_eq!(evidence.wikitext_anchor, "ledger:Q1");
    }

    #[test]
    fn parser_never_panics_on_junk() {
        let tree = parse_toon("::: [weird\n\t  key: : :\n}}}\n");
        assert!(tree.as_map().is_some());
    }
}
