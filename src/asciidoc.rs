//! AsciiDoc (a practical subset) → Markdown.
//!
//! The subset: document title and section levels (`=`, `==`, …), bullet
//! and ordered lists with `*`/`**` or `.`/`..` nesting, listing/literal
//! delimited blocks (`----`, `....`) with an optional `[source,lang]`
//! language, example/sidebar blocks, quote blocks (`____`), comments
//! (`//` line and `////` block), attribute lines (`:name: value`) and
//! block attributes (`[...]`) which are dropped, block titles (`.Title`),
//! admonitions (`NOTE:` …), thematic breaks (`'''`), description lists
//! (`term:: definition`) and inline markup — `*strong*`, `_emphasis_`,
//! `+literal+`, `` `mono` ``, `https://…[text]`, `link:…[text]` and
//! `<<xref,text>>`.
//!
//! Like `rst`, everything lands as Markdown first, so every target
//! agrees with the others about what the document says. `|===` tables
//! become GFM tables, with `2+`/`.2+` spans and style prefixes flattened
//! the way the rST reader flattens its grid spans.

use crate::CResult;

/// Convert the subset described in the module docs to canonical Markdown.
pub fn to_markdown(input: &str) -> CResult<String> {
    let lines: Vec<&str> = input.lines().collect();
    let mut blocks: Vec<String> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut fence_language = String::new();

    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            flush(&mut blocks, &mut paragraph);
            i += 1;
            continue;
        }

        // Comments: `//` lines and `////` blocks.
        if line.trim_start().starts_with("////") {
            flush(&mut blocks, &mut paragraph);
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("////") {
                i += 1;
            }
            i += 1;
            continue;
        }
        if line.trim_start().starts_with("//") {
            flush(&mut blocks, &mut paragraph);
            i += 1;
            continue;
        }

        // Attribute lines are metadata (`:toc: left`).
        if is_attribute_line(line) {
            flush(&mut blocks, &mut paragraph);
            i += 1;
            continue;
        }

        // Block attributes: `[source,rust]` keeps the language for the
        // next listing block, anything else is dropped.
        if line.trim_start().starts_with('[') && line.trim_end().ends_with(']') {
            flush(&mut blocks, &mut paragraph);
            fence_language = source_language(line);
            i += 1;
            continue;
        }

        // A block title (`.Title`) belongs to the block below it; `. item`
        // is an ordered list marker, so the dot must not be followed by a
        // space or another dot.
        if let Some(rest) = line.trim_start().strip_prefix('.') {
            if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('.') {
                flush(&mut blocks, &mut paragraph);
                i += 1;
                continue;
            }
        }

        // Tables: `|===` … `|===`, one row per line, the first row is the
        // header (the shape GFM tables have).
        if line.trim() == "|===" {
            flush(&mut blocks, &mut paragraph);
            // The block attribute (`[cols="1,1"]`, `[%header]`) was consumed
            // by the bracket branch above but is still in `lines`.
            let attribute = i
                .checked_sub(1)
                .and_then(|prev| lines.get(prev))
                .and_then(|line| {
                    let trimmed = line.trim();
                    (trimmed.starts_with('[') && trimmed.ends_with(']')).then_some(trimmed)
                });
            let (rendered, consumed) = render_table(&lines[i..], attribute);
            if !rendered.is_empty() {
                blocks.push(rendered.trim_end().to_string());
            }
            i += consumed;
            continue;
        }

        // Delimited blocks.
        if let Some(kind) = delimiter_kind(line) {
            flush(&mut blocks, &mut paragraph);
            let (rendered, consumed) = render_delimited(&lines[i..], kind, &fence_language);
            if !rendered.is_empty() {
                blocks.push(rendered);
            }
            fence_language.clear();
            i += consumed;
            continue;
        }

        // Thematic break.
        if line.trim() == "'''" {
            flush(&mut blocks, &mut paragraph);
            blocks.push("---".to_string());
            i += 1;
            continue;
        }

        // Headings.
        if let Some((level, text)) = section(line) {
            flush(&mut blocks, &mut paragraph);
            blocks.push(format!("{} {}", "#".repeat(level), inline(text)));
            i += 1;
            continue;
        }

        // Lists.
        if bullet_text(line).is_some() {
            flush(&mut blocks, &mut paragraph);
            let (md, consumed) = render_list(&lines[i..], false);
            blocks.push(md);
            i += consumed;
            continue;
        }
        if ordered_text(line).is_some() {
            flush(&mut blocks, &mut paragraph);
            let (md, consumed) = render_list(&lines[i..], true);
            blocks.push(md);
            i += consumed;
            continue;
        }

        // Description lists: `Term:: definition`.
        if let Some((term, definition)) = description(line) {
            flush(&mut blocks, &mut paragraph);
            blocks.push(format!("**{}**: {}", inline(term), inline(definition)));
            i += 1;
            continue;
        }

        // Admonitions.
        if let Some((label, text)) = admonition(line) {
            flush(&mut blocks, &mut paragraph);
            blocks.push(format!("> **{label}:** {}", inline(text)));
            i += 1;
            continue;
        }

        // A line that starts with a space is a literal line.
        if line.starts_with(' ') {
            flush(&mut blocks, &mut paragraph);
            let mut body: Vec<String> = Vec::new();
            while i < lines.len() {
                let current = lines[i];
                if current.trim().is_empty() || !current.starts_with(' ') {
                    break;
                }
                body.push(current.trim_start().to_string());
                i += 1;
            }
            blocks.push(format!("```\n{}\n```", body.join("\n")));
            continue;
        }

        paragraph.push(line.trim().to_string());
        i += 1;
    }
    flush(&mut blocks, &mut paragraph);

    let mut out = blocks.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

