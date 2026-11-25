//! A Model Context Protocol server over stdio: JSON-RPC 2.0,
//! newline-delimited, no SDK and no dependencies — the protocol is
//! small enough that the crate's own JSON parser drives it.
//!
//! What it offers a local AI client is the library itself: `convert`,
//! `convert_file`, `front_matter`, and the introspection tools
//! `formats`, `matrix` and `capabilities`. Tool failures come back as
//! `isError: true` with the `DC_*` code in the message, so the model
//! can read what went wrong and retry differently; protocol mistakes
//! (unknown method, malformed JSON) come back as JSON-RPC errors.
//!
//! stdout carries protocol messages only — anything else belongs on
//! stderr.

use std::io::{BufRead, Write};

use crate::convert::{self, FrontMatterMode, Options};
use crate::json::{parse_json, Json};
use crate::platform;
use crate::{CResult, ERR_CORRUPT, ERR_INVALID_ARG, VERSION};

/// The newest protocol revision this server speaks.
pub const LATEST_PROTOCOL: &str = "2025-06-18";
/// Revisions the server accepts from a client as-is.
const KNOWN_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// A protocol-level failure: JSON-RPC code plus a message for the
/// client's log (not the `DC_*` codes, which travel in tool results).
type RpcError = (i32, String);

const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;

/// The tool names, in the order `tools/list` returns them.
const TOOL_NAMES: &[&str] = &[
    "convert",
    "convert_file",
    "front_matter",
    "formats",
    "matrix",
    "capabilities",
];

const INSTRUCTIONS: &str =
    "DocConvert converts documents between markdown, html, csv, tsv, json and \
text. Use `convert` for text you already hold and `convert_file` for paths on disk; pairs outside \
`matrix` are not supported and fail loudly rather than degrading. `front_matter` reads YAML \
metadata, and `capabilities` reports this build's formats, pairs, options and error codes in one \
object.";

/// `tools/list` payload. Kept as literal JSON so it reads like the MCP
/// documentation; `tools_payload_is_well_formed` parses it in tests.
const TOOLS: &str = r#"[
  {
    "name": "convert",
    "description": "Convert a document between Markdown, HTML, CSV, TSV, JSON and plain text and return the result. Unsupported pairs fail with a clear message instead of guessing.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "input": { "type": "string", "description": "The document text to convert." },
        "from": { "type": "string", "description": "Source format: markdown, html, csv, tsv, json or text (aliases md, mkd, htm, xhtml, tab, txt, plain accepted)." },
        "to": { "type": "string", "description": "Target format: markdown, html, csv, tsv, json or text." },
        "front_matter": { "type": "string", "enum": ["strip", "table"], "description": "What md->html and md->text do with a YAML front matter block: strip it (default) or render it as a key/value table." }
      },
      "required": ["input", "from", "to"],
      "additionalProperties": false
    }
  },
  {
    "name": "convert_file",
    "description": "Convert a document on disk. Formats are inferred from the file extensions unless \"from\"/\"to\" are given. Writes the output file and returns {written, bytes, from, to}.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "input_path": { "type": "string", "description": "Path of the file to read." },
        "output_path": { "type": "string", "description": "Path of the file to write." },
        "from": { "type": "string", "description": "Source format; inferred from input_path when omitted." },
        "to": { "type": "string", "description": "Target format; inferred from output_path when omitted." },
        "front_matter": { "type": "string", "enum": ["strip", "table"] }
      },
      "required": ["input_path", "output_path"],
      "additionalProperties": false
    }
  },
  {
    "name": "front_matter",
    "description": "Read the YAML front matter of a Markdown document as JSON ({} when there is none or it is malformed). Provide either \"input\" (text) or \"path\" (file).",
    "inputSchema": {
      "type": "object",
      "properties": {
        "input": { "type": "string", "description": "Markdown text to inspect." },
        "path": { "type": "string", "description": "Markdown file to inspect." }
      },
      "additionalProperties": false
    }
  },
  {
    "name": "formats",
    "description": "List the format names this build knows, as a JSON array.",
    "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
  },
  {
    "name": "matrix",
    "description": "List every supported {\"from\",\"to\"} conversion pair as a JSON array. Anything outside this list is NOT_SUPPORTED.",
    "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
  },
  {
    "name": "capabilities",
    "description": "Everything about this build in one object: version, protocol revision, formats, supported pairs, options, tools and error codes.",
    "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
  }
]"#;

