//! HTML → Markdown and HTML → plain text.
//!
//! A small forgiving DOM parser (elements, attributes, text, entity
//! decoding; script/style contents dropped) plus two renderers.
//! Input is expected to be roughly well-formed HTML — the element
//! set of the Markdown renderer above and common hand-written pages.

use crate::CResult;

#[derive(Debug, Clone)]
enum Node {
    Text(String),
    Elem {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    },
}

const VOID: &[&str] = &[
    "br", "hr", "img", "meta", "link", "input", "col", "wbr", "source",
];

fn parse_html(input: &str) -> CResult<Vec<Node>> {
    type OpenElem = (String, Vec<(String, String)>, Vec<Node>);
    let bytes: Vec<char> = input.chars().collect();
    let mut root: Vec<Node> = Vec::new();
    let mut stack: Vec<OpenElem> = Vec::new();
    let mut pos = 0usize;

    let push_node = |stack: &mut Vec<OpenElem>, root: &mut Vec<Node>, node: Node| {
        if let Some(top) = stack.last_mut() {
            top.2.push(node);
        } else {
            root.push(node);
        }
    };

    while pos < bytes.len() {
        if bytes[pos] == '<' {
            // Comment / doctype / PI: skip to '>'.
            if bytes[pos..].starts_with(&['<', '!', '-', '-']) {
                while pos + 2 < bytes.len()
                    && !(bytes[pos] == '-' && bytes[pos + 1] == '-' && bytes[pos + 2] == '>')
                {
                    pos += 1;
                }
                pos = (pos + 3).min(bytes.len());
                continue;
            }
            if bytes.get(pos + 1) == Some(&'!') || bytes.get(pos + 1) == Some(&'?') {
                while pos < bytes.len() && bytes[pos] != '>' {
                    pos += 1;
                }
                pos += 1;
                continue;
            }
            // Tag.
            let closing = bytes.get(pos + 1) == Some(&'/');
            let name_start = pos + if closing { 2 } else { 1 };
            let mut j = name_start;
            while j < bytes.len() && (bytes[j].is_alphanumeric() || bytes[j] == '-') {
                j += 1;
            }
            if j == name_start {
                // A stray '<' — literal text.
                push_node(&mut stack, &mut root, Node::Text("<".into()));
                pos += 1;
                continue;
            }
            let name: String = bytes[name_start..j]
                .iter()
                .collect::<String>()
                .to_lowercase();
            // Attributes until '>' (quote-aware).
            let mut attrs: Vec<(String, String)> = Vec::new();
            let mut self_closing = false;
            let mut k = j;
            while k < bytes.len() && bytes[k] != '>' {
                match bytes[k] {
                    '/' if bytes.get(k + 1) == Some(&'>') => {
                        self_closing = true;
                        k += 1;
                        break;
                    }
                    c if c.is_whitespace() => k += 1,
                    _ => {
                        let a_start = k;
                        while k < bytes.len()
                            && !bytes[k].is_whitespace()
                            && bytes[k] != '='
                            && bytes[k] != '>'
                            && bytes[k] != '/'
                        {
                            k += 1;
                        }
                        let a_name: String =
                            bytes[a_start..k].iter().collect::<String>().to_lowercase();
                        while k < bytes.len() && bytes[k].is_whitespace() {
                            k += 1;
                        }
                        let mut a_val = String::new();
                        if bytes.get(k) == Some(&'=') {
                            k += 1;
                            while k < bytes.len() && bytes[k].is_whitespace() {
                                k += 1;
                            }
                            if let Some(&q) = bytes.get(k) {
                                if q == '"' || q == '\'' {
                                    k += 1;
                                    let v_start = k;
                                    while k < bytes.len() && bytes[k] != q {
                                        k += 1;
                                    }
                                    a_val = bytes[v_start..k].iter().collect();
                                    k += 1;
                                } else {
                                    let v_start = k;
                                    while k < bytes.len()
                                        && !bytes[k].is_whitespace()
                                        && bytes[k] != '>'
                                    {
                                        k += 1;
                                    }
                                    a_val = bytes[v_start..k].iter().collect();
                                }
                            }
                        }
                        if !a_name.is_empty() {
                            attrs.push((a_name, decode_entities(&a_val)));
                        }
                    }
                }
            }
            pos = (k + 1).min(bytes.len());

            if closing {
                // Pop until the matching open tag (implicit closes).
                if let Some(idx) = stack.iter().rposition(|(n, _, _)| *n == name) {
                    while stack.len() > idx {
                        let (n, a, children) = stack.pop().unwrap();
                        push_node(
                            &mut stack,
                            &mut root,
                            Node::Elem {
                                name: n,
                                attrs: a,
                                children,
                            },
                        );
                    }
                }
                continue;
            }
            if name == "script" || name == "style" {
                // Drop element + contents up to the close tag.
                let close = format!("</{name}");
                let rest: String = bytes[pos..].iter().collect();
                if let Some(found) = rest.to_lowercase().find(&close) {
                    pos += found;
                    while pos < bytes.len() && bytes[pos] != '>' {
                        pos += 1;
                    }
                    pos += 1;
                } else {
                    pos = bytes.len();
                }
                continue;
            }
            if VOID.contains(&name.as_str()) || self_closing {
                push_node(
                    &mut stack,
                    &mut root,
                    Node::Elem {
                        name,
                        attrs,
                        children: Vec::new(),
                    },
                );
            } else {
                stack.push((name, attrs, Vec::new()));
            }
        } else {
            // Text run to the next '<'.
            let start = pos;
            while pos < bytes.len() && bytes[pos] != '<' {
                pos += 1;
            }
            let text: String = bytes[start..pos].iter().collect();
            push_node(&mut stack, &mut root, Node::Text(decode_entities(&text)));
        }
    }
    while let Some((n, a, children)) = stack.pop() {
        push_node(
            &mut stack,
            &mut root,
            Node::Elem {
                name: n,
                attrs: a,
                children,
            },
        );
    }
    Ok(root)
}

pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let semi = after.find(';');
        match semi {
            Some(end) if end <= 10 => {
                let entity = &after[..end];
                let decoded = match entity {
                    "amp" => Some("&".to_string()),
                    "lt" => Some("<".to_string()),
                    "gt" => Some(">".to_string()),
                    "quot" => Some("\"".to_string()),
                    "apos" => Some("'".to_string()),
                    "nbsp" => Some("\u{A0}".to_string()),
                    "mdash" => Some("—".to_string()),
                    "ndash" => Some("–".to_string()),
                    "hellip" => Some("…".to_string()),
                    "copy" => Some("©".to_string()),
                    _ => {
                        if let Some(num) = entity.strip_prefix('#') {
                            let cp = if let Some(hex) =
                                num.strip_prefix('x').or_else(|| num.strip_prefix('X'))
                            {
                                u32::from_str_radix(hex, 16).ok()
                            } else {
                                num.parse::<u32>().ok()
                            };
                            cp.and_then(char::from_u32).map(|c| c.to_string())
                        } else {
                            None
                        }
                    }
                };
                match decoded {
                    Some(d) => {
                        out.push_str(&d);
                        rest = &after[end + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = after;
                    }
                }
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_ws {
                out.push(' ');
            }
            last_ws = true;
        } else {
            out.push(c);
            last_ws = false;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Markdown renderer
// ---------------------------------------------------------------------------

pub fn html_to_markdown(input: &str) -> CResult<String> {
    let nodes = parse_html(input)?;
    let mut out = String::new();
    render_blocks_md(&nodes, &mut out, 0);
    Ok(out.trim().to_string() + "\n")
}

fn render_blocks_md(nodes: &[Node], out: &mut String, depth: usize) {
    for node in nodes {
        match node {
            Node::Text(t) => {
                let t = collapse_ws(t);
                if !t.trim().is_empty() {
                    out.push_str(t.trim());
                    out.push_str("\n\n");
                }
            }
            Node::Elem {
                name,
                attrs,
                children,
            } => {
                match name.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        let level = name[1..].parse::<usize>().unwrap_or(1);
                        out.push_str(&"#".repeat(level));
                        out.push(' ');
                        out.push_str(&inline_md(children));
                        out.push_str("\n\n");
                    }
                    "p" => {
                        out.push_str(&inline_md(children));
                        out.push_str("\n\n");
                    }
                    "pre" => {
                        out.push_str("```\n");
                        out.push_str(&raw_text(children));
                        out.push_str("\n```\n\n");
                    }
                    "blockquote" => {
                        let mut inner = String::new();
                        render_blocks_md(children, &mut inner, depth);
                        for line in inner.trim().lines() {
                            out.push_str("> ");
                            out.push_str(line);
                            out.push('\n');
                        }
                        out.push('\n');
                    }
                    "ul" | "ol" => {
                        render_list_md(children, name == "ol", out, depth);
                        out.push('\n');
                    }
                    "hr" => out.push_str("---\n\n"),
                    "table" => {
                        render_table_md(children, out);
                    }
                    "div" | "section" | "article" | "main" | "header" | "footer" | "body"
                    | "html" | "figure" | "figcaption" => {
                        render_blocks_md(children, out, depth);
                    }
                    _ => {
                        // Inline element at block level.
                        let s = inline_md(std::slice::from_ref(node));
                        if !s.trim().is_empty() {
                            out.push_str(s.trim());
                            out.push_str("\n\n");
                        }
                    }
                }
                let _ = attrs;
            }
        }
    }
}

fn render_list_md(items: &[Node], ordered: bool, out: &mut String, depth: usize) {
    let mut n = 0usize;
    for item in items {
        if let Node::Elem { name, children, .. } = item {
            if name == "li" {
                n += 1;
                let marker = if ordered {
                    format!("{n}. ")
                } else {
                    "- ".to_string()
                };
                out.push_str(&"  ".repeat(depth));
                out.push_str(&marker);
                // li children: inline content + possible nested list.
                let mut first = true;
                for child in children {
                    match child {
                        Node::Elem { name: cn, .. } if cn == "ul" || cn == "ol" => {
                            out.push('\n');
                            if let Node::Elem {
                                children: gc,
                                name: gn,
                                ..
                            } = child
                            {
                                render_list_md(gc, gn == "ol", out, depth + 1);
                            }
                        }
                        _ => {
                            if first {
                                out.push_str(&inline_md(std::slice::from_ref(child)));
                                first = false;
                            } else {
                                out.push_str(&inline_md(std::slice::from_ref(child)));
                            }
                        }
                    }
                }
                out.push('\n');
            }
        }
    }
}

fn render_table_md(children: &[Node], out: &mut String) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    fn collect_rows(nodes: &[Node], rows: &mut Vec<Vec<String>>) {
        for node in nodes {
            if let Node::Elem { name, children, .. } = node {
                match name.as_str() {
                    "tr" => {
                        let mut cells = Vec::new();
                        for cell in children {
                            if let Node::Elem {
                                name: cn,
                                children: cc,
                                ..
                            } = cell
                            {
                                if cn == "td" || cn == "th" {
                                    cells.push(inline_md(cc).replace('|', "\\|"));
                                }
                            }
                        }
                        if !cells.is_empty() {
                            rows.push(cells);
                        }
                    }
                    "thead" | "tbody" | "tfoot" => collect_rows(children, rows),
                    _ => {}
                }
            }
        }
    }
    collect_rows(children, &mut rows);
    if rows.is_empty() {
        return;
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for (i, row) in rows.iter().enumerate() {
        let mut cells = row.clone();
        cells.resize(cols, String::new());
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
        if i == 0 {
            out.push('|');
            for _ in 0..cols {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out.push('\n');
}

fn inline_md(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&collapse_ws(t)),
            Node::Elem {
                name,
                attrs,
                children,
            } => match name.as_str() {
                "strong" | "b" => {
                    out.push_str("**");
                    out.push_str(&inline_md(children));
                    out.push_str("**");
                }
                "em" | "i" => {
                    out.push('*');
                    out.push_str(&inline_md(children));
                    out.push('*');
                }
                "del" | "s" => {
                    out.push_str("~~");
                    out.push_str(&inline_md(children));
                    out.push_str("~~");
                }
                "code" => {
                    out.push('`');
                    out.push_str(&raw_text(children));
                    out.push('`');
                }
                "a" => {
                    let href = attr(attrs, "href").unwrap_or("");
                    let text = inline_md(children);
                    if href.is_empty() {
                        out.push_str(&text);
                    } else {
                        out.push_str(&format!("[{text}]({href})"));
                    }
                }
                "img" => {
                    let src = attr(attrs, "src").unwrap_or("");
                    let alt = attr(attrs, "alt").unwrap_or("");
                    out.push_str(&format!("![{alt}]({src})"));
                }
                "br" => out.push('\n'),
                _ => out.push_str(&inline_md(children)),
            },
        }
    }
    out.trim().to_string()
}

fn raw_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Elem { children, .. } => out.push_str(&raw_text(children)),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Plain-text renderer
// ---------------------------------------------------------------------------

pub fn html_to_text(input: &str) -> CResult<String> {
    let nodes = parse_html(input)?;
    let mut out = String::new();
    render_blocks_text(&nodes, &mut out);
    // Normalize: cap blank runs at one empty line.
    let mut cleaned = String::new();
    let mut blanks = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        cleaned.push_str(line);
        cleaned.push('\n');
    }
    Ok(cleaned.trim().to_string() + "\n")
}