fn flush(blocks: &mut Vec<String>, paragraph: &mut Vec<String>) {
    if paragraph.is_empty() {
        return;
    }
    let text: Vec<String> = paragraph
        .iter()
        .filter(|line| line.as_str() != "+")
        .map(|line| inline(line))
        .collect();
    if !text.is_empty() {
        blocks.push(text.join(" "));
    }
    paragraph.clear();
}

fn is_attribute_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix(':') else {
        return false;
    };
    matches!(
        rest.find(':'),
        Some(at)
            if at > 0
                && rest[..at]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    )
}

fn source_language(line: &str) -> String {
    let inner = line.trim().trim_start_matches('[').trim_end_matches(']');
    let mut parts = inner.split(',');
    let first = parts.next().unwrap_or("").trim();
    if first == "source" || first == "listing" || first == "code" {
        let language = parts.next().unwrap_or("").trim();
        return language
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '-' || *c == '#')
            .collect();
    }
    String::new()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delimiter {
    Listing,
    Literal,
    Quote,
    Example,
}

fn delimiter_kind(line: &str) -> Option<Delimiter> {
    let trimmed = line.trim_end();
    if trimmed.len() < 4 {
        return None;
    }
    let first = trimmed.chars().next()?;
    if !trimmed.chars().all(|c| c == first) {
        return None;
    }
    match first {
        '-' => Some(Delimiter::Listing),
        '.' if trimmed.len() >= 4 => Some(Delimiter::Literal),
        '_' => Some(Delimiter::Quote),
        '=' => Some(Delimiter::Example),
        '*' => Some(Delimiter::Example),
        _ => None,
    }
}

