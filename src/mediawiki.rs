//! MediaWiki wikitext (a practical subset) → Markdown.
//!
//! The subset: `== Heading ==` sections (one `=` per HTML level),
//! `''italic''`, `'''bold'''` and `'''''both'''''`, internal links
//! (`[[Page]]`, `[[Page|label]]`, sections included), external links
//! (`[https://… label]`, `[https://…]`), `*`/`#` lists with any nesting,
//! definition lists (`;term`/`:definition`), tables (`{|` … `|}` with
//! `|+` captions, `!` headers and `|-` rows), preformatted text (a leading
//! space, `<pre>`, `<source lang="…">`, `<syntaxhighlight>`), `<ref>`
//! footnotes, `----` rules, and the inline `<code>`/`<tt>`/`<kbd>`,
//! `<sup>`/`<sub>`, `<nowiki>` and `<br>` tags.
//!
//! Dropped on purpose: templates (`{{…}}`) and parser functions, magic
//! words (`__NOTOC__`), signatures (`~~~~`), `<gallery>`/`<math>`/
//! `<timeline>`/`<references/>`, and `Category:`/`File:`/`Image:` links —
//! they name things that live in the wiki, not in the document. An
//! interwiki link (`[[w:Page|label]]`) is kept, as a plain link to its
//! target.
//!
//! Like `rst`, `asciidoc` and `docbook`, everything lands as Markdown
//! first, so every target agrees with the others about what the document
//! says.

use crate::CResult;

/// Markdown has no deeper heading than this, and neither does MediaWiki.
const MAX_LEVEL: usize = 6;

/// Deeper list nesting is flattened rather than refused.
const MAX_LIST_DEPTH: usize = 6;

/// Convert the subset described in the module docs to canonical Markdown.
pub fn to_markdown(input: &str) -> CResult<String> {
    let lines: Vec<&str> = input.lines().collect();
    let mut notes = Notes::default();
    let mut blocks: Vec<String> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut in_comment = false;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // A comment may run over several lines; nothing inside it is text.
        if in_comment {
            in_comment = !line.contains("-->");
            i += 1;
            continue;
        }
        if trimmed.contains("<!--") {
            in_comment = !trimmed.contains("-->");
            let text = strip_comments(line);
            let text = text.trim().to_string();
            if !text.is_empty() {
                paragraph.push(inline(&text, &mut notes));
            }
            i += 1;
            continue;
        }

        if trimmed.is_empty() {
            flush(&mut blocks, &mut paragraph);
            i += 1;
            continue;
        }

        // A redirect is a pointer, not prose: keep the target as a link.
        if let Some(target) = redirect_target(trimmed) {
            flush(&mut blocks, &mut paragraph);
            let rendered = inline(target, &mut notes);
            if !rendered.trim().is_empty() {
                blocks.push(rendered);
            }
            i += 1;
            continue;
        }

        if let Some(level) = heading_level(line) {
            flush(&mut blocks, &mut paragraph);
            let text = heading_text(line, level);
            let rendered = inline(text, &mut notes);
            if !rendered.is_empty() {
                blocks.push(format!("{} {}", "#".repeat(level), rendered));
            }
            i += 1;
            continue;
        }

        if is_rule(trimmed) {
            flush(&mut blocks, &mut paragraph);
            blocks.push("---".to_string());
            i += 1;
            continue;
        }

        if trimmed.starts_with("{|") {
            flush(&mut blocks, &mut paragraph);
            let (rendered, consumed) = render_table(&lines[i..], &mut notes);
            blocks.extend(rendered);
            i += consumed;
            continue;
        }

        if list_marker(line).is_some() {
            flush(&mut blocks, &mut paragraph);
            let (rendered, consumed) = render_list(&lines[i..], &mut notes);
            if !rendered.is_empty() {
                blocks.push(rendered.trim_end().to_string());
            }
            i += consumed;
            continue;
        }

        // Block-level tags: `<pre>`, `<source>`, `<poem>`, `<nowiki>`, and
        // the ones that only ever hold things this reader drops.
        if let Some(tag) = block_tag(trimmed) {
            flush(&mut blocks, &mut paragraph);
            let (body, consumed) = block_body(&lines[i..], &tag.name);
            match tag.name.as_str() {
                "pre" => blocks.push(code_block(&body, "")),
                "source" | "syntaxhighlight" => {
                    let language = tag
                        .attr("lang")
                        .or_else(|| tag.attr("language"))
                        .unwrap_or("");
                    blocks.push(code_block(&body, &sanitize_language(language)));
                }
                "poem" => {
                    let inner = body.trim_matches('\n');
                    if !inner.trim().is_empty() {
                        blocks.push(quote(inner));
                    }
                }
                "nowiki" => {
                    let inner = body.trim_matches('\n').trim();
                    if !inner.is_empty() {
                        blocks.push(inner.to_string());
                    }
                }
                "gallery" | "math" | "timeline" | "score" | "references" | "chem" => {}
                _ => {
                    let inner = inline(&body, &mut notes);
                    if !inner.trim().is_empty() {
                        blocks.push(inner);
                    }
                }
            }
            i += consumed;
            continue;
        }

        // A line that starts with a space is preformatted text.
        if line.starts_with(' ') && !trimmed.is_empty() {
            flush(&mut blocks, &mut paragraph);
            let mut body: Vec<String> = Vec::new();
            while i < lines.len() {
                let current = lines[i];
                if !current.starts_with(' ') || current.trim().is_empty() {
                    break;
                }
                body.push(current.strip_prefix(' ').unwrap_or(current).to_string());
                i += 1;
            }
            blocks.push(code_block(&body.join("\n"), ""));
            continue;
        }

        paragraph.push(inline(trimmed, &mut notes));
        i += 1;
    }
    flush(&mut blocks, &mut paragraph);
    if !notes.defs.is_empty() {
        blocks.push(notes.defs.join("\n"));
    }

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
    let text = paragraph.join(" ");
    paragraph.clear();
    let text = text.trim().to_string();
    if !text.is_empty() {
        blocks.push(text);
    }
}

