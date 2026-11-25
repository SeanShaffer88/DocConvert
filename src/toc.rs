//! Tables of contents: the headings of a Markdown or HTML document as
//! nested links, rendered as Markdown, HTML or JSON.
//!
//! Sources are Markdown, HTML, reStructuredText, AsciiDoc and OPML —
//! the last three are read as Markdown first, so their anchors match the
//! ids a conversion writes.
//!
//! The anchors come from `markdown::slugify` (and its per-document
//! dedupe), the same function that gives Markdown→HTML headings their
//! `id="…"` when `{"heading_ids":true}` is set — so a generated TOC
//! points at anchors the conversion actually wrote.

use crate::convert::normalize_format;
use crate::html;
use crate::json::Json;
use crate::markdown::{self, Slugs};
use crate::{CResult, ERR_INVALID_ARG, ERR_NOT_SUPPORTED};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub level: usize,
    pub text: String,
    pub anchor: String,
}

/// Read the headings of `input` — Markdown, HTML, reStructuredText or
/// AsciiDoc.
pub fn entries(input: &str, from: &str) -> CResult<Vec<Entry>> {
    let from = normalize_format(from)?;
    let mut slugs = Slugs::default();
    if from == "html" {
        // HTML keeps the ids the document already carries.
        return Ok(html::headings(input)?
            .into_iter()
            .map(|(level, text, id)| {
                let anchor = match id {
                    Some(id) if !id.trim().is_empty() => id,
                    _ => slugs.next(&text),
                };
                Entry {
                    level,
                    text,
                    anchor,
                }
            })
            .collect());
    }
    // Everything else is read as Markdown first, exactly as it is for
    // conversion, so the anchors stay consistent with the output.
    let body = match from.as_str() {
        "markdown" => input.to_string(),
        "rst" => crate::rst::to_markdown(input)?,
        "asciidoc" => crate::asciidoc::to_markdown(input)?,
        other => {
            return Err((
                ERR_NOT_SUPPORTED,
                format!("toc: a {other} document has no headings to collect"),
            ))
        }
    };
    Ok(markdown::headings(&body)?
        .into_iter()
        .map(|(level, text)| {
            let anchor = slugs.next(&text);
            Entry {
                level,
                text,
                anchor,
            }
        })
        .collect())
}

/// `entries` plus a rendering: `markdown`, `html` or `json`.
pub fn render_from(input: &str, from: &str, flavor: &str) -> CResult<String> {
    let entries = entries(input, from)?;
    render(&entries, flavor)
}

pub fn render(entries: &[Entry], flavor: &str) -> CResult<String> {
    match flavor.trim().to_lowercase().as_str() {
        "markdown" | "md" => Ok(render_markdown(entries)),
        "html" => Ok(render_html(entries)),
        "json" => Ok(to_json(entries)),
        other => Err((
            ERR_INVALID_ARG,
            format!("unknown toc format '{other}' (expected markdown, html or json)"),
        )),
    }
}

/// A nested Markdown list, indented two spaces per level.
pub fn render_markdown(entries: &[Entry]) -> String {
    let levels = depths(entries);
    let mut out = String::new();
    for (entry, depth) in entries.iter().zip(levels.iter()) {
        out.push_str(&"  ".repeat(*depth));
        out.push_str(&format!("- [{}](#{})\n", entry.text, entry.anchor));
    }
    out
}

/// Nested `<ul>`/`<li>` markup.
pub fn render_html(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let levels = depths(entries);
    let mut out = String::from("<ul>");
    let mut open = 0usize;
    for (index, (entry, depth)) in entries.iter().zip(levels.iter()).enumerate() {
        if index > 0 {
            if *depth > open {
                while open < *depth {
                    out.push_str("\n<ul>");
                    open += 1;
                }
            } else {
                while open > *depth {
                    out.push_str("</li>\n</ul>");
                    open -= 1;
                }
                out.push_str("</li>");
            }
        }
        out.push_str(&format!(
            "\n<li><a href=\"#{}\">{}</a>",
            entry.anchor,
            crate::escape_html(&entry.text)
        ));
    }
    while open > 0 {
        out.push_str("</li>\n</ul>");
        open -= 1;
    }
    out.push_str("</li>\n</ul>");
    out
}