fn render_delimited(lines: &[&str], kind: Delimiter, language: &str) -> (String, usize) {
    let opener = lines[0].trim_end().to_string();
    let mut body: Vec<String> = Vec::new();
    let mut i = 1usize;
    while i < lines.len() {
        if lines[i].trim_end() == opener {
            i += 1;
            break;
        }
        body.push(lines[i].to_string());
        i += 1;
    }
    let text = body.join("\n").trim_end().to_string();
    let rendered = match kind {
        Delimiter::Listing | Delimiter::Literal => {
            format!("```{language}\n{text}\n```")
        }
        Delimiter::Quote => text
            .lines()
            .map(|line| {
                if line.trim().is_empty() {
                    ">".to_string()
                } else {
                    format!("> {}", inline(line))
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Delimiter::Example => text,
    };
    (rendered, i)
}

/// A `|===` table. Cells may wrap across lines, span columns (`2+|`) or
/// rows (`.2+|`), and carry a style prefix (`a|`, `h|`, `l|`) — the styles
/// are dropped because GFM has no per-cell formatting, and the spans are
/// flattened: the text sits in the cell where it starts and the cells it
/// covers stay empty.
fn render_table(lines: &[&str], attribute: Option<&str>) -> (String, usize) {
    let mut i = 1usize;
    let mut body = String::new();
    while i < lines.len() {
        if lines[i].trim() == "|===" {
            i += 1;
            break;
        }
        body.push_str(lines[i]);
        body.push('\n');
        i += 1;
    }
    let cells = parse_cells(&body);
    if cells.is_empty() {
        return (String::new(), i);
    }
    let columns = column_count(attribute)
        .unwrap_or_else(|| first_line_cells(&body))
        .max(1);
    (crate::markdown::table(&layout_cells(cells, columns)), i)
}

/// One cell as written: its text, and what it covers.
struct Cell {
    text: String,
    colspan: usize,
    rowspan: usize,
}

/// Split the body into cells at every `|`, reading the specifier that sits
/// just before each wall (`2+`, `.3+`, `a`, `hl+`, or nothing).
fn parse_cells(body: &str) -> Vec<Cell> {
    let chars: Vec<char> = body.chars().collect();
    // Every wall, plus where the specifier in front of it begins.
    let mut walls: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '|' {
            index += 1;
            continue;
        }
        let mut spec_start = index;
        while spec_start > 0 && is_spec_char(chars[spec_start - 1]) {
            spec_start -= 1;
        }
        // Only a run that starts a token is a specifier: `red|` is text.
        let starts_token = spec_start == 0 || chars[spec_start - 1].is_whitespace();
        let (start, colspan, rowspan) = if starts_token {
            let spec: String = chars[spec_start..index].iter().collect();
            let (colspan, rowspan) = parse_span(&spec);
            (spec_start, colspan, rowspan)
        } else {
            (index, 1, 1)
        };
        walls.push((index, start, colspan, rowspan));
        index += 1;
    }
    let mut cells = Vec::with_capacity(walls.len());
    for (position, &(wall, _, colspan, rowspan)) in walls.iter().enumerate() {
        let from = wall + 1;
        let to = walls
            .get(position + 1)
            .map(|next| next.1)
            .unwrap_or(chars.len())
            .max(from);
        let text: String = chars[from..to].iter().collect();
        cells.push(Cell {
            text: collapse(&text),
            colspan,
            rowspan,
        });
    }
    cells
}

fn is_spec_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '.' | '+' | 'a' | 'd' | 'e' | 'h' | 'l' | 'm' | 's')
}

/// `2+` is two columns, `.3+` three rows; a bare style prefix (`a`, `hl`)
/// covers nothing.
fn parse_span(spec: &str) -> (usize, usize) {
    let mut colspan = 1usize;
    let mut rowspan = 1usize;
    let chars: Vec<char> = spec.chars().collect();
    let mut index = 0usize;
    let mut digits = String::new();
    while index < chars.len() && chars[index].is_ascii_digit() {
        digits.push(chars[index]);
        index += 1;
    }
    if !digits.is_empty() {
        colspan = digits.parse().unwrap_or(1);
    }
    if chars.get(index) == Some(&'.') {
        index += 1;
        let mut digits = String::new();
        while index < chars.len() && chars[index].is_ascii_digit() {
            digits.push(chars[index]);
            index += 1;
        }
        if !digits.is_empty() {
            rowspan = digits.parse().unwrap_or(1);
        }
    }
    (colspan.max(1), rowspan.max(1))
}

