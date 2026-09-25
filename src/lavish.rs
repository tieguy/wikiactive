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
            if trimmed.is_empty() {
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
                // KV-style entries under the header form object rows
                // (e.g. `prompts[1]:` followed by `prompt: …` pairs);
                // plain scalar lines are table rows.
                let kv_like = deeper
                    .iter()
                    .any(|(_, l)| l.contains(": ") || (l.ends_with(':') && table_key(l).is_none()));
                if kv_like {
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
        out.push((
            "__line".to_string(),
            Toon::Str(strip_quotes(line).to_string()),
        ));
        *pos += 1;
    }
    out
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
                            .map(|b| crate::anchors::RangeBoundary {
                                selector: b
                                    .iter()
                                    .find(|(k, _)| k == "selector")
                                    .and_then(|(_, v)| v.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                path: Vec::new(),
                                offset: b
                                    .iter()
                                    .find(|(k, _)| k == "offset")
                                    .and_then(|(_, v)| v.as_str())
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0),
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
pub fn open_session(artifact: &std::path::Path, no_open: bool) -> std::io::Result<Output> {
    let file = artifact.to_string_lossy().to_string();
    let mut cmd = lavish_command(&[&file]);
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

    #[test]
    fn parser_never_panics_on_junk() {
        let tree = parse_toon("::: [weird\n\t  key: : :\n}}}\n");
        assert!(tree.as_map().is_some());
    }
}
