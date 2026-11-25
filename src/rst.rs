//! reStructuredText (a practical subset) → Markdown.
//!
//! The subset is the one reStructuredText documents actually open with:
//! sections (underlined, overlined and nested by adornment order),
//! bullet and enumerated lists with indentation nesting, literal blocks
//! introduced by a trailing `::`, block quotes, transitions, comments
//! and directives (dropped, with their indented bodies), field lists
//! (dropped), and inline markup — ``literal``, `*emphasis*`,
//! `**strong**`, ``` `text <url>`_ ``` links, interpreted text roles and
//! substitutions.
//!
//! Everything lands as Markdown first, and the Markdown renderers do the
//! rest, so `rst → markdown` and `rst → html` can never disagree about
//! what a document says. Grid and simple tables are out of scope: they
//! are a layout language of their own (see ROADMAP.md).

use crate::CResult;

const ADORNMENTS: &[char] = &[
    '=', '-', '~', '^', '"', '\'', '#', '*', '+', '_', '<', '>', '`',
];

/// Convert the subset described in the module docs to canonical Markdown.
pub fn to_markdown(input: &str) -> CResult<String> {
    let lines: Vec<&str> = input.lines().collect();
    let mut blocks: Vec<String> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut styles: Vec<char> = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            flush(&mut blocks, &mut paragraph);
            i += 1;
            continue;
        }

        // An overline title: adornment, title, the same adornment.
        if is_adornment(line) && i + 2 < lines.len() && is_adornment(lines[i + 2]) {
            let title = lines[i + 1].trim();
            if !title.is_empty() && !is_adornment(lines[i + 1]) && same_style(line, lines[i + 2]) {
                flush(&mut blocks, &mut paragraph);
                let level = style_level(&mut styles, adornment_char(line));
                blocks.push(heading(level, title));
                i += 3;
                continue;
            }
        }

        // An underlined title.
        if i + 1 < lines.len()
            && is_adornment(lines[i + 1])
            && !is_adornment(line)
            && indent_of(line) == 0
        {
            flush(&mut blocks, &mut paragraph);
            let level = style_level(&mut styles, adornment_char(lines[i + 1]));
            blocks.push(heading(level, line.trim()));
            i += 2;
            continue;
        }

        // Comments and directives swallow their indented bodies.
        if line.trim_start().starts_with("..") {
            flush(&mut blocks, &mut paragraph);
            let base = indent_of(line);
            i += 1;
            while i < lines.len() && (lines[i].trim().is_empty() || indent_of(lines[i]) > base) {
                i += 1;
            }
            continue;
        }

        // Field lists (`:author: Sean`) are metadata, not prose.
        if is_field(line) {
            flush(&mut blocks, &mut paragraph);
            i += 1;
            continue;
        }

        // Tables: grid (`+---+`) and simple (`===  ===`) both become GFM.
        if let Some((md, consumed)) = table(&lines[i..]) {
            flush(&mut blocks, &mut paragraph);
            blocks.push(md.trim_end().to_string());
            i += consumed;
            continue;
        }

        // A transition is a lone adornment of four or more characters.
        if is_adornment(line) && line.trim().len() >= 4 && indent_of(line) == 0 {
            flush(&mut blocks, &mut paragraph);
            blocks.push("---".to_string());
            i += 1;
            continue;
        }

        // Bullet and enumerated lists.
        if bullet_text(line).is_some() {
            flush(&mut blocks, &mut paragraph);
            let (md, consumed) = render_list(&lines[i..], false);
            blocks.push(md);
            i += consumed;
            continue;
        }
        if enumerated_text(line).is_some() {
            flush(&mut blocks, &mut paragraph);
            let (md, consumed) = render_list(&lines[i..], true);
            blocks.push(md);
            i += consumed;
            continue;
        }

        // An indented run at the left margin is a block quote.
        if indent_of(line) > 0 {
            flush(&mut blocks, &mut paragraph);
            let (md, consumed) = render_quote(&lines[i..]);
            blocks.push(md);
            i += consumed;
            continue;
        }

        // `Text::` introduces a literal block.
        let trimmed = line.trim_end();
        if trimmed.ends_with("::") {
            paragraph.push(trimmed[..trimmed.len() - 1].to_string());
            flush(&mut blocks, &mut paragraph);
            i += 1;
            while i < lines.len() && lines[i].trim().is_empty() {
                i += 1;
            }
            let base = lines.get(i).map_or(0, |l| indent_of(l));
            let mut body: Vec<String> = Vec::new();
            while i < lines.len() {
                let current = lines[i];
                if current.trim().is_empty() {
                    body.push(String::new());
                    i += 1;
                    continue;
                }
                if indent_of(current) < base {
                    break;
                }
                body.push(dedent(current, base));
                i += 1;
            }
            while body.last().is_some_and(|l| l.trim().is_empty()) {
                body.pop();
            }
            if !body.is_empty() {
                blocks.push(format!("```\n{}\n```", body.join("\n")));
            }
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

/// A grid or simple table → a GFM table, plus how many lines it used.
fn table(lines: &[&str]) -> Option<(String, usize)> {
    let first = lines[0].trim_end();
    if is_grid_border(first) {
        return grid_table(lines);
    }
    if is_simple_border(first) {
        return simple_table(lines);
    }
    None
}

fn is_grid_border(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3
        && trimmed.starts_with('+')
        && trimmed.ends_with('+')
        && trimmed.chars().all(|c| matches!(c, '+' | '-' | '='))
}

fn grid_table(lines: &[&str]) -> Option<(String, usize)> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut header_rows: Option<usize> = None;
    let mut i = 1usize;
    while i < lines.len() {
        let line = lines[i].trim_end();
        if is_grid_border(line) {
            if header_rows.is_none() && line.contains('=') {
                header_rows = Some(rows.len());
            }
            i += 1;
            if i >= lines.len() || !lines[i].trim_start().starts_with('|') {
                break;
            }
            continue;
        }
        if !line.trim_start().starts_with('|') {
            break;
        }
        rows.push(split_grid_row(line));
        i += 1;
    }
    if rows.is_empty() {
        return None;
    }
    Some((assemble(rows, header_rows), i))
}

