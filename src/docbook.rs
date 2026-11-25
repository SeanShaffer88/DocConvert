//! DocBook (a practical subset) → Markdown.
//!
//! DocBook is an XML document model, so this reader is a small XML tree
//! walker rather than a line scanner. The subset: the sectioning elements
//! (`set`, `book`, `article`, `part`, `chapter`, `section`, `sect1`–`sect5`,
//! `simplesect`, `preface`, `appendix`, `glossary`, `bibliography`,
//! `reference`, `refentry`), `title` as their headings, `para`/`simpara`
//! as paragraphs, `programlisting`/`screen`/`literallayout`/`synopsis` as
//! fenced code with their `language`, `itemizedlist`/`orderedlist`/`simplelist`
//! as lists, `variablelist` as term-and-definition paragraphs, CALS and
//! HTML-shaped tables (`tgroup`/`row`/`entry` and `tr`/`td`/`th`),
//! `blockquote`/`epigraph`, the five admonitions, `figure`/`example`
//! captions, `footnote` as a GFM footnote, and the inline set —
//! `emphasis` (with `role="bold"`), `strong`/`bold`,
//! `literal`/`code`/`filename`/`command`/`option`/`varname`/… as code
//! spans, `link`/`ulink`/`uri`/`email`/`xref`, `superscript`/`subscript`.
//!
//! Comments, processing instructions and the doctype (internal subset and
//! all) are dropped, entities are decoded (`&amp;`, `&#233;`, …) and CDATA
//! is taken verbatim. Indexing and metadata elements (`indexterm`,
//! `keywordset`, `abstract`'s siblings, `info`) contribute their text only
//! where it is prose.
//!
//! Like `rst` and `asciidoc`, everything lands as Markdown first, so every
//! target agrees with the others about what the document says.

use crate::{corrupt, CResult};

/// Refuse documents nested deeper than this. XML can nest as deep as the
/// author likes and the DLL's stack belongs to the host, so a runaway
/// document is `DC_ERR_CORRUPT` instead.
const MAX_DEPTH: usize = 128;

/// The deepest heading Markdown has.
const MAX_LEVEL: usize = 6;

/// Convert the subset described in the module docs to canonical Markdown.
pub fn to_markdown(input: &str) -> CResult<String> {
    let root = parse_document(input)?;
    let mut notes = Notes::default();
    let mut blocks = render_children(&root.children, 0, &mut notes);
    if !notes.defs.is_empty() {
        blocks.push(notes.defs.join("\n"));
    }
    let mut out = blocks.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

// ---------------------------------------------------------------- the tree

#[derive(Debug)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

#[derive(Debug)]
enum Node {
    Text(String),
    Element(Element),
}

impl Element {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

fn attach(root: &mut Element, stack: &mut [Element], element: Element) {
    let node = Node::Element(element);
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => root.children.push(node),
    }
}

fn push_text(root: &mut Element, stack: &mut [Element], text: &str) {
    if text.is_empty() {
        return;
    }
    let node = Node::Text(decode_entities(text));
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => root.children.push(node),
    }
}

/// Close the innermost element with this name, and anything still open
/// inside it. A mismatched end tag is not worth an error when the
/// document still says what it means.
fn close_element(root: &mut Element, stack: &mut Vec<Element>, name: &str) {
    let Some(at) = stack.iter().rposition(|element| element.name == name) else {
        return;
    };
    while stack.len() > at {
        let Some(done) = stack.pop() else { break };
        attach(root, stack, done);
    }
}

