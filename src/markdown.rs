//! Markdown → HTML and Markdown → plain text.
//!
//! Covers the CommonMark constructs documents actually use: ATX and
//! setext headings, paragraphs, emphasis/strong/strikethrough, code
//! spans and fenced blocks, inline and reference links/images,
//! autolinks, link titles, blockquotes, ordered/unordered lists with
//! nesting, GFM task lists, horizontal rules, and GFM pipe tables. Raw HTML inside Markdown is
//! escaped, not passed through (documented: the output is meant to be
//! safe by default).

use std::collections::HashMap;

use crate::{corrupt, escape_html, CResult};

/// Footnote definitions collected from the document, plus the numbering
/// assigned while rendering. GFM numbers footnotes by the order of their
/// first reference, not by where they are defined.
#[derive(Debug, Default, Clone)]
struct Notes {
    order: Vec<String>,
    numbers: HashMap<String, usize>,
    texts: HashMap<String, String>,
}

impl Notes {
    fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Link reference definitions, keyed by normalized label:
/// `[docs]: https://example.com "Title"` → `"docs"` → `(url, title)`.
pub type Refs = HashMap<String, (String, String)>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Flavor {
    Html,
    Text,
}

pub fn convert_markdown(input: &str, flavor: Flavor) -> CResult<String> {
    convert_markdown_with(input, flavor, false)
}

/// `convert_markdown`, with GitHub-style `id="slug"` anchors on the
/// headings when `heading_ids` is set (what `{"heading_ids":true}`
/// asks for, and what makes a generated table of contents clickable).
pub fn convert_markdown_with(input: &str, flavor: Flavor, heading_ids: bool) -> CResult<String> {
    let collected = collect_definitions(input);
    let notes = collect_notes(&collected.body, collected.note_texts);
    let refs = &collected.refs;
    let lines: Vec<&str> = collected.body.iter().map(|s| s.as_str()).collect();
    let blocks = parse_blocks(&lines);
    let mut slugs = Slugs::default();
    let rendered: Vec<String> = blocks
        .iter()
        .map(|b| render_block(b, flavor, refs, &notes, &mut slugs, heading_ids))
        .collect();
    let sep = match flavor {
        Flavor::Html => "\n",
        Flavor::Text => "\n\n",
    };
    let mut out = rendered.join(sep);
    if !notes.is_empty() {
        out.push_str(&render_notes(&notes, flavor, refs));
    }
    if flavor == Flavor::Text && !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

/// The footnote block appended to the document, in reference order.
fn render_notes(notes: &Notes, flavor: Flavor, refs: &Refs) -> String {
    let empty = Notes::default();
    match flavor {
        Flavor::Html => {
            let mut out = String::from("\n<section class=\"footnotes\">\n<ol>");
            for (index, label) in notes.order.iter().enumerate() {
                let number = index + 1;
                let text = notes.texts.get(label).map(String::as_str).unwrap_or("");
                out.push_str(&format!(
                    "\n<li id=\"fn-{number}\"><p>{} <a href=\"#fnref-{number}\">↩</a></p></li>",
                    inline_ctx(text, Flavor::Html, refs, &empty)
                ));
            }
            out.push_str("\n</ol>\n</section>");
            out
        }
        Flavor::Text => {
            let mut out = String::from("\n\nFootnotes\n\n");
            for (index, label) in notes.order.iter().enumerate() {
                let text = notes.texts.get(label).map(String::as_str).unwrap_or("");
                out.push_str(&format!(
                    "[{}] {}\n",
                    index + 1,
                    inline_ctx(text, Flavor::Text, refs, &empty)
                ));
            }
            out.trim_end().to_string()
        }
    }
}

/// Markdown→CSV when the whole document is one GFM table — optionally
/// under a single heading, the shape a README payload usually takes.
/// Prose (or no table at all) is an error rather than a guess: there
/// would be no rows to speak of.
pub fn markdown_to_csv(input: &str) -> CResult<String> {
    Ok(crate::csv::write_csv(&markdown_rows(input)?))
}

/// The table rows behind markdown→csv and markdown→tsv.
pub fn markdown_rows(input: &str) -> CResult<Vec<Vec<String>>> {
    let collected = collect_definitions(input);
    let refs = &collected.refs;
    let raw: Vec<&str> = collected.body.iter().map(|s| s.as_str()).collect();
    let blocks = parse_blocks(&raw);
    let mut table: Option<&Block> = None;
    let mut heading = false;
    for block in &blocks {
        match block {
            Block::Heading(..) if table.is_none() && !heading => heading = true,
            Block::Table(..) if table.is_none() => table = Some(block),
            _ => {
                return Err(corrupt(
                    "markdown→csv needs one table (and at most one heading before it)",
                ))
            }
        }
    }
    let (headers, rows) = match table {
        Some(Block::Table(headers, _, rows)) => (headers, rows),
        _ => return Err(corrupt("markdown→csv: no table found")),
    };
    // Cells are Markdown, so render them to plain text first.
    let empty = Notes::default();
    let cell = |text: &str| inline_ctx(text, Flavor::Text, refs, &empty);
    let width = headers.len();
    let mut out: Vec<Vec<String>> = Vec::with_capacity(rows.len() + 1);
    out.push(headers.iter().map(|header| cell(header)).collect());
    for row in rows {
        let mut cells: Vec<String> = row.iter().map(|c| cell(c)).collect();
        cells.resize(width, String::new());
        out.push(cells);
    }
    Ok(out)
}

/// GitHub-style heading anchors: lowercase, punctuation dropped,
/// whitespace and dashes collapsed to a single dash.
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            for lower in c.to_lowercase() {
                out.push(lower);
            }
        } else if c == '_' {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push('_');
        } else if c.is_whitespace() || c == '-' {
            pending_dash = true;
        }
    }
    out
}

/// Anchors already handed out in a document, so repeated headings get
/// `-1`, `-2`, … the way GitHub numbers them.
#[derive(Debug, Default)]
pub struct Slugs {
    seen: HashMap<String, usize>,
}

