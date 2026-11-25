//! DocConvert — document conversion between Markdown, HTML, CSV, TSV,
//! JSON, plain text, reStructuredText, AsciiDoc and OPML behind one
//! clean C ABI, in pure Rust with zero dependencies. Every parser is hand-written and scope-honest:
//! the Markdown renderer covers the CommonMark constructs documents
//! actually use (headings, emphasis, code, inline and reference
//! links, lists, quotes, fenced code, GFM tables) and understands
//! YAML front matter, the HTML reader handles the element set the
//! Markdown writer emits plus common hand-written HTML and can lift
//! a table out as CSV, CSV and TSV are RFC 4180, JSON is a full
//! parser/serializer, and the reStructuredText, AsciiDoc and OPML
//! readers land as Markdown so every target agrees with the others.

pub mod asciidoc;
pub mod convert;
pub mod csv;
pub mod ffi;
pub mod frontmatter;
pub mod html;
pub mod json;
pub mod markdown;
pub mod mcp;
pub mod opml;
pub mod platform;
pub mod rst;
pub mod stream;
pub mod toc;

/// The crate version, straight from `Cargo.toml`: the one place a release
/// number is written. `dc_version` and the CLI both read it, and
/// `tests/file_io.rs` pins the numeric parts and the shipped header to it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const VERSION_MAJOR: u32 = 0;
pub const VERSION_MINOR: u32 = 10;
pub const VERSION_PATCH: u32 = 2;

// Error codes (mirror include/docconvert.h).
pub const DC_OK: i32 = 0;
pub const ERR_INVALID_ARG: i32 = 1;
pub const ERR_NOT_SUPPORTED: i32 = 2;
pub const ERR_NOT_FOUND: i32 = 3;
pub const ERR_ACCESS_DENIED: i32 = 4;
pub const ERR_PLATFORM: i32 = 5;
pub const ERR_INTERNAL: i32 = 6;
/// Extension beyond the shared 0–6 set: input that fails to parse.
pub const ERR_CORRUPT: i32 = 7;

/// Every code the crate can return, with a one-line meaning — the CLI,
/// the MCP server, and the docs all read this table so they cannot
/// drift apart.
pub const ERROR_CODES: &[(i32, &str)] = &[
    (DC_OK, "ok"),
    (ERR_INVALID_ARG, "invalid argument"),
    (ERR_NOT_SUPPORTED, "conversion not supported"),
    (ERR_NOT_FOUND, "file not found"),
    (ERR_ACCESS_DENIED, "access denied"),
    (ERR_PLATFORM, "platform error"),
    (ERR_INTERNAL, "internal error"),
    (ERR_CORRUPT, "input failed to parse"),
];

/// The crate-wide result: failures carry a stable code + message.
pub type CResult<T> = Result<T, (i32, String)>;

/// Render one failure for a human or a model: `docconvert: <message>`
/// with the library's own prefix applied exactly once, whichever layer
/// (ABI, CLI, MCP) ends up printing it.
pub fn error_message(message: &str) -> &str {
    message.strip_prefix("docconvert: ").unwrap_or(message)
}

pub fn corrupt(msg: impl Into<String>) -> (i32, String) {
    (ERR_CORRUPT, format!("docconvert: {}", msg.into()))
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a string for inclusion inside JSON double quotes.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(escape_html("<a & b>"), "&lt;a &amp; b&gt;");
        assert_eq!(json_escape("a\"b"), "a\\\"b");
    }
}
