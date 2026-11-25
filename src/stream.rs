//! Block-at-a-time file conversion.
//!
//! The string API is the contract, and most conversions genuinely need
//! the whole document. These do not, so they stream:
//!
//! - **identity copies** of the byte-faithful formats (a 64 KiB copy),
//! - **text** → markdown / html, one paragraph at a time,
//! - **CSV and TSV** → csv / tsv / text / markdown, one RFC 4180 record
//!   at a time (records are split quote-aware and then parsed by
//!   `csv::parse_*`, so a streamed record cannot disagree with the
//!   whole-file path),
//! - **markdown** → html / text / csv / tsv, one block at a time.
//!
//! Markdown has document-wide constructs — reference link definitions
//! and footnotes — and a streamed pass would see them too late. So the
//! Markdown stream watches for them: if one appears (or the document
//! opens with YAML front matter) it drops what it has written and redoes
//! the conversion from the whole file. The output is therefore identical
//! either way; only the memory profile differs.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

use crate::convert::{self, BYTE_FAITHFUL};
use crate::csv::{parse_csv, parse_tsv, write_record};
use crate::markdown::{self, Flavor};
use crate::{CResult, ERR_CORRUPT, ERR_PLATFORM};

/// `convert::convert_file_streaming`'s engine.
pub fn convert_file(
    in_path: &str,
    out_path: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> CResult<u64> {
    let from = match from {
        Some(name) => convert::normalize_format(name)?,
        None => convert::format_for_path(in_path).ok_or((
            crate::ERR_INVALID_ARG,
            format!("cannot detect the input format from '{in_path}' — pass it explicitly"),
        ))?,
    };
    let to = match to {
        Some(name) => convert::normalize_format(name)?,
        None => convert::format_for_path(out_path).ok_or((
            crate::ERR_INVALID_ARG,
            format!("cannot detect the output format from '{out_path}' — pass it explicitly"),
        ))?,
    };
    if from == to && BYTE_FAITHFUL.contains(&from.as_str()) {
        return crate::platform::copy_file(in_path, out_path);
    }
    let streamed = match (from.as_str(), to.as_str()) {
        ("text", "markdown") => stream_text(in_path, out_path, Target::Markdown),
        ("text", "html") => stream_text(in_path, out_path, Target::Html),
        ("csv", "csv")
        | ("csv", "tsv")
        | ("csv", "text")
        | ("tsv", "csv")
        | ("tsv", "tsv")
        | ("tsv", "text") => stream_records(in_path, out_path, from == "tsv", to.as_str()),
        ("csv", "markdown") | ("tsv", "markdown") => {
            stream_records(in_path, out_path, from == "tsv", to.as_str())
        }
        ("markdown", "html") => stream_markdown(in_path, out_path, Flavor::Html),
        ("markdown", "text") => stream_markdown(in_path, out_path, Flavor::Text),
        ("markdown", "csv") | ("markdown", "tsv") => {
            stream_markdown_rows(in_path, out_path, to == "tsv")
        }
        _ => Ok(None),
    };
    match streamed? {
        Some(bytes) => Ok(bytes),
        None => whole_file(in_path, out_path, &from, &to),
    }
}

/// The whole-document path, shared with `convert::convert_file_streaming`
/// when a streamed attempt turns out to be inexact.
pub fn whole_file(in_path: &str, out_path: &str, from: &str, to: &str) -> CResult<u64> {
    let bytes = crate::platform::read_file(in_path)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| (ERR_CORRUPT, format!("'{in_path}' is not valid UTF-8 text")))?;
    let converted = convert::convert_with_options(&text, from, to, &convert::Options::default())?;
    crate::platform::write_file(out_path, converted.as_bytes())?;
    Ok(converted.len() as u64)
}

fn reader(path: &str) -> CResult<BufReader<File>> {
    let file = File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => (crate::ERR_NOT_FOUND, format!("file not found: '{path}'")),
        std::io::ErrorKind::PermissionDenied => (
            crate::ERR_ACCESS_DENIED,
            format!("permission denied reading '{path}'"),
        ),
        _ => (ERR_PLATFORM, format!("cannot read '{path}': {e}")),
    })?;
    Ok(BufReader::new(file))
}

fn writer(path: &str) -> CResult<BufWriter<File>> {
    let file = File::create(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => (
            crate::ERR_ACCESS_DENIED,
            format!("permission denied writing '{path}'"),
        ),
        _ => (ERR_PLATFORM, format!("cannot write '{path}': {e}")),
    })?;
    Ok(BufWriter::new(file))
}

