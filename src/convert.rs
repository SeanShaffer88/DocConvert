//! Format registry + the conversion matrix.

use crate::csv::{parse_csv, parse_tsv, write_csv, write_tsv};
use crate::frontmatter;
use crate::html::{html_rows, html_to_csv, html_to_markdown, html_to_text};
use crate::json::{parse_json, Json};
use crate::markdown::{convert_markdown, convert_markdown_with, markdown_rows, Flavor};
use crate::{corrupt, CResult, ERR_INVALID_ARG, ERR_NOT_SUPPORTED};

pub const FORMATS: &[&str] = &[
    "markdown", "html", "csv", "tsv", "json", "text", "rst", "asciidoc", "opml",
];

/// Every `{from, to}` pair this version supports. `dc_matrix_json()`
/// is generated from this table and `matrix_is_exact` asserts that
/// `convert` agrees with it, so the advertised matrix and the dispatch
/// can never drift apart.
pub const MATRIX: &[(&str, &str)] = &[
    ("markdown", "html"),
    ("markdown", "text"),
    ("markdown", "csv"),
    ("html", "markdown"),
    ("html", "text"),
    ("html", "csv"),
    ("html", "json"),
    ("text", "html"),
    ("text", "markdown"),
    ("csv", "json"),
    ("json", "csv"),
    ("csv", "markdown"),
    ("csv", "html"),
    ("csv", "text"),
    ("csv", "tsv"),
    ("tsv", "csv"),
    ("tsv", "json"),
    ("tsv", "markdown"),
    ("tsv", "html"),
    ("tsv", "text"),
    ("json", "tsv"),
    ("html", "tsv"),
    ("markdown", "tsv"),
    ("json", "text"),
    ("json", "html"),
    ("json", "markdown"),
    // Documents with their own syntax arrive as a source: the three
    // readers land as Markdown and the Markdown renderers do the rest.
    ("rst", "markdown"),
    ("rst", "html"),
    ("rst", "text"),
    ("asciidoc", "markdown"),
    ("asciidoc", "html"),
    ("asciidoc", "text"),
    ("opml", "markdown"),
    ("opml", "html"),
    ("opml", "text"),
    ("markdown", "opml"),
    ("html", "opml"),
];

/// The `dc_formats_json()` payload — shared by the ABI and the CLI so
/// both describe the build identically.
pub fn formats_json() -> String {
    let items: Vec<String> = FORMATS.iter().map(|f| format!("\"{f}\"")).collect();
    format!("[{}]", items.join(","))
}

/// The `dc_matrix_json()` payload, generated from [`MATRIX`].
pub fn matrix_json() -> String {
    let items: Vec<String> = MATRIX
        .iter()
        .map(|(from, to)| format!("{{\"from\":\"{from}\",\"to\":\"{to}\"}}"))
        .collect();
    format!("[{}]", items.join(","))
}

/// The `error_codes` payload (`{"0":"ok",...}`) shared by the CLI and
/// the MCP server.
pub fn error_codes_json() -> String {
    let items: Vec<String> = crate::ERROR_CODES
        .iter()
        .map(|(code, meaning)| format!("\"{code}\":\"{meaning}\""))
        .collect();
    format!("{{{}}}", items.join(","))
}

/// What md→html and md→text do with a YAML front matter block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrontMatterMode {
    /// Drop it from the rendered document (the default). The metadata
    /// is still reachable with `dc_front_matter_json`.
    #[default]
    Strip,
    /// Render it as a two-column table so nothing is lost.
    Table,
}

impl FrontMatterMode {
    /// Parse the spelling used by the ABI options object and both front
    /// ends ("strip"/"none" or "table"/"keep").
    pub fn parse(text: &str) -> CResult<FrontMatterMode> {
        match text.trim().to_lowercase().as_str() {
            "strip" | "none" => Ok(FrontMatterMode::Strip),
            "table" | "keep" => Ok(FrontMatterMode::Table),
            other => Err((
                ERR_INVALID_ARG,
                format!("unknown front_matter mode '{other}' (expected 'strip' or 'table')"),
            )),
        }
    }
}

