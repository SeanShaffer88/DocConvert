//! Markdown → HTML and Markdown → plain text.
//!
//! Covers the CommonMark constructs documents actually use: ATX and
//! setext headings, paragraphs, emphasis/strong/strikethrough, code
//! spans and fenced blocks, links and images, autolinks, blockquotes,
//! ordered/unordered lists with nesting, horizontal rules, and GFM
//! pipe tables. Raw HTML inside Markdown is escaped, not passed
//! through (documented: the output is meant to be safe by default).

use crate::{escape_html, CResult};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Flavor {
    Html,
    Text,
}

pub fn convert_markdown(input: &str, flavor: Flavor) -> CResult<String> {
    let lines: Vec<&str> = input.lines().collect();
    let blocks = parse_blocks(&lines);
    let rendered: Vec<String> = blocks.iter().map(|b| render_block(b, flavor)).collect();
    let sep = match flavor {
        Flavor::Html => "\n",
        Flavor::Text => "\n\n",
    };
    let mut out = rendered.join(sep);
    if flavor == Flavor::Text && !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

#[derive(Debug, Clone)]
enum Block {
    Heading(usize, String),
    Paragraph(String),
    Code(String, String), // info, body
    Quote(Vec<Block>),
    List(bool, Vec<Vec<Block>>), // ordered, items
    Rule,
    Table(Vec<String>, Vec<char>, Vec<Vec<String>>), // headers, aligns ('l','c','r'), rows
}

fn parse_blocks(lines: &[&str]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        // Fenced code.
        if let Some(info) = fence_start(trimmed) {
            let mut body = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                body.push_str(lines[i]);
                body.push('\n');
                i += 1;
            }
            i += 1; // closing fence (or EOF)
            blocks.push(Block::Code(info, body));
            continue;
        }
        // ATX heading.
        if let Some((level, text)) = atx_heading(trimmed) {
            blocks.push(Block::Heading(level, text));
            i += 1;
            continue;
        }
        // Blockquote: gather consecutive > lines, parse the inside.
        if trimmed.starts_with('>') {
            let mut inner_lines: Vec<String> = Vec::new();
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let l = lines[i].trim_start().trim_start_matches('>');
                inner_lines.push(l.strip_prefix(' ').unwrap_or(l).to_string());
                i += 1;
            }
            let refs: Vec<&str> = inner_lines.iter().map(|s| s.as_str()).collect();
            blocks.push(Block::Quote(parse_blocks(&refs)));
            continue;
        }
        // List.
        if list_marker(line).is_some() {
            let (list, consumed) = parse_list(lines, i);
            blocks.push(list);
            i += consumed;
            continue;
        }
        // Table: header row followed by a delimiter row.
        if trimmed.contains('|') && i + 1 < lines.len() && is_table_delimiter(lines[i + 1]) {
            let headers = split_row(trimmed);
            let aligns = delimiter_aligns(lines[i + 1]);
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() {
                let t = lines[i].trim();
                if t.is_empty() || !t.contains('|') {
                    break;
                }
                rows.push(split_row(t));
                i += 1;
            }
            blocks.push(Block::Table(headers, aligns, rows));
            continue;
        }
        // Horizontal rule.
        if is_rule(trimmed) {
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }
        // Paragraph (with setext check on the following line).
        let mut para = vec![line.to_string()];
        i += 1;
        let mut setext_level = 0;
        while i < lines.len() {
            let t = lines[i].trim();
            if t.is_empty() {
                break;
            }
            if t.chars().all(|c| c == '=') && !t.is_empty() {
                setext_level = 1;
                i += 1;
                break;
            }
            if t.len() >= 3 && t.chars().all(|c| c == '-') {
                setext_level = 2;
                i += 1;
                break;
            }
            if fence_start(t).is_some()
                || atx_heading(t).is_some()
                || t.starts_with('>')
                || list_marker(lines[i]).is_some()
                || is_rule(t)
            {
                break;
            }
            para.push(lines[i].to_string());
            i += 1;
        }
        let text = para.join("\n");
        if setext_level > 0 {
            blocks.push(Block::Heading(setext_level, text));
        } else {
            blocks.push(Block::Paragraph(text));
        }
    }
    blocks
}

fn fence_start(trimmed: &str) -> Option<String> {
    trimmed
        .strip_prefix("```")
        .map(|rest| rest.trim().to_string())
}

