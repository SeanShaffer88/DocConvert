//! YAML front matter — the optional `---` block at the top of a
//! Markdown document (`title: …`, `tags: [a, b]`, `draft: true`).
//!
//! Scope is deliberate: the constructs front matter actually uses —
//! a flat mapping, nested mappings, simple `- item` lists, inline
//! `[a, b]` lists, and `|` / `>` block scalars. Anchors, aliases,
//! tags, and multi-document streams are out of scope and stay
//! literal strings. Scalars follow the same inference rule as the
//! CSV→JSON path: numbers and booleans become JSON values, but
//! leading-zero codes like `007` stay strings.

use crate::json::Json;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrontMatter {
    pub fields: Vec<(String, Json)>,
}

impl FrontMatter {
    /// The front matter as a JSON object (used by
    /// `dc_front_matter_json`).
    pub fn to_json(&self) -> Json {
        Json::Object(self.fields.clone())
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A two-column GFM table — how front matter is rendered when a
    /// conversion is told to keep it (`{"front_matter":"table"}`)
    /// instead of dropping it.
    pub fn to_markdown_table(&self) -> String {
        let mut out = String::from("| Key | Value |\n| --- | --- |");
        for (k, v) in &self.fields {
            out.push_str("\n| ");
            out.push_str(&table_cell(k));
            out.push_str(" | ");
            out.push_str(&table_cell(&scalar_text(v)));
            out.push_str(" |");
        }
        out
    }
}

fn table_cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

fn scalar_text(v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::Bool(b) => b.to_string(),
        Json::Num(n) => n.clone(),
        Json::Str(s) => s.clone(),
        other => other.to_compact(),
    }
}

/// Split a document into (front matter, body). The opening `---` must
/// be the very first line (a UTF-8 BOM is tolerated, leading blank
/// lines are not) and the block must close with `---` or `...`;
/// anything else leaves the input untouched. Malformed YAML is *not*
/// silently swallowed — the text then stays part of the document.
pub fn split(input: &str) -> (Option<FrontMatter>, &str) {
    let text = input.strip_prefix('\u{feff}').unwrap_or(input);
    let bom = input.len() - text.len();
    let mut lines = text.split_inclusive('\n');
    let first = match lines.next() {
        Some(line) => line,
        None => return (None, input),
    };
    if first.trim_end_matches(['\r', '\n']).trim_end() != "---" {
        return (None, input);
    }
    let mut yaml = String::new();
    let mut consumed = first.len();
    let mut closed = false;
    for line in lines {
        let content = line.trim_end_matches(['\r', '\n']);
        consumed += line.len();
        let fence = content.trim_end();
        if fence == "---" || fence == "..." {
            closed = true;
            break;
        }
        yaml.push_str(content);
        yaml.push('\n');
    }
    if !closed {
        return (None, input);
    }
    let body = &input[(bom + consumed).min(input.len())..];
    (
        Some(FrontMatter {
            fields: parse(&yaml),
        }),
        body,
    )
}

/// Parse the YAML subset described in the module docs. Errors are
/// impossible by construction: lines that do not fit the subset are
/// skipped rather than failing the whole document.
pub fn parse(yaml: &str) -> Vec<(String, Json)> {
    let lines: Vec<Line> = yaml.lines().filter_map(Line::new).collect();
    let mut idx = 0usize;
    parse_mapping(&lines, &mut idx, 0)
}

struct Line<'a> {
    indent: usize,
    text: &'a str,
}

impl<'a> Line<'a> {
    fn new(raw: &'a str) -> Option<Line<'a>> {
        let trimmed = raw.trim_end();
        if trimmed.is_empty() {
            return None;
        }
        let content = trimmed.trim_start();
        if content.starts_with('#') {
            return None;
        }
        Some(Line {
            indent: trimmed.len() - content.len(),
            text: content,
        })
    }
}

fn is_list_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn parse_mapping(lines: &[Line], idx: &mut usize, indent: usize) -> Vec<(String, Json)> {
    let mut out: Vec<(String, Json)> = Vec::new();
    while *idx < lines.len() {
        let line = &lines[*idx];
        if line.indent < indent || is_list_item(line.text) {
            break;
        }
        if line.indent > indent {
            *idx += 1; // stray over-indented line
            continue;
        }
        let (key, rest) = match split_key(line.text) {
            Some(kv) => kv,
            None => {
                *idx += 1;
                continue;
            }
        };
        *idx += 1;
        if rest.is_empty() {
            if *idx < lines.len() && lines[*idx].indent > indent {
                let child_indent = lines[*idx].indent;
                if is_list_item(lines[*idx].text) {
                    out.push((key, Json::Array(parse_list(lines, idx, child_indent))));
                } else {
                    out.push((key, Json::Object(parse_mapping(lines, idx, child_indent))));
                }
            } else {
                out.push((key, Json::Null));
            }
        } else if is_block_scalar(rest) {
            out.push((key, Json::Str(block_scalar(lines, idx, indent, rest))));
        } else {
            out.push((key, scalar(rest)));
        }
    }
    out
}

fn parse_list(lines: &[Line], idx: &mut usize, indent: usize) -> Vec<Json> {
    let mut out = Vec::new();
    while *idx < lines.len() {
        let line = &lines[*idx];
        if line.indent < indent || !is_list_item(line.text) {
            break;
        }
        if line.indent > indent {
            *idx += 1;
            continue;
        }
        let rest = line.text[1..].trim();
        *idx += 1;
        out.push(if rest.is_empty() {
            Json::Null
        } else {
            scalar(rest)
        });
    }
    out
}

fn is_block_scalar(rest: &str) -> bool {
    matches!(rest, "|" | ">" | "|-" | ">-" | "|+" | ">+")
}

fn block_scalar(lines: &[Line], idx: &mut usize, indent: usize, style: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    while *idx < lines.len() && lines[*idx].indent > indent {
        parts.push(lines[*idx].text);
        *idx += 1;
    }
    let joined = if style.starts_with('>') {
        parts.join(" ")
    } else {
        parts.join("\n")
    };
    if style.ends_with('+') {
        format!("{joined}\n")
    } else {
        joined
    }
}

fn split_key(text: &str) -> Option<(String, &str)> {
    let idx = text.find(':')?;
    let key = text[..idx].trim();
    if key.is_empty() {
        return None;
    }
    let rest = &text[idx + 1..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None; // `http://…` is a value, not a key
    }
    Some((unquote(key), rest.trim()))
}

fn unquote(text: &str) -> String {
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        return text[1..text.len() - 1].to_string();
    }
    text.to_string()
}