fn render_blocks_text(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(t) => {
                let t = collapse_ws(t);
                if !t.trim().is_empty() {
                    out.push_str(t.trim());
                    out.push('\n');
                }
            }
            Node::Elem { name, children, .. } => match name.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" => {
                    out.push_str(&inline_text(children));
                    out.push_str("\n\n");
                }
                "pre" => {
                    out.push_str(&raw_text(children));
                    out.push_str("\n\n");
                }
                "ul" | "ol" => {
                    let mut n = 0;
                    for item in children {
                        if let Node::Elem {
                            name: iname,
                            children: ic,
                            ..
                        } = item
                        {
                            if iname == "li" {
                                n += 1;
                                let marker = if name == "ol" {
                                    format!("{n}. ")
                                } else {
                                    "- ".into()
                                };
                                out.push_str(&marker);
                                out.push_str(&inline_text(ic));
                                out.push('\n');
                            }
                        }
                    }
                    out.push('\n');
                }
                "blockquote" => {
                    let mut inner = String::new();
                    render_blocks_text(children, &mut inner);
                    for line in inner.trim().lines() {
                        out.push_str("> ");
                        out.push_str(line);
                        out.push('\n');
                    }
                    out.push('\n');
                }
                "hr" => out.push_str("---\n\n"),
                "table" => {
                    fn rows_of(nodes: &[Node], out: &mut String) {
                        for node in nodes {
                            if let Node::Elem { name, children, .. } = node {
                                match name.as_str() {
                                    "tr" => {
                                        let mut cells = Vec::new();
                                        for cell in children {
                                            if let Node::Elem { children: cc, .. } = cell {
                                                cells.push(inline_text(cc));
                                            }
                                        }
                                        out.push_str(&cells.join(" | "));
                                        out.push('\n');
                                    }
                                    "thead" | "tbody" | "tfoot" => rows_of(children, out),
                                    _ => {}
                                }
                            }
                        }
                    }
                    rows_of(children, out);
                    out.push('\n');
                }
                "script" | "style" => {}
                _ => render_blocks_text(children, out),
            },
        }
    }
}