fn parse_document(input: &str) -> CResult<Element> {
    let mut root = Element {
        name: String::from("#document"),
        attrs: Vec::new(),
        children: Vec::new(),
    };
    let mut stack: Vec<Element> = Vec::new();
    let mut i = 0usize;
    while i < input.len() {
        let Some(found) = input[i..].find('<') else {
            push_text(&mut root, &mut stack, &input[i..]);
            break;
        };
        let open = i + found;
        if open > i {
            push_text(&mut root, &mut stack, &input[i..open]);
        }
        if input[open..].starts_with("<!--") {
            let Some(end) = input[open + 4..].find("-->") else {
                break;
            };
            i = open + 4 + end + 3;
            continue;
        }
        if input[open..].starts_with("<![CDATA[") {
            let Some(end) = input[open + 9..].find("]]>") else {
                break;
            };
            let text = input[open + 9..open + 9 + end].to_string();
            match stack.last_mut() {
                Some(parent) => parent.children.push(Node::Text(text)),
                None => root.children.push(Node::Text(text)),
            }
            i = open + 9 + end + 3;
            continue;
        }
        if input[open..].starts_with("<?") {
            let Some(end) = input[open..].find("?>") else {
                break;
            };
            i = open + end + 2;
            continue;
        }
        if input[open..].starts_with("<!") {
            // The doctype, whose internal subset may hold `>` inside `[…]`.
            let mut depth = 0usize;
            let mut j = open + 2;
            let mut end = None;
            while j < input.len() {
                match input.as_bytes()[j] {
                    b'[' => depth += 1,
                    b']' => depth = depth.saturating_sub(1),
                    b'>' if depth == 0 => {
                        end = Some(j);
                        break;
                    }
                    _ => {}
                }
                j += 1;
            }
            let Some(end) = end else { break };
            i = end + 1;
            continue;
        }
        let Some(close) = input[open..].find('>') else {
            break;
        };
        let tag = &input[open + 1..open + close];
        i = open + close + 1;
        if let Some(name) = tag.strip_prefix('/') {
            close_element(&mut root, &mut stack, name.trim());
            continue;
        }
        let self_closing = tag.trim_end().ends_with('/');
        let body = tag.trim_end().trim_end_matches('/');
        let (name, attrs) = parse_tag(body);
        if name.is_empty() {
            continue;
        }
        let element = Element {
            name,
            attrs,
            children: Vec::new(),
        };
        if self_closing {
            attach(&mut root, &mut stack, element);
            continue;
        }
        if stack.len() >= MAX_DEPTH {
            return Err(corrupt(
                "docbook: the document nests deeper than 128 elements",
            ));
        }
        stack.push(element);
    }
    while let Some(done) = stack.pop() {
        attach(&mut root, &mut stack, done);
    }
    Ok(root)
}

fn parse_tag(body: &str) -> (String, Vec<(String, String)>) {
    let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
    let name = body[..name_end].to_string();
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut rest = &body[name_end..];
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().to_lowercase();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            break;
        }
        let Some(end) = after[1..].find(quote) else {
            break;
        };
        let value = decode_entities(&after[1..1 + end]);
        if !key.is_empty() {
            attrs.push((key, value));
        }
        rest = &after[1 + end + 1..];
    }
    (name, attrs)
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else { break };
        let decoded = match &rest[1..end] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            other => numeric_entity(other),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            // An entity we do not know is left exactly as written: a
            // document that declares its own entities still reads.
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn numeric_entity(entity: &str) -> Option<char> {
    let digits = entity.strip_prefix('#')?;
    let code = match digits
        .strip_prefix('x')
        .or_else(|| digits.strip_prefix('X'))
    {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse::<u32>().ok()?,
    };
    char::from_u32(code)
}

// ------------------------------------------------------------- inline text

/// Footnote definitions, in the order the references appear.
#[derive(Debug, Default)]
struct Notes {
    defs: Vec<String>,
}

/// Append one text node to an inline run, collapsing whitespace runs —
/// keeping a single space at the edges, so words either side of an
/// `<emphasis>` stay apart.
fn push_text_node(out: &mut String, text: &str) {
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        push_escaped(out, c);
    }
    if pending_space && !out.is_empty() {
        out.push(' ');
    }
}

/// Markdown's own punctuation, escaped so DocBook prose cannot turn into
/// markup on the way out.
fn push_escaped(out: &mut String, c: char) {
    if matches!(c, '\\' | '`' | '*' | '[' | ']') {
        out.push('\\');
    }
    out.push(c);
}

fn inline_children(children: &[Node], notes: &mut Notes) -> String {
    let mut out = String::new();
    for child in children {
        match child {
            Node::Text(text) => push_text_node(&mut out, text),
            Node::Element(element) => out.push_str(&inline_element(element, notes)),
        }
    }
    out.trim().to_string()
}

