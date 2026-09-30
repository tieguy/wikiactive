//! Shared presentation for every HTML surface: the one stylesheet (the
//! review artifact embeds it, so the file on disk stays self-contained;
//! `wa serve` pages link nothing external either) and the page shell the
//! served pages share.

use std::fmt::Write as _;

/// The stylesheet. Served pages and the review artifact both embed it;
/// the served review re-injects it after the artifact's own `<style>` so
/// an artifact rendered by an older build picks up the current look.
pub const CSS: &str = include_str!("ui.css");

/// HTML-escape text.
#[must_use]
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// "1 comment" / "3 comments".
#[must_use]
pub fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Breadcrumb trail: `(label, href)` ancestors, then the current page.
#[must_use]
pub fn crumbs(trail: &[(&str, &str)], here: &str) -> String {
    let mut out = String::from("<nav class=\"crumbs\">");
    for (label, href) in trail {
        let _ = write!(out, "<a href=\"{}\">{}</a> › ", esc(href), esc(label));
    }
    let _ = write!(out, "{}</nav>", esc(here));
    out
}

/// A complete served page: shell, stylesheet, breadcrumbs, body.
#[must_use]
pub fn page(title: &str, trail: &[(&str, &str)], body: &str) -> String {
    let nav = if trail.is_empty() {
        String::new()
    } else {
        crumbs(trail, title)
    };
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} | wikiloop</title>\n<style>\n{CSS}</style>\n</head>\n\
         <body>\n<main>\n{nav}\n{body}\n</main>\n</body></html>",
        title = esc(title),
    )
}

/// Escape text and turn bare http(s) URLs into links (outcome messages
/// carry diff URLs the operator wants to click).
#[must_use]
pub fn linkify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for (j, token) in line.split(' ').enumerate() {
            if j > 0 {
                out.push(' ');
            }
            if token.starts_with("http://") || token.starts_with("https://") {
                let _ = write!(out, "<a href=\"{0}\">{0}</a>", esc(token));
            } else {
                out.push_str(&esc(token));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{linkify, plural};

    #[test]
    fn linkify_escapes_text_and_links_urls() {
        let html = linkify("published: https://example.org/w?diff=1&oldid=0 <ok>\nnext");
        assert!(
            html.contains(
                "<a href=\"https://example.org/w?diff=1&amp;oldid=0\">https://example.org/w?diff=1&amp;oldid=0</a>"
            ),
            "{html}"
        );
        assert!(html.contains("&lt;ok&gt;\nnext"), "{html}");
    }

    #[test]
    fn plural_counts() {
        assert_eq!(plural(1, "comment"), "1 comment");
        assert_eq!(plural(0, "comment"), "0 comments");
    }
}