/// Conversion options; `Options::default()` is the documented
/// behaviour of every pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub front_matter: FrontMatterMode,
    /// Give Markdown→HTML headings GitHub-style `id="slug"` anchors, so
    /// a table of contents can link to them. Off by default, because a
    /// bare `<h2>` is the CommonMark output.
    pub heading_ids: bool,
}

impl Options {
    /// Parse the ABI's options object, e.g. `{"front_matter":"table"}`.
    /// Unknown keys and values are INVALID_ARG — options are never
    /// ignored silently.
    pub fn from_json(text: &str) -> CResult<Options> {
        let mut options = Options::default();
        if text.trim().is_empty() {
            return Ok(options);
        }
        let entries = match parse_json(text)? {
            Json::Object(entries) => entries,
            _ => return Err((ERR_INVALID_ARG, "options must be a JSON object".to_string())),
        };
        for (key, value) in entries {
            match key.as_str() {
                "front_matter" => {
                    let mode = match value {
                        Json::Str(mode) => mode,
                        _ => {
                            return Err((
                                ERR_INVALID_ARG,
                                "options.front_matter must be a string".to_string(),
                            ))
                        }
                    };
                    options.front_matter = FrontMatterMode::parse(&mode)?;
                }
                "heading_ids" => {
                    options.heading_ids = match value {
                        Json::Bool(flag) => flag,
                        _ => {
                            return Err((
                                ERR_INVALID_ARG,
                                "options.heading_ids must be true or false".to_string(),
                            ))
                        }
                    };
                }
                other => {
                    return Err((
                        ERR_INVALID_ARG,
                        format!("unknown conversion option '{other}'"),
                    ))
                }
            }
        }
        Ok(options)
    }
}

pub fn normalize_format(name: &str) -> CResult<String> {
    let f = match name.trim().to_lowercase().as_str() {
        "markdown" | "md" | "mkd" => "markdown",
        "html" | "htm" | "xhtml" => "html",
        "csv" => "csv",
        "tsv" | "tab" => "tsv",
        "json" => "json",
        "text" | "txt" | "plain" => "text",
        "rst" | "rest" | "restructuredtext" => "rst",
        "asciidoc" | "adoc" | "ascii" => "asciidoc",
        "opml" => "opml",
        other => {
            return Err((
                ERR_INVALID_ARG,
                format!(
                    "unknown format '{other}' (known: markdown, html, csv, tsv, json, text, \
                     rst, asciidoc, opml)"
                ),
            ))
        }
    };
    Ok(f.to_string())
}

/// Detect a format from a file name's extension.
pub fn format_for_path(path: &str) -> Option<String> {
    let ext = path.rsplit('.').next()?;
    if ext == path {
        return None; // no dot at all
    }
    normalize_format(ext).ok()
}

pub fn convert(input: &str, from: &str, to: &str) -> CResult<String> {
    convert_with_options(input, from, to, &Options::default())
}