/// Run the stdio loop until the client closes stdin. Each request gets
/// exactly one response line; notifications get none.
pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> std::io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if let Some(response) = handle_line(&line) {
            output.write_all(response.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

/// Handle one newline-delimited message. `None` means "no response"
/// (a notification, or a response from the client, which this server
/// never asks for but must tolerate).
pub fn handle_line(line: &str) -> Option<String> {
    let text = line.trim();
    if text.is_empty() {
        return None;
    }
    let message = match parse_json(text) {
        Ok(message) => message,
        Err((_, message)) => return Some(error_response(&Json::Null, PARSE_ERROR, &message)),
    };
    let entries = match &message {
        Json::Object(entries) => entries.clone(),
        _ => {
            return Some(error_response(
                &Json::Null,
                INVALID_REQUEST,
                "expected a JSON-RPC object",
            ))
        }
    };
    let id = field_of(&entries, "id").cloned();
    let method = match field_of(&entries, "method") {
        Some(Json::Str(method)) => method.clone(),
        _ => return None, // a client response, or a message with no method
    };
    let params = field_of(&entries, "params").cloned();
    // No id means a notification: it gets no reply.
    let id = id?;
    match method.as_str() {
        "initialize" => Some(response(&id, initialize_result(params.as_ref()))),
        "ping" => Some(response(&id, object(vec![]))),
        "tools/list" => Some(response(&id, tools_list_result())),
        "tools/call" => match tool_call_result(params.as_ref()) {
            Ok(result) => Some(response(&id, result)),
            Err((code, message)) => Some(error_response(&id, code, &message)),
        },
        "resources/list" => Some(response(
            &id,
            object(vec![("resources", Json::Array(Vec::new()))]),
        )),
        "prompts/list" => Some(response(
            &id,
            object(vec![("prompts", Json::Array(Vec::new()))]),
        )),
        other => Some(error_response(
            &id,
            METHOD_NOT_FOUND,
            &format!("unknown method '{other}'"),
        )),
    }
}

fn response(id: &Json, result: Json) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}",
        id.to_compact(),
        result.to_compact()
    )
}

fn error_response(id: &Json, code: i32, message: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":{},\"message\":\"{}\"}}}}",
        id.to_compact(),
        code,
        crate::json_escape(message)
    )
}

fn initialize_result(params: Option<&Json>) -> Json {
    let requested = params
        .and_then(|params| field(params, "protocolVersion"))
        .and_then(|version| match version {
            Json::Str(version) => Some(version.clone()),
            _ => None,
        });
    let protocol = match requested {
        Some(version) if KNOWN_PROTOCOLS.contains(&version.as_str()) => version,
        _ => LATEST_PROTOCOL.to_string(),
    };
    object(vec![
        ("protocolVersion", string(&protocol)),
        (
            "capabilities",
            object(vec![(
                "tools",
                object(vec![("listChanged", Json::Bool(false))]),
            )]),
        ),
        (
            "serverInfo",
            object(vec![
                ("name", string("docconvert-mcp")),
                ("version", string(VERSION)),
            ]),
        ),
        ("instructions", string(INSTRUCTIONS)),
    ])
}

fn tools_list_result() -> Json {
    let tools = parse_json(TOOLS).unwrap_or(Json::Array(Vec::new()));
    object(vec![("tools", tools)])
}

