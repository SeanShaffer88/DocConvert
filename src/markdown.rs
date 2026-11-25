//! Markdown → HTML and Markdown → plain text.
//!
//! Covers the CommonMark constructs documents actually use: ATX and
//! setext headings, paragraphs, emphasis/strong/strikethrough, code
//! spans and fenced blocks, inline and reference links/images,
//! autolinks, blockquotes, ordered/unordered lists with nesting,
//! horizontal rules, and GFM pipe tables. Raw HTML inside Markdown is
//! escaped, not passed through (documented: the output is meant to be
//! safe by default).

use std::collections::HashMap;

use crate::{escape_html, CResult};

/// Link reference definitions, keyed by normalized label:
/// `[docs]: https://example.com "Title"` → `"docs"` → `(url, title)`.
pub type Refs = HashMap<String, (String, String)>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Flavor {
    Html,
    Text,
}

pub fn convert_markdown(input: &str, flavor: Flavor) -> CResult<String> {
    let (refs, lines) = collect_definitions(input);
    let lines: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let blocks = parse_blocks(&lines);
    let rendered: Vec<String> = blocks
        .iter()
        .map(|b| render_block(b, flavor, &refs))
        .collect();
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

/// Normalize a reference label the CommonMark way: trim, collapse
/// internal whitespace, case-fold.
pub fn normalize_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut pending_space = false;
    for c in label.trim().chars() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(c);
    }
    out.to_lowercase()
}

/// Pull `[label]: url "title"` lines out of the document (fenced code
/// is left alone) and hand back the remaining lines. Definitions are
/// document-wide in CommonMark, so this runs before block parsing.
fn collect_definitions(input: &str) -> (Refs, Vec<String>) {
    let mut refs = Refs::new();
    let mut kept: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            kept.push(line.to_string());
            continue;
        }
        if !in_fence {
            if let Some((label, url, title)) = parse_definition(line) {
                refs.entry(normalize_label(&label)).or_insert((url, title));
                continue;
            }
        }
        kept.push(line.to_string());
    }
    (refs, kept)
}