fn scalar(raw: &str) -> Json {
    let text = raw.trim();
    if text.is_empty() {
        return Json::Null;
    }
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        return Json::Str(unescape_double(&text[1..text.len() - 1]));
    }
    if text.len() >= 2 && text.starts_with('\'') && text.ends_with('\'') {
        return Json::Str(text[1..text.len() - 1].replace("''", "'"));
    }
    match text {
        "true" | "True" | "TRUE" => return Json::Bool(true),
        "false" | "False" | "FALSE" => return Json::Bool(false),
        "null" | "Null" | "NULL" | "~" => return Json::Null,
        _ => {}
    }
    if text.len() >= 2 && text.starts_with('[') && text.ends_with(']') {
        let inner = &text[1..text.len() - 1];
        if inner.trim().is_empty() {
            return Json::Array(Vec::new());
        }
        return Json::Array(inner.split(',').map(scalar).collect());
    }
    match number_text(text) {
        Some(n) => Json::Num(n),
        None => Json::Str(text.to_string()),
    }
}

/// Numbers keep their source text (no precision loss), and
/// leading-zero values stay strings — `version: 007` is a code.
fn number_text(text: &str) -> Option<String> {
    if text.len() > 1 && text.starts_with('0') && !text.starts_with("0.") {
        return None;
    }
    let numeric = text
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'));
    if numeric && text.parse::<f64>().is_ok() {
        Some(text.to_string())
    } else {
        None
    }
}

fn unescape_double(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(input: &str) -> (Option<FrontMatter>, String) {
        let (fm, body) = split(input);
        (fm, body.to_string())
    }

    #[test]
    fn split_and_parse() {
        let src = "---\ntitle: \"Release 2.0\"\ndraft: false\ntags: [docs, \"how to\"]\ncode: 007\ncount: 12\nnested:\n  author: Ada\n  site: https://example.com\nlist:\n  - one\n  - 2\nsummary: >\n  two lines\n  joined\n---\n# Body\n";
        let (fm, body) = fields(src);
        let fm = fm.expect("front matter");
        assert_eq!(body, "# Body\n");
        assert_eq!(fm.get("title"), Some(&Json::Str("Release 2.0".into())));
        assert_eq!(fm.get("draft"), Some(&Json::Bool(false)));
        assert_eq!(
            fm.get("tags"),
            Some(&Json::Array(vec![
                Json::Str("docs".into()),
                Json::Str("how to".into())
            ]))
        );
        assert_eq!(fm.get("code"), Some(&Json::Str("007".into())));
        assert_eq!(fm.get("count"), Some(&Json::Num("12".into())));
        assert_eq!(
            fm.get("nested"),
            Some(&Json::Object(vec![
                ("author".to_string(), Json::Str("Ada".into())),
                ("site".to_string(), Json::Str("https://example.com".into())),
            ]))
        );
        assert_eq!(
            fm.get("list"),
            Some(&Json::Array(vec![
                Json::Str("one".into()),
                Json::Num("2".into())
            ]))
        );
        assert_eq!(
            fm.get("summary"),
            Some(&Json::Str("two lines joined".into()))
        );
    }

    #[test]
    fn absent_or_unterminated() {
        let (fm, body) = fields("# Just a document\n");
        assert!(fm.is_none());
        assert_eq!(body, "# Just a document\n");
        // Unterminated fence: not front matter, text preserved.
        let (fm, body) = fields("---\ntitle: x\n");
        assert!(fm.is_none());
        assert_eq!(body, "---\ntitle: x\n");
        // An indented `---` is not an opening fence.
        let (fm, _) = fields("    ---\ntitle: x\n---\n");
        assert!(fm.is_none());
    }

    #[test]
    fn bom_and_crlf() {
        let (fm, body) = split("\u{feff}---\r\ntitle: Hi\r\n---\r\nBody\r\n");
        let fm = fm.expect("front matter");
        assert_eq!(fm.get("title"), Some(&Json::Str("Hi".into())));
        assert_eq!(body, "Body\r\n");
    }

    #[test]
    fn table_rendering() {
        let (fm, _) = split("---\ntitle: Hi\npipe: a|b\n---\n");
        let table = fm.unwrap().to_markdown_table();
        assert!(
            table.starts_with("| Key | Value |\n| --- | --- |"),
            "{table}"
        );
        assert!(table.contains("| title | Hi |"), "{table}");
        assert!(table.contains("| pipe | a\\|b |"), "{table}");
    }
}