fn write(writer: &mut BufWriter<File>, text: &str) -> CResult<()> {
    writer
        .write_all(text.as_bytes())
        .map_err(|e| (ERR_PLATFORM, format!("cannot write the document: {e}")))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Markdown,
    Html,
}

/// text → markdown / html, one paragraph at a time.
fn stream_text(in_path: &str, out_path: &str, target: Target) -> CResult<Option<u64>> {
    let mut source = reader(in_path)?;
    let mut sink = writer(out_path)?;
    let mut bytes = 0usize;
    let mut paragraph: Vec<String> = Vec::new();
    let mut pending_blank = 0usize;
    let mut raw = String::new();
    loop {
        raw.clear();
        let read = source
            .read_line(&mut raw)
            .map_err(|e| (ERR_PLATFORM, format!("cannot read '{in_path}': {e}")))?;
        if read == 0 {
            break;
        }
        let line = raw.trim_end_matches(['\n', '\r']).to_string();
        if line.trim().is_empty() {
            flush_paragraph(&mut sink, &mut paragraph, target, &mut bytes)?;
            pending_blank += 1;
            continue;
        }
        if target == Target::Markdown {
            // text → markdown is the document with its tail trimmed.
            for _ in 0..pending_blank {
                write(&mut sink, "\n")?;
                bytes += 1;
            }
            pending_blank = 0;
            let with_newline = format!("{line}\n");
            write(&mut sink, &with_newline)?;
            bytes += with_newline.len();
        } else {
            pending_blank = 0;
            paragraph.push(line);
        }
    }
    flush_paragraph(&mut sink, &mut paragraph, target, &mut bytes)?;
    if target == Target::Markdown && bytes == 0 {
        // An empty document is still a document: `trim_end() + "\n"`.
        write(&mut sink, "\n")?;
        bytes += 1;
    }
    sink.flush()
        .map_err(|e| (ERR_PLATFORM, format!("cannot write '{out_path}': {e}")))?;
    Ok(Some(bytes as u64))
}

fn flush_paragraph(
    sink: &mut BufWriter<File>,
    paragraph: &mut Vec<String>,
    target: Target,
    bytes: &mut usize,
) -> CResult<()> {
    if paragraph.is_empty() {
        return Ok(());
    }
    let text = paragraph.join("\n");
    paragraph.clear();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let rendered = match target {
        Target::Html => convert::convert(trimmed, "text", "html")?,
        Target::Markdown => convert::convert(trimmed, "text", "markdown")?,
    };
    write(sink, &rendered)?;
    *bytes += rendered.len();
    Ok(())
}

/// CSV/TSV → csv / tsv / text / markdown, one record at a time.
fn stream_records(in_path: &str, out_path: &str, tsv_in: bool, to: &str) -> CResult<Option<u64>> {
    let mut source = reader(in_path)?;
    let mut sink = writer(out_path)?;
    let mut bytes = 0usize;
    let to_markdown = to == "markdown";
    let out_delim = match to {
        "tsv" => '\t',
        "csv" => ',',
        // "text", and anything the markdown branch does not use
        _ => {
            if tsv_in {
                '\t'
            } else {
                ','
            }
        }
    };
    let mut in_quotes = false;
    let mut record = String::new();
    let mut raw = String::new();
    // A markdown table has one width for every row, so a ragged file cannot
    // be streamed into one without knowing the widest row up front: stop
    // and let the whole-file path have it.
    let mut width: Option<usize> = None;
    let mut ragged = false;
    let mut header_written = false;
    let mut write_record_text = |text: &str, sink: &mut BufWriter<File>| -> CResult<usize> {
        let rows = if tsv_in {
            parse_tsv(text)?
        } else {
            parse_csv(text)?
        };
        let mut written = 0usize;
        for row in &rows {
            // Markdown needs a header separator after the very first row,
            // which is exactly what `markdown::table` emits for one row.
            let record = if to_markdown {
                let expected = *width.get_or_insert(row.len());
                if row.len() != expected {
                    ragged = true;
                    continue;
                }
                if !header_written {
                    header_written = true;
                    markdown::table(std::slice::from_ref(row))
                } else {
                    markdown::table_row(row)
                }
            } else {
                write_record(row, out_delim)
            };
            write(sink, &record)?;
            written += record.len();
        }
        Ok(written)
    };
    loop {
        raw.clear();
        let read = source
            .read_line(&mut raw)
            .map_err(|e| (ERR_PLATFORM, format!("cannot read '{in_path}': {e}")))?;
        if read == 0 {
            break;
        }
        record.push_str(&raw);
        // A quote toggles the quoted state; `""` inside quotes is an
        // escaped quote and leaves the state alone.
        let chars: Vec<char> = raw.chars().collect();
        let mut index = 0usize;
        while index < chars.len() {
            if chars[index] == '"' {
                if in_quotes && chars.get(index + 1) == Some(&'"') {
                    index += 2;
                    continue;
                }
                in_quotes = !in_quotes;
            }
            index += 1;
        }
        if in_quotes {
            continue;
        }
        let text = std::mem::take(&mut record);
        bytes += write_record_text(&text, &mut sink)?;
    }
    if !record.is_empty() {
        let text = std::mem::take(&mut record);
        bytes += write_record_text(&text, &mut sink)?;
    }
    if ragged {
        return Ok(None);
    }
    sink.flush()
        .map_err(|e| (ERR_PLATFORM, format!("cannot write '{out_path}': {e}")))?;
    Ok(Some(bytes as u64))
}