/// The column count a `cols="1,2,3"` attribute asks for (`3*` is three).
fn column_count(attribute: Option<&str>) -> Option<usize> {
    let attribute = attribute?;
    let start = attribute.find("cols")? + 4;
    let rest = &attribute[start..];
    let rest = rest.trim_start_matches(['=', ' ']);
    let rest = rest.trim_start_matches('"');
    let end = rest.find('"').unwrap_or(rest.len());
    let spec = &rest[..end];
    let mut total = 0usize;
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.strip_suffix('*') {
            Some(count) => total += count.trim().parse::<usize>().unwrap_or(0),
            None => total += 1,
        }
    }
    (total > 0).then_some(total)
}

fn first_line_cells(body: &str) -> usize {
    body.lines()
        .map(|line| line.matches('|').count())
        .find(|count| *count > 0)
        .unwrap_or(1)
}

/// Place cells left to right, wrapping at the column count. A cell that
/// spans columns takes the ones beside it; a cell that spans rows reserves
/// those columns below, so the next row's cells step around it — and every
/// covered position stays empty because GFM has no spans.
fn layout_cells(cells: Vec<Cell>, columns: usize) -> Vec<Vec<String>> {
    let columns = columns.max(1);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut reserved: Vec<Vec<bool>> = Vec::new();
    let mut row = 0usize;
    let mut column = 0usize;
    for cell in cells {
        ensure(&mut rows, &mut reserved, row, columns);
        while column < columns && reserved[row][column] {
            column += 1;
        }
        if column >= columns {
            row += 1;
            column = 0;
            ensure(&mut rows, &mut reserved, row, columns);
            while column < columns && reserved[row][column] {
                column += 1;
            }
        }
        let colspan = cell.colspan.min(columns - column);
        rows[row][column] = collapse(&cell.text);
        for covered in rows[row].iter_mut().skip(column + 1).take(colspan - 1) {
            covered.clear();
        }
        for extra in 1..cell.rowspan.max(1) {
            ensure(&mut rows, &mut reserved, row + extra, columns);
            reserved[row + extra][column] = true;
            rows[row + extra][column].clear();
        }
        column += colspan;
        if column >= columns {
            row += 1;
            column = 0;
        }
    }
    // A trailing row can be empty because a row span covered it; only the
    // blank tail is dropped.
    while rows
        .last()
        .is_some_and(|line| line.iter().all(|cell| cell.is_empty()))
    {
        rows.pop();
    }
    rows
}

fn ensure(rows: &mut Vec<Vec<String>>, reserved: &mut Vec<Vec<bool>>, row: usize, columns: usize) {
    while rows.len() <= row {
        rows.push(vec![String::new(); columns]);
        reserved.push(vec![false; columns]);
    }
}

/// A cell's text is one line in the table: whitespace, including the line
/// breaks a wrapped cell was written across, collapses.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn section(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_end();
    let level = trimmed.chars().take_while(|c| *c == '=').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    let text = rest.strip_prefix(' ')?;
    if text.trim().is_empty() {
        return None;
    }
    Some((level, text.trim()))
}

fn bullet_text(line: &str) -> Option<(usize, &str)> {
    // Nesting comes from marker repetition, so indentation carries no
    // meaning here — trimmed markers are markers either way.
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return Some((1, rest.trim()));
    }
    let stars = trimmed.chars().take_while(|c| *c == '*').count();
    if stars == 0 || stars > 5 {
        return None;
    }
    let rest = trimmed[stars..].strip_prefix(' ')?;
    Some((stars, rest.trim()))
}

fn ordered_text(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let dots = trimmed.chars().take_while(|c| *c == '.').count();
    if dots == 0 || dots > 5 {
        return None;
    }
    let rest = trimmed[dots..].strip_prefix(' ')?;
    Some((dots, rest.trim()))
}