fn inline_element(el: &Element, notes: &mut Notes) -> String {
    match el.name.as_str() {
        "emphasis" | "citetitle" => {
            let text = inline_children(&el.children, notes);
            if text.is_empty() {
                return String::new();
            }
            if el.attr("role") == Some("bold") {
                format!("**{text}**")
            } else {
                format!("*{text}*")
            }
        }
        "strong" | "bold" => {
            let text = inline_children(&el.children, notes);
            if text.is_empty() {
                String::new()
            } else {
                format!("**{text}**")
            }
        }
        "literal" | "code" | "function" | "classname" | "methodname" | "varname" | "filename"
        | "command" | "option" | "envar" | "parameter" | "prompt" | "userinput" | "constant"
        | "symbol" | "type" | "property" | "tag" | "markup" | "sgmltag" | "systemitem"
        | "computeroutput" | "replaceable" | "package" => {
            let text = raw_text(&el.children);
            let text = text.trim();
            if text.is_empty() {
                String::new()
            } else if text.contains('`') {
                // A code span cannot hold its own delimiter; keep the text
                // instead of writing a span that will not close.
                text.to_string()
            } else {
                format!("`{text}`")
            }
        }
        "link" | "ulink" => {
            let target = el
                .attr("url")
                .or_else(|| el.attr("href"))
                .map(|url| url.to_string())
                .or_else(|| el.attr("linkend").map(|id| format!("#{id}")));
            let text = inline_children(&el.children, notes);
            match target {
                Some(url) if !url.trim().is_empty() => {
                    let url = url.trim();
                    let label = if text.is_empty() {
                        url.to_string()
                    } else {
                        text
                    };
                    format!("[{label}]({url})")
                }
                _ => text,
            }
        }
        "uri" => {
            let url = inline_children(&el.children, notes);
            if url.is_empty() {
                String::new()
            } else {
                format!("[{url}]({url})")
            }
        }
        "email" => {
            let address = inline_children(&el.children, notes);
            if address.is_empty() {
                String::new()
            } else {
                format!("[{address}](mailto:{address})")
            }
        }
        "xref" => {
            let text = inline_children(&el.children, notes);
            match el.attr("linkend") {
                Some(id) if !text.is_empty() => format!("[{text}](#{id})"),
                Some(id) => format!("[{id}](#{id})"),
                None => text,
            }
        }
        "footnote" => {
            let number = notes.defs.len() + 1;
            let body = inline_children(&el.children, notes);
            let body = if body.is_empty() {
                "note".to_string()
            } else {
                body.replace(['\n', '\r'], " ")
            };
            notes.defs.push(format!("[^{number}]: {body}"));
            format!("[^{number}]")
        }
        "title" => {
            let text = inline_children(&el.children, notes);
            if text.is_empty() {
                String::new()
            } else {
                format!("**{text}**")
            }
        }
        // The graphic itself has no Markdown spelling; its alt text is the
        // part that carries meaning.
        "inlinemediaobject" | "imageobject" | "mediaobject" | "alt" | "textobject" => {
            inline_children(&el.children, notes)
        }
        "anchor" | "indexterm" | "indextermzone" | "keywordset" | "remark" | "comment" => {
            String::new()
        }
        _ => inline_children(&el.children, notes),
    }
}

fn raw_text(children: &[Node]) -> String {
    let mut out = String::new();
    for child in children {
        match child {
            Node::Text(text) => out.push_str(text),
            Node::Element(element) => out.push_str(&raw_text(&element.children)),
        }
    }
    out
}

// ------------------------------------------------------------- block level

/// How much deeper a sectioning element nests the document. `book` and
/// `article` are the document itself, so their title is the top heading.
fn section_delta(name: &str) -> Option<usize> {
    match name {
        "set" | "book" | "article" => Some(0),
        "part" | "preface" | "appendix" | "glossary" | "bibliography" | "reference" | "chapter"
        | "section" | "simplesect" | "sect1" | "sect2" | "sect3" | "sect4" | "sect5"
        | "refentry" | "refsect1" | "refsect2" | "refsect3" => Some(1),
        _ => None,
    }
}