fn split_grid_row(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_matches('|');
    trimmed
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// A run of `=` groups: the column spans of a simple table.
fn border_spans(line: &str) -> Option<Vec<(usize, usize)>> {
    let chars: Vec<char> = line.trim_end().chars().collect();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '=' {
            let start = index;
            while index < chars.len() && chars[index] == '=' {
                index += 1;
            }
            spans.push((start, index));
        } else {
            index += 1;
        }
    }
    if spans.len() < 2 {
        return None;
    }
    Some(spans)
}

fn is_simple_border(line: &str) -> bool {
    border_spans(line).is_some() && line.trim().chars().all(|c| c == '=' || c == ' ')
}

fn simple_table(lines: &[&str]) -> Option<(String, usize)> {
    let spans = border_spans(lines[0])?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut header_rows: Option<usize> = None;
    let mut i = 1usize;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            break;
        }
        if is_simple_border(line) {
            if header_rows.is_none() {
                header_rows = Some(rows.len());
                i += 1;
                continue;
            }
            i += 1;
            break;
        }
        rows.push(split_simple_row(line, &spans));
        i += 1;
    }
    if rows.is_empty() {
        return None;
    }
    Some((assemble(rows, header_rows), i))
}

fn split_simple_row(line: &str, spans: &[(usize, usize)]) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    spans
        .iter()
        .map(|&(start, end)| {
            let from = start.min(chars.len());
            let to = end.min(chars.len());
            chars[from..to]
                .iter()
                .collect::<String>()
                .trim()
                .to_string()
        })
        .collect()
}

/// Header rows are the ones before the table's interior border; a GFM
/// table takes one header row, so the rest become body rows.
fn assemble(rows: Vec<Vec<String>>, header_rows: Option<usize>) -> String {
    let split = match header_rows {
        Some(count) if count >= 1 && count < rows.len() => count,
        _ => 1,
    };
    let mut ordered: Vec<Vec<String>> = Vec::with_capacity(rows.len());
    ordered.push(rows[split - 1].clone());
    for (index, row) in rows.into_iter().enumerate() {
        if index != split - 1 {
            ordered.push(row);
        }
    }
    crate::markdown::table(&ordered)
}

fn flush(blocks: &mut Vec<String>, paragraph: &mut Vec<String>) {
    if paragraph.is_empty() {
        return;
    }
    let text: Vec<String> = paragraph.iter().map(|line| inline(line)).collect();
    blocks.push(text.join(" "));
    paragraph.clear();
}

fn heading(level: usize, text: &str) -> String {
    format!("{} {}", "#".repeat(level.clamp(1, 6)), inline(text))
}

fn style_level(styles: &mut Vec<char>, style: char) -> usize {
    if let Some(position) = styles.iter().position(|&s| s == style) {
        return position + 1;
    }
    if styles.len() >= 6 {
        return 6;
    }
    styles.push(style);
    styles.len()
}