fn render_list(lines: &[&str], ordered: bool) -> (String, usize) {
    let mut out = String::new();
    let mut counters: Vec<usize> = vec![0; 6];
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            let mut ahead = i;
            while ahead < lines.len() && lines[ahead].trim().is_empty() {
                ahead += 1;
            }
            let continues = match lines.get(ahead) {
                Some(next) => {
                    if ordered {
                        ordered_text(next).is_some()
                    } else {
                        bullet_text(next).is_some()
                    }
                }
                None => false,
            };
            if continues {
                i = ahead;
                continue;
            }
            break;
        }
        let item = if ordered {
            ordered_text(line)
        } else {
            bullet_text(line)
        };
        let Some((depth, text)) = item else { break };
        let depth = depth.saturating_sub(1).min(5);
        counters[depth] += 1;
        for slot in counters.iter_mut().skip(depth + 1) {
            *slot = 0;
        }
        let marker = if ordered {
            format!("{}. ", counters[depth])
        } else {
            "- ".to_string()
        };
        out.push_str(&"  ".repeat(depth));
        out.push_str(&marker);
        out.push_str(&inline(text));
        out.push('\n');
        i += 1;
    }
    (out.trim_end().to_string(), i)
}

fn description(line: &str) -> Option<(&str, &str)> {
    if line.starts_with(' ') {
        return None;
    }
    let trimmed = line.trim_end();
    let at = trimmed.find("::")?;
    let term = trimmed[..at].trim();
    let definition = trimmed[at + 2..].trim();
    if term.is_empty() {
        return None;
    }
    Some((term, definition))
}

fn admonition(line: &str) -> Option<(&'static str, &str)> {
    for label in ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"] {
        if let Some(rest) = line.strip_prefix(label) {
            if let Some(text) = rest.strip_prefix(": ") {
                return Some((label, text.trim()));
            }
        }
    }
    None
}

/// The inline subset.
fn inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];

        // `+literal+` and `#mark#` are monospace in AsciiDoc.
        if c == '+' || c == '#' {
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == c) {
                if close > 0 {
                    let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                    out.push('`');
                    out.push_str(inner.trim());
                    out.push('`');
                    i = i + close + 2;
                    continue;
                }
            }
        }

        // `*strong*` → `**strong**`.
        if c == '*' {
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == '*') {
                if close > 0 {
                    let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                    if !inner.trim().is_empty() && !inner.starts_with(' ') && !inner.ends_with(' ')
                    {
                        out.push_str("**");
                        out.push_str(inner.trim());
                        out.push_str("**");
                        i = i + close + 2;
                        continue;
                    }
                }
            }
        }

        // `_emphasis_` → `*emphasis*` (intraword underscores stay).
        if c == '_' {
            let at_word_start = i == 0 || !chars[i - 1].is_alphanumeric();
            if at_word_start {
                if let Some(close) = chars[i + 1..].iter().position(|&x| x == '_') {
                    if close > 0 {
                        let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                        if !inner.trim().is_empty()
                            && !inner.starts_with(' ')
                            && !inner.ends_with(' ')
                        {
                            out.push('*');
                            out.push_str(inner.trim());
                            out.push('*');
                            i = i + close + 2;
                            continue;
                        }
                    }
                }
            }
        }

        // `https://…[text]`, `link:…[text]`, `xref:…[text]`.
        if c.is_ascii_alphabetic() {
            let rest: String = chars[i..].iter().collect();
            if let Some((consumed, rendered)) = macro_link(&rest) {
                out.push_str(&rendered);
                i += consumed;
                continue;
            }
        }

        // `<<id,text>>`.
        if c == '<' {
            let rest: String = chars[i..].iter().collect();
            if let Some((consumed, rendered)) = xref(&rest) {
                out.push_str(&rendered);
                i += consumed;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }
    out
}

fn macro_link(rest: &str) -> Option<(usize, String)> {
    let open = rest.find('[')?;
    let close = rest[open..].find(']')? + open;
    let target = rest[..open].trim();
    let label = rest[open + 1..close].trim();
    // The target is a single token: anything with whitespace in it is
    // prose that happens to precede a bracket.
    if target.is_empty() || target.chars().any(char::is_whitespace) {
        return None;
    }
    let url = match target
        .strip_prefix("link:")
        .or_else(|| target.strip_prefix("xref:"))
    {
        Some(url) => url,
        // `https://example.com[text]` needs no prefix.
        None if target.contains("://") => target,
        None => return None,
    };
    if url.is_empty() {
        return None;
    }
    let text = if label.is_empty() { url } else { label };
    Some((close + 1, format!("[{text}]({url})")))
}