fn code_block(body: &str, language: &str) -> String {
    let body = body.trim_end_matches('\n');
    format!("```{language}\n{body}\n```")
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

fn sanitize_language(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '-' || *c == '#')
        .collect()
}

// ----------------------------------------------------------- block markers

/// `== Heading ==` → 2. A heading must be closed with at least as many
/// `=` as it opened with, the way the software insists.
fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    let open = trimmed.chars().take_while(|c| *c == '=').count();
    if open == 0 || open > MAX_LEVEL {
        return None;
    }
    let close = trimmed.chars().rev().take_while(|c| *c == '=').count();
    if close < open || trimmed.len() <= open + close {
        return None;
    }
    let inner = &trimmed[open..trimmed.len() - close];
    if inner.trim().is_empty() {
        return None;
    }
    Some(open)
}

fn heading_text(line: &str, level: usize) -> &str {
    let trimmed = line.trim();
    let close = trimmed.chars().rev().take_while(|c| *c == '=').count();
    let inner = &trimmed[level..trimmed.len() - close];
    inner.trim()
}

/// `----` and longer are horizontal rules.
fn is_rule(trimmed: &str) -> bool {
    trimmed.len() >= 4 && trimmed.chars().all(|c| c == '-')
}

fn redirect_target(trimmed: &str) -> Option<&str> {
    let lower = trimmed.to_lowercase();
    if !lower.starts_with("#redirect") {
        return None;
    }
    let rest = trimmed["#redirect".len()..].trim_start_matches(':').trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

/// The list markers at the start of a line (`*`, `#`, `;`, `:`), and the
/// text after them.
fn list_marker(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    let marker: String = trimmed
        .chars()
        .take_while(|c| matches!(c, '*' | '#' | ';' | ':'))
        .collect();
    if marker.is_empty() {
        return None;
    }
    let rest = &trimmed[marker.len()..];
    Some((marker, rest.trim()))
}

fn render_list(lines: &[&str], notes: &mut Notes) -> (String, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut counters: Vec<usize> = vec![0; MAX_LIST_DEPTH + 1];
    let mut kinds: Vec<char> = vec![' '; MAX_LIST_DEPTH + 1];
    let mut i = 0usize;
    while i < lines.len() {
        let Some((markers, text)) = list_marker(lines[i]) else {
            break;
        };
        let depth = markers.len().min(MAX_LIST_DEPTH);
        let kind = markers.chars().last().unwrap_or('*');
        let indent = "  ".repeat(depth.saturating_sub(1));
        for slot in counters.iter_mut().skip(depth + 1) {
            *slot = 0;
        }
        for slot in kinds.iter_mut().skip(depth + 1) {
            *slot = ' ';
        }
        // A bullet list does not continue the numbering of an ordered one.
        if kinds[depth] != kind {
            counters[depth] = 0;
            kinds[depth] = kind;
        }

        // A definition list: `;term: definition` on one line, or `;term`
        // with a `:definition` under it.
        if kind == ';' {
            if let Some((head, body)) = text.split_once(':') {
                let term = inline(head.trim(), notes);
                let body = inline(body.trim(), notes);
                if !term.is_empty() && !body.is_empty() {
                    out.push(format!("{indent}**{term}**: {body}"));
                    i += 1;
                    continue;
                }
            }
            let term = inline(text, notes);
            let definition = lines.get(i + 1).and_then(|next| list_marker(next)).filter(
                |(next_markers, next_text)| {
                    next_markers.len() == markers.len()
                        && next_markers.ends_with(':')
                        && !next_text.is_empty()
                },
            );
            match definition {
                Some((_, body)) => {
                    let body = inline(body, notes);
                    out.push(format!("{indent}**{term}**: {body}"));
                    i += 2;
                }
                None => {
                    out.push(format!("{indent}**{term}**"));
                    i += 1;
                }
            }
            continue;
        }
        if kind == ':' {
            // A definition without a term of its own.
            let body = inline(text, notes);
            out.push(format!("{indent}{body}"));
            i += 1;
            continue;
        }

        counters[depth] += 1;
        let marker = if kind == '#' {
            format!("{}. ", counters[depth])
        } else {
            "- ".to_string()
        };
        out.push(format!("{indent}{marker}{}", inline(text, notes)));
        i += 1;
    }
    (out.join("\n"), i)
}

// ------------------------------------------------------------------ tables

/// The body of a block-level tag that the reader handles line by line,
/// from the opening tag to its partner.
fn block_body(lines: &[&str], name: &str) -> (String, usize) {
    let closer = format!("</{name}>");
    let mut body: Vec<&str> = Vec::new();
    let mut i = 1usize;
    while i < lines.len() {
        let line = lines[i];
        if let Some(at) = line.to_lowercase().find(&closer) {
            body.push(&line[..at]);
            i += 1;
            break;
        }
        body.push(line);
        i += 1;
    }
    let mut text = body.join("\n");
    // The opening tag may carry content on its own line (`<pre>code`).
    let opener = lines.first().copied().unwrap_or("");
    if let Some(at) = opener.find('>') {
        let rest = opener[at + 1..].to_string();
        if !rest.trim().is_empty() {
            text = format!("{rest}\n{text}");
        }
    }
    (text, i)
}

fn render_table(lines: &[&str], notes: &mut Notes) -> (Vec<String>, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut header_rows = 0usize;
    let mut current: Vec<String> = Vec::new();
    let mut current_header = false;
    let mut caption = String::new();
    let mut i = 1usize;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("|}") {
            i += 1;
            break;
        }
        if line.is_empty() {
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix("|+") {
            let text = strip_cell_attrs(rest);
            caption = inline(text, notes);
            i += 1;
            continue;
        }
        if line.starts_with("|-") {
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
                if current_header {
                    header_rows += 1;
                }
                current_header = false;
            }
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix('!') {
            current_header = true;
            push_cells(&mut current, rest, "!!", notes);
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix('|') {
            push_cells(&mut current, rest, "||", notes);
            i += 1;
            continue;
        }
        // A line that starts with none of the above continues the last cell.
        if let Some(last) = current.last_mut() {
            last.push(' ');
            last.push_str(&inline(line, notes));
        }
        i += 1;
    }
    if !current.is_empty() {
        rows.push(current);
        if current_header {
            header_rows += 1;
        }
    }

    if !caption.trim().is_empty() {
        out.push(format!("**{caption}**"));
    }
    // A GFM table has a header row; a wikitext table may not, so the shape
    // is kept and an empty header goes on top rather than a data row being
    // promoted into one.
    if header_rows == 0 && !rows.is_empty() {
        let width = rows.first().map(Vec::len).unwrap_or(0);
        rows.insert(0, vec![String::new(); width]);
    }
    if !rows.is_empty() {
        let rendered = crate::markdown::table(&rows);
        if !rendered.is_empty() {
            out.push(rendered.trim_end().to_string());
        }
    }
    (out, i.max(1))
}

fn push_cells(cells: &mut Vec<String>, text: &str, separator: &str, notes: &mut Notes) {
    for part in text.split(separator) {
        let text = strip_cell_attrs(part);
        cells.push(inline(text, notes));
    }
}

/// A cell's `style="…"` / `class="…"` prefix is presentation, not text.
fn strip_cell_attrs(text: &str) -> &str {
    let Some(at) = text.find('|') else {
        return text.trim();
    };
    let head = text[..at].trim();
    if head.is_empty() {
        return text[at + 1..].trim();
    }
    let looks_like_attrs = head.split_whitespace().all(|piece| {
        let name = piece.split('=').next().unwrap_or("");
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && (piece.contains('=') || name == "align" || name == "scope")
    });
    if looks_like_attrs {
        text[at + 1..].trim()
    } else {
        text.trim()
    }
}

// ------------------------------------------------------------------ inline

#[derive(Debug, Default)]
struct Notes {
    defs: Vec<String>,
    names: Vec<(String, usize)>,
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("<!--") {
        out.push_str(&rest[..at]);
        match rest[at + 4..].find("-->") {
            Some(end) => rest = &rest[at + 4 + end + 3..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn inline(text: &str, notes: &mut Notes) -> String {
    inline_scan(text, notes, true)
}

fn inline_scan(text: &str, notes: &mut Notes, formatting: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];

        if c == '<' {
            if let Some((consumed, rendered)) = inline_tag(&chars, i, notes) {
                out.push_str(&rendered);
                i += consumed;
                continue;
            }
            out.push('<');
            i += 1;
            continue;
        }
        if c == '&' {
            if let Some((consumed, decoded)) = entity(&chars, i) {
                out.push(decoded);
                i += consumed;
                continue;
            }
        }
        if !formatting {
            out.push(c);
            i += 1;
            continue;
        }

        if c == '\'' {
            if let Some((consumed, rendered)) = emphasis(&chars, i) {
                out.push_str(&rendered);
                i += consumed;
                continue;
            }
        }

        if c == '[' {
            if chars.get(i + 1) == Some(&'[') {
                if let Some((consumed, rendered)) = internal_link(&chars, i, notes) {
                    out.push_str(&rendered);
                    i += consumed;
                    continue;
                }
            } else if let Some((consumed, rendered)) = external_link(&chars, i) {
                out.push_str(&rendered);
                i += consumed;
                continue;
            }
        }

        if c == '{' && chars.get(i + 1) == Some(&'{') {
            if let Some(consumed) = template(&chars, i) {
                i += consumed;
                continue;
            }
        }

        if c == '_' && chars.get(i + 1) == Some(&'_') {
            if let Some(consumed) = magic_word(&chars, i) {
                i += consumed;
                continue;
            }
        }

        if c == '~' {
            let run = chars[i..].iter().take_while(|ch| **ch == '~').count();
            if run >= 3 {
                i += run;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }
    out
}

/// `'''''both'''''`, `'''bold'''`, `''italic''`.
fn emphasis(chars: &[char], at: usize) -> Option<(usize, String)> {
    let run = chars[at..].iter().take_while(|c| **c == '\'').count();
    let width = match run {
        0..=1 => return None,
        2 => 2,
        3 => 3,
        _ => 5,
    };
    let marker: String = chars[at..at + width].iter().collect();
    let close = find_seq(chars, &marker, at + width)?;
    if close == at + width {
        return None;
    }
    let inner: String = chars[at + width..close].iter().collect();
    let rendered = match width {
        2 => format!("*{inner}*"),
        3 => format!("**{inner}**"),
        _ => format!("***{inner}***"),
    };
    Some((close + width - at, rendered))
}

/// `[[Page]]`, `[[Page|label]]`, `[[Page#Section|label]]`.
fn internal_link(chars: &[char], at: usize, notes: &mut Notes) -> Option<(usize, String)> {
    let close = find_seq(chars, "]]", at + 2)?;
    let inner: String = chars[at + 2..close].iter().collect();
    let (target, label) = match inner.split_once('|') {
        Some((target, label)) => (target.trim().to_string(), label.trim().to_string()),
        None => (inner.trim().to_string(), String::new()),
    };
    let consumed = close + 2 - at;
    if target.is_empty() {
        return Some((consumed, String::new()));
    }
    let namespace = target.split(':').next().unwrap_or("").trim().to_lowercase();
    // Things that live in the wiki, not in the document.
    if matches!(
        namespace.as_str(),
        "category" | "file" | "image" | "media" | "special" | "help" | "template" | "user"
    ) {
        return Some((consumed, String::new()));
    }
    let text = if label.is_empty() {
        inline_scan(&target, notes, true)
    } else {
        inline_scan(&label, notes, true)
    };
    Some((consumed, format!("[{text}]({target})")))
}

/// `[https://example.com label]`, or `[https://example.com]`.
fn external_link(chars: &[char], at: usize) -> Option<(usize, String)> {
    let close = find_seq(chars, "]", at + 1)?;
    let inner: String = chars[at + 1..close].iter().collect();
    let mut parts = inner.splitn(2, char::is_whitespace);
    let url = parts.next().unwrap_or("").trim();
    let label = parts.next().unwrap_or("").trim();
    if !looks_like_url(url) {
        return None;
    }
    let consumed = close + 1 - at;
    if label.is_empty() {
        return Some((consumed, format!("<{url}>")));
    }
    Some((consumed, format!("[{label}]({url})")))
}

fn looks_like_url(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("mailto:")
        || lower.starts_with("irc://")
        || lower.starts_with("news:")
        || lower.starts_with("//")
}

/// `{{template}}` and `{{{parameter}}}` disappear with everything inside
/// them, nesting included.
fn template(chars: &[char], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = at;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '{' && chars[i + 1] == '{' {
            depth += 1;
            i += 2;
            continue;
        }
        if i + 1 < chars.len() && chars[i] == '}' && chars[i + 1] == '}' {
            depth = depth.saturating_sub(1);
            i += 2;
            if depth == 0 {
                return Some(i - at);
            }
            continue;
        }
        i += 1;
    }
    None
}

/// `__NOTOC__` and friends are directives; they carry no text.
fn magic_word(chars: &[char], at: usize) -> Option<usize> {
    let close = find_seq(chars, "__", at + 2)?;
    let word: String = chars[at + 2..close].iter().collect();
    if word.is_empty() || !word.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        return None;
    }
    Some(close + 2 - at)
}

// -------------------------------------------------------------- inline tags

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    self_closing: bool,
}

impl Tag {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

fn parse_tag(chars: &[char], at: usize) -> Option<(Tag, usize)> {
    if chars.get(at) != Some(&'<') {
        return None;
    }
    let close = find_char(chars, '>', at + 1)?;
    let body: String = chars[at + 1..close].iter().collect();
    if body.starts_with('/') || body.starts_with('!') || body.starts_with('?') {
        return None;
    }
    let self_closing = body.trim_end().ends_with('/');
    let body = body.trim_end().trim_end_matches('/');
    let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
    let name = body[..name_end].to_lowercase();
    if name.is_empty() {
        return None;
    }
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
        if !key.is_empty() {
            attrs.push((key, after[1..1 + end].to_string()));
        }
        rest = &after[1 + end + 1..];
    }
    Some((
        Tag {
            name,
            attrs,
            self_closing,
        },
        close + 1 - at,
    ))
}

/// The inline tags, as (consumed, rendered), where `consumed` counts from
/// the opening `<`.
fn inline_tag(chars: &[char], at: usize, notes: &mut Notes) -> Option<(usize, String)> {
    let (tag, tag_len) = parse_tag(chars, at)?;
    let body_at = at + tag_len;
    match tag.name.as_str() {
        "br" => return Some((tag_len, "  \n".to_string())),
        "references" => return Some((tag_len, String::new())),
        "ref" => {
            let name = tag.attr("name").map(|name| name.to_string());
            let existing = name.as_ref().and_then(|name| {
                notes
                    .names
                    .iter()
                    .find(|(known, _)| known == name)
                    .map(|(_, number)| *number)
            });
            if tag.self_closing {
                let rendered = existing
                    .map(|number| format!("[^{number}]"))
                    .unwrap_or_default();
                return Some((tag_len, rendered));
            }
            let end = find_seq(chars, "</ref>", body_at)?;
            let body: String = chars[body_at..end].iter().collect();
            let number = match existing {
                Some(number) => number,
                None => {
                    let number = notes.defs.len() + 1;
                    if let Some(name) = &name {
                        notes.names.push((name.clone(), number));
                    }
                    let rendered = inline(&body, notes).replace(['\n', '\r'], " ");
                    notes.defs.push(format!("[^{number}]: {rendered}"));
                    number
                }
            };
            return Some((end + "</ref>".len() - at, format!("[^{number}]")));
        }
        "nowiki" => {
            let (text, end) = closing_body(chars, body_at, "nowiki")?;
            return Some((end - at, text));
        }
        "code" | "tt" | "kbd" | "samp" | "var" | "output" => {
            let (inner, end) = closing_body(chars, body_at, &tag.name)?;
            let text = inner.trim();
            if text.is_empty() {
                return Some((end - at, String::new()));
            }
            if text.contains('`') {
                return Some((end - at, text.to_string()));
            }
            return Some((end - at, format!("`{text}`")));
        }
        "sup" | "sub" | "small" | "big" | "u" | "s" | "strike" | "del" | "ins" | "span" | "div"
        | "center" | "font" | "abbr" | "b" | "i" | "em" | "strong" => {
            let Some((inner, end)) = closing_body(chars, body_at, &tag.name) else {
                return Some((tag_len, String::new()));
            };
            let rendered = inline_scan(&inner, notes, true);
            let rendered = match tag.name.as_str() {
                "b" | "strong" => format!("**{rendered}**"),
                "i" | "em" => format!("*{rendered}*"),
                _ => rendered,
            };
            return Some((end - at, rendered));
        }
        "gallery" | "math" | "timeline" | "score" | "chem" => {
            if tag.self_closing {
                return Some((tag_len, String::new()));
            }
            let Some((_, end)) = closing_body(chars, body_at, &tag.name) else {
                return Some((tag_len, String::new()));
            };
            return Some((end - at, String::new()));
        }
        _ => {}
    }
    // An unknown tag is dropped but its text is not.
    let Some((inner, end)) = closing_body(chars, body_at, &tag.name) else {
        return Some((tag_len, String::new()));
    };
    Some((end - at, inline_scan(&inner, notes, true)))
}

/// The text between here and `</name>`, and the index just past that.
fn closing_body(chars: &[char], from: usize, name: &str) -> Option<(String, usize)> {
    let closer = format!("</{name}>");
    let at = find_seq(chars, &closer, from)?;
    let inner: String = chars[from..at].iter().collect();
    Some((inner, at + closer.chars().count()))
}

fn find_char(chars: &[char], needle: char, from: usize) -> Option<usize> {
    chars
        .iter()
        .skip(from)
        .position(|c| *c == needle)
        .map(|at| at + from)
}

fn find_seq(chars: &[char], needle: &str, from: usize) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() || chars.len() < needle.len() {
        return None;
    }
    let last = chars.len() - needle.len();
    let mut i = from;
    while i <= last {
        if chars[i..i + needle.len()] == needle[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn entity(chars: &[char], at: usize) -> Option<(usize, char)> {
    let mut i = at + 1;
    let mut name = String::new();
    while i < chars.len() && chars[i] != ';' && name.len() < 12 {
        name.push(chars[i]);
        i += 1;
    }
    if i >= chars.len() || chars[i] != ';' {
        return None;
    }
    let decoded = match name.as_str() {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        other => {
            let digits = other.strip_prefix('#')?;
            let code = match digits
                .strip_prefix('x')
                .or_else(|| digits.strip_prefix('X'))
            {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse::<u32>().ok()?,
            };
            char::from_u32(code)
        }
    }?;
    Some((i + 1 - at, decoded))
}

/// The block-level opening tag, when the line starts with one: a tag in
/// the middle of a line (`A<br>B`) is inline, not a block.
fn block_tag(trimmed: &str) -> Option<Tag> {
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.first() != Some(&'<') {
        return None;
    }
    let (tag, _) = parse_tag(&chars, 0)?;
    let name = tag.name.as_str();
    if matches!(
        name,
        "pre"
            | "source"
            | "syntaxhighlight"
            | "poem"
            | "nowiki"
            | "gallery"
            | "math"
            | "timeline"
            | "score"
            | "chem"
            | "references"
            | "div"
            | "table"
    ) {
        return Some(tag);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{convert_markdown, Flavor};

    #[test]
    fn headings_and_paragraphs() {
        let doc = "Lead in.\n\n== Install ==\n\nFirst paragraph\nwrapped over two lines.\n\n=== Windows ===\n\nSecond.\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "Lead in.\n\n## Install\n\nFirst paragraph wrapped over two lines.\n\n### Windows\n\nSecond.\n"
        );
    }

    #[test]
    fn bold_and_italic() {
        let doc = "Plain ''soft'', '''loud''' and '''''both'''''.\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "Plain *soft*, **loud** and ***both***.\n"
        );
        let html = convert_markdown(&to_markdown(doc).unwrap(), Flavor::Html).unwrap();
        assert!(html.contains("<em>soft</em>"), "{html}");
        assert!(html.contains("<strong>loud</strong>"), "{html}");
    }

    #[test]
    fn links_lose_their_wiki_syntax() {
        let doc = "See [[Page]], [[Page|the page]] and [https://example.com the site].\n\n\
                   [[Category:Things]] [[File:Logo.png|thumb|A logo]]\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "See [Page](Page), [the page](Page) and [the site](https://example.com).\n"
        );
    }

    #[test]
    fn lists_nest_and_definitions_read_as_terms() {
        let doc = "* one\n** nested\n# first\n# second\n;term: definition\n";
        let md = to_markdown(doc).unwrap();
        assert!(
            md.starts_with("- one\n  - nested\n1. first\n2. second"),
            "{md}"
        );
        assert!(md.contains("**term**: definition"), "{md}");
    }

    #[test]
    fn code_keeps_its_language() {
        let doc = "<source lang=\"rust\">fn main() {}\n</source>\n\n indented code\n\nAfter.\n";
        let md = to_markdown(doc).unwrap();
        assert_eq!(
            md,
            "```rust\nfn main() {}\n```\n\n```\nindented code\n```\n\nAfter.\n"
        );
    }

    #[test]
    fn tables_become_gfm() {
        let doc = "{| class=\"wikitable\"\n|+ Caption\n|-\n! Name !! Qty\n|-\n| Milk || 2\n|}\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "**Caption**\n\n| Name | Qty |\n| --- | --- |\n| Milk | 2 |\n"
        );
        // No header row: an empty one goes on top, so no data is promoted.
        let headless = "{|\n|-\n| a || b\n|}\n";
        assert_eq!(
            to_markdown(headless).unwrap(),
            "|  |  |\n| --- | --- |\n| a | b |\n"
        );
    }

    #[test]
    fn templates_and_magic_words_disappear() {
        let doc = "{{Infobox|name=x}}Text __NOTOC__ more ~~~~\n";
        assert_eq!(to_markdown(doc).unwrap(), "Text  more\n");
    }

    #[test]
    fn refs_become_footnotes() {
        let doc = "Text<ref name=\"a\">The note.</ref> and <ref name=\"a\" /> again.\n";
        let md = to_markdown(doc).unwrap();
        assert_eq!(md, "Text[^1] and [^1] again.\n\n[^1]: The note.\n");
        let html = convert_markdown(&md, Flavor::Html).unwrap();
        assert!(html.contains("footnote"), "{html}");
    }

    #[test]
    fn entities_nowiki_and_breaks() {
        let doc = "A &amp; B<br>Caf&#233; and <nowiki>''raw''</nowiki>.\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "A & B  \nCaf\u{e9} and ''raw''.\n"
        );
    }

    #[test]
    fn rules_and_redirects() {
        let doc = "#REDIRECT [[Other page]]\n\n----\n";
        assert_eq!(
            to_markdown(doc).unwrap(),
            "[Other page](Other page)\n\n---\n"
        );
    }
}
