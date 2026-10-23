//! Minimal, correct JSON: full parser (objects preserve key order)
//! and serializers (compact + pretty). Numbers are kept as their
//! source text so round-trips don't lose precision.

use crate::{corrupt, CResult};

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
        if self.chars[self.pos..].starts_with(&word.chars().collect::<Vec<_>>()[..]) {
            self.pos += word.len();
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
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        {
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        // Validate loosely via f64 parse.
        text.parse::<f64>()
            .map_err(|_| corrupt(format!("json: invalid number '{text}'")))?;
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