pub fn convert_with_options(
    input: &str,
    from: &str,
    to: &str,
    options: &Options,
) -> CResult<String> {
    let from = normalize_format(from)?;
    let to = normalize_format(to)?;
    if from == to {
        // Validate parseable inputs even for identity conversions.
        return match from.as_str() {
            "json" => Ok(parse_json(input)?.to_pretty()),
            "csv" => Ok(write_csv(&parse_csv(input)?)),
            "tsv" => Ok(write_tsv(&parse_tsv(input)?)),
            _ => Ok(input.to_string()),
        };
    }
    match (from.as_str(), to.as_str()) {
        ("markdown", "html") => convert_markdown_with(
            &markdown_body(input, options),
            Flavor::Html,
            options.heading_ids,
        ),
        ("markdown", "text") => convert_markdown(&markdown_body(input, options), Flavor::Text),
        ("html", "markdown") => html_to_markdown(input),
        ("html", "text") => html_to_text(input),
        ("html", "csv") => html_to_csv(input),
        ("html", "json") => Ok(rows_to_json(&html_rows(input)?)),
        ("html", "tsv") => Ok(write_tsv(&html_rows(input)?)),
        ("markdown", "csv") => Ok(write_csv(&markdown_rows(&markdown_body(input, options))?)),
        ("markdown", "tsv") => Ok(write_tsv(&markdown_rows(&markdown_body(input, options))?)),
        ("text", "html") => Ok(text_to_html(input)),
        ("text", "markdown") => Ok(input.trim_end().to_string() + "\n"),
        ("csv", "json") => Ok(rows_to_json(&parse_csv(input)?)),
        ("tsv", "json") => Ok(rows_to_json(&parse_tsv(input)?)),
        ("json", "csv") => Ok(write_csv(&json_rows(input)?)),
        ("json", "tsv") => Ok(write_tsv(&json_rows(input)?)),
        ("csv", "markdown") => Ok(rows_to_markdown_table(&parse_csv(input)?)),
        ("tsv", "markdown") => Ok(rows_to_markdown_table(&parse_tsv(input)?)),
        ("csv", "html") => Ok(rows_to_html_table(&parse_csv(input)?)),
        ("tsv", "html") => Ok(rows_to_html_table(&parse_tsv(input)?)),
        ("csv", "text") => Ok(write_csv(&parse_csv(input)?)),
        ("tsv", "text") => Ok(write_tsv(&parse_tsv(input)?)),
        ("csv", "tsv") => Ok(write_tsv(&parse_csv(input)?)),
        ("tsv", "csv") => Ok(write_csv(&parse_tsv(input)?)),
        ("json", "text") => Ok(parse_json(input)?.to_pretty()),
        ("json", "html") => Ok(format!(
            "<pre>{}</pre>",
            crate::escape_html(&parse_json(input)?.to_pretty())
        )),
        ("json", "markdown") => Ok(format!(
            "```json\n{}\n```\n",
            parse_json(input)?.to_pretty()
        )),
        // The readers: rst and AsciiDoc become Markdown, then the Markdown
        // renderers finish the job, so every target agrees with the others.
        ("rst", "markdown") => crate::rst::to_markdown(input),
        ("rst", "html") => convert_markdown_with(
            &crate::rst::to_markdown(input)?,
            Flavor::Html,
            options.heading_ids,
        ),
        ("rst", "text") => convert_markdown(&crate::rst::to_markdown(input)?, Flavor::Text),
        ("asciidoc", "markdown") => crate::asciidoc::to_markdown(input),
        ("asciidoc", "html") => convert_markdown_with(
            &crate::asciidoc::to_markdown(input)?,
            Flavor::Html,
            options.heading_ids,
        ),
        ("asciidoc", "text") => {
            convert_markdown(&crate::asciidoc::to_markdown(input)?, Flavor::Text)
        }
        ("opml", "markdown") => crate::opml::to_markdown(input),
        ("opml", "html") => convert_markdown_with(
            &crate::opml::to_markdown(input)?,
            Flavor::Html,
            options.heading_ids,
        ),
        ("opml", "text") => convert_markdown(&crate::opml::to_markdown(input)?, Flavor::Text),
        // Outlines out: a document's headings are the outline.
        ("markdown", "opml") => Ok(crate::opml::from_headings(
            &crate::markdown::headings(&markdown_body(input, options))?,
            None,
        )),
        ("html", "opml") => Ok(crate::opml::from_headings(&heading_pairs(input)?, None)),
        ("markdown", "json") | ("text", "csv") | ("text", "json") => Err((
            ERR_NOT_SUPPORTED,
            format!("conversion {from} → {to} is not supported"),
        )),
        _ => Err((
            ERR_NOT_SUPPORTED,
            format!("conversion {from} → {to} is not supported"),
        )),
    }
}

/// The identity conversions whose output is the input byte for byte —
/// the ones a streaming copy can serve without parsing anything.
const BYTE_FAITHFUL: &[&str] = &["markdown", "html", "text", "rst", "asciidoc", "opml"];