fn inline_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&collapse_ws(t)),
            Node::Elem {
                name,
                attrs,
                children,
            } => match name.as_str() {
                "a" => {
                    let text = inline_text(children);
                    let href = attr(attrs, "href").unwrap_or("");
                    if href.is_empty() || href == text {
                        out.push_str(&text);
                    } else {
                        out.push_str(&format!("{text} ({href})"));
                    }
                }
                "br" => out.push('\n'),
                "img" => {
                    let alt = attr(attrs, "alt").unwrap_or("");
                    if !alt.is_empty() {
                        out.push_str(&format!("[image: {alt}]"));
                    }
                }
                _ => out.push_str(&inline_text(children)),
            },
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_md_basics() {
        let md = html_to_markdown(
            "<h1>Title</h1><p>Some <strong>bold</strong> and <a href=\"https://e.com\">a link</a>.</p><ul><li>one</li><li>two</li></ul>",
        )
        .unwrap();
        assert!(md.contains("# Title"), "{md}");
        assert!(
            md.contains("Some **bold** and [a link](https://e.com)."),
            "{md}"
        );
        assert!(md.contains("- one"), "{md}");
        assert!(md.contains("- two"), "{md}");
    }

    #[test]
    fn entities_and_script_dropping() {
        let text = html_to_text(
            "<p>Fish &amp; Chips &#8212; tasty</p><script>var x = 1;</script><p>Second&nbsp;para</p>",
        )
        .unwrap();
        assert!(text.contains("Fish & Chips — tasty"), "{text}");
        assert!(!text.contains("var x"), "{text}");
        assert!(text.contains("Second"), "{text}");
    }

    #[test]
    fn table_to_md() {
        let md = html_to_markdown(
            "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
        )
        .unwrap();
        assert!(md.contains("| A | B |"), "{md}");
        assert!(md.contains("| --- | --- |"), "{md}");
        assert!(md.contains("| 1 | 2 |"), "{md}");
    }
}