fn is_adornment(line: &str) -> bool {
    let trimmed = line.trim();
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(first) if ADORNMENTS.contains(&first) && trimmed.len() >= 2 => {
            chars.all(|c| c == first)
        }
        _ => false,
    }
}

fn adornment_char(line: &str) -> char {
    line.trim().chars().next().unwrap_or('=')
}

fn same_style(a: &str, b: &str) -> bool {
    adornment_char(a) == adornment_char(b)
}

fn is_field(line: &str) -> bool {
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
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ')
                && (rest[at + 1..].is_empty() || rest[at + 1..].starts_with(' '))
    )
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn dedent(line: &str, base: usize) -> String {
    let mut rest = line;
    for _ in 0..base {
        if let Some(stripped) = rest.strip_prefix(' ') {
            rest = stripped;
        } else {
            break;
        }
    }
    rest.trim_end().to_string()
}

fn bullet_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if !matches!(first, '-' | '*' | '+' | '•') {
        return None;
    }
    let rest = chars.as_str();
    let text = rest.strip_prefix(' ')?;
    Some(text.trim())
}

fn enumerated_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &trimmed[digits.len()..];
    let rest = match rest.strip_prefix('.') {
        Some(rest) => rest,
        None => rest.strip_prefix(')')?,
    };
    let text = rest.strip_prefix(' ')?;
    if digits.is_empty() {
        // `#.` continues an enumeration without a number.
        return None;
    }
    Some(text.trim())
}

fn render_list(lines: &[&str], ordered: bool) -> (String, usize) {
    let mut out = String::new();
    let mut indents: Vec<usize> = Vec::new();
    let mut counters: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let mut line = lines[i];
        if line.trim().is_empty() {
            // A blank line continues the list when another item follows.
            let mut ahead = i;
            while ahead < lines.len() && lines[ahead].trim().is_empty() {
                ahead += 1;
            }
            match lines.get(ahead) {
                Some(next) if bullet_text(next).is_some() || enumerated_text(next).is_some() => {
                    i = ahead;
                    line = next;
                }
                _ => break,
            }
        }
        let text = if ordered {
            enumerated_text(line)
        } else {
            bullet_text(line)
        };
        let Some(text) = text else { break };
        let indent = indent_of(line);
        while indents.last().is_some_and(|&depth| indent < depth) {
            indents.pop();
            counters.pop();
        }
        if indents.last().map_or(true, |&depth| indent > depth) {
            indents.push(indent);
            counters.push(0);
        }
        let depth = indents.len() - 1;
        counters[depth] += 1;
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
        // Continuation lines belong to this item.
        while i < lines.len() {
            let next = lines[i];
            if next.trim().is_empty() {
                break;
            }
            let is_marker = bullet_text(next).is_some() || enumerated_text(next).is_some();
            if is_marker || indent_of(next) <= indent {
                break;
            }
            out.push_str(&" ".repeat(marker.len() + depth * 2));
            out.push_str(&inline(next.trim()));
            out.push('\n');
            i += 1;
        }
    }
    (out.trim_end().to_string(), i)
}

fn render_quote(lines: &[&str]) -> (String, usize) {
    let base = indent_of(lines[0]);
    let mut quoted: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            break;
        }
        if indent_of(line) < base {
            break;
        }
        quoted.push(dedent(line, base));
        i += 1;
    }
    let text: Vec<String> = quoted
        .iter()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {}", inline(line))
            }
        })
        .collect();
    (text.join("\n"), i)
}