/// `dc_convert_file_streaming`: convert a file without holding it twice
/// when the pair is an identity conversion (those are byte-faithful), and
/// fall back to the whole-buffer path for everything else — those
/// conversions need the whole document to be correct. Returns the bytes
/// written.
pub fn convert_file_streaming(
    in_path: &str,
    out_path: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> CResult<u64> {
    let from = match from {
        Some(name) => normalize_format(name)?,
        None => format_for_path(in_path).ok_or((
            ERR_INVALID_ARG,
            format!("cannot detect the input format from '{in_path}' — pass it explicitly"),
        ))?,
    };
    let to = match to {
        Some(name) => normalize_format(name)?,
        None => format_for_path(out_path).ok_or((
            ERR_INVALID_ARG,
            format!("cannot detect the output format from '{out_path}' — pass it explicitly"),
        ))?,
    };
    if from == to && BYTE_FAITHFUL.contains(&from.as_str()) {
        return crate::platform::copy_file(in_path, out_path);
    }
    let bytes = crate::platform::read_file(in_path)?;
    let text = String::from_utf8(bytes).map_err(|_| {
        (
            crate::ERR_CORRUPT,
            format!("'{in_path}' is not valid UTF-8 text"),
        )
    })?;
    let converted = convert_with_options(&text, &from, &to, &Options::default())?;
    crate::platform::write_file(out_path, converted.as_bytes())?;
    Ok(converted.len() as u64)
}

/// Strip the YAML front matter, or render it as a table when the
/// caller asked to keep it.
fn markdown_body(input: &str, options: &Options) -> String {
    let (front, body) = frontmatter::split(input);
    match (front, options.front_matter) {
        (Some(front), FrontMatterMode::Table) => {
            format!("{}\n\n{}", front.to_markdown_table(), body)
        }
        _ => body.to_string(),
    }
}

/// A page's headings as (level, text) — the outline in `html → opml`.
fn heading_pairs(input: &str) -> CResult<Vec<(usize, String)>> {
    Ok(crate::html::headings(input)?
        .into_iter()
        .map(|(level, text, _id)| (level, text))
        .collect())
}

fn text_to_html(input: &str) -> String {
    let mut out = String::new();
    for para in input.split("\n\n") {
        let p = para.trim();
        if p.is_empty() {
            continue;
        }
        out.push_str("<p>");
        let lines: Vec<String> = p.lines().map(crate::escape_html).collect();
        out.push_str(&lines.join("<br>\n"));
        out.push_str("</p>\n");
    }
    out
}

/// A table as an array of objects: the first row is the header, cells
/// become JSON scalars (numbers, booleans, nulls; leading-zero codes
/// stay strings).
fn rows_to_json(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "[]".to_string();
    }
    let headers = &rows[0];
    let mut objects = Vec::new();
    for row in &rows[1..] {
        let mut obj = Vec::new();
        for (j, h) in headers.iter().enumerate() {
            let cell = row.get(j).cloned().unwrap_or_default();
            obj.push((h.clone(), scalar_of(&cell)));
        }
        objects.push(Json::Object(obj));
    }
    Json::Array(objects).to_pretty()
}

/// Numeric-looking cells become JSON numbers; "true"/"false" become
/// booleans; empty becomes null; everything else stays a string.
fn scalar_of(cell: &str) -> Json {
    if cell.is_empty() {
        return Json::Null;
    }
    if cell == "true" {
        return Json::Bool(true);
    }
    if cell == "false" {
        return Json::Bool(false);
    }
    let numeric_chars = cell
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'));
    // Leading-zero values like "007" are codes, not quantities:
    // keep them strings (a bare "0" and "0.5" are numbers).
    let leading_zero_code = cell.len() > 1
        && cell.starts_with('0')
        && !cell.starts_with("0.")
        && !cell.starts_with("0e")
        && !cell.starts_with("0E");
    if numeric_chars && !leading_zero_code && cell.parse::<f64>().is_ok() {
        return Json::Num(cell.to_string());
    }
    Json::Str(cell.to_string())
}