pub fn to_json(entries: &[Entry]) -> String {
    let items: Vec<Json> = entries
        .iter()
        .map(|entry| {
            Json::Object(vec![
                ("level".to_string(), Json::Num(entry.level.to_string())),
                ("text".to_string(), Json::Str(entry.text.clone())),
                ("anchor".to_string(), Json::Str(entry.anchor.clone())),
            ])
        })
        .collect();
    Json::Array(items).to_compact()
}

/// Nesting depth per heading: one level deeper when the heading is
/// deeper than the one before it, back out on the way up.
fn depths(entries: &[Entry]) -> Vec<usize> {
    let mut out = Vec::with_capacity(entries.len());
    let mut depth = 0usize;
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            let previous = entries[index - 1].level;
            if entry.level > previous {
                depth += 1;
            } else if entry.level < previous {
                depth = depth.saturating_sub(previous - entry.level);
            }
        }
        out.push(depth);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Title\n\n## Install\n\n### Linux\n\n### macOS\n\n## Usage\n\n#### Odd\n";

    #[test]
    fn markdown_headings_become_a_tree() {
        let entries = entries(DOC, "markdown").unwrap();
        assert_eq!(entries.len(), 6);
        assert_eq!(entries[0].level, 1);
        assert_eq!(entries[0].anchor, "title");
        assert_eq!(entries[2].anchor, "linux");
        let md = render_markdown(&entries);
        assert_eq!(
            md,
            "- [Title](#title)\n  - [Install](#install)\n    - [Linux](#linux)\n    \
             - [macOS](#macos)\n  - [Usage](#usage)\n    - [Odd](#odd)\n"
        );
    }

    #[test]
    fn html_toc_is_balanced() {
        let entries = entries(DOC, "markdown").unwrap();
        let html = render_html(&entries);
        assert!(html.starts_with("<ul>"), "{html}");
        assert!(html.ends_with("</ul>"), "{html}");
        assert_eq!(html.matches("<ul>").count(), html.matches("</ul>").count());
        assert_eq!(html.matches("<li>").count(), html.matches("</li>").count());
        assert!(html.contains("<a href=\"#linux\">Linux</a>"), "{html}");
        assert!(render_html(&[]).is_empty());
    }

    #[test]
    fn json_entries_and_duplicates() {
        let entries = entries("# Same\n\n# Same\n", "markdown").unwrap();
        assert_eq!(entries[0].anchor, "same");
        assert_eq!(entries[1].anchor, "same-1");
        let json = to_json(&entries);
        assert_eq!(
            json,
            "[{\"level\":1,\"text\":\"Same\",\"anchor\":\"same\"},\
             {\"level\":1,\"text\":\"Same\",\"anchor\":\"same-1\"}]"
        );
    }

    #[test]
    fn html_input_keeps_its_ids() {
        let entries = entries(
            "<h1>Doc</h1><h2 id=\"custom\">Second</h2><h3>Third</h3>",
            "html",
        )
        .unwrap();
        assert_eq!(entries[0].anchor, "doc");
        assert_eq!(entries[1].anchor, "custom");
        assert_eq!(entries[2].anchor, "third");
        assert_eq!(entries[2].level, 3);
    }

    #[test]
    fn rst_and_asciidoc_headings() {
        let rst = "Guide\n=====\n\nInstall\n-------\n";
        let entries = super::entries(rst, "rst").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].anchor, "guide");
        assert_eq!(entries[1].anchor, "install");

        let adoc = "= Guide\n\n== Install\n";
        let entries = super::entries(adoc, "adoc").unwrap();
        assert_eq!(entries[1].level, 2);
        assert_eq!(
            render_markdown(&entries),
            "- [Guide](#guide)\n  - [Install](#install)\n"
        );
    }

    #[test]
    fn unknown_inputs_are_rejected() {
        assert_eq!(entries("a\tb", "csv").unwrap_err().0, ERR_NOT_SUPPORTED);
        let entries = entries("# A\n", "markdown").unwrap();
        assert_eq!(render(&entries, "pdf").unwrap_err().0, ERR_INVALID_ARG);
        assert!(render(&entries, "JSON").is_ok());
    }
}