fn tool_call_result(params: Option<&Json>) -> Result<Json, RpcError> {
    let params = params.ok_or((INVALID_PARAMS, "tools/call needs params".to_string()))?;
    let name = match field(params, "name") {
        Some(Json::Str(name)) => name.clone(),
        _ => {
            return Err((
                INVALID_PARAMS,
                "tools/call needs a string \"name\"".to_string(),
            ))
        }
    };
    if !TOOL_NAMES.contains(&name.as_str()) {
        return Err((INVALID_PARAMS, format!("unknown tool '{name}'")));
    }
    let arguments = field(params, "arguments")
        .cloned()
        .unwrap_or(Json::Object(Vec::new()));
    Ok(match run_tool(&name, &arguments) {
        Ok(text) => text_result(&text, false),
        Err((code, message)) => text_result(
            &format!(
                "docconvert: {} (code {code})",
                crate::error_message(&message)
            ),
            true,
        ),
    })
}

fn text_result(text: &str, is_error: bool) -> Json {
    object(vec![
        (
            "content",
            Json::Array(vec![object(vec![
                ("type", string("text")),
                ("text", string(text)),
            ])]),
        ),
        ("isError", Json::Bool(is_error)),
    ])
}

fn run_tool(name: &str, arguments: &Json) -> CResult<String> {
    match name {
        "convert" => tool_convert(arguments),
        "convert_file" => tool_convert_file(arguments),
        "front_matter" => tool_front_matter(arguments),
        "formats" => Ok(convert::formats_json()),
        "matrix" => Ok(convert::matrix_json()),
        "capabilities" => Ok(capabilities_json()),
        other => Err((ERR_INVALID_ARG, format!("unknown tool '{other}'"))),
    }
}

fn tool_convert(arguments: &Json) -> CResult<String> {
    let input = required_string(arguments, "input")?;
    let from = required_string(arguments, "from")?;
    let to = required_string(arguments, "to")?;
    let options = options_from(arguments)?;
    convert::convert_with_options(&input, &from, &to, &options)
}

fn tool_convert_file(arguments: &Json) -> CResult<String> {
    let input_path = required_string(arguments, "input_path")?;
    let output_path = required_string(arguments, "output_path")?;
    let from = match optional_string(arguments, "from")? {
        Some(from) => from,
        None => convert::format_for_path(&input_path).ok_or((
            ERR_INVALID_ARG,
            format!("cannot detect the input format from '{input_path}' — pass \"from\""),
        ))?,
    };
    let to = match optional_string(arguments, "to")? {
        Some(to) => to,
        None => convert::format_for_path(&output_path).ok_or((
            ERR_INVALID_ARG,
            format!("cannot detect the output format from '{output_path}' — pass \"to\""),
        ))?,
    };
    let options = options_from(arguments)?;
    let text = String::from_utf8(platform::read_file(&input_path)?).map_err(|_| {
        (
            ERR_CORRUPT,
            format!("'{input_path}' is not valid UTF-8 text"),
        )
    })?;
    let converted = convert::convert_with_options(&text, &from, &to, &options)?;
    platform::write_file(&output_path, converted.as_bytes())?;
    Ok(object(vec![
        ("written", string(&output_path)),
        ("bytes", Json::Num(converted.len().to_string())),
        ("from", string(&from)),
        ("to", string(&to)),
    ])
    .to_compact())
}

fn tool_front_matter(arguments: &Json) -> CResult<String> {
    let input = match optional_string(arguments, "input")? {
        Some(input) => input,
        None => {
            let path = required_string(arguments, "path")?;
            String::from_utf8(platform::read_file(&path)?)
                .map_err(|_| (ERR_CORRUPT, format!("'{path}' is not valid UTF-8 text")))?
        }
    };
    let (front, _body) = crate::frontmatter::split(&input);
    Ok(match front {
        Some(front) => front.to_json().to_compact(),
        None => "{}".to_string(),
    })
}

fn options_from(arguments: &Json) -> CResult<Options> {
    let mut options = Options::default();
    if let Some(mode) = optional_string(arguments, "front_matter")? {
        options.front_matter = FrontMatterMode::parse(&mode)?;
    }
    Ok(options)
}

fn capabilities_json() -> String {
    let tools: Vec<Json> = TOOL_NAMES.iter().map(|name| string(name)).collect();
    object(vec![
        ("name", string("docconvert-mcp")),
        ("version", string(VERSION)),
        ("protocol", string(LATEST_PROTOCOL)),
        ("tools", Json::Array(tools)),
        (
            "options",
            object(vec![(
                "front_matter",
                Json::Array(vec![string("strip"), string("table")]),
            )]),
        ),
        ("formats", parsed_or(convert::formats_json())),
        ("matrix", parsed_or(convert::matrix_json())),
        ("error_codes", parsed_or(convert::error_codes_json())),
    ])
    .to_compact()
}