/// JSON (an array of objects, or a single object) as table rows with a
/// union-of-keys header.
fn json_rows(input: &str) -> CResult<Vec<Vec<String>>> {
    let value = parse_json(input)?;
    let items = match &value {
        Json::Array(items) => items.clone(),
        Json::Object(_) => vec![value.clone()],
        _ => {
            return Err(corrupt(
                "json→csv needs an array of objects (or one object)",
            ))
        }
    };
    if items.is_empty() {
        return Ok(Vec::new());
    }
    // Union of keys in first-seen order.
    let mut keys: Vec<String> = Vec::new();
    for item in &items {
        if let Json::Object(entries) = item {
            for (k, _) in entries {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
        } else {
            return Err(corrupt(
                "json→csv needs every array element to be an object",
            ));
        }
    }
    let mut rows = vec![keys.clone()];
    for item in &items {
        if let Json::Object(entries) = item {
            let row = keys
                .iter()
                .map(|k| {
                    entries
                        .iter()
                        .find(|(ek, _)| ek == k)
                        .map(|(_, v)| v.as_csv_scalar())
                        .unwrap_or_default()
                })
                .collect();
            rows.push(row);
        }
    }
    Ok(rows)
}

fn rows_to_markdown_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<String> = row.iter().map(|c| c.replace('|', "\\|")).collect();
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
        if i == 0 {
            out.push('|');
            for _ in &cells {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out
}

fn rows_to_html_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "<table>\n</table>".to_string();
    }
    let mut out = String::from("<table>\n<thead>\n<tr>");
    for cell in &rows[0] {
        out.push_str(&format!("<th>{}</th>", crate::escape_html(cell)));
    }
    out.push_str("</tr>\n</thead>\n<tbody>");
    for row in &rows[1..] {
        out.push_str("\n<tr>");
        for cell in row {
            out.push_str(&format!("<td>{}</td>", crate::escape_html(cell)));
        }
        out.push_str("</tr>");
    }
    out.push_str("\n</tbody>\n</table>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_highlights() {
        let json = convert("name,age,admin\nAda,36,true\nBob,,false\n", "csv", "json").unwrap();
        assert!(json.contains("\"age\": 36"), "{json}");
        assert!(json.contains("\"age\": null"), "{json}");
        assert!(json.contains("\"admin\": false"), "{json}");
        let csv = convert(r#"[{"a":1,"b":"x"},{"a":2,"c":true}]"#, "json", "csv").unwrap();
        assert!(csv.starts_with("a,b,c\r\n"), "{csv}");
        assert!(csv.contains("1,x,\r\n"), "{csv}");
        let table = convert("A,B\n1,2\n", "csv", "markdown").unwrap();
        assert!(
            table.contains("| A | B |") && table.contains("| --- | --- |"),
            "{table}"
        );
        let roundtrip = convert(
            &convert("# T\n\nHello **there**.\n", "md", "html").unwrap(),
            "html",
            "markdown",
        )
        .unwrap();
        assert!(roundtrip.contains("# T"), "{roundtrip}");
        assert!(roundtrip.contains("Hello **there**."), "{roundtrip}");
        // markdown→csv wants a table: prose is a parse-style failure.
        assert_eq!(
            convert("x", "markdown", "csv").unwrap_err().0,
            crate::ERR_CORRUPT
        );
        assert_eq!(format_for_path("notes.MD").as_deref(), Some("markdown"));
        assert_eq!(format_for_path("noext"), None);
        // Leading-zero codes stay strings.
        let j = convert("code\n007\n", "csv", "json").unwrap();
        assert!(j.contains("\"007\""), "{j}");
    }

    fn sample(name: &str) -> &'static str {
        match name {
            // A single table under a heading: every listed pair can
            // handle this shape, including markdown→csv.
            "markdown" => "# Title\n\n| A | B |\n|---|---|\n| 1 | 2 |\n",
            "html" => {
                "<h1>Title</h1><p>Hello <strong>there</strong></p>\
                       <table><tr><th>A</th></tr><tr><td>1</td></tr></table>"
            }
            "csv" => "name,qty\nMilk,2\n",
            "tsv" => "name\tqty\nMilk\t2\n",
            "json" => "{\"a\": [1, 2], \"b\": null}",
            "rst" => "Title\n=====\n\nPart\n----\n\n* one\n* two\n",
            "asciidoc" => "= Title\n\n== Part\n\n* one\n* two\n",
            "opml" => {
                "<?xml version=\"1.0\"?><opml version=\"2.0\"><body>\
                 <outline text=\"One\"><outline text=\"Two\"/></outline></body></opml>"
            }
            _ => "plain text\n\nsecond paragraph\n",
        }
    }

    /// The advertised matrix and the dispatch must agree exactly:
    /// every listed pair converts, every unlisted pair is
    /// NOT_SUPPORTED (never a silent guess).
    #[test]
    fn matrix_is_exact() {
        for &(from, to) in MATRIX {
            let out = convert(sample(from), from, to)
                .unwrap_or_else(|e| panic!("{from}→{to} is listed but failed: {e:?}"));
            assert!(!out.is_empty(), "{from}→{to} produced nothing");
        }
        for &from in FORMATS {
            for &to in FORMATS {
                if from == to {
                    continue; // identity conversions are always available
                }
                let listed = MATRIX.contains(&(from, to));
                let result = convert(sample(from), from, to);
                assert_eq!(
                    result.is_ok(),
                    listed,
                    "{from}→{to}: listed={listed}, got {result:?}"
                );
                if let Err((code, _)) = result {
                    assert_eq!(code, ERR_NOT_SUPPORTED, "{from}→{to}");
                }
            }
        }
    }

    #[test]
    fn html_to_csv_pair() {
        let csv = convert(
            "<table><tr><th>Name</th><th>Qty</th></tr><tr><td>Milk</td><td>2</td></tr></table>",
            "html",
            "csv",
        )
        .unwrap();
        assert_eq!(csv, "Name,Qty\r\nMilk,2\r\n");
        // A page without a table is a parse-style failure, not empty output.
        assert_eq!(
            convert("<p>nothing</p>", "html", "csv").unwrap_err().0,
            crate::ERR_CORRUPT
        );
    }

    /// The readers and the outline: a document syntax becomes Markdown,
    /// then the Markdown renderers finish; headings become OPML.
    #[test]
    fn document_syntaxes_and_outlines() {
        let rst = "Guide\n=====\n\n* one\n* two\n";
        let html = convert(rst, "rst", "html").unwrap();
        assert!(html.contains("<h1>Guide</h1>"), "{html}");
        assert!(html.contains("<li>one</li>"), "{html}");
        let md = convert(rst, "rst", "markdown").unwrap();
        assert_eq!(md, "# Guide\n\n- one\n- two\n");
        let text = convert(rst, "rst", "text").unwrap();
        assert!(text.contains("Guide"), "{text}");

        let adoc = "= Guide\n\n== Install\n\n* one\n";
        let html = convert(adoc, "adoc", "html").unwrap();
        assert!(html.contains("<h2>Install</h2>"), "{html}");

        // Headings are the outline.
        let opml = convert("# Guide\n\n## Install\n\n### Windows\n", "markdown", "opml").unwrap();
        assert!(opml.contains("<opml version=\"2.0\">"), "{opml}");
        assert!(opml.contains("<outline text=\"Guide\">"), "{opml}");
        assert!(opml.contains("<outline text=\"Windows\"/>"), "{opml}");

        let back = convert(&opml, "opml", "markdown").unwrap();
        assert_eq!(back, "- Guide\n  - Install\n    - Windows\n");
        let html = convert(&opml, "opml", "html").unwrap();
        assert!(html.contains("<ul>") && html.contains("Guide"), "{html}");

        // A page's headings are an outline too.
        let from_html = convert("<h1>A</h1><h2>B</h2>", "html", "opml").unwrap();
        assert!(from_html.contains("<outline text=\"B\"/>"), "{from_html}");

        // Aliases and extensions line up.
        assert_eq!(normalize_format("rest").unwrap(), "rst");
        assert_eq!(normalize_format("adoc").unwrap(), "asciidoc");
        assert_eq!(format_for_path("doc.rst").as_deref(), Some("rst"));
        assert_eq!(format_for_path("notes.adoc").as_deref(), Some("asciidoc"));
        assert_eq!(format_for_path("feeds.opml").as_deref(), Some("opml"));
    }

    /// TSV is first-class: wherever CSV is a source or target, TSV is
    /// too, with the same rows underneath.
    #[test]
    fn tsv_mirrors_csv() {
        let csv = "name,note\r\nMilk,\"2, l\"\r\n";
        let tsv = convert(csv, "csv", "tsv").unwrap();
        assert_eq!(tsv, "name\tnote\r\nMilk\t2, l\r\n");
        assert_eq!(convert(&tsv, "tsv", "csv").unwrap(), csv);

        let json = convert("a\tb\n1\t2\n", "tsv", "json").unwrap();
        assert!(json.contains("\"a\": 1"), "{json}");
        assert!(json.contains("\"b\": 2"), "{json}");
        let back = convert("[{\"a\":1,\"b\":2}]", "json", "tsv").unwrap();
        assert_eq!(back, "a\tb\r\n1\t2\r\n");

        let table = convert("| A | B |\n|---|---|\n| 1 | 2 |\n", "markdown", "tsv").unwrap();
        assert_eq!(table, "A\tB\r\n1\t2\r\n");
        let html = convert(
            "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
            "html",
            "tsv",
        )
        .unwrap();
        assert_eq!(html, "A\tB\r\n1\t2\r\n");
        assert!(convert("a\tb\n", "tsv", "html")
            .unwrap()
            .contains("<th>a</th>"));
        assert_eq!(
            convert("a\tb\n", "tsv", "markdown").unwrap(),
            "| a | b |\n| --- | --- |\n"
        );
        // A tab inside a cell is quoted, so the row count survives.
        let quoted = convert("h1\th2\r\nv\t\"a\tb\"\r\n", "tsv", "tsv").unwrap();
        assert!(quoted.contains("\"a\tb\""), "{quoted}");
        // TSV files are recognized by extension, and the alias works.
        assert_eq!(format_for_path("data.tsv").as_deref(), Some("tsv"));
        assert_eq!(normalize_format("tab").unwrap(), "tsv");
    }

    #[test]
    fn front_matter_modes() {
        let doc = "---\ntitle: Notes\ntags: [a, b]\n---\n# Body\n";
        // Default: stripped from the rendered document.
        let html = convert(doc, "markdown", "html").unwrap();
        assert_eq!(html, "<h1>Body</h1>");
        // Opted in: rendered as a table, nothing lost.
        let options = Options::from_json("{\"front_matter\":\"table\"}").unwrap();
        let html = convert_with_options(doc, "markdown", "html", &options).unwrap();
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("<td>title</td>"), "{html}");
        assert!(html.contains("<td>Notes</td>"), "{html}");
        assert!(html.contains("<h1>Body</h1>"), "{html}");
        // Text flavor too.
        let options = Options::from_json("{\"front_matter\":\"table\"}").unwrap();
        let text = convert_with_options(doc, "md", "txt", &options).unwrap();
        assert!(text.contains("title | Notes"), "{text}");
        assert!(text.contains("Body"), "{text}");
    }

    #[test]
    fn options_are_strict() {
        assert_eq!(
            Options::from_json("{\"front_matter\":\"nope\"}")
                .unwrap_err()
                .0,
            ERR_INVALID_ARG
        );
        assert_eq!(
            Options::from_json("{\"unknown\":true}").unwrap_err().0,
            ERR_INVALID_ARG
        );
        assert_eq!(Options::from_json("[]").unwrap_err().0, ERR_INVALID_ARG);
        // Empty options mean defaults.
        assert_eq!(Options::from_json("").unwrap(), Options::default());
        assert_eq!(Options::from_json("{}").unwrap(), Options::default());
    }
}