/// markdown → csv / tsv, one block at a time.
///
/// The whole-file path wants exactly one table, optionally behind one
/// heading, so the stream does too: a heading is skipped, a table is
/// written record by record, and anything else is the same parse error the
/// whole-document path raises.
fn stream_markdown_rows(in_path: &str, out_path: &str, tsv_out: bool) -> CResult<Option<u64>> {
    let mut source = reader(in_path)?;
    let mut sink = writer(out_path)?;
    let mut bytes = 0usize;
    let mut block: Vec<String> = Vec::new();
    let mut blanks = 0usize;
    let mut in_fence = false;
    let mut first = true;
    let mut heading_seen = false;
    let mut table_seen = false;
    let mut raw = String::new();
    loop {
        raw.clear();
        let read = source
            .read_line(&mut raw)
            .map_err(|e| (ERR_PLATFORM, format!("cannot read '{in_path}': {e}")))?;
        if read == 0 {
            break;
        }
        let line = raw.trim_end_matches(['\n', '\r']).to_string();
        if in_fence {
            block.push(line.clone());
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                in_fence = false;
            }
            continue;
        }
        if line.trim().is_empty() {
            blanks += 1;
            continue;
        }
        if first {
            // YAML front matter is document-wide: convert the whole file.
            if line.trim_end() == "---" {
                return Ok(None);
            }
            first = false;
        }
        if block.is_empty() {
            block.push(line);
            continue;
        }
        if blanks > 0 && !continues(&block, &line) {
            match emit_rows(
                &mut sink,
                &block,
                tsv_out,
                &mut heading_seen,
                &mut table_seen,
            )? {
                Some(written) => bytes += written,
                None => return Ok(None),
            }
            block.clear();
        } else {
            for _ in 0..blanks {
                block.push(String::new());
            }
        }
        blanks = 0;
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = true;
        }
        block.push(line);
    }
    if !block.is_empty() {
        match emit_rows(
            &mut sink,
            &block,
            tsv_out,
            &mut heading_seen,
            &mut table_seen,
        )? {
            Some(written) => bytes += written,
            None => return Ok(None),
        }
    }
    sink.flush()
        .map_err(|e| (ERR_PLATFORM, format!("cannot write '{out_path}': {e}")))?;
    Ok(Some(bytes as u64))
}

/// One Markdown block → CSV/TSV records. `Ok(None)` means "this file needs
/// the whole-document path".
fn emit_rows(
    sink: &mut BufWriter<File>,
    block: &[String],
    tsv_out: bool,
    heading_seen: &mut bool,
    table_seen: &mut bool,
) -> CResult<Option<usize>> {
    let text = block.join("\n");
    if markdown::has_document_wide_constructs(&text) {
        return Ok(None);
    }
    if text.trim_start().starts_with('#') && !*table_seen {
        // The one heading the whole-file path tolerates.
        if *heading_seen {
            return Err((
                ERR_CORRUPT,
                "markdown→csv needs one table (and at most one heading before it)".to_string(),
            ));
        }
        *heading_seen = true;
        return Ok(Some(0));
    }
    if *table_seen {
        return Err((
            ERR_CORRUPT,
            "markdown→csv needs one table (and at most one heading before it)".to_string(),
        ));
    }
    let rows = markdown::markdown_rows(&text)?;
    *table_seen = true;
    let delim = if tsv_out { '\t' } else { ',' };
    let mut written = 0usize;
    for row in &rows {
        let record = crate::csv::write_record(row, delim);
        write(sink, &record)?;
        written += record.len();
    }
    Ok(Some(written))
}

