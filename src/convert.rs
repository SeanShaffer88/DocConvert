//! Format registry + the conversion matrix.

use crate::csv::{parse_csv, write_csv};
use crate::frontmatter;
use crate::html::{html_to_csv, html_to_markdown, html_to_text};
use crate::json::{parse_json, Json};
use crate::markdown::{convert_markdown, Flavor};
use crate::{corrupt, CResult, ERR_INVALID_ARG, ERR_NOT_SUPPORTED};

pub const FORMATS: &[&str] = &["markdown", "html", "csv", "json", "text"];

/// Every `{from, to}` pair this version supports. `dc_matrix_json()`
/// is generated from this table and `matrix_is_exact` asserts that
/// `convert` agrees with it, so the advertised matrix and the dispatch
/// can never drift apart.
pub const MATRIX: &[(&str, &str)] = &[
    ("markdown", "html"),
    ("markdown", "text"),
    ("html", "markdown"),
    ("html", "text"),
    ("html", "csv"),
    ("text", "html"),
    ("text", "markdown"),
    ("csv", "json"),
    ("json", "csv"),
    ("csv", "markdown"),
    ("csv", "html"),
    ("csv", "text"),
    ("json", "text"),
    ("json", "html"),
    ("json", "markdown"),
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
        "csv" | "tsv" => "csv",
        "json" => "json",
        "text" | "txt" | "plain" => "text",
        other => {
            return Err((
                ERR_INVALID_ARG,
                format!("unknown format '{other}' (known: markdown, html, csv, json, text)"),
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
            _ => Ok(input.to_string()),
        };
    }
    match (from.as_str(), to.as_str()) {
        ("markdown", "html") => convert_markdown(&markdown_body(input, options), Flavor::Html),
        ("markdown", "text") => convert_markdown(&markdown_body(input, options), Flavor::Text),
        ("html", "markdown") => html_to_markdown(input),
        ("html", "text") => html_to_text(input),
        ("html", "csv") => html_to_csv(input),
        ("text", "html") => Ok(text_to_html(input)),
        ("text", "markdown") => Ok(input.trim_end().to_string() + "\n"),
        ("csv", "json") => csv_to_json(input),
        ("json", "csv") => json_to_csv(input),
        ("csv", "markdown") => csv_to_markdown_table(input),
        ("csv", "html") => csv_to_html_table(input),
        ("csv", "text") => Ok(write_csv(&parse_csv(input)?)),
        ("json", "text") => Ok(parse_json(input)?.to_pretty()),
        ("json", "html") => Ok(format!(
            "<pre>{}</pre>",
            crate::escape_html(&parse_json(input)?.to_pretty())
        )),
        ("json", "markdown") => Ok(format!(
            "```json\n{}\n```\n",
            parse_json(input)?.to_pretty()
        )),
        ("html", "json")
        | ("markdown", "csv")
        | ("markdown", "json")
        | ("text", "csv")
        | ("text", "json") => Err((
            ERR_NOT_SUPPORTED,
            format!("conversion {from} → {to} is not supported"),
        )),
        _ => Err((
            ERR_NOT_SUPPORTED,
            format!("conversion {from} → {to} is not supported"),
        )),
    }
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

fn csv_to_json(input: &str) -> CResult<String> {
    let rows = parse_csv(input)?;
    if rows.is_empty() {
        return Ok("[]".to_string());
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
    Ok(Json::Array(objects).to_pretty())
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

fn json_to_csv(input: &str) -> CResult<String> {
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
        return Ok(String::new());
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
    Ok(write_csv(&rows))
}

fn csv_to_markdown_table(input: &str) -> CResult<String> {
    let rows = parse_csv(input)?;
    if rows.is_empty() {
        return Ok(String::new());
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
    Ok(out)
}

fn csv_to_html_table(input: &str) -> CResult<String> {
    let rows = parse_csv(input)?;
    if rows.is_empty() {
        return Ok("<table>\n</table>".to_string());
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
    Ok(out)
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
        assert!(convert("x", "markdown", "csv").is_err());
        assert_eq!(format_for_path("notes.MD").as_deref(), Some("markdown"));
        assert_eq!(format_for_path("noext"), None);
        // Leading-zero codes stay strings.
        let j = convert("code\n007\n", "csv", "json").unwrap();
        assert!(j.contains("\"007\""), "{j}");
    }

    fn sample(name: &str) -> &'static str {
        match name {
            "markdown" => "# Title\n\nHello **there** and [a link](https://e.com).\n",
            "html" => {
                "<h1>Title</h1><p>Hello <strong>there</strong></p>\
                       <table><tr><th>A</th></tr><tr><td>1</td></tr></table>"
            }
            "csv" => "name,qty\nMilk,2\n",
            "json" => "{\"a\": [1, 2], \"b\": null}",
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