/// `[label]: https://example.com "Title"` with up to three spaces of
/// indentation. Returns `(label, url, title)`.
fn parse_definition(line: &str) -> Option<(String, String, String)> {
    let indent = line.len() - line.trim_start().len();
    if indent > 3 {
        return None;
    }
    let text = line.trim();
    let rest = text.strip_prefix('[')?;
    let close = rest.find("]:")?;
    let label = rest[..close].trim();
    if label.is_empty() || label.contains('[') || label.contains(']') {
        return None;
    }
    let after = rest[close + 2..].trim();
    if after.is_empty() {
        return None;
    }
    let (url, title) = if let Some(stripped) = after.strip_prefix('<') {
        let end = stripped.find('>')?;
        let title = after[end + 2..].trim();
        (stripped[..end].to_string(), title.to_string())
    } else {
        match after.find(char::is_whitespace) {
            Some(w) => (after[..w].to_string(), after[w..].trim().to_string()),
            None => (after.to_string(), String::new()),
        }
    };
    if url.is_empty() {
        return None;
    }
    let title = title
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('(')
        .trim_matches(')')
        .to_string();
    Some((label.to_string(), url, title))
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
    let indent = line_indent(line);
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

/// Byte offset just past the leading whitespace — always a character
/// boundary, unlike `line.len() - line.trim_start().len()`.
fn line_indent(line: &str) -> usize {
    let mut offset = 0;
    for (i, c) in line.char_indices() {
        if !c.is_whitespace() {
            return i;
        }
        offset = i + c.len_utf8();
    }
    offset
}

/// Slice a line from a byte column, snapping forward to a character
/// boundary: `l[cut..]` would panic on a line indented with non-ASCII
/// whitespace.
fn tail_from(line: &str, column: usize) -> String {
    if line.len() <= column {
        return String::new();
    }
    let mut cut = column;
    while cut < line.len() && !line.is_char_boundary(cut) {
        cut += 1;
    }
    line[cut..].to_string()
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
                let first = tail_from(line, content_col);
                items.push(vec![first]);
                i += 1;
                // Continuation lines: blank, or indented to the
                // content column, or a nested list marker.
                while i < lines.len() {
                    let l = lines[i];
                    let l_indent = line_indent(l);
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
                        items.last_mut().unwrap().push(tail_from(l, cut));
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

fn render_block(b: &Block, flavor: Flavor, refs: &Refs) -> String {
    match (b, flavor) {
        (Block::Heading(level, text), Flavor::Html) => {
            format!(
                "<h{level}>{}</h{level}>",
                inline_with_refs(text, flavor, refs)
            )
        }
        (Block::Heading(_, text), Flavor::Text) => inline_with_refs(text, flavor, refs),
        (Block::Paragraph(text), Flavor::Html) => {
            format!("<p>{}</p>", inline_with_refs(text, flavor, refs))
        }
        (Block::Paragraph(text), Flavor::Text) => inline_with_refs(text, flavor, refs),
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
            let inside: Vec<String> = inner
                .iter()
                .map(|x| render_block(x, flavor, refs))
                .collect();
            format!("<blockquote>\n{}\n</blockquote>", inside.join("\n"))
        }
        (Block::Quote(inner), Flavor::Text) => {
            let inside: Vec<String> = inner
                .iter()
                .map(|x| render_block(x, flavor, refs))
                .collect();
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
                            item.iter().map(|x| render_block(x, flavor, refs)).collect();
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
                            item.iter().map(|x| render_block(x, flavor, refs)).collect();
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
                    inline_with_refs(h, flavor, refs)
                ));
            }
            out.push_str("</tr>\n</thead>\n<tbody>");
            for row in rows {
                out.push_str("\n<tr>");
                for (j, cell) in row.iter().enumerate() {
                    out.push_str(&format!(
                        "<td{}>{}</td>",
                        align_attr(aligns.get(j)),
                        inline_with_refs(cell, flavor, refs)
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
    inline_with_refs(text, flavor, &Refs::new())
}

/// `inline`, with the document's link reference definitions available
/// (needed for `[label][ref]`, `[label][]`, and `[label]` links).
pub fn inline_with_refs(text: &str, flavor: Flavor, refs: &Refs) -> String {
    InlineParser {
        chars: text.chars().collect(),
        pos: 0,
        flavor,
        refs,
    }
    .run()
}

struct InlineParser<'a> {
    chars: Vec<char>,
    pos: usize,
    flavor: Flavor,
    refs: &'a Refs,
}

impl<'a> InlineParser<'a> {
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

    /// At '[' (link) or '![' (image). Consumes an inline link
    /// `[label](dest "title")`, a reference link `[label][ref]`,
    /// `[label][]`, or `[label]`, or the image forms of each.
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
        if depth != 0 {
            self.pos = save;
            return false;
        }
        let label: String = self.chars[label_start..j].iter().collect();
        match self.chars.get(j + 1) {
            Some('(') => match self.inline_destination(j + 2) {
                Some((url, after)) => {
                    self.pos = after;
                    self.emit_link(out, &label, &url, image);
                    true
                }
                None => {
                    self.pos = save;
                    false
                }
            },
            Some('[') => {
                let mut k = j + 2;
                while k < self.chars.len() && self.chars[k] != ']' {
                    k += 1;
                }
                if k >= self.chars.len() {
                    self.pos = save;
                    return false;
                }
                let explicit: String = self.chars[j + 2..k].iter().collect();
                let key = if explicit.trim().is_empty() {
                    normalize_label(&label)
                } else {
                    normalize_label(&explicit)
                };
                match self.refs.get(&key).cloned() {
                    Some((url, _title)) => {
                        self.pos = k + 1;
                        self.emit_link(out, &label, &url, image);
                        true
                    }
                    None => {
                        self.pos = save;
                        false
                    }
                }
            }
            _ => {
                // Shortcut reference: [label] with a matching definition.
                match self.refs.get(&normalize_label(&label)).cloned() {
                    Some((url, _title)) => {
                        self.pos = j + 1;
                        self.emit_link(out, &label, &url, image);
                        true
                    }
                    None => {
                        self.pos = save;
                        false
                    }
                }
            }
        }
    }

    /// Parse `dest "title")` starting just past the '(' of an inline
    /// link. Returns the destination and the index just past the ')'.
    fn inline_destination(&self, start: usize) -> Option<(String, usize)> {
        let mut k = start;
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
        if parens != 0 || k >= self.chars.len() {
            return None;
        }
        let raw: String = self.chars[start..k].iter().collect();
        let dest = raw.trim();
        let url = match dest.find(char::is_whitespace) {
            Some(w) => dest[..w].trim(),
            None => dest,
        };
        Some((url.to_string(), k + 1))
    }

    /// Emit a link or image in the current flavor.
    fn emit_link(&mut self, out: &mut String, label: &str, url: &str, image: bool) {
        let label_html = inline_with_refs(label, self.flavor, self.refs);
        match (self.flavor, image) {
            (Flavor::Html, false) => out.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                escape_html(url),
                label_html
            )),
            (Flavor::Html, true) => out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\">",
                escape_html(url),
                escape_html(label)
            )),
            (Flavor::Text, false) => {
                out.push_str(&label_html);
                out.push_str(&format!(" ({url})"));
            }
            (Flavor::Text, true) => out.push_str(&format!("[image: {label_html}]")),
        }
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