/// markdown → html / text, one block at a time, with a whole-document
/// fallback for the constructs that need the whole file.
fn stream_markdown(in_path: &str, out_path: &str, flavor: Flavor) -> CResult<Option<u64>> {
    let mut source = reader(in_path)?;
    let mut sink = writer(out_path)?;
    let mut bytes = 0usize;
    let mut block: Vec<String> = Vec::new();
    let mut blanks = 0usize;
    let mut in_fence = false;
    let mut first = true;
    let mut raw = String::new();
    let separator = if flavor == Flavor::Html { "\n" } else { "\n\n" };
    loop {
        raw.clear();
        let read = source
            .read_line(&mut raw)
            .map_err(|e| (ERR_PLATFORM, format!("cannot read '{in_path}': {e}")))?;
        if read == 0 {
            break;
        }
        let line = raw.trim_end_matches(['\n', '\r']).to_string();
        if in_fence {
            block.push(line.clone());
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                in_fence = false;
            }
            continue;
        }
        if line.trim().is_empty() {
            blanks += 1;
            continue;
        }
        if first {
            // YAML front matter is document-wide: convert the whole file.
            if line.trim_end() == "---" {
                return Ok(None);
            }
            first = false;
        }
        if block.is_empty() {
            block.push(line);
            continue;
        }
        if blanks > 0 && !continues(&block, &line) {
            if !emit_block(&mut sink, &block, flavor, separator, &mut bytes)? {
                return Ok(None);
            }
            block.clear();
        } else {
            for _ in 0..blanks {
                block.push(String::new());
            }
        }
        blanks = 0;
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = true;
        }
        block.push(line);
    }
    if !block.is_empty() && !emit_block(&mut sink, &block, flavor, separator, &mut bytes)? {
        return Ok(None);
    }
    if flavor == Flavor::Text && bytes > 0 {
        write(&mut sink, "\n")?;
        bytes += 1;
    }
    sink.flush()
        .map_err(|e| (ERR_PLATFORM, format!("cannot write '{out_path}': {e}")))?;
    Ok(Some(bytes as u64))
}

/// Render one block. Returns false when the document uses a construct
/// that needs the whole file (reference or footnote definitions).
fn emit_block(
    sink: &mut BufWriter<File>,
    block: &[String],
    flavor: Flavor,
    separator: &str,
    bytes: &mut usize,
) -> CResult<bool> {
    let text = block.join("\n");
    if markdown::has_document_wide_constructs(&text) {
        return Ok(false);
    }
    let rendered = markdown::convert_markdown(&text, flavor)?;
    let rendered = rendered.trim_end_matches('\n');
    if rendered.is_empty() {
        return Ok(true);
    }
    if *bytes > 0 {
        write(sink, separator)?;
        *bytes += separator.len();
    }
    write(sink, rendered)?;
    *bytes += rendered.len();
    Ok(true)
}