fn render_children(children: &[Node], level: usize, notes: &mut Notes) -> Vec<String> {
    let nodes: Vec<&Node> = children.iter().collect();
    render_nodes(&nodes, level, notes)
}

fn render_nodes(children: &[&Node], level: usize, notes: &mut Notes) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut pending = String::new();
    for child in children {
        match child {
            Node::Text(text) => push_text_node(&mut pending, text),
            Node::Element(element) => {
                let text = pending.trim().to_string();
                pending.clear();
                if !text.is_empty() {
                    blocks.push(text);
                }
                render_element(element, level, notes, &mut blocks);
            }
        }
    }
    let text = pending.trim().to_string();
    if !text.is_empty() {
        blocks.push(text);
    }
    blocks
}

fn render_element(el: &Element, level: usize, notes: &mut Notes, blocks: &mut Vec<String>) {
    let name = el.name.as_str();
    if let Some(delta) = section_delta(name) {
        let level = (level + delta).clamp(1, MAX_LEVEL);
        let nodes: Vec<&Node> = el.children.iter().collect();
        let title_at = nodes
            .iter()
            .position(|node| matches!(node, Node::Element(child) if child.name == "title"));
        if let Some(at) = title_at {
            if let Node::Element(title) = nodes[at] {
                let text = inline_children(&title.children, notes);
                if !text.is_empty() {
                    blocks.push(format!("{} {}", "#".repeat(level), text));
                }
            }
        }
        let mut rest: Vec<&Node> = nodes.clone();
        if let Some(at) = title_at {
            rest.remove(at);
        }
        blocks.extend(render_nodes(&rest, level, notes));
        return;
    }

    match name {
        "para" | "simpara" | "formalpara" | "subtitle" => {
            let text = inline_children(&el.children, notes);
            if !text.is_empty() {
                blocks.push(text);
            }
        }
        "programlisting" | "screen" | "literallayout" | "synopsis" | "address" => {
            let language = el
                .attr("language")
                .map(sanitize_language)
                .unwrap_or_default();
            let body = raw_text(&el.children);
            let body = body.trim_matches('\n');
            blocks.push(format!("```{language}\n{body}\n```"));
        }
        "itemizedlist" | "orderedlist" | "simplelist" => {
            let ordered = name == "orderedlist";
            blocks.push(render_list(el, ordered, level, notes));
        }
        "variablelist" => render_variable_list(el, level, notes, blocks),
        "table" | "informaltable" => render_table(el, notes, blocks),
        "blockquote" | "epigraph" => {
            let inner = render_children(&el.children, level, notes).join("\n\n");
            if !inner.trim().is_empty() {
                blocks.push(quote(&inner));
            }
        }
        "note" | "tip" | "warning" | "caution" | "important" => {
            let label = match name {
                "note" => "Note",
                "tip" => "Tip",
                "warning" => "Warning",
                "caution" => "Caution",
                _ => "Important",
            };
            let body = render_children(&el.children, level, notes);
            let mut text = format!("**{label}:**");
            if !body.is_empty() {
                text.push('\n');
                text.push_str(&body.join("\n\n"));
            }
            blocks.push(quote(&text));
        }
        "figure" | "example" | "informalfigure" | "informalexample" => {
            // A figure is its caption; the graphic itself has no Markdown
            // spelling, so the caption becomes the block and the rest of
            // the figure is read as usual.
            let mut rest: Vec<&Node> = Vec::new();
            for child in &el.children {
                match child {
                    Node::Element(child_el) if child_el.name == "title" => {
                        let caption = inline_children(&child_el.children, notes);
                        if !caption.is_empty() {
                            blocks.push(format!("**{caption}**"));
                        }
                    }
                    _ => rest.push(child),
                }
            }
            blocks.extend(render_nodes(&rest, level, notes));
        }
        "indexterm" | "indextermzone" | "keywordset" | "remark" | "comment" | "anchor" | "info"
        | "bookinfo" | "articleinfo" | "chapterinfo" | "sectioninfo" | "appendixinfo"
        | "prefaceinfo" | "glossaryinfo" | "bibliographyinfo" | "refmeta" | "refentryinfo"
        | "revhistory" | "legalnotice" | "toc" | "lot" | "index" => {}
        _ => {
            let inner = render_children(&el.children, level, notes);
            blocks.extend(inner);
        }
    }
}

