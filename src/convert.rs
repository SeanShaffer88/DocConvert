//! Format registry + the conversion matrix.

use crate::csv::{parse_csv, write_csv};
use crate::html::{html_to_markdown, html_to_text};
use crate::json::{parse_json, Json};
use crate::markdown::{convert_markdown, Flavor};
use crate::{corrupt, CResult, ERR_INVALID_ARG, ERR_NOT_SUPPORTED};

pub const FORMATS: &[&str] = &["markdown", "html", "csv", "json", "text"];

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
        ("markdown", "html") => convert_markdown(input, Flavor::Html),
        ("markdown", "text") => convert_markdown(input, Flavor::Text),
        ("html", "markdown") => html_to_markdown(input),
        ("html", "text") => html_to_text(input),
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
        ("html", "csv")
        | ("html", "json")
        | ("markdown", "csv")
        | ("markdown", "json")
        | ("text", "csv")
        | ("text", "json") => Err((
            ERR_NOT_SUPPORTED,
            format!("conversion {from} → {to} is not supported in v0.1"),
        )),
        _ => Err((
            ERR_NOT_SUPPORTED,
            format!("conversion {from} → {to} is not supported in v0.1"),
        )),
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
}