/// Does the next line belong to the block a blank line interrupted?
fn continues(block: &[String], next: &str) -> bool {
    let first = block
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.as_str())
        .unwrap_or("");
    if indent_of(first) >= 4 {
        // Indented code blocks may contain blank lines.
        return indent_of(next) >= 4;
    }
    if list_marker(first) {
        return list_marker(next) || indent_of(next) >= 2 || next.trim_start().starts_with('>');
    }
    if first.trim_start().starts_with('>') {
        return next.trim_start().starts_with('>');
    }
    false
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn list_marker(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return true;
    }
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return false;
    }
    let rest = &trimmed[digits.len()..];
    rest.starts_with(". ") || rest.starts_with(") ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("docconvert-stream-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn text_streams_to_html_and_markdown() {
        let dir = scratch("text");
        let source = dir.join("in.txt");
        let html = dir.join("out.html");
        let markdown = dir.join("out.md");
        std::fs::write(&source, "one\ntwo\n\nthree\n\n\n").expect("write");

        let bytes = convert_file(
            source.to_str().unwrap(),
            html.to_str().unwrap(),
            Some("text"),
            Some("html"),
        )
        .expect("stream");
        assert!(bytes > 0);
        assert_eq!(
            std::fs::read_to_string(&html).expect("read"),
            "<p>one<br>\ntwo</p>\n<p>three</p>\n"
        );

        convert_file(
            source.to_str().unwrap(),
            markdown.to_str().unwrap(),
            Some("text"),
            Some("markdown"),
        )
        .expect("stream");
        assert_eq!(
            std::fs::read_to_string(&markdown).expect("read"),
            "one\ntwo\n\nthree\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The table pairs stream too, and a streamed table must be the same
    /// bytes the whole-document path writes.
    #[test]
    fn table_pairs_stream_like_the_whole_file() {
        let dir = scratch("tables");

        // CSV → markdown: the first record becomes the header.
        let csv_in = dir.join("in.csv");
        let md = dir.join("out.md");
        std::fs::write(&csv_in, "Name,Qty\nMilk,2\nBread,3\n").expect("write");
        convert_file(
            csv_in.to_str().unwrap(),
            md.to_str().unwrap(),
            Some("csv"),
            Some("markdown"),
        )
        .expect("stream");
        assert_eq!(
            std::fs::read_to_string(&md).expect("read"),
            "| Name | Qty |\n| --- | --- |\n| Milk | 2 |\n| Bread | 3 |\n"
        );

        // A ragged file cannot be a GFM table without knowing its widest
        // row, so the stream hands it back: same output, just not streamed.
        let ragged = dir.join("ragged.csv");
        let ragged_out = dir.join("ragged.md");
        std::fs::write(&ragged, "a,b\n1\n").expect("write");
        convert_file(
            ragged.to_str().unwrap(),
            ragged_out.to_str().unwrap(),
            Some("csv"),
            Some("markdown"),
        )
        .expect("fallback");
        assert_eq!(
            std::fs::read_to_string(&ragged_out).expect("read"),
            "| a | b |\n| --- | --- |\n| 1 |  |\n"
        );

        // Markdown → CSV: one heading may precede the table.
        let doc = dir.join("doc.md");
        let out_csv = dir.join("doc.csv");
        std::fs::write(
            &doc,
            "# Totals\n\n| Name | Qty |\n| --- | --- |\n| Milk | 2 |\n",
        )
        .expect("write");
        convert_file(
            doc.to_str().unwrap(),
            out_csv.to_str().unwrap(),
            Some("markdown"),
            Some("csv"),
        )
        .expect("stream");
        assert_eq!(
            std::fs::read_to_string(&out_csv).expect("read"),
            "Name,Qty\r\nMilk,2\r\n"
        );

        // Two tables are what the whole-file path refuses, so it refuses too.
        let two = dir.join("two.md");
        std::fs::write(&two, "| a |\n| --- |\n| 1 |\n\n| b |\n| --- |\n| 2 |\n").expect("write");
        let two_out = dir.join("two.csv");
        let error = convert_file(
            two.to_str().unwrap(),
            two_out.to_str().unwrap(),
            Some("markdown"),
            Some("csv"),
        )
        .expect_err("two tables");
        assert_eq!(error.0, ERR_CORRUPT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn records_stream_quote_aware() {
        let dir = scratch("records");
        let source = dir.join("in.csv");
        let out = dir.join("out.tsv");
        std::fs::write(&source, "a,b\n\"x,y\",\"line\nbreak\"\n").expect("write");
        convert_file(
            source.to_str().unwrap(),
            out.to_str().unwrap(),
            Some("csv"),
            Some("tsv"),
        )
        .expect("stream");
        // The whole-file path is the reference.
        let whole =
            convert::convert(&std::fs::read_to_string(&source).unwrap(), "csv", "tsv").unwrap();
        assert_eq!(std::fs::read_to_string(&out).expect("read"), whole);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn markdown_streams_blocks_and_falls_back_for_definitions() {
        let dir = scratch("markdown");
        let source = dir.join("in.md");
        let out = dir.join("out.html");
        let body =
            "# Title\n\nA [link][ref] and a list:\n\n- one\n- two\n\n[ref]: https://example.com\n";
        std::fs::write(&source, body).expect("write");

        // The definition forces the whole-document path…
        assert!(stream_markdown(
            source.to_str().unwrap(),
            out.to_str().unwrap(),
            Flavor::Html
        )
        .unwrap()
        .is_none());
        // …and that path matches convert() exactly.
        convert_file(
            source.to_str().unwrap(),
            out.to_str().unwrap(),
            Some("markdown"),
            Some("html"),
        )
        .expect("stream");
        assert_eq!(
            std::fs::read_to_string(&out).expect("read"),
            convert::convert(body, "markdown", "html").unwrap()
        );

        // A plain document streams and still matches the whole-file path.
        let plain = "# Title\n\nFirst paragraph\nwrapped over two lines.\n\nSecond.\n";
        std::fs::write(&source, plain).expect("write");
        convert_file(
            source.to_str().unwrap(),
            out.to_str().unwrap(),
            Some("markdown"),
            Some("html"),
        )
        .expect("stream");
        assert_eq!(
            std::fs::read_to_string(&out).expect("read"),
            convert::convert(plain, "markdown", "html").unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