impl Slugs {
    pub fn next(&mut self, text: &str) -> String {
        let base = slugify(text);
        let count = self.seen.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            base
        } else {
            format!("{base}-{}", *count - 1)
        }
    }
}

/// The headings of a Markdown document in document order, as
/// (level, plain text) — the input to a table of contents.
pub fn headings(input: &str) -> CResult<Vec<(usize, String)>> {
    let collected = collect_definitions(input);
    let notes = collect_notes(&collected.body, collected.note_texts);
    let raw: Vec<&str> = collected.body.iter().map(|s| s.as_str()).collect();
    let blocks = parse_blocks(&raw);
    let mut out = Vec::new();
    collect_headings(&blocks, &collected.refs, &notes, &mut out);
    Ok(out)
}

fn collect_headings(blocks: &[Block], refs: &Refs, notes: &Notes, out: &mut Vec<(usize, String)>) {
    for block in blocks {
        match block {
            Block::Heading(level, text) => {
                out.push((*level, inline_ctx(text, Flavor::Text, refs, notes)));
            }
            Block::Quote(inner) => collect_headings(inner, refs, notes, out),
            Block::List(_, items) => {
                for item in items {
                    collect_headings(item, refs, notes, out);
                }
            }
            _ => {}
        }
    }
}

/// Which character canonical emphasis uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Emphasis {
    /// `*emphasis*` / `**strong**` — the default, and what GitHub writes.
    #[default]
    Asterisk,
    /// `_emphasis_` / `__strong__`.
    Underscore,
}

impl Emphasis {
    pub fn parse(text: &str) -> CResult<Emphasis> {
        match text.trim().to_lowercase().as_str() {
            "asterisk" | "star" | "*" => Ok(Emphasis::Asterisk),
            "underscore" | "under" | "_" => Ok(Emphasis::Underscore),
            other => Err((
                crate::ERR_INVALID_ARG,
                format!("unknown emphasis style '{other}' (expected asterisk or underscore)"),
            )),
        }
    }

    fn marker(self) -> char {
        match self {
            Emphasis::Asterisk => '*',
            Emphasis::Underscore => '_',
        }
    }
}

/// Which character canonical code fences use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fence {
    /// Triple backticks — the default.
    #[default]
    Backtick,
    /// Triple tildes, for documents whose code is full of backticks.
    Tilde,
}

impl Fence {
    pub fn parse(text: &str) -> CResult<Fence> {
        match text.trim().to_lowercase().as_str() {
            "backtick" | "backticks" | "`" => Ok(Fence::Backtick),
            "tilde" | "tildes" | "~" => Ok(Fence::Tilde),
            other => Err((
                crate::ERR_INVALID_ARG,
                format!("unknown fence style '{other}' (expected backtick or tilde)"),
            )),
        }
    }

    fn character(self) -> char {
        match self {
            Fence::Backtick => '`',
            Fence::Tilde => '~',
        }
    }
}

/// The style layer `normalize` can apply on top of its canonical form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub emphasis: Emphasis,
    pub fence: Fence,
}

impl Style {
    /// A style spec: the same two choices, as one flag.
    ///
    /// Comma- or space-separated tokens, applied left to right so the last
    /// one wins:
    ///
    /// - `github` — the default (asterisk emphasis, backtick fences);
    /// - `asterisk` / `underscore` — the emphasis marker;
    /// - `backtick` / `tilde` — the fence character;
    /// - `emphasis=<x>` and `fence=<y>` — the same thing, spelled out, for
    ///   config files that would rather not depend on token order.
    pub fn parse_spec(spec: &str) -> CResult<Style> {
        let mut style = Style::default();
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Ok(style);
        }
        for token in trimmed.split([',', ' ', '\t']).filter(|t| !t.is_empty()) {
            let token = token.trim().to_lowercase();
            match token.as_str() {
                "github" | "default" => style = Style::default(),
                "asterisk" | "star" | "emphasis=asterisk" | "emphasis=star" => {
                    style.emphasis = Emphasis::Asterisk
                }
                "underscore" | "emphasis=underscore" | "emphasis=under" => {
                    style.emphasis = Emphasis::Underscore
                }
                "backtick" | "backticks" | "fence=backtick" => style.fence = Fence::Backtick,
                "tilde" | "tildes" | "fence=tilde" => style.fence = Fence::Tilde,
                other => {
                    return Err((
                        crate::ERR_INVALID_ARG,
                        format!(
                            "unknown normalize style '{other}' (try github, asterisk, \
                             underscore, backtick or tilde)"
                        ),
                    ))
                }
            }
        }
        Ok(style)
    }

    /// The presets `capabilities` advertises and `--style` accepts alone.
    pub fn presets() -> &'static [&'static str] {
        &["github", "asterisk", "underscore", "backtick", "tilde"]
    }

    /// The ABI's style object, e.g. `{"emphasis":"underscore"}`.
    pub fn from_json(text: &str) -> CResult<Style> {
        use crate::json::{parse_json, Json};
        let mut style = Style::default();
        if text.trim().is_empty() {
            return Ok(style);
        }
        let entries = match parse_json(text)? {
            Json::Object(entries) => entries,
            _ => {
                return Err((
                    crate::ERR_INVALID_ARG,
                    "normalize options must be a JSON object".to_string(),
                ))
            }
        };
        for (key, value) in entries {
            let text = match value {
                Json::Str(text) => text,
                _ => {
                    return Err((
                        crate::ERR_INVALID_ARG,
                        format!("normalize option '{key}' must be a string"),
                    ))
                }
            };
            match key.as_str() {
                "emphasis" => style.emphasis = Emphasis::parse(&text)?,
                "fence" => style.fence = Fence::parse(&text)?,
                "style" => style = Style::parse_spec(&text)?,
                other => {
                    return Err((
                        crate::ERR_INVALID_ARG,
                        format!("unknown normalize option '{other}'"),
                    ))
                }
            }
        }
        Ok(style)
    }
}

/// Whether a fragment defines reference links or footnotes — the two
/// Markdown constructs that are document-wide, and therefore the two a
/// block-at-a-time reader cannot resolve on its own.
pub fn has_document_wide_constructs(input: &str) -> bool {
    let collected = collect_definitions(input);
    !collected.link_defs.is_empty() || !collected.note_defs.is_empty()
}

