//! CSV per RFC 4180: quoted fields, embedded commas/quotes/newlines,
//! CRLF or LF endings. Writing quotes only when needed.

use crate::{corrupt, CResult};

pub fn parse_csv(input: &str) -> CResult<Vec<Vec<String>>> {
    let bytes = input.as_bytes();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut i = 0usize;
    let mut in_quotes = false;
    let mut field_started = false;
    let mut any_input = false;
    while i < bytes.len() {
        let b = bytes[i];
        any_input = true;
        if in_quotes {
            match b {
                b'"' => {
                    if bytes.get(i + 1) == Some(&b'"') {
                        field.push('"');
                        i += 2;
                    } else {
                        in_quotes = false;
                        i += 1;
                    }
                }
                _ => {
                    // Preserve the byte as UTF-8 text: collect the
                    // full character.
                    let ch_len = utf8_len(bytes, i);
                    field.push_str(&input[i..i + ch_len]);
                    i += ch_len;
                }
            }
        } else {
            match b {
                b'"' if !field_started => {
                    in_quotes = true;
                    field_started = true;
                    i += 1;
                }
                b',' => {
                    row.push(std::mem::take(&mut field));
                    field_started = false;
                    i += 1;
                }
                b'\r' | b'\n' => {
                    if b == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                    row.push(std::mem::take(&mut field));
                    field_started = false;
                    rows.push(std::mem::take(&mut row));
                    i += 1;
                }
                _ => {
                    let ch_len = utf8_len(bytes, i);
                    field.push_str(&input[i..i + ch_len]);
                    field_started = true;
                    i += ch_len;
                }
            }
        }
    }
    if in_quotes {
        return Err(corrupt("csv: unterminated quoted field"));
    }
    // Trailing field/row when the file doesn't end with a newline.
    if field_started || !row.is_empty() || !field.is_empty() {
        row.push(field);
        rows.push(row);
    }
    if !any_input {
        return Ok(Vec::new());
    }
    Ok(rows)
}

fn utf8_len(bytes: &[u8], i: usize) -> usize {
    let b = bytes[i];
    let want = if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    };
    want.min(bytes.len() - i)
}

pub fn write_csv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        let cells: Vec<String> = row.iter().map(|c| quote_cell(c)).collect();
        out.push_str(&cells.join(","));
        out.push_str("\r\n");
    }
    out
}

fn quote_cell(cell: &str) -> String {
    if cell.contains(',') || cell.contains('"') || cell.contains('\n') || cell.contains('\r') {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4180_roundtrip() {
        let src = "name,quote,note\r\n\"Smith, John\",\"said \"\"hi\"\"\",\"line1\nline2\"\r\nplain,simple,\r\n";
        let rows = parse_csv(src).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1][0], "Smith, John");
        assert_eq!(rows[1][1], "said \"hi\"");
        assert_eq!(rows[1][2], "line1\nline2");
        assert_eq!(rows[2][2], "");
        let back = write_csv(&rows);
        assert_eq!(parse_csv(&back).unwrap(), rows);
        // LF-only + no trailing newline.
        let rows = parse_csv("a,b\n1,2").unwrap();
        assert_eq!(rows, vec![vec!["a", "b"], vec!["1", "2"]]);
        assert!(parse_csv("\"unterminated").is_err());
    }
}