/// The inline subset: literals, links, roles and substitutions.
fn inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            let mut run = 0usize;
            while i + run < chars.len() && chars[i + run] == '`' {
                run += 1;
            }
            if let Some(close) = find_run(&chars, i + run, run) {
                let inner: String = chars[i + run..close].iter().collect();
                let mut after = close + run;
                let mut underscore_run = 0usize;
                while after + underscore_run < chars.len() && chars[after + underscore_run] == '_' {
                    underscore_run += 1;
                }
                let mut body = inner.trim().to_string();
                let link = split_link(&body);
                if let Some((label, url)) = link {
                    out.push_str(&format!("[{label}]({url})"));
                    after += underscore_run.min(1);
                } else {
                    body = strip_role(&body);
                    cut_role_prefix(&mut out);
                    out.push('`');
                    out.push_str(&body);
                    out.push('`');
                    after += underscore_run;
                }
                i = after;
                continue;
            }
        }
        if c == '|' {
            if let Some(close) = chars[i + 1..].iter().position(|&c| c == '|') {
                let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                out.push_str(&inner);
                i = i + close + 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// ``` `label <url>`_ ``` → label and url.
fn split_link(body: &str) -> Option<(String, String)> {
    let open = body.find('<')?;
    let close = body[open..].find('>')? + open;
    let label = body[..open].trim();
    let url = body[open + 1..close].trim();
    if label.is_empty() || url.is_empty() {
        return None;
    }
    Some((label.to_string(), url.to_string()))
}

/// `:role:`text`` → `text`.
fn cut_role_prefix(out: &mut String) {
    if !out.ends_with(':') {
        return;
    }
    let end = out.len() - 1;
    let mut start = None;
    for (index, ch) in out[..end].char_indices().rev() {
        if ch == ':' {
            start = Some(index);
            break;
        }
        if !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
            return;
        }
    }
    if let Some(index) = start {
        out.truncate(index);
    }
}

fn strip_role(body: &str) -> String {
    let Some(rest) = body.strip_prefix(':') else {
        return body.to_string();
    };
    match rest.find(':') {
        Some(at) if at > 0 => rest[at + 1..].to_string(),
        _ => body.to_string(),
    }
}

fn find_run(chars: &[char], from: usize, run: usize) -> Option<usize> {
    let mut i = from;
    while i + run <= chars.len() {
        let mut matches = true;
        for k in 0..run {
            if chars[i + k] != '`' {
                matches = false;
                break;
            }
        }
        if matches && chars.get(i + run) != Some(&'`') {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{convert_markdown, Flavor};

    #[test]
    fn sections_follow_adornment_order() {
        let rst = "Title\n=====\n\nPart\n----\n\nDeeper\n~~~~~~\n";
        assert_eq!(
            to_markdown(rst).unwrap(),
            "# Title\n\n## Part\n\n### Deeper\n"
        );
        let overlined = "=====\nOver\n=====\n\nUnder\n-----\n";
        assert_eq!(to_markdown(overlined).unwrap(), "# Over\n\n## Under\n");
    }

    #[test]
    fn lists_nest_by_indentation() {
        let rst = "* one\n* two\n\n  * nested\n\n1. first\n2. second\n";
        let md = to_markdown(rst).unwrap();
        assert!(md.starts_with("- one\n- two\n"), "{md}");
        assert!(md.contains("  - nested"), "{md}");
        assert!(md.contains("1. first\n2. second"), "{md}");
    }

    #[test]
    fn literal_blocks_become_fences() {
        let rst = "Example::\n\n    let x = 1;\n    let y = 2;\n";
        assert_eq!(
            to_markdown(rst).unwrap(),
            "Example:\n\n```\nlet x = 1;\nlet y = 2;\n```\n"
        );
    }

    #[test]
    fn inline_markup_lands_as_markdown() {
        let rst = "A ``literal`` and `text <https://example.com>`_ plus :code:`x`.\n";
        let md = to_markdown(rst).unwrap();
        assert_eq!(
            md,
            "A `literal` and [text](https://example.com) plus `x`.\n"
        );
        // And the Markdown renderer agrees with itself.
        let html = convert_markdown(&md, Flavor::Html).unwrap();
        assert!(html.contains("<code>literal</code>"), "{html}");
        assert!(
            html.contains("<a href=\"https://example.com\">text</a>"),
            "{html}"
        );
    }

    #[test]
    fn tables_become_markdown_tables() {
        let simple = "=====  =====\nName   Qty\n=====  =====\nMilk   2\nBread  3\n=====  =====\n";
        assert_eq!(
            to_markdown(simple).unwrap(),
            "| Name | Qty |\n| --- | --- |\n| Milk | 2 |\n| Bread | 3 |\n"
        );

        let grid = "+------+-----+\n| Name | Qty |\n+======+=====+\n| Milk | 2   |\n\
                    +------+-----+\n";
        assert_eq!(
            to_markdown(grid).unwrap(),
            "| Name | Qty |\n| --- | --- |\n| Milk | 2 |\n"
        );

        // And the table renders through the Markdown pipeline.
        let html = convert_markdown(&to_markdown(simple).unwrap(), Flavor::Html).unwrap();
        assert!(html.contains("<th>Name</th>"), "{html}");
        assert!(html.contains("<td>Milk</td>"), "{html}");
    }

    #[test]
    fn comments_directives_and_quotes() {
        let rst = ".. note::\n   invisible\n\nVisible text.\n\n    A quote.\n\n----\n";
        let md = to_markdown(rst).unwrap();
        assert!(!md.contains("invisible"), "{md}");
        assert!(md.contains("Visible text."), "{md}");
        assert!(md.contains("> A quote."), "{md}");
        assert!(md.trim_end().ends_with("---"), "{md}");
    }
}