/// Canonical Markdown: one blank line between blocks, `#` headings, `-`
/// bullets, sequential numbers in ordered lists, GFM tables with their
/// alignment row, `> ` quotes, and the document's link and footnote
/// definitions collected at the end.
pub fn normalize(input: &str) -> CResult<String> {
    normalize_with(input, Style::default())
}

/// `normalize`, with the opt-in style layer: which character emphasis
/// and code fences use. Structure is canonical either way.
pub fn normalize_with(input: &str, style: Style) -> CResult<String> {
    let collected = collect_definitions(input);
    let notes = collect_notes(&collected.body, collected.note_texts);
    let raw: Vec<&str> = collected.body.iter().map(|s| s.as_str()).collect();
    let blocks = parse_blocks(&raw);
    let mut parts: Vec<String> = Vec::new();
    let rendered = render_markdown_blocks(&blocks, &collected.refs, &notes, style);
    if !rendered.is_empty() {
        parts.push(rendered);
    }
    if !collected.link_defs.is_empty() {
        parts.push(collected.link_defs.join("\n"));
    }
    if !collected.note_defs.is_empty() {
        parts.push(collected.note_defs.join("\n"));
    }
    let mut out = parts.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

fn render_markdown_blocks(blocks: &[Block], refs: &Refs, notes: &Notes, style: Style) -> String {
    let parts: Vec<String> = blocks
        .iter()
        .map(|block| {
            render_markdown_block(block, refs, notes, style)
                .trim_end()
                .to_string()
        })
        .filter(|text| !text.is_empty())
        .collect();
    parts.join("\n\n")
}

fn render_markdown_block(block: &Block, refs: &Refs, notes: &Notes, style: Style) -> String {
    match block {
        Block::Heading(level, text) => normalize_inline(
            &format!("{} {}", "#".repeat(*level), text.trim()),
            style.emphasis.marker(),
        ),
        Block::Paragraph(text) => normalize_inline(
            &text
                .lines()
                .enumerate()
                .map(|(index, line)| {
                    if index == 0 {
                        line.trim_start()
                    } else {
                        line.trim_end()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            style.emphasis.marker(),
        ),
        Block::Code(info, body) => {
            let text = body.trim_end_matches('\n');
            let fence = fence_for(text, style.fence);
            format!("{fence}{}\n{text}\n{fence}", info.trim())
        }
        Block::Quote(inner) => {
            let inner = render_markdown_blocks(inner, refs, notes, style);
            inner
                .lines()
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
        Block::List(ordered, items) => {
            let mut out: Vec<String> = Vec::new();
            for (index, item) in items.iter().enumerate() {
                let marker = if *ordered {
                    format!("{}. ", index + 1)
                } else {
                    "- ".to_string()
                };
                let pad = " ".repeat(marker.len());
                let inner = render_markdown_blocks(item, refs, notes, style);
                let mut lines = inner.lines();
                let mut text = format!("{marker}{}", lines.next().unwrap_or(""));
                for line in lines {
                    text.push('\n');
                    if !line.trim().is_empty() {
                        text.push_str(&pad);
                    }
                    text.push_str(line);
                }
                out.push(text);
            }
            out.join("\n")
        }
        Block::Rule => "---".to_string(),
        Block::Table(headers, aligns, rows) => {
            let mut out = String::new();
            out.push_str("| ");
            out.push_str(&joined_cells(headers));
            out.push_str(" |\n|");
            for index in 0..headers.len() {
                out.push_str(match aligns.get(index) {
                    Some('l') => " :--- |",
                    Some('c') => " :---: |",
                    Some('r') => " ---: |",
                    _ => " --- |",
                });
            }
            for row in rows {
                out.push_str("\n| ");
                out.push_str(&joined_cells(row));
                out.push_str(" |");
            }
            out
        }
    }
}

/// A GFM table: the first row is the header, pipes inside cells are
/// escaped, and every row after it is a body row.
pub fn table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        out.push_str(&table_row_padded(row, width));
        if index == 0 {
            out.push('|');
            for _ in 0..width {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out
}

/// One GFM table row, without the header separator — the piece a streamed
/// CSV→Markdown conversion writes per record.
pub fn table_row(cells: &[String]) -> String {
    table_row_padded(cells, cells.len())
}

fn table_row_padded(row: &[String], width: usize) -> String {
    let mut cells: Vec<String> = row.iter().map(|cell| cell.replace('|', "\\|")).collect();
    cells.resize(width, String::new());
    format!("| {} |\n", cells.join(" | "))
}

/// Canonical inline markup: emphasis in the *other* marker becomes the
/// requested one (`_x_` for the asterisk style, `*x*` for the underscore
/// style), and code spans lose their padding and any surplus backticks.
/// Line breaks inside a paragraph are left alone — they are editorial,
/// not structural.
fn normalize_inline(text: &str, marker: char) -> String {
    let other = if marker == '*' { '_' } else { '*' };
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
                let trimmed = inner.trim();
                // Only a span that needs no padding is rewritten. When the
                // text itself starts or ends with a backtick, CommonMark's
                // padding (`\`\` \` \`\` `) would be read back as a
                // different run structure on the next pass — normalize would
                // then not be idempotent, and repeating it would keep
                // changing the document. Such a span stays byte for byte.
                if !trimmed.is_empty() && !trimmed.starts_with('`') && !trimmed.ends_with('`') {
                    out.push('`');
                    out.push_str(trimmed);
                    out.push('`');
                    i = close + run;
                    continue;
                }
            }
        }
        if c == other {
            let mut run = 0usize;
            while i + run < chars.len() && chars[i + run] == other {
                run += 1;
            }
            let opens = i == 0 || !chars[i - 1].is_alphanumeric();
            let starts_word = chars
                .get(i + run)
                .is_some_and(|next| !next.is_whitespace() && *next != other);
            if opens && starts_word && run <= 2 {
                if let Some(close) = find_run(&chars, i + run, run) {
                    let ends_word = close > i + run && !chars[close - 1].is_whitespace();
                    let inner: String = chars[i + run..close].iter().collect();
                    // Only a rewrite that reads back the same way counts. If
                    // the marker would touch another emphasis character (or
                    // the span's own text holds one), the new run could pair
                    // with it on the next pass — `__<_>_` became `_*<*>_`
                    // and then `**<*>*`. Those spans stay as written.
                    let before = i.checked_sub(1).map(|index| chars[index]);
                    let after = chars.get(close + run).copied();
                    let safe = [before, after]
                        .iter()
                        .all(|neighbour| *neighbour != Some(other) && *neighbour != Some(marker))
                        && !inner.contains(marker);
                    if ends_word && safe {
                        out.push_str(&marker.to_string().repeat(run));
                        out.push_str(&inner);
                        out.push_str(&marker.to_string().repeat(run));
                        i = close + run;
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// A fence at least three characters long that cannot appear inside the
/// body, so the block always closes where it opened.
fn fence_for(body: &str, fence: Fence) -> String {
    let character = fence.character();
    let mut longest = 0usize;
    let mut run = 0usize;
    for c in body.chars() {
        if c == character {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    character.to_string().repeat((longest + 1).max(3))
}

/// The next run of `run` identical `needle` characters at or after
/// `from`, not part of a longer run.
fn find_run(chars: &[char], from: usize, run: usize) -> Option<usize> {
    let needle = if from > 0 { chars[from - 1] } else { '`' };
    let mut i = from;
    while i + run <= chars.len() {
        let matches = chars[i..i + run].iter().all(|&c| c == needle);
        if matches && chars.get(i + run) != Some(&needle) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn joined_cells(cells: &[String]) -> String {
    cells
        .iter()
        .map(|cell| cell.trim())
        .collect::<Vec<_>>()
        .join(" | ")
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

/// A document split into what the parsers need: the reference links,
/// the footnote texts, the body lines with definitions removed, and the
/// definitions themselves in the order they appeared (so normalization
/// can put them back verbatim).
#[derive(Debug, Default)]
struct Collected {
    refs: Refs,
    note_texts: HashMap<String, String>,
    body: Vec<String>,
    link_defs: Vec<String>,
    note_defs: Vec<String>,
}

/// Pull `[label]: url "title"` and `[^label]: note` lines out of the
/// document (fenced code is left alone) and hand back the remaining
/// lines. Definitions are document-wide, so this runs before block
/// parsing.
fn collect_definitions(input: &str) -> Collected {
    let mut refs = Refs::new();
    let mut notes: HashMap<String, String> = HashMap::new();
    let mut kept: Vec<String> = Vec::new();
    let mut link_defs: Vec<String> = Vec::new();
    let mut note_defs: Vec<String> = Vec::new();
    let mut current_note_line: Option<usize> = None;
    let mut in_fence = false;
    let mut open_note: Option<String> = None;
    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            kept.push(line.to_string());
            continue;
        }
        if !in_fence {
            // A note's continuation: indented lines after the definition.
            if let Some(label) = open_note.clone() {
                let indent = line_indent(line);
                if !line.trim().is_empty() && indent >= 4 {
                    if let Some(text) = notes.get_mut(&label) {
                        text.push(' ');
                        text.push_str(line.trim());
                        if let Some(at) = current_note_line {
                            if let Some(def) = note_defs.get_mut(at) {
                                def.push(' ');
                                def.push_str(line.trim());
                            }
                        }
                    }
                    continue;
                }
                open_note = None;
            }
            if let Some((label, text)) = parse_footnote_definition(line) {
                if !notes.contains_key(&label) {
                    notes.insert(label.clone(), text.clone());
                    note_defs.push(format!("[^{label}]: {text}"));
                    current_note_line = Some(note_defs.len() - 1);
                }
                open_note = Some(label);
                continue;
            }
            if let Some((label, url, title)) = parse_definition(line) {
                if !refs.contains_key(&normalize_label(&label)) {
                    link_defs.push(line.trim().to_string());
                }
                refs.entry(normalize_label(&label)).or_insert((url, title));
                continue;
            }
        }
        kept.push(line.to_string());
    }
    Collected {
        refs,
        note_texts: notes,
        body: kept,
        link_defs,
        note_defs,
    }
}

/// `[^label]: note text` with up to three spaces of indentation.
fn parse_footnote_definition(line: &str) -> Option<(String, String)> {
    if line_indent(line) > 3 {
        return None;
    }
    let rest = line.trim().strip_prefix("[^")?;
    let close = rest.find("]:")?;
    let label = rest[..close].trim();
    if label.is_empty() || label.contains(']') || label.contains('[') {
        return None;
    }
    Some((label.to_string(), rest[close + 2..].trim().to_string()))
}

/// The footnotes this document refers to, numbered in order of first
/// reference. Fenced code is skipped, and references to undefined notes
/// are left as literal text.
fn collect_notes(lines: &[String], texts: HashMap<String, String>) -> Notes {
    let mut notes = Notes {
        order: Vec::new(),
        numbers: HashMap::new(),
        texts,
    };
    let mut in_fence = false;
    for line in lines {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = line.as_str();
        while let Some(open) = rest.find("[^") {
            let after = &rest[open + 2..];
            let Some(close) = after.find(']') else {
                break;
            };
            let label = &after[..close];
            let known = notes.texts.contains_key(label) && !notes.numbers.contains_key(label);
            if known {
                notes.order.push(label.to_string());
                let number = notes.order.len();
                notes.numbers.insert(label.to_string(), number);
            }
            rest = &after[close..];
        }
    }
    notes
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
    Some((label.to_string(), url, parse_title(&title)))
}

/// Split an inline destination of the form `url`, `<url>`, `url "title"`,
/// `url 'title'` or `url (title)` into the URL and the raw title.
fn split_destination(raw: &str) -> (String, String) {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('<') {
        if let Some(end) = rest.find('>') {
            return (rest[..end].to_string(), parse_title(&rest[end + 1..]));
        }
    }
    match raw.find(char::is_whitespace) {
        Some(w) => (raw[..w].to_string(), parse_title(&raw[w..])),
        None => (raw.to_string(), String::new()),
    }
}

/// The title part of a link destination: `"t"`, `'t'` or `(t)`, with
/// backslash escapes of the delimiters undone. Anything else is empty.
fn parse_title(raw: &str) -> String {
    let raw = raw.trim();
    let delim = match raw.chars().next() {
        Some(d @ ('"' | '\'' | '(')) => d,
        _ => return String::new(),
    };
    let closer = if delim == '(' { ')' } else { delim };
    let count = raw.chars().count();
    if count < 2 || !raw.ends_with(closer) {
        return String::new();
    }
    let inner: String = raw.chars().skip(1).take(count - 2).collect();
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(next @ ('"' | '\'' | '(' | ')' | '\\')) => out.push(next),
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
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

/// How deeply blocks may nest — a quote in a quote in a list item. The
/// parser and the renderers both walk one frame per level, so this is what
/// keeps a hostile document from overflowing the stack; past the cap the
/// remaining lines stay literal text rather than being dropped.
const MAX_BLOCK_DEPTH: usize = 48;

fn parse_blocks(lines: &[&str]) -> Vec<Block> {
    parse_blocks_at(lines, 0)
}

fn parse_blocks_at(lines: &[&str], depth: usize) -> Vec<Block> {
    if depth >= MAX_BLOCK_DEPTH {
        let text = lines.join("\n");
        return if text.trim().is_empty() {
            Vec::new()
        } else {
            vec![Block::Paragraph(text)]
        };
    }
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
            blocks.push(Block::Quote(parse_blocks_at(&refs, depth + 1)));
            continue;
        }
        // List.
        if list_marker(line).is_some() {
            let (list, consumed) = parse_list(lines, i, depth);
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

fn parse_list(lines: &[&str], start: usize, depth: usize) -> (Block, usize) {
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
            parse_blocks_at(&refs, depth + 1)
        })
        .collect();
    (Block::List(ordered, item_blocks), consumed)
}

/// A GFM task-list marker at the start of an item: `[ ] todo` /
/// `[x] done` → (checked, remaining text).
fn task_marker(text: &str) -> Option<(bool, &str)> {
    let rest = text.trim_start().strip_prefix('[')?;
    let mut chars = rest.chars();
    let checked = match chars.next()? {
        ' ' => false,
        'x' | 'X' => true,
        _ => return None,
    };
    let after = chars.as_str().strip_prefix(']')?;
    // GFM wants a space (or nothing) between the box and the text.
    match after.strip_prefix(' ') {
        Some(body) => Some((checked, body)),
        None if after.is_empty() => Some((checked, "")),
        None => None,
    }
}

/// The task marker of a list item, if its first block is a paragraph
/// that opens with one.
fn item_task(item: &[Block]) -> Option<(bool, &str)> {
    match item.first() {
        Some(Block::Paragraph(text)) => task_marker(text),
        _ => None,
    }
}

fn render_block(
    b: &Block,
    flavor: Flavor,
    refs: &Refs,
    notes: &Notes,
    slugs: &mut Slugs,
    heading_ids: bool,
) -> String {
    match (b, flavor) {
        (Block::Heading(level, text), Flavor::Html) => {
            let inner = inline_ctx(text, flavor, refs, notes);
            if heading_ids {
                let anchor = slugs.next(&inline_ctx(text, Flavor::Text, refs, notes));
                format!("<h{level} id=\"{anchor}\">{inner}</h{level}>")
            } else {
                format!("<h{level}>{inner}</h{level}>")
            }
        }
        (Block::Heading(_, text), Flavor::Text) => inline_ctx(text, flavor, refs, notes),
        (Block::Paragraph(text), Flavor::Html) => {
            format!("<p>{}</p>", inline_ctx(text, flavor, refs, notes))
        }
        (Block::Paragraph(text), Flavor::Text) => inline_ctx(text, flavor, refs, notes),
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
                .map(|x| render_block(x, flavor, refs, notes, slugs, heading_ids))
                .collect();
            format!("<blockquote>\n{}\n</blockquote>", inside.join("\n"))
        }
        (Block::Quote(inner), Flavor::Text) => {
            let inside: Vec<String> = inner
                .iter()
                .map(|x| render_block(x, flavor, refs, notes, slugs, heading_ids))
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
                        let task = item_task(item);
                        if let Some((checked, _)) = task {
                            out.push_str(if checked {
                                "<input type=\"checkbox\" disabled checked> "
                            } else {
                                "<input type=\"checkbox\" disabled> "
                            });
                        }
                        let parts: Vec<String> = item
                            .iter()
                            .enumerate()
                            .map(|(i, block)| match (i, task) {
                                (0, Some((_, rest))) => inline_ctx(rest, flavor, refs, notes),
                                _ => render_block(block, flavor, refs, notes, slugs, heading_ids),
                            })
                            .collect();
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
                        let mut marker = if *ordered {
                            format!("{}. ", n + 1)
                        } else {
                            "- ".to_string()
                        };
                        let task = item_task(item);
                        if let Some((checked, _)) = task {
                            marker.push_str(if checked { "[x] " } else { "[ ] " });
                        }
                        let parts: Vec<String> = item
                            .iter()
                            .enumerate()
                            .map(|(i, block)| match (i, task) {
                                (0, Some((_, rest))) => inline_ctx(rest, flavor, refs, notes),
                                _ => render_block(block, flavor, refs, notes, slugs, heading_ids),
                            })
                            .collect();
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
                    inline_ctx(h, flavor, refs, notes)
                ));
            }
            out.push_str("</tr>\n</thead>\n<tbody>");
            for row in rows {
                out.push_str("\n<tr>");
                for (j, cell) in row.iter().enumerate() {
                    out.push_str(&format!(
                        "<td{}>{}</td>",
                        align_attr(aligns.get(j)),
                        inline_ctx(cell, flavor, refs, notes)
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
    inline_ctx(text, flavor, refs, &Notes::default())
}

/// The inline renderer with every document-level input it needs.
fn inline_ctx(text: &str, flavor: Flavor, refs: &Refs, notes: &Notes) -> String {
    InlineParser {
        chars: text.chars().collect(),
        pos: 0,
        flavor,
        refs,
        notes,
    }
    .run()
}

struct InlineParser<'a> {
    chars: Vec<char>,
    pos: usize,
    flavor: Flavor,
    refs: &'a Refs,
    notes: &'a Notes,
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
        // A footnote reference is `[^label]` and nothing else: `[^x](…)`
        // stays an ordinary link with a caret in its label.
        if !image {
            if let Some(name) = label.strip_prefix('^') {
                let bare = !matches!(self.chars.get(j + 1), Some('(') | Some('['));
                if let Some(number) = self.notes.numbers.get(name) {
                    if bare {
                        self.pos = j + 1;
                        self.emit_note(out, *number);
                        return true;
                    }
                }
            }
        }
        match self.chars.get(j + 1) {
            Some('(') => match self.inline_destination(j + 2) {
                Some((url, title, after)) => {
                    self.pos = after;
                    self.emit_link(out, &label, &url, &title, image);
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
                    Some((url, title)) => {
                        self.pos = k + 1;
                        self.emit_link(out, &label, &url, &title, image);
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
                    Some((url, title)) => {
                        self.pos = j + 1;
                        self.emit_link(out, &label, &url, &title, image);
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
    /// link. Returns the destination, the title (empty when absent) and
    /// the index just past the ')'.
    fn inline_destination(&self, start: usize) -> Option<(String, String, usize)> {
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
        let (url, title) = split_destination(&raw);
        Some((url, title, k + 1))
    }

    /// Emit a footnote reference: a superscript link in HTML, a bracketed
    /// number in the text flavor (matching the appended footnote block).
    fn emit_note(&mut self, out: &mut String, number: usize) {
        match self.flavor {
            Flavor::Html => out.push_str(&format!(
                "<sup><a href=\"#fn-{number}\" id=\"fnref-{number}\">{number}</a></sup>"
            )),
            Flavor::Text => out.push_str(&format!("[{number}]")),
        }
    }

    /// Emit a link or image in the current flavor. Titles reach the
    /// HTML output as `title="…"`; the text flavor keeps the label and
    /// URL only.
    fn emit_link(&mut self, out: &mut String, label: &str, url: &str, title: &str, image: bool) {
        let label_html = inline_ctx(label, self.flavor, self.refs, self.notes);
        let title_attr = if title.is_empty() {
            String::new()
        } else {
            format!(" title=\"{}\"", escape_html(title))
        };
        match (self.flavor, image) {
            (Flavor::Html, false) => out.push_str(&format!(
                "<a href=\"{}\"{}>{}</a>",
                escape_html(url),
                title_attr,
                label_html
            )),
            (Flavor::Html, true) => out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\"{}>",
                escape_html(url),
                escape_html(label),
                title_attr
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
    fn normalize_rewrites_blocks_canonically() {
        let messy = "#  Title\n\n* one\n* two\n\n3. three\n7. seven\n\n\
                     |a|b|\n|:--|--:|\n|1|2|\n\n>   quoted\n\n---\n\n\
                     Text [ref].\n\n[ref]: https://example.com \"T\"\n";
        let canonical = "# Title\n\n- one\n- two\n\n1. three\n2. seven\n\n\
                         | a | b |\n| :--- | ---: |\n| 1 | 2 |\n\n> quoted\n\n---\n\n\
                         Text [ref].\n\n[ref]: https://example.com \"T\"\n";
        let once = normalize(messy).unwrap();
        assert_eq!(once, canonical);
        // Structure only: a second pass changes nothing.
        assert_eq!(normalize(&once).unwrap(), once);
    }

    #[test]
    fn normalize_styles_are_opt_in() {
        let input = "_soft_ and __loud__\n\n```\ncode\n```\n";
        // Default: asterisk emphasis, backtick fences.
        assert_eq!(
            normalize(input).unwrap(),
            "*soft* and **loud**\n\n```\ncode\n```\n"
        );
        let underscore = Style {
            emphasis: Emphasis::Underscore,
            fence: Fence::Backtick,
        };
        assert_eq!(
            normalize_with(input, underscore).unwrap(),
            "_soft_ and __loud__\n\n```\ncode\n```\n"
        );
        let tilde = Style {
            emphasis: Emphasis::Asterisk,
            fence: Fence::Tilde,
        };
        assert_eq!(
            normalize_with(input, tilde).unwrap(),
            "*soft* and **loud**\n\n~~~\ncode\n~~~\n"
        );
        // The underscore style converts asterisks, not underscores.
        assert_eq!(
            normalize_with("*soft* and **loud**\n", underscore).unwrap(),
            "_soft_ and __loud__\n"
        );
        // And it parses from the ABI's object.
        let parsed =
            Style::from_json("{\"emphasis\":\"underscore\",\"fence\":\"tilde\"}").expect("style");
        assert_eq!(parsed, underscore_with_tilde());
        assert_eq!(
            Style::from_json("{\"emphasis\":\"pdf\"}").unwrap_err().0,
            crate::ERR_INVALID_ARG
        );
    }

    fn underscore_with_tilde() -> Style {
        Style {
            emphasis: Emphasis::Underscore,
            fence: Fence::Tilde,
        }
    }

    /// One flag for the pair: presets, and a comma list that reads in
    /// either order.
    /// The second thing `tests/fuzz.rs` found, shrunk to four characters:
    /// converting `_>_` to `*>*` turned the stray underscores around it
    /// into an emphasis pair, so the next pass had something new to convert.
    #[test]
    fn emphasis_rewrites_never_create_a_new_pair() {
        let once = normalize("__<_>_\n").expect("normalize");
        assert_eq!(once, normalize(&once).expect("normalize twice"));
        assert_eq!(once, "__<*>*\n");
        // The canonical rewrite still happens where it is safe…
        assert_eq!(normalize("_soft_\n").unwrap(), "*soft*\n");
        assert_eq!(normalize("__loud__\n").unwrap(), "**loud**\n");
        // …and not where the marker would collide with what is beside it.
        for text in ["*_x_*", "_a*b_", "_ x _", "a_b_c"] {
            let once = normalize(text).expect("normalize");
            assert_eq!(once, normalize(&once).expect("normalize twice"), "{text:?}");
        }
    }

    /// The failure that `tests/fuzz.rs` found first (seed 1, round 5): a
    /// stray backtick run in a paragraph used to gain padding spaces on the
    /// first pass, and those spaces let the second pass read the run
    /// differently — normalize changed its own output.
    #[test]
    fn code_spans_normalize_to_a_fixed_point() {
        let input = "<table>&amp;-->{`</script>%|:--|(\u{1f600}b___<table><p>```:::![[}\
                     <table>| --- |\n<script><p>|||\u{e9}|===<tr></div>[^1]: note![%- [^1]'|:<script>";
        let once = normalize(input).expect("normalize");
        assert_eq!(once, normalize(&once).expect("normalize twice"));
        // Every other shape of backtick run is stable too.
        for text in [
            "```",
            "` ` `",
            "`` ` ``",
            "``x`` and `y`",
            "`a` ``b`` ```c```",
            "``a`b``",
            "` `",
        ] {
            let once = normalize(text).expect("normalize");
            assert_eq!(once, normalize(&once).expect("normalize twice"), "{text:?}");
        }
        // A span that needs no padding still loses its surplus backticks and
        // its padding, which is the whole point of the pass.
        assert_eq!(normalize("`` x ``\n").unwrap(), "`x`\n");
        assert_eq!(normalize("`  y  `\n").unwrap(), "`y`\n");
    }

    #[test]
    fn deep_block_nesting_stays_text_instead_of_crashing() {
        let quoted = format!("{}deep\n", "> ".repeat(2_000));
        let once = normalize(&quoted).expect("normalize");
        assert!(once.contains("deep"), "{once}");
        assert_eq!(normalize(&once).unwrap(), once);
        let listed = "> - ".repeat(1_000) + "deep\n";
        assert!(normalize(&listed).unwrap().contains("deep"));
    }

    #[test]
    fn style_specs_cover_presets_and_lists() {
        assert_eq!(Style::parse_spec("github").unwrap(), Style::default());
        assert_eq!(
            Style::parse_spec("underscore,tilde").unwrap(),
            Style {
                emphasis: Emphasis::Underscore,
                fence: Fence::Tilde,
            }
        );
        // Later tokens win, so a preset can be corrected after it.
        assert_eq!(
            Style::parse_spec("underscore tilde fence=backtick").unwrap(),
            Style {
                emphasis: Emphasis::Underscore,
                fence: Fence::Backtick,
            }
        );
        assert_eq!(
            Style::parse_spec("emphasis=underscore,backtick").unwrap(),
            Style {
                emphasis: Emphasis::Underscore,
                fence: Fence::Backtick,
            }
        );
        assert_eq!(Style::parse_spec("  ").unwrap(), Style::default());
        assert_eq!(
            Style::parse_spec("pdf").unwrap_err().0,
            crate::ERR_INVALID_ARG
        );
        // The ABI object accepts the same spec.
        assert_eq!(
            Style::from_json("{\"style\":\"underscore,tilde\"}").unwrap(),
            Style {
                emphasis: Emphasis::Underscore,
                fence: Fence::Tilde,
            }
        );
        assert!(Style::presets().contains(&"github"));
    }

    #[test]
    fn fences_grow_when_the_body_contains_them() {
        // A fence one longer than anything inside the body.
        assert_eq!(fence_for("plain code", Fence::Backtick), "```");
        assert_eq!(fence_for("a ``` fence", Fence::Backtick), "````");
        assert_eq!(fence_for("plain code", Fence::Tilde), "~~~");
        assert_eq!(fence_for("a ~~~ run", Fence::Tilde), "~~~~");
    }

    #[test]
    fn normalize_keeps_footnotes_at_the_end() {
        let input = "Claim[^a].\n\n[^a]: the note\n    continued here\n\n# After\n";
        let out = normalize(input).unwrap();
        assert_eq!(
            out,
            "Claim[^a].\n\n# After\n\n[^a]: the note continued here\n"
        );
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
    fn reference_links() {
        let md = "[docs]: https://example.com/docs \"Docs\"\n\
                  [short]: <https://example.com/s> 'Short'\n\n\
                  See [the docs][docs], [docs], [short][] and ![logo][docs].\n\n\
                  An [undefined] ref stays literal.\n";
        let out = html(md);
        assert!(
            out.contains("<a href=\"https://example.com/docs\" title=\"Docs\">the docs</a>"),
            "{out}"
        );
        assert!(
            out.contains("<a href=\"https://example.com/docs\" title=\"Docs\">docs</a>"),
            "{out}"
        );
        assert!(
            out.contains("<a href=\"https://example.com/s\" title=\"Short\">short</a>"),
            "{out}"
        );
        assert!(
            out.contains("<img src=\"https://example.com/docs\" alt=\"logo\" title=\"Docs\">"),
            "{out}"
        );
        assert!(out.contains("[undefined] ref stays literal"), "{out}");
        // Labels normalize like CommonMark: trim, collapse, case-fold.
        let out = html("[Big   Ref]: https://e.com\n\n[big ref]\n");
        assert!(
            out.contains("<a href=\"https://e.com\">big ref</a>"),
            "{out}"
        );
        // Definitions inside fenced code stay code.
        let out = html("[x]: https://e.com\n\n[x][]\n\n```\n[x]: https://not-a-def.com\n```\n");
        assert!(out.contains("<a href=\"https://e.com\">x</a>"), "{out}");
        assert!(out.contains("[x]: https://not-a-def.com"), "{out}");
        // The text flavor keeps destinations visible.
        let text = convert_markdown("[d]: https://e.com\n\nSee [d].\n", Flavor::Text).unwrap();
        assert_eq!(text, "See d (https://e.com).\n");
    }

    /// Continuation lines indented with non-ASCII whitespace used to be
    /// sliced at a raw byte column — a panic inside a UTF-8 character.
    #[test]
    fn unicode_indentation_is_safe() {
        let out = html("- item\n\u{3000}continuation\n");
        assert!(out.contains("continuation"), "{out}");
        let out = html("\u{a0}1. one\n\u{a0}\u{a0}still one\n");
        assert!(!out.is_empty(), "{out}");
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
    #[test]
    fn link_titles() {
        // Inline titles in every delimiter, plus images and escaping.
        let out = html(
            "[a](https://e.com \"Double\") [b](https://e.com 'Single') [c](https://e.com (Paren))\n",
        );
        assert!(
            out.contains("<a href=\"https://e.com\" title=\"Double\">a</a>"),
            "{out}"
        );
        assert!(
            out.contains("<a href=\"https://e.com\" title=\"Single\">b</a>"),
            "{out}"
        );
        assert!(
            out.contains("<a href=\"https://e.com\" title=\"Paren\">c</a>"),
            "{out}"
        );
        let out = html("![logo](logo.png \"The logo\")\n");
        assert!(
            out.contains("<img src=\"logo.png\" alt=\"logo\" title=\"The logo\">"),
            "{out}"
        );
        // A title is markup, so it is escaped like the rest.
        let out = html("[x](https://e.com \"a &amp; <b>\")\n");
        assert!(out.contains("title=\"a &amp;amp; &lt;b&gt;\""), "{out}");
        // Escaped delimiters inside the title survive.
        let out = html("[x](https://e.com \"say \\\"hi\\\"\")\n");
        assert!(out.contains("title=\"say &quot;hi&quot;\""), "{out}");
        // Angle-bracket destinations may carry spaces.
        let out = html("[x](<https://e.com/a b> \"Spaced\")\n");
        assert!(
            out.contains("<a href=\"https://e.com/a b\" title=\"Spaced\">x</a>"),
            "{out}"
        );
        // The text flavor keeps label and URL, not the title.
        let text = convert_markdown("[x](https://e.com \"T\")\n", Flavor::Text).unwrap();
        assert_eq!(text, "x (https://e.com)\n");
    }

    #[test]
    fn task_lists() {
        let md = "- [ ] call the plumber\n- [x] buy milk\n- [X] pay rent\n- plain item\n";
        let out = html(md);
        assert!(
            out.contains("<li><input type=\"checkbox\" disabled> call the plumber</li>"),
            "{out}"
        );
        assert!(
            out.contains("<li><input type=\"checkbox\" disabled checked> buy milk</li>"),
            "{out}"
        );
        assert!(
            out.contains("<li><input type=\"checkbox\" disabled checked> pay rent</li>"),
            "{out}"
        );
        assert!(out.contains("<li>plain item</li>"), "{out}");

        let text = convert_markdown(md, Flavor::Text).unwrap();
        assert_eq!(
            text,
            "- [ ] call the plumber\n- [x] buy milk\n- [x] pay rent\n- plain item\n"
        );

        // Nested and ordered task items still nest.
        let out = html("1. [x] first\n2. [ ] second\n");
        assert!(out.contains("<ol>"), "{out}");
        assert!(
            out.contains("<li><input type=\"checkbox\" disabled checked> first</li>"),
            "{out}"
        );
        // A bracket that is not a task marker stays text.
        let out = html("- [not a task] just brackets\n");
        assert!(
            out.contains("just brackets") && out.contains("[not a task]"),
            "{out}"
        );
    }
    #[test]
    fn footnotes() {
        let md = "Text with a note[^a] and another[^long].\n\n\
                  [^a]: First note.\n\
                  [^long]: A longer note\n    with a continuation line.\n";
        let out = html(md);
        assert!(
            out.contains("<sup><a href=\"#fn-1\" id=\"fnref-1\">1</a></sup>"),
            "{out}"
        );
        assert!(
            out.contains("<sup><a href=\"#fn-2\" id=\"fnref-2\">2</a></sup>"),
            "{out}"
        );
        assert!(out.contains("<section class=\"footnotes\">"), "{out}");
        assert!(out.contains("<li id=\"fn-1\"><p>First note."), "{out}");
        // Continuation lines join the note.
        assert!(out.contains("with a continuation line."), "{out}");
        assert!(out.contains("href=\"#fnref-2\""), "{out}");

        // Numbering follows the first reference, not the definition order.
        let out = html("B[^two] then A[^one]\n\n[^one]: One\n[^two]: Two\n");
        let first = out.find("#fn-1").expect("first note");
        assert!(out[first..].contains("Two"), "{out}");

        // Undefined notes and code fences stay literal.
        let out = html("Nothing here[^missing].\n\n```\nref[^a]\n```\n");
        assert!(out.contains("[^missing]"), "{out}");
        assert!(out.contains("[^a]"), "{out}");
        assert!(!out.contains("<section class=\"footnotes\">"), "{out}");

        // The text flavor numbers them and lists them at the end.
        let text = convert_markdown(md, Flavor::Text).unwrap();
        assert!(
            text.contains("Text with a note[1] and another[2]."),
            "{text}"
        );
        assert!(text.contains("Footnotes"), "{text}");
        assert!(text.contains("[1] First note."), "{text}");
    }

    #[test]
    fn markdown_table_to_csv() {
        let csv = markdown_to_csv("| Name | Qty |\n|---|---|\n| Milk | 2 |\n").unwrap();
        assert_eq!(csv, "Name,Qty\r\nMilk,2\r\n");
        // A heading above the table is fine.
        let csv = markdown_to_csv("# Inventory\n\n| A |\n|---|\n| **1** |\n").unwrap();
        assert_eq!(csv, "A\r\n1\r\n", "inline markup is flattened");
        // Prose, or no table at all, is an error rather than a guess.
        assert!(markdown_to_csv("Just prose.\n").is_err());
        assert!(markdown_to_csv("# Only a heading\n").is_err());
        assert!(markdown_to_csv("| A |\n|---|\n| 1 |\n\nMore prose.\n").is_err());
        // Two tables are ambiguous.
        assert!(markdown_to_csv("| A |\n|---|\n| 1 |\n\n| B |\n|---|\n| 2 |\n").is_err());
    }
}
