//! Minimal, correct JSON: full parser (objects preserve key order)
//! and serializers (compact + pretty). Numbers are kept as their
//! source text so round-trips don't lose precision.

use crate::{corrupt, CResult};

/// How deeply arrays and objects may nest. `parse_json` is recursive
/// descent, so this is also the depth of the stack it uses — and of the
/// `Drop` walk afterwards. Deeper input is an error, not a crash.
const MAX_DEPTH: usize = 128;

/// Whether `text` is exactly a JSON number:
/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`.
///
/// This is the grammar our serializers promise, so it is also the test
/// both number-inferring call sites use: `f64::from_str` is far too
/// generous (`1.`, `.5`, `+1`, `01` all parse), and emitting any of those
/// as a `Json::Num` would make DocConvert's own JSON unparseable.
pub fn is_json_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let at = |index: usize| bytes.get(index).copied();
    let mut i = 0usize;
    if at(0) == Some(b'-') {
        i += 1;
    }
    match at(i) {
        Some(b'0') => i += 1,
        Some(c) if c.is_ascii_digit() => {
            while matches!(at(i), Some(c) if c.is_ascii_digit()) {
                i += 1;
            }
        }
        _ => return false,
    }
    if at(i) == Some(b'.') {
        i += 1;
        if !matches!(at(i), Some(c) if c.is_ascii_digit()) {
            return false;
        }
        while matches!(at(i), Some(c) if c.is_ascii_digit()) {
            i += 1;
        }
    }
    if matches!(at(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(at(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !matches!(at(i), Some(c) if c.is_ascii_digit()) {
            return false;
        }
        while matches!(at(i), Some(c) if c.is_ascii_digit()) {
            i += 1;
        }
    }
    i == bytes.len()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

pub fn parse_json(input: &str) -> CResult<Json> {
    let chars: Vec<char> = input.chars().collect();
    let mut p = Parser {
        chars: &chars,
        pos: 0,
        depth: 0,
    };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(corrupt(format!("json: trailing characters at {}", p.pos)));
    }
    Ok(v)
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }
    fn expect(&mut self, c: char) -> CResult<()> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(corrupt(format!("json: expected '{c}' at {}", self.pos)))
        }
    }
    fn value(&mut self) -> CResult<Json> {
        if self.depth >= MAX_DEPTH {
            return Err(corrupt(format!(
                "json: nesting deeper than {MAX_DEPTH} levels at {}",
                self.pos
            )));
        }
        self.depth += 1;
        let result = self.value_inner();
        self.depth -= 1;
        result
    }
    fn value_inner(&mut self) -> CResult<Json> {
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let mut obj = Vec::new();
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.pos += 1;
                    return Ok(Json::Object(obj));
                }
                loop {
                    self.skip_ws();
                    let key = match self.value()? {
                        Json::Str(s) => s,
                        _ => return Err(corrupt("json: object key is not a string")),
                    };
                    self.skip_ws();
                    self.expect(':')?;
                    self.skip_ws();
                    let val = self.value()?;
                    obj.push((key, val));
                    self.skip_ws();
                    match self.peek() {
                        Some(',') => {
                            self.pos += 1;
                        }
                        Some('}') => {
                            self.pos += 1;
                            return Ok(Json::Object(obj));
                        }
                        _ => return Err(corrupt("json: expected ',' or '}' in object")),
                    }
                }
            }
            Some('[') => {
                self.pos += 1;
                let mut arr = Vec::new();
                self.skip_ws();
                if self.peek() == Some(']') {
                    self.pos += 1;
                    return Ok(Json::Array(arr));
                }
                loop {
                    self.skip_ws();
                    arr.push(self.value()?);
                    self.skip_ws();
                    match self.peek() {
                        Some(',') => {
                            self.pos += 1;
                        }
                        Some(']') => {
                            self.pos += 1;
                            return Ok(Json::Array(arr));
                        }
                        _ => return Err(corrupt("json: expected ',' or ']' in array")),
                    }
                }
            }
            Some('"') => Ok(Json::Str(self.string()?)),
            Some('t') => self.keyword("true", Json::Bool(true)),
            Some('f') => self.keyword("false", Json::Bool(false)),
            Some('n') => self.keyword("null", Json::Null),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            other => Err(corrupt(format!("json: unexpected {:?}", other))),
        }
    }
    fn keyword(&mut self, word: &str, value: Json) -> CResult<Json> {
        let end = self.pos + word.len();
        if self
            .chars
            .get(self.pos..end)
            .unwrap_or(&[])
            .iter()
            .copied()
            .eq(word.chars())
        {
            self.pos = end;
            Ok(value)
        } else {
            Err(corrupt("json: invalid literal"))
        }
    }
    fn string(&mut self) -> CResult<String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let c = self
                .peek()
                .ok_or_else(|| corrupt("json: unterminated string"))?;
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.peek().ok_or_else(|| corrupt("json: bad escape"))?;
                    self.pos += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{C}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hex: String = (0..4)
                                .filter_map(|_| {
                                    let c = self.peek()?;
                                    self.pos += 1;
                                    Some(c)
                                })
                                .collect();
                            let mut cp = u32::from_str_radix(&hex, 16)
                                .map_err(|_| corrupt("json: bad \\u escape"))?;
                            // Surrogate pair?
                            if (0xD800..0xDC00).contains(&cp)
                                && self.peek() == Some('\\')
                                && self.chars.get(self.pos + 1) == Some(&'u')
                            {
                                self.pos += 2;
                                let hex2: String = (0..4)
                                    .filter_map(|_| {
                                        let c = self.peek()?;
                                        self.pos += 1;
                                        Some(c)
                                    })
                                    .collect();
                                if let Ok(lo) = u32::from_str_radix(&hex2, 16) {
                                    if (0xDC00..0xE000).contains(&lo) {
                                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    }
                                }
                            }
                            out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(corrupt("json: unknown escape")),
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err(corrupt("json: control character in string"));
                }
                c => out.push(c),
            }
        }
    }
    fn number(&mut self) -> CResult<Json> {
        let start = self.pos;
        while matches!(self.peek(), Some(c)
            if c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        {
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        // JSON's grammar, not f64's: `01`, `1.` and `+1` are text. Accepting
        // them here would let a caller's document through and then have our
        // own output fail to parse.
        if !is_json_number(&text) {
            return Err(corrupt(format!("json: invalid number '{text}' at {start}")));
        }
        Ok(Json::Num(text))
    }
}

impl Json {
    pub fn to_compact(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0, false);
        s
    }
    pub fn to_pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0, true);
        s
    }
    fn write(&self, out: &mut String, indent: usize, pretty: bool) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => out.push_str(n),
            Json::Str(s) => {
                out.push('"');
                out.push_str(&crate::json_escape(s));
                out.push('"');
            }
            Json::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&"  ".repeat(indent + 1));
                    }
                    item.write(out, indent + 1, pretty);
                }
                if pretty && !items.is_empty() {
                    out.push('\n');
                    out.push_str(&"  ".repeat(indent));
                }
                out.push(']');
            }
            Json::Object(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&"  ".repeat(indent + 1));
                    }
                    out.push('"');
                    out.push_str(&crate::json_escape(k));
                    out.push_str(if pretty { "\": " } else { "\":" });
                    v.write(out, indent + 1, pretty);
                }
                if pretty && !entries.is_empty() {
                    out.push('\n');
                    out.push_str(&"  ".repeat(indent));
                }
                out.push('}');
            }
        }
    }

    /// The scalar as text for CSV: strings as-is, numbers/bools raw,
    /// null empty, composites as compact JSON.
    pub fn as_csv_scalar(&self) -> String {
        match self {
            Json::Null => String::new(),
            Json::Bool(b) => b.to_string(),
            Json::Num(n) => n.clone(),
            Json::Str(s) => s.clone(),
            other => other.to_compact(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_json_s_not_f64_s_grammar() {
        assert!(is_json_number("0"));
        assert!(is_json_number("-0.5"));
        assert!(is_json_number("12"));
        assert!(is_json_number("1e9"));
        assert!(is_json_number("-1.5E-3"));
        for bad in [
            "", "0.5 ", "1.", ".5", "+1", "01", "007", "1e", "--1", "NaN", "1_000", ".",
        ] {
            assert!(!is_json_number(bad), "{bad:?} is not a JSON number");
        }
        // And a document is not a number: these all fail to parse.
        for bad in ["1.", ".5", "+1", "01", "007", "1e", "--1", "", "."] {
            assert!(
                parse_json(bad).is_err(),
                "{bad:?} must not parse as a JSON value"
            );
        }
        // The lenient cases used to slip through a bare f64 parse.
        assert!(parse_json("01").is_err());
        assert!(parse_json("[1.]").is_err());
        assert!(parse_json(".5").is_err());
        // And the grammar still accepts what it always did.
        assert_eq!(parse_json("-1.5E-3").unwrap(), Json::Num("-1.5E-3".into()));
    }

    #[test]
    fn nesting_is_capped_instead_of_overflowing_the_stack() {
        let deep = "[".repeat(4096) + &"]".repeat(4096);
        assert!(parse_json(&deep).is_err());
        let deep = "{\"a\":".repeat(4096) + "1" + &"}".repeat(4096);
        assert!(parse_json(&deep).is_err());
        // Two levels are still two levels.
        assert!(parse_json("[[1]]").is_ok());
    }

    #[test]
    fn parse_and_roundtrip() {
        let v = parse_json(r#"{"a": [1, 2.5, "xé", true, null], "b": {"c": []}}"#).unwrap();
        assert_eq!(
            v.to_compact(),
            r#"{"a":[1,2.5,"xé",true,null],"b":{"c":[]}}"#
        );
        let pretty = v.to_pretty();
        assert!(pretty.contains("\n  \"a\": ["), "{pretty}");
        // Reparse pretty — same value.
        assert_eq!(parse_json(&pretty).unwrap(), v);
        // Surrogate pair.
        let s = parse_json(r#""\uD83D\uDE00""#).unwrap();
        assert_eq!(s, Json::Str("😀".into()));
        assert!(parse_json("{bad}").is_err());
        assert!(parse_json("[1,]").is_err());
        assert!(parse_json("").is_err());
    }
}