fn atx_heading(trimmed: &str) -> Option<(usize, String)> {
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim().to_string();
    Some((hashes, text))
}

fn is_rule(trimmed: &str) -> bool {
    let t: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3
        && (t.chars().all(|c| c == '-')
            || t.chars().all(|c| c == '*')
            || t.chars().all(|c| c == '_'))
}

fn is_table_delimiter(line: &str) -> bool {
    let t = line.trim();
    t.contains('-')
        && t.chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace())
}

fn delimiter_aligns(line: &str) -> Vec<char> {
    split_row(line.trim())
        .iter()
        .map(|cell| {
            let c = cell.trim();
            match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => 'c',
                (false, true) => 'r',
                _ => 'l',
            }
        })
        .collect()
}

fn split_row(line: &str) -> Vec<String> {
    let t = line.trim().trim_matches('|');
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// Returns (ordered, marker width incl. following space) for a list
/// item line at any indent.
fn list_marker(line: &str) -> Option<(bool, usize, usize)> {
    let indent = line.len() - line.trim_start().len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let mut chars = rest.chars();
    match chars.next()? {
        '-' | '*' | '+' => {
            let after = &rest[1..];
            if after.starts_with(' ') || after.is_empty() {
                Some((false, indent, 2))
            } else {
                None
            }
        }
        c if c.is_ascii_digit() => {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            let after = &rest[digits.len()..];
            if (after.starts_with('.') || after.starts_with(')'))
                && (after.len() == 1 || after[1..].starts_with(' '))
            {
                Some((true, indent, digits.len() + 2))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_list(lines: &[&str], start: usize) -> (Block, usize) {
    let (ordered, base_indent, _) = list_marker(lines[start]).unwrap();
    let mut items: Vec<Vec<String>> = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let line = lines[i];
        match list_marker(line) {
            Some((o, indent, marker_w)) if o == ordered && indent == base_indent => {
                let content_col = indent + marker_w;
                let first = if line.len() >= content_col {
                    line[content_col..].to_string()
                } else {
                    String::new()
                };
                items.push(vec![first]);
                i += 1;
                // Continuation lines: blank, or indented to the
                // content column, or a nested list marker.
                while i < lines.len() {
                    let l = lines[i];
                    let l_indent = l.len() - l.trim_start().len();
                    if l.trim().is_empty() {
                        // Look ahead: blank ends the list unless the
                        // next line belongs to an item.
                        if i + 1 < lines.len()
                            && list_marker(lines[i + 1]).is_some_and(|(o, ind, _)| {
                                (o == ordered && ind == base_indent) || ind > base_indent
                            })
                        {
                            items.last_mut().unwrap().push(String::new());
                            i += 1;
                            continue;
                        }
                        break;
                    }
                    if list_marker(l).is_some_and(|(o, ind, _)| o == ordered && ind == base_indent)
                    {
                        break;
                    }
                    if l_indent >= content_col || list_marker(l).is_some() {
                        let cut = content_col.min(l_indent);
                        items.last_mut().unwrap().push(l[cut..].to_string());
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            _ => break,
        }
    }
    let consumed = i - start;
    let item_blocks: Vec<Vec<Block>> = items
        .iter()
        .map(|lines| {
            let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            parse_blocks(&refs)
        })
        .collect();
    (Block::List(ordered, item_blocks), consumed)
}

fn render_block(b: &Block, flavor: Flavor) -> String {
    match (b, flavor) {
        (Block::Heading(level, text), Flavor::Html) => {
            format!("<h{level}>{}</h{level}>", inline(text, flavor))
        }
        (Block::Heading(_, text), Flavor::Text) => inline(text, flavor),
        (Block::Paragraph(text), Flavor::Html) => format!("<p>{}</p>", inline(text, flavor)),
        (Block::Paragraph(text), Flavor::Text) => inline(text, flavor),
        (Block::Code(info, body), Flavor::Html) => {
            let class = if info.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", escape_html(info))
            };
            format!(
                "<pre><code{class}>{}</code></pre>",
                escape_html(body.trim_end_matches('\n'))
            )
        }
        (Block::Code(_, body), Flavor::Text) => body.trim_end_matches('\n').to_string(),
        (Block::Quote(inner), Flavor::Html) => {
            let inside: Vec<String> = inner.iter().map(|x| render_block(x, flavor)).collect();
            format!("<blockquote>\n{}\n</blockquote>", inside.join("\n"))
        }
        (Block::Quote(inner), Flavor::Text) => {
            let inside: Vec<String> = inner.iter().map(|x| render_block(x, flavor)).collect();
            inside
                .join("\n\n")
                .lines()
                .map(|l| format!("> {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        (Block::List(ordered, items), flavor) => {
            let mut out = String::new();
            match flavor {
                Flavor::Html => {
                    out.push_str(if *ordered { "<ol>" } else { "<ul>" });
                    for item in items {
                        out.push_str("\n<li>");
                        let parts: Vec<String> =
                            item.iter().map(|x| render_block(x, flavor)).collect();
                        // Tight rendering: strip the single <p> wrap.
                        let joined = parts.join("\n");
                        let joined = joined
                            .strip_prefix("<p>")
                            .and_then(|s| s.strip_suffix("</p>"))
                            .map(|s| s.to_string())
                            .unwrap_or(joined);
                        out.push_str(&joined);
                        out.push_str("</li>");
                    }
                    out.push_str(if *ordered { "\n</ol>" } else { "\n</ul>" });
                    out
                }
                Flavor::Text => {
                    for (n, item) in items.iter().enumerate() {
                        let marker = if *ordered {
                            format!("{}. ", n + 1)
                        } else {
                            "- ".into()
                        };
                        let parts: Vec<String> =
                            item.iter().map(|x| render_block(x, flavor)).collect();
                        out.push_str(&marker);
                        out.push_str(&parts.join("\n  "));
                        out.push('\n');
                    }
                    out.trim_end().to_string()
                }
            }
        }
        (Block::Rule, Flavor::Html) => "<hr>".to_string(),
        (Block::Rule, Flavor::Text) => "---".to_string(),
        (Block::Table(headers, aligns, rows), Flavor::Html) => {
            let mut out = String::from("<table>\n<thead>\n<tr>");
            for (j, h) in headers.iter().enumerate() {
                out.push_str(&format!(
                    "<th{}>{}</th>",
                    align_attr(aligns.get(j)),
                    inline(h, flavor)
                ));
            }
            out.push_str("</tr>\n</thead>\n<tbody>");
            for row in rows {
                out.push_str("\n<tr>");
                for (j, cell) in row.iter().enumerate() {
                    out.push_str(&format!(
                        "<td{}>{}</td>",
                        align_attr(aligns.get(j)),
                        inline(cell, flavor)
                    ));
                }
                out.push_str("</tr>");
            }
            out.push_str("\n</tbody>\n</table>");
            out
        }
        (Block::Table(headers, _, rows), Flavor::Text) => {
            let mut out = headers.join(" | ");
            for row in rows {
                out.push('\n');
                out.push_str(&row.join(" | "));
            }
            out
        }
    }
}

fn align_attr(a: Option<&char>) -> &'static str {
    match a {
        Some('c') => " style=\"text-align:center\"",
        Some('r') => " style=\"text-align:right\"",
        _ => "",
    }
}

/// Inline renderer: emphasis, strong, strike, code spans, links,
/// images, autolinks; everything else is literal text.
pub fn inline(text: &str, flavor: Flavor) -> String {
    InlineParser {
        chars: text.chars().collect(),
        pos: 0,
        flavor,
    }
    .run()
}

struct InlineParser {
    chars: Vec<char>,
    pos: usize,
    flavor: Flavor,
}

impl InlineParser {
    fn run(&mut self) -> String {
        let mut out = String::new();
        self.parse_into(&mut out, None);
        out
    }

    fn peek2(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }
    fn starts(&self, s: &str) -> bool {
        self.chars[self.pos..].starts_with(&s.chars().collect::<Vec<_>>()[..])
    }

    /// Parse until `stop` (an emphasis closer) or the end.
    fn parse_into(&mut self, out: &mut String, stop: Option<&str>) {
        while self.pos < self.chars.len() {
            if let Some(s) = stop {
                if self.starts(s) {
                    // Underscore closers must not be intraword.
                    if !(s.starts_with('_') && self.next_is_alnum()) {
                        self.pos += s.len();
                        return;
                    }
                }
            }
            let c = self.chars[self.pos];
            match c {
                '\\' => {
                    if let Some(n) = self.peek2() {
                        if n.is_ascii_punctuation() {
                            self.push_text(out, &n.to_string());
                            self.pos += 2;
                            continue;
                        }
                    }
                    self.push_text(out, "\\");
                    self.pos += 1;
                }
                '`' => {
                    let ticks = self.count_run('`');
                    let fence: String = "`".repeat(ticks);
                    match self.find_from(&fence) {
                        Some(end) => {
                            let code: String = self.chars[self.pos + ticks..end].iter().collect();
                            let code = code.replace('\n', " ");
                            match self.flavor {
                                Flavor::Html => {
                                    out.push_str("<code>");
                                    out.push_str(&escape_html(code.trim()));
                                    out.push_str("</code>");
                                }
                                Flavor::Text => out.push_str(&code),
                            }
                            self.pos = end + ticks;
                        }
                        None => {
                            self.push_text(out, &fence);
                            self.pos += ticks;
                        }
                    }
                }
                '!' if self.peek2() == Some('[') => {
                    if !self.try_link(out, true) {
                        self.push_text(out, "!");
                        self.pos += 1;
                    }
                }
                '[' => {
                    if !self.try_link(out, false) {
                        self.push_text(out, "[");
                        self.pos += 1;
                    }
                }
                '<' => {
                    // Autolink <https://…> or <mail@x>.
                    let rest: String = self.chars[self.pos..].iter().collect();
                    if let Some(end) = rest.find('>') {
                        let inside = &rest[1..end];
                        if inside.starts_with("http://")
                            || inside.starts_with("https://")
                            || (inside.contains('@') && !inside.contains(' '))
                        {
                            match self.flavor {
                                Flavor::Html => out.push_str(&format!(
                                    "<a href=\"{}\">{}</a>",
                                    escape_html(inside),
                                    escape_html(inside)
                                )),
                                Flavor::Text => out.push_str(inside),
                            }
                            self.pos += end + 1;
                            continue;
                        }
                    }
                    self.push_text(out, "<");
                    self.pos += 1;
                }
                '~' if self.starts("~~") => {
                    self.pos += 2;
                    let before = out.len();
                    let mut inner = String::new();
                    self.parse_into(&mut inner, Some("~~"));
                    if inner.is_empty() && out.len() == before {
                        self.push_text(out, "~~");
                    } else {
                        match self.flavor {
                            Flavor::Html => {
                                out.push_str("<del>");
                                out.push_str(&inner);
                                out.push_str("</del>");
                            }
                            Flavor::Text => out.push_str(&inner),
                        }
                    }
                }
                '*' | '_' => {
                    let double = self.peek2() == Some(c);
                    // Underscore openers must not be intraword.
                    if c == '_' && self.prev_is_alnum() {
                        self.push_text(out, "_");
                        self.pos += 1;
                        continue;
                    }
                    let delim = if double {
                        format!("{c}{c}")
                    } else {
                        c.to_string()
                    };
                    self.pos += delim.len();
                    let mut inner = String::new();
                    self.parse_into(&mut inner, Some(&delim));
                    if inner.is_empty() {
                        self.push_text(out, &delim);
                    } else {
                        match (self.flavor, double) {
                            (Flavor::Html, true) => {
                                out.push_str("<strong>");
                                out.push_str(&inner);
                                out.push_str("</strong>");
                            }
                            (Flavor::Html, false) => {
                                out.push_str("<em>");
                                out.push_str(&inner);
                                out.push_str("</em>");
                            }
                            (Flavor::Text, _) => out.push_str(&inner),
                        }
                    }
                }
                '\n' => {
                    out.push('\n');
                    self.pos += 1;
                }
                _ => {
                    self.push_text(out, &c.to_string());
                    self.pos += 1;
                }
            }
        }
    }

    fn push_text(&self, out: &mut String, s: &str) {
        match self.flavor {
            Flavor::Html => out.push_str(&escape_html(s)),
            Flavor::Text => out.push_str(s),
        }
    }

    fn prev_is_alnum(&self) -> bool {
        self.pos > 0 && self.chars[self.pos - 1].is_alphanumeric()
    }
    fn next_is_alnum(&self) -> bool {
        self.chars
            .get(self.pos + 2)
            .map(|c| c.is_alphanumeric())
            .unwrap_or(false)
    }
    fn count_run(&self, c: char) -> usize {
        let mut n = 0;
        while self.chars.get(self.pos + n) == Some(&c) {
            n += 1;
        }
        n
    }
    fn find_from(&self, needle: &str) -> Option<usize> {
        let needle_chars: Vec<char> = needle.chars().collect();
        let start = self.pos + needle_chars.len();
        (start..=self.chars.len().saturating_sub(needle_chars.len()))
            .find(|&i| self.chars[i..].starts_with(&needle_chars[..]))
    }

    /// At '[' (link) or '![' (image). Consumes through ')' on success.
    fn try_link(&mut self, out: &mut String, image: bool) -> bool {
        let save = self.pos;
        self.pos += if image { 2 } else { 1 };
        // Find the matching ']' (no nesting support beyond one level
        // of brackets in the label).
        let label_start = self.pos;
        let mut depth = 1i32;
        let mut j = self.pos;
        while j < self.chars.len() {
            match self.chars[j] {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if depth != 0 || self.chars.get(j + 1) != Some(&'(') {
            self.pos = save;
            return false;
        }
        let label: String = self.chars[label_start..j].iter().collect();
        // Destination until the matching ')'.
        let dest_start = j + 2;
        let mut k = dest_start;
        let mut parens = 1i32;
        while k < self.chars.len() {
            match self.chars[k] {
                '(' => parens += 1,
                ')' => {
                    parens -= 1;
                    if parens == 0 {
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        if parens != 0 {
            self.pos = save;
            return false;
        }
        let dest_raw: String = self.chars[dest_start..k].iter().collect();
        let dest = dest_raw.trim();
        // Optional "title" after whitespace.
        let (url, _title) = match dest.find(char::is_whitespace) {
            Some(w) => (dest[..w].trim(), dest[w..].trim().trim_matches('"')),
            None => (dest, ""),
        };
        self.pos = k + 1;
        let label_html = inline(&label, self.flavor);
        match (self.flavor, image) {
            (Flavor::Html, false) => out.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                escape_html(url),
                label_html
            )),
            (Flavor::Html, true) => out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\">",
                escape_html(url),
                escape_html(&label)
            )),
            (Flavor::Text, false) => {
                out.push_str(&label_html);
                out.push_str(&format!(" ({url})"));
            }
            (Flavor::Text, true) => {
                out.push_str(&format!("[image: {label_html}]"));
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(md: &str) -> String {
        convert_markdown(md, Flavor::Html).unwrap()
    }

    #[test]
    fn headings_and_emphasis() {
        let out = html("# Title\n\nSome **bold** and *italic* and `code`.\n");
        assert_eq!(
            out,
            "<h1>Title</h1>\n<p>Some <strong>bold</strong> and <em>italic</em> and <code>code</code>.</p>"
        );
    }

    #[test]
    fn lists_links_tables() {
        let md = "- one\n- two with [a link](https://example.com)\n\n1. first\n2. second\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
        let out = html(md);
        assert!(
            out.contains("<ul>") && out.contains("<li>one</li>"),
            "{out}"
        );
        assert!(
            out.contains("<a href=\"https://example.com\">a link</a>"),
            "{out}"
        );
        assert!(out.contains("<ol>"), "{out}");
        assert!(
            out.contains("<table>") && out.contains("<td>2</td>"),
            "{out}"
        );
    }

    #[test]
    fn code_fence_and_quote() {
        let out = html("```rust\nfn main() { println!(\"<hi>\"); }\n```\n\n> quoted **text**\n");
        assert!(
            out.contains("<pre><code class=\"language-rust\">fn main()"),
            "{out}"
        );
        assert!(out.contains("&lt;hi&gt;"), "{out}");
        assert!(out.contains("<blockquote>"), "{out}");
        assert!(out.contains("<strong>text</strong>"), "{out}");
    }

    #[test]
    fn raw_html_is_escaped() {
        let out = html("Hello <script>alert(1)</script>");
        assert!(out.contains("&lt;script&gt;"), "{out}");
    }

    #[test]
    fn text_flavor() {
        let out = convert_markdown(
            "# Hi\n\nSome **bold** text and [x](https://e.com).\n",
            Flavor::Text,
        )
        .unwrap();
        assert_eq!(out, "Hi\n\nSome bold text and x (https://e.com).\n");
    }
}
