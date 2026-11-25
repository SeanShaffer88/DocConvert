//! OPML: outlines in, outlines out.
//!
//! OPML is the outline interchange format (podcast clients, feed
//! readers, note apps): a `<body>` of nested `<outline text="…">`
//! elements, with `_note` for the body text under an item. Reading uses
//! the HTML reader — OPML is XML with the same shape — and writing emits
//! escaped attributes, so any document's heading outline round-trips as
//! nested bullets.
//!
//! `opml → markdown` is a nested bullet list; `markdown → opml` and
//! `html → opml` build the outline from the document's headings, which
//! is the mapping that keeps both directions lossless in structure.

use crate::html::{parse_html, Node};
use crate::CResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    pub text: String,
    pub note: Option<String>,
    pub children: Vec<Outline>,
}

/// Parse an OPML document into its outline tree.
pub fn parse(input: &str) -> CResult<Vec<Outline>> {
    let nodes = parse_html(input)?;
    Ok(walk_body(&nodes))
}

fn walk_body(nodes: &[Node]) -> Vec<Outline> {
    // Prefer `<body>` when the document has one; otherwise take whatever
    // outlines are reachable (some tools omit the wrapper).
    if let Some(body) = find_element(nodes, "body") {
        return outlines(body);
    }
    outlines(nodes)
}

fn find_element<'a>(nodes: &'a [Node], name: &str) -> Option<&'a [Node]> {
    for node in nodes {
        if let Node::Elem {
            name: element,
            children,
            ..
        } = node
        {
            if element == name {
                return Some(children);
            }
            if let Some(found) = find_element(children, name) {
                return Some(found);
            }
        }
    }
    None
}

fn outlines(nodes: &[Node]) -> Vec<Outline> {
    let mut out = Vec::new();
    for node in nodes {
        let Node::Elem {
            name,
            attrs,
            children,
        } = node
        else {
            continue;
        };
        if name == "outline" {
            out.push(Outline {
                text: attr(attrs, "text").unwrap_or_default().to_string(),
                note: attr(attrs, "_note")
                    .or_else(|| attr(attrs, "note"))
                    .map(str::to_string),
                children: outlines(children),
            });
        } else {
            // `<head>`, `<body>` or any other wrapper: keep descending.
            out.extend(outlines(children));
        }
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// The outline tree as a nested Markdown bullet list.
pub fn to_markdown(input: &str) -> CResult<String> {
    let tree = parse(input)?;
    if tree.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::new();
    render_markdown(&tree, 0, &mut out);
    Ok(out)
}

fn render_markdown(items: &[Outline], depth: usize, out: &mut String) {
    for item in items {
        out.push_str(&"  ".repeat(depth));
        out.push_str("- ");
        out.push_str(&item.text.replace('\n', " "));
        out.push('\n');
        if let Some(note) = &item.note {
            let note = note.trim();
            if !note.is_empty() {
                for line in note.lines() {
                    out.push_str(&"  ".repeat(depth + 1));
                    out.push_str(line.trim());
                    out.push('\n');
                }
            }
        }
        render_markdown(&item.children, depth + 1, out);
    }
}

/// An OPML document from a document's headings (level, text), nested the
/// way the headings nest.
pub fn from_headings(headings: &[(usize, String)], title: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<opml version=\"2.0\">\n");
    out.push_str("  <head>\n");
    out.push_str(&format!(
        "    <title>{}</title>\n",
        escape_attr(title.unwrap_or("Outline"))
    ));
    out.push_str("  </head>\n");
    out.push_str("  <body>\n");
    render_xml(headings, 0, 2, &mut out);
    out.push_str("  </body>\n");
    out.push_str("</opml>\n");
    out
}

/// Emit the headings from `start` on as one nested level, returning the
/// index after the run.
fn render_xml(headings: &[(usize, String)], start: usize, depth: usize, out: &mut String) -> usize {
    let pad = "  ".repeat(depth);
    let mut index = start;
    while index < headings.len() {
        let (level, text) = &headings[index];
        // A heading at or above the level that opened this run belongs to
        // an outer level.
        if index > start && *level <= headings[start].0 {
            break;
        }
        let mut children_end = index + 1;
        while children_end < headings.len() && headings[children_end].0 > *level {
            children_end += 1;
        }
        if children_end > index + 1 {
            out.push_str(&format!("{pad}<outline text=\"{}\">\n", escape_attr(text)));
            render_xml(headings, index + 1, depth + 1, out);
            out.push_str(&format!("{pad}</outline>\n"));
        } else {
            out.push_str(&format!("{pad}<outline text=\"{}\"/>\n", escape_attr(text)));
        }
        index = children_end;
    }
    index
}

fn escape_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{convert_markdown, Flavor};

    const DOC: &str = "<?xml version=\"1.0\"?>\n<opml version=\"2.0\">\n  <head>\
                        <title>Demo</title></head>\n  <body>\n    \
                        <outline text=\"One\">\n      <outline text=\"Two\" _note=\"a note\"/>\n    \
                        </outline>\n  </body>\n</opml>\n";

    #[test]
    fn reads_the_tree() {
        let tree = parse(DOC).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].text, "One");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].text, "Two");
        assert_eq!(tree[0].children[0].note.as_deref(), Some("a note"));
    }

    #[test]
    fn markdown_is_nested_bullets() {
        let md = to_markdown(DOC).unwrap();
        assert_eq!(md, "- One\n  - Two\n    a note\n");
        let html = convert_markdown(&md, Flavor::Html).unwrap();
        assert!(html.contains("<li>One"), "{html}");
        assert!(html.contains("<li>Two"), "{html}");
    }

    #[test]
    fn headings_become_an_outline() {
        let headings = vec![
            (1usize, "Guide".to_string()),
            (2, "Install".to_string()),
            (3, "Windows".to_string()),
            (2, "Run".to_string()),
        ];
        let opml = from_headings(&headings, Some("Docs & more"));
        assert!(opml.contains("<title>Docs &amp; more</title>"), "{opml}");
        assert!(opml.contains("<outline text=\"Guide\">"), "{opml}");
        assert!(opml.contains("<outline text=\"Windows\"/>"), "{opml}");
        // And it reads back to the same shape.
        let tree = parse(&opml).unwrap();
        assert_eq!(tree[0].text, "Guide");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].children[0].text, "Windows");
    }

    #[test]
    fn empty_documents_stay_empty() {
        assert_eq!(to_markdown("<opml><body/></opml>").unwrap(), "");
        assert!(parse("not opml at all").unwrap().is_empty());
    }
}