/// Re-parse a payload built elsewhere so it nests as real JSON rather
/// than as a quoted string.
fn parsed_or(payload: String) -> Json {
    parse_json(&payload).unwrap_or(Json::Null)
}

fn object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn string(text: &str) -> Json {
    Json::Str(text.to_string())
}

fn field<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Json::Object(entries) => entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn field_of<'a>(entries: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    entries
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn required_string(arguments: &Json, key: &str) -> CResult<String> {
    match field(arguments, key) {
        Some(Json::Str(text)) => Ok(text.clone()),
        Some(_) => Err((ERR_INVALID_ARG, format!("\"{key}\" must be a string"))),
        None => Err((
            ERR_INVALID_ARG,
            format!("missing required argument \"{key}\""),
        )),
    }
}

fn optional_string(arguments: &Json, key: &str) -> CResult<Option<String>> {
    match field(arguments, key) {
        Some(Json::Str(text)) => Ok(Some(text.clone())),
        Some(_) => Err((ERR_INVALID_ARG, format!("\"{key}\" must be a string"))),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: i32, method: &str) -> String {
        format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{{}}}}")
    }

    fn parsed(response: Option<String>) -> Json {
        parse_json(&response.expect("a response")).expect("valid JSON")
    }

    fn result(response: &Json) -> &Json {
        field(response, "result").expect("result field")
    }

    fn text_of(response: &Json) -> String {
        let content = field(result(response), "content").expect("content");
        match content {
            Json::Array(items) => match items.first() {
                Some(item) => match field(item, "text") {
                    Some(Json::Str(text)) => text.clone(),
                    other => panic!("no text in {other:?}"),
                },
                None => panic!("empty content"),
            },
            other => panic!("content should be an array, got {other:?}"),
        }
    }

    #[test]
    fn tools_payload_is_well_formed() {
        let tools = parse_json(TOOLS).expect("tools JSON");
        match tools {
            Json::Array(items) => {
                assert_eq!(items.len(), TOOL_NAMES.len());
                for (item, expected) in items.iter().zip(TOOL_NAMES.iter()) {
                    match field(item, "name") {
                        Some(Json::Str(name)) => assert_eq!(name, expected),
                        other => panic!("bad tool name {other:?}"),
                    }
                    assert!(field(item, "description").is_some());
                    match field(item, "inputSchema") {
                        Some(schema) => match field(schema, "type") {
                            Some(Json::Str(kind)) => assert_eq!(kind, "object"),
                            other => panic!("bad schema type {other:?}"),
                        },
                        None => panic!("tool {expected} has no inputSchema"),
                    }
                }
            }
            other => panic!("tools must be an array, got {other:?}"),
        }
    }

    #[test]
    fn initialize_negotiates_the_protocol() {
        let request = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":\
                       {\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\
                       \"clientInfo\":{\"name\":\"t\",\"version\":\"0\"}}}";
        let response = parsed(handle_line(request));
        assert_eq!(
            field(result(&response), "protocolVersion"),
            Some(&Json::Str("2024-11-05".into()))
        );
        // An unknown revision falls back to the newest one we speak.
        let request = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":\
                       {\"protocolVersion\":\"1999-01-01\"}}";
        let response = parsed(handle_line(request));
        assert_eq!(
            field(result(&response), "protocolVersion"),
            Some(&Json::Str(LATEST_PROTOCOL.into()))
        );
        let info = field(result(&response), "serverInfo").expect("serverInfo");
        assert_eq!(
            field(info, "name"),
            Some(&Json::Str("docconvert-mcp".into()))
        );
        assert_eq!(field(info, "version"), Some(&Json::Str(VERSION.into())));
    }

    #[test]
    fn notifications_get_no_reply() {
        assert!(
            handle_line("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}").is_none()
        );
        assert!(handle_line(
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\
                              \"params\":{\"requestId\":1}}"
        )
        .is_none());
        assert!(handle_line("   ").is_none());
    }

    #[test]
    fn convert_round_trip() {
        let request = "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":\
                       {\"name\":\"convert\",\"arguments\":{\"input\":\"# Hi\\n\\n**b**\\n\",\
                       \"from\":\"md\",\"to\":\"html\"}}}";
        let response = parsed(handle_line(request));
        assert_eq!(text_of(&response), "<h1>Hi</h1>\n<p><strong>b</strong></p>");
        assert_eq!(
            field(result(&response), "isError"),
            Some(&Json::Bool(false))
        );

        // Front matter kept as a table.
        let request = "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":\
                       {\"name\":\"convert\",\"arguments\":{\"input\":\"---\\ntitle: Hi\\n---\\n\\n# B\\n\",\
                       \"from\":\"md\",\"to\":\"html\",\"front_matter\":\"table\"}}}";
        let response = parsed(handle_line(request));
        assert!(text_of(&response).contains("<td>Hi</td>"));
        assert!(text_of(&response).contains("<h1>B</h1>"));
    }

    #[test]
    fn tool_errors_are_reported_not_thrown() {
        let request = "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":\
                       {\"name\":\"convert\",\"arguments\":{\"input\":\"x\",\"from\":\"text\",\
                       \"to\":\"csv\"}}}";
        let response = parsed(handle_line(request));
        assert_eq!(field(result(&response), "isError"), Some(&Json::Bool(true)));
        assert!(text_of(&response).contains("not supported"));
        assert!(text_of(&response).contains("(code 2)"));

        // Missing arguments are tool errors too, naming the argument.
        let request = "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":\
                       {\"name\":\"convert\",\"arguments\":{\"from\":\"md\",\"to\":\"html\"}}}";
        let response = parsed(handle_line(request));
        assert!(text_of(&response).contains("missing required argument \"input\""));
    }

    #[test]
    fn protocol_errors_are_json_rpc_errors() {
        let error_code = |response: Json| match field(&response, "error") {
            Some(error) => match field(error, "code") {
                Some(Json::Num(code)) => code.clone(),
                other => panic!("bad error code {other:?}"),
            },
            None => panic!("no error in {response:?}"),
        };
        assert_eq!(error_code(parsed(handle_line("{oops"))), "-32700");
        assert_eq!(error_code(parsed(handle_line("[]"))), "-32600");
        assert_eq!(
            error_code(parsed(handle_line(&call(7, "nope/nothing")))),
            "-32601"
        );
        let unknown_tool = "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"tools/call\",\"params\":\
                            {\"name\":\"nope\",\"arguments\":{}}}";
        assert_eq!(error_code(parsed(handle_line(unknown_tool))), "-32602");
    }

    #[test]
    fn introspection_tools_answer() {
        for (tool, key) in [("formats", "formats"), ("matrix", "matrix")] {
            let request = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/call\",\"params\":\
                 {{\"name\":\"{tool}\",\"arguments\":{{}}}}}}"
            );
            let response = parsed(handle_line(&request));
            let payload = parse_json(&text_of(&response)).expect("tool JSON");
            match payload {
                Json::Array(items) => assert!(!items.is_empty(), "{key} should not be empty"),
                other => panic!("{key} should be an array, got {other:?}"),
            }
        }
        let request = "{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"tools/call\",\"params\":\
                       {\"name\":\"capabilities\",\"arguments\":{}}}";
        let response = parsed(handle_line(request));
        let payload = parse_json(&text_of(&response)).expect("capabilities JSON");
        assert!(field(&payload, "error_codes").is_some());
        assert!(field(&payload, "tools").is_some());
    }

    #[test]
    fn serve_writes_one_line_per_request() {
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n\
                     {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
                     {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let mut output: Vec<u8> = Vec::new();
        serve(std::io::Cursor::new(input), &mut output).expect("serve");
        let text = String::from_utf8(output).expect("UTF-8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        for line in lines {
            parse_json(line).expect("each output line is one JSON message");
        }
    }
}