fn sanitize_language(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '-' || *c == '#')
        .collect()
}

fn quote(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_list(el: &Element, ordered: bool, level: usize, notes: &mut Notes) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut counter = 0usize;
    for child in &el.children {
        let Node::Element(item) = child else {
            continue;
        };
        if item.name != "listitem" && item.name != "member" {
            continue;
        }
        counter += 1;
        let marker = if ordered {
            format!("{counter}. ")
        } else {
            "- ".to_string()
        };
        let pieces = render_children(&item.children, level, notes);
        let text = pieces.join("\n\n");
        let pad = " ".repeat(marker.len());
        let mut lines = text.lines();
        let mut rendered = format!("{marker}{}", lines.next().unwrap_or(""));
        for line in lines {
            rendered.push('\n');
            if !line.trim().is_empty() {
                rendered.push_str(&pad);
            }
            rendered.push_str(line);
        }
        out.push(rendered);
    }
    out.join("\n")
}

fn render_variable_list(el: &Element, level: usize, notes: &mut Notes, blocks: &mut Vec<String>) {
    for child in &el.children {
        let Node::Element(entry) = child else {
            continue;
        };
        if entry.name != "varlistentry" {
            continue;
        }
        let mut term = String::new();
        for part in &entry.children {
            let Node::Element(part_el) = part else {
                continue;
            };
            match part_el.name.as_str() {
                "term" if term.is_empty() => term = inline_children(&part_el.children, notes),
                "listitem" => {
                    let body = render_children(&part_el.children, level, notes).join(" ");
                    let body = body.trim();
                    if body.is_empty() {
                        continue;
                    }
                    let line = if term.is_empty() {
                        body.to_string()
                    } else {
                        format!("**{term}**: {body}")
                    };
                    blocks.push(line);
                }
                _ => {}
            }
        }
    }
}