fn xref(rest: &str) -> Option<(usize, String)> {
    let inner = rest.strip_prefix("<<")?;
    let close = inner.find(">>")?;
    let body = &inner[..close];
    let label = match body.split_once(',') {
        Some((_, label)) => label.trim(),
        None => body.trim(),
    };
    if label.is_empty() {
        return None;
    }
    Some((close + 4, format!("[{label}]")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{convert_markdown, Flavor};

    #[test]
    fn sections_and_title() {
        let doc = "= Guide\n\n== Install\n\n=== Windows\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "# Guide\n\n## Install\n\n### Windows\n"
        );
    }

    #[test]
    fn lists_nest_by_marker_repetition() {
        let doc = "* one\n** nested\n* two\n\n. first\n. second\n";
        let md = to_markdown(doc).unwrap();
        assert!(md.starts_with("- one\n  - nested\n- two"), "{md}");
        assert!(md.contains("1. first\n2. second"), "{md}");
    }

    #[test]
    fn source_blocks_keep_their_language() {
        let doc = "[source,rust]\n----\nfn main() {}\n----\n\nAfter.\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "```rust\nfn main() {}\n```\n\nAfter.\n"
        );
    }

    #[test]
    fn inline_markup_lands_as_markdown() {
        let doc = "Use *strong*, _soft_, `mono`, +literal+ and https://example.com[the site].\n";
        let md = to_markdown(doc).unwrap();
        assert!(md.contains("**strong**"), "{md}");
        assert!(md.contains("*soft*"), "{md}");
        assert!(md.contains("`literal`"), "{md}");
        assert!(md.contains("[the site](https://example.com)"), "{md}");
        let html = convert_markdown(&md, Flavor::Html).unwrap();
        assert!(html.contains("<strong>strong</strong>"), "{html}");
    }

    #[test]
    fn table_spans_styles_and_wrapped_cells() {
        let spanning = "|===\n| A | B | C\n2+|across two |C2\n|===\n";
        assert_eq!(
            to_markdown(spanning).unwrap(),
            "| A | B | C |\n| --- | --- | --- |\n| across two |  | C2 |\n"
        );

        let tall = "|===\n| a | b\n.2+|tall |one\n|two\n|===\n";
        assert_eq!(
            to_markdown(tall).unwrap(),
            "| a | b |\n| --- | --- |\n| tall | one |\n|  | two |\n"
        );

        // A style prefix changes nothing about the text or the columns.
        let styled = "[cols=\"1,1\"]\n|===\n| Name | Qty\na|Milk | 2\n|===\n";
        assert_eq!(
            to_markdown(styled).unwrap(),
            "| Name | Qty |\n| --- | --- |\n| Milk | 2 |\n"
        );

        // A cell may be written across several lines.
        let wrapped = "|===\n| a | b\n  continued | e\n|===\n";
        assert_eq!(
            to_markdown(wrapped).unwrap(),
            "| a | b continued |\n| --- | --- |\n| e |  |\n"
        );
    }

    #[test]
    fn tables_become_markdown_tables() {
        let doc = "[cols=\"1,1\"]\n|===\n| Name | Qty\n| Milk | 2\n| Bread | 3\n|===\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "| Name | Qty |\n| --- | --- |\n| Milk | 2 |\n| Bread | 3 |\n"
        );
    }

    #[test]
    fn comments_attributes_and_admonitions() {
        let doc = ":toc: left\n// hidden\n////\nalso hidden\n////\n\nNOTE: read this.\n\nTerm:: definition\n";
        let md = to_markdown(doc).unwrap();
        assert!(!md.contains("hidden"), "{md}");
        assert!(md.contains("> **NOTE:** read this."), "{md}");
        assert!(md.contains("**Term**: definition"), "{md}");
    }
}