fn render_table(el: &Element, notes: &mut Notes, blocks: &mut Vec<String>) {
    for child in &el.children {
        let Node::Element(child_el) = child else {
            continue;
        };
        if child_el.name != "title" {
            continue;
        }
        let caption = inline_children(&child_el.children, notes);
        if !caption.is_empty() {
            blocks.push(format!("**{caption}**"));
        }
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    collect_rows(el, notes, &mut rows);
    if rows.is_empty() {
        return;
    }
    let rendered = crate::markdown::table(&rows);
    if !rendered.is_empty() {
        blocks.push(rendered.trim_end().to_string());
    }
}

/// Rows of a table, wherever the shape puts them: CALS (`tgroup` →
/// `row` → `entry`) or the HTML-ish `tr`/`td`/`th` a converted document
/// often carries.
fn collect_rows(el: &Element, notes: &mut Notes, rows: &mut Vec<Vec<String>>) {
    for child in &el.children {
        let Node::Element(child_el) = child else {
            continue;
        };
        match child_el.name.as_str() {
            "row" | "tr" => {
                let mut cells: Vec<String> = Vec::new();
                for cell in &child_el.children {
                    let Node::Element(cell_el) = cell else {
                        continue;
                    };
                    if cell_el.name != "entry" && cell_el.name != "td" && cell_el.name != "th" {
                        continue;
                    }
                    cells.push(inline_children(&cell_el.children, notes));
                }
                if !cells.is_empty() {
                    rows.push(cells);
                }
            }
            _ => collect_rows(child_el, notes, rows),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{convert_markdown, Flavor};

    #[test]
    fn book_chapters_and_sections_become_headings() {
        let doc = r#"<?xml version="1.0"?>
<book>
  <title>The Manual</title>
  <chapter>
    <title>Install</title>
    <para>First paragraph.</para>
    <section>
      <title>Windows</title>
      <para>Second paragraph.</para>
    </section>
  </chapter>
</book>"#;
        assert_eq!(
            to_markdown(doc).unwrap(),
            "# The Manual\n\n## Install\n\nFirst paragraph.\n\n### Windows\n\nSecond paragraph.\n"
        );
    }

    #[test]
    fn inline_markup_lands_as_markdown() {
        let doc = "<para>Use <emphasis>this</emphasis>, <emphasis role=\"bold\">that</emphasis> \
                   and <literal>code()</literal>; see <link url=\"https://example.com\">the site</link> \
                   and <ulink url=\"https://example.org\">the other</ulink>.</para>";
        let md = to_markdown(doc).unwrap();
        assert_eq!(
            md,
            "Use *this*, **that** and `code()`; see [the site](https://example.com) and \
             [the other](https://example.org).\n"
        );
        let html = convert_markdown(&md, Flavor::Html).unwrap();
        assert!(html.contains("<em>this</em>"), "{html}");
        assert!(html.contains("<strong>that</strong>"), "{html}");
        assert!(html.contains("<code>code()</code>"), "{html}");
        assert!(html.contains("href=\"https://example.com\""), "{html}");
    }

    #[test]
    fn listings_keep_their_language_and_their_bytes() {
        let doc = "<para>Before.</para><programlisting language=\"rust\">fn main() {\n    println!();\n}</programlisting>";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "Before.\n\n```rust\nfn main() {\n    println!();\n}\n```\n"
        );
    }

    #[test]
    fn lists_nest_and_count() {
        let doc = "<orderedlist><listitem><para>first</para></listitem>\
                   <listitem><para>second</para><itemizedlist><listitem><para>nested</para>\
                   </listitem></itemizedlist></listitem></orderedlist>";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "1. first\n2. second\n\n   - nested\n"
        );
    }

    #[test]
    fn tables_come_out_as_gfm() {
        let doc = r#"<informaltable>
          <tgroup cols="2">
            <thead><row><entry>Name</entry><entry>Qty</entry></row></thead>
            <tbody><row><entry>Milk</entry><entry>2</entry></row></tbody>
          </tgroup>
        </informaltable>"#;
        assert_eq!(
            to_markdown(doc).unwrap(),
            "| Name | Qty |\n| --- | --- |\n| Milk | 2 |\n"
        );
    }

    #[test]
    fn admonitions_and_quotes_are_blockquotes() {
        let doc =
            "<warning><para>Careful.</para></warning><blockquote><para>Quoted.</para></blockquote>";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "> **Warning:**\n> Careful.\n\n> Quoted.\n"
        );
    }

    #[test]
    fn footnotes_become_definitions() {
        let doc = "<para>Text<footnote><para>The note.</para></footnote> more.</para>";
        let md = to_markdown(doc).unwrap();
        assert_eq!(md, "Text[^1] more.\n\n[^1]: The note.\n");
        let html = convert_markdown(&md, Flavor::Html).unwrap();
        assert!(html.contains("footnote"), "{html}");
    }

    #[test]
    fn entities_cdata_and_prologue_are_handled() {
        let doc = "<!DOCTYPE book [\n<!ENTITY later \"set aside\">\n]>\n\
                   <?xml-stylesheet href=\"x.css\"?>\n\
                   <!-- a comment with <para>noise</para> -->\n\
                   <para>A &amp; B, caf&#233;, &#x2014; and <![CDATA[raw <b>bytes</b>]]>.</para>";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "A & B, caf\u{e9}, \u{2014} and raw <b>bytes</b>.\n"
        );
    }

    #[test]
    fn unclosed_and_unknown_elements_still_read() {
        let doc = "<para>One <emphasis>two</para><para>Three</para><widget>four</widget>";
        assert_eq!(to_markdown(doc).unwrap(), "One *two*\n\nThree\n\nfour\n");
    }

    #[test]
    fn deep_nesting_is_refused_not_a_crash() {
        let deep = format!("{}text{}", "<para>".repeat(400), "</para>".repeat(400));
        let err = to_markdown(&deep).unwrap_err();
        assert_eq!(err.0, crate::ERR_CORRUPT);
    }
}
