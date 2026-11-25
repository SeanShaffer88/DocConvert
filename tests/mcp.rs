//! The MCP server, driven the way a local AI client drives it: spawn
//! the binary, speak newline-delimited JSON-RPC on its stdin, read one
//! response line per request.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use docconvert::json::{parse_json, Json};
use docconvert::json_escape;

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Session {
    fn start() -> Session {
        let mut child = Command::new(server_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn docconvert-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Session {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send(&mut self, message: &str) {
        writeln!(self.stdin, "{message}").expect("write message");
        self.stdin.flush().expect("flush");
    }

    fn read(&mut self) -> Json {
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).expect("read response");
        assert!(read > 0, "the server closed stdout");
        parse_json(line.trim()).unwrap_or_else(|error| panic!("{error:?} in line {line:?}"))
    }

    /// A request with params as raw JSON; asserts the echoed id matches.
    fn request(&mut self, method: &str, params: &str) -> Json {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}"
        ));
        let response = self.read();
        assert_eq!(
            field(&response, "id"),
            Some(&Json::Num(id.to_string())),
            "id mismatch in {response:?}"
        );
        response
    }

    fn call(&mut self, tool: &str, arguments: &str) -> Json {
        let params = format!("{{\"name\":\"{tool}\",\"arguments\":{arguments}}}");
        let response = self.request("tools/call", &params);
        assert!(
            field(&response, "error").is_none(),
            "tool {tool} failed at the protocol level: {response:?}"
        );
        field(&response, "result").expect("result").clone()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Cargo hands integration tests the binary's path; the fallback keeps
/// this working if the env var ever goes missing.
fn server_path() -> PathBuf {
    match option_env!("CARGO_BIN_EXE_docconvert-mcp") {
        Some(path) => PathBuf::from(path),
        None => {
            let mut path = std::env::current_exe().expect("test executable");
            path.pop();
            path.pop();
            let name = if cfg!(windows) {
                "docconvert-mcp.exe"
            } else {
                "docconvert-mcp"
            };
            path.join(name)
        }
    }
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

fn text_of(result: &Json) -> String {
    match field(result, "content") {
        Some(Json::Array(items)) => match items.first() {
            Some(item) => match field(item, "text") {
                Some(Json::Str(text)) => text.clone(),
                other => panic!("no text in content: {other:?}"),
            },
            None => panic!("empty content"),
        },
        other => panic!("content should be an array: {other:?}"),
    }
}

fn is_error(result: &Json) -> bool {
    matches!(field(result, "isError"), Some(Json::Bool(true)))
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docconvert-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn as_str(path: &Path) -> String {
    path.to_str().expect("UTF-8 scratch path").to_string()
}

#[test]
fn handshake_and_tool_list() {
    let mut session = Session::start();
    let request = "{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\
                   \"clientInfo\":{\"name\":\"integration\",\"version\":\"0\"}}";
    let response = session.request("initialize", request);
    let result = field(&response, "result").expect("result");
    assert_eq!(
        field(result, "protocolVersion"),
        Some(&Json::Str("2025-06-18".into()))
    );
    let info = field(result, "serverInfo").expect("serverInfo");
    assert_eq!(
        field(info, "name"),
        Some(&Json::Str("docconvert-mcp".into()))
    );
    assert_eq!(
        field(info, "version"),
        Some(&Json::Str(docconvert::VERSION.into()))
    );
    assert!(field(result, "instructions").is_some());

    // A notification must not draw a reply — the very next line belongs
    // to the following request.
    session.send("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}");

    let response = session.request("tools/list", "{}");
    let tools = match field(&response, "result").and_then(|result| field(result, "tools")) {
        Some(Json::Array(tools)) => tools.clone(),
        other => panic!("tools should be an array: {other:?}"),
    };
    let names: Vec<String> = tools
        .iter()
        .map(|tool| match field(tool, "name") {
            Some(Json::Str(name)) => name.clone(),
            other => panic!("bad tool name: {other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        vec![
            "convert",
            "convert_file",
            "front_matter",
            "toc",
            "normalize",
            "formats",
            "matrix",
            "capabilities"
        ]
    );
    for tool in &tools {
        let schema = field(tool, "inputSchema").expect("inputSchema");
        assert_eq!(
            field(schema, "type"),
            Some(&Json::Str("object".into())),
            "schema type for {tool:?}"
        );
    }

    // Ping is part of the protocol's liveness check.
    let pong = session.request("ping", "{}");
    assert!(field(&pong, "result").is_some());
}

#[test]
fn convert_tool_returns_the_document() {
    let mut session = Session::start();
    let result = session.call(
        "convert",
        "{\"input\":\"# Hi\\n\\nSome **bold** text.\\n\",\"from\":\"md\",\"to\":\"html\"}",
    );
    assert!(!is_error(&result));
    assert_eq!(
        text_of(&result),
        "<h1>Hi</h1>\n<p>Some <strong>bold</strong> text.</p>"
    );

    // Front matter is stripped by default and rendered when asked.
    let doc =
        "{\"input\":\"---\\ntitle: Notes\\n---\\n\\n# Body\\n\",\"from\":\"md\",\"to\":\"html\"";
    let stripped = session.call("convert", &format!("{doc}}}"));
    assert_eq!(text_of(&stripped), "<h1>Body</h1>");
    let kept = session.call("convert", &format!("{doc},\"front_matter\":\"table\"}}"));
    assert!(
        text_of(&kept).contains("<td>Notes</td>"),
        "{}",
        text_of(&kept)
    );
}

#[test]
fn front_matter_tool_reads_metadata() {
    let mut session = Session::start();
    let result = session.call(
        "front_matter",
        "{\"input\":\"---\\ntitle: Hi\\ndraft: false\\ntags: [a, b]\\n---\\nBody\\n\"}",
    );
    assert_eq!(
        text_of(&result),
        "{\"title\":\"Hi\",\"draft\":false,\"tags\":[\"a\",\"b\"]}"
    );

    let empty = session.call("front_matter", "{\"input\":\"# no metadata\\n\"}");
    assert_eq!(text_of(&empty), "{}");
}

#[test]
fn convert_file_tool_writes_the_output() {
    let dir = scratch_dir("files");
    let md = dir.join("notes.md");
    let html = dir.join("notes.html");
    std::fs::write(&md, "# Hi\n\nSome **bold** text.\n").expect("write md");

    let mut session = Session::start();
    let arguments = format!(
        "{{\"input_path\":\"{}\",\"output_path\":\"{}\"}}",
        json_escape(&as_str(&md)),
        json_escape(&as_str(&html))
    );
    let result = session.call("convert_file", &arguments);
    assert!(!is_error(&result), "{}", text_of(&result));

    let report = parse_json(&text_of(&result)).expect("report JSON");
    assert_eq!(field(&report, "written"), Some(&Json::Str(as_str(&html))));
    assert_eq!(field(&report, "from"), Some(&Json::Str("markdown".into())));
    assert_eq!(field(&report, "to"), Some(&Json::Str("html".into())));
    assert_eq!(
        std::fs::read_to_string(&html).expect("read html"),
        "<h1>Hi</h1>\n<p>Some <strong>bold</strong> text.</p>"
    );
}

#[test]
fn failures_are_readable_tool_errors() {
    let mut session = Session::start();

    // Unsupported pair: isError plus the code.
    let result = session.call(
        "convert",
        "{\"input\":\"x\",\"from\":\"text\",\"to\":\"csv\"}",
    );
    assert!(is_error(&result));
    assert!(
        text_of(&result).contains("not supported"),
        "{}",
        text_of(&result)
    );
    assert!(text_of(&result).contains("(code 2)"));

    // Missing file: code 3.
    let missing = session.call(
        "convert_file",
        "{\"input_path\":\"definitely-not-here.md\",\"output_path\":\"out.html\"}",
    );
    assert!(is_error(&missing));
    assert!(
        text_of(&missing).contains("(code 3)"),
        "{}",
        text_of(&missing)
    );

    // Protocol-level mistakes are JSON-RPC errors, not tool results.
    let response = session.request("nope/nothing", "{}");
    assert!(field(&response, "error").is_some());

    let unknown_tool = session.request("tools/call", "{\"name\":\"nope\",\"arguments\":{}}");
    assert!(field(&unknown_tool, "error").is_some());

    // The server survives all of that and still answers.
    let alive = session.request("ping", "{}");
    assert!(field(&alive, "result").is_some());
}

#[test]
fn toc_and_normalize_tools() {
    let mut session = Session::start();

    let doc = "# Guide\n\n## Install\n\n### Windows\n\n## Run\n";
    let markdown = session.call("toc", &format!("{{\"input\":\"{}\"}}", json_escape(doc)));
    assert!(!is_error(&markdown), "{markdown:?}");
    assert_eq!(
        text_of(&markdown).trim_end(),
        "- [Guide](#guide)\n  - [Install](#install)\n    - [Windows](#windows)\n  - [Run](#run)"
    );

    let json = session.call(
        "toc",
        &format!("{{\"input\":\"{}\",\"format\":\"json\"}}", json_escape(doc)),
    );
    match parse_json(&text_of(&json)).expect("toc JSON") {
        Json::Array(items) => {
            assert_eq!(items.len(), 4);
            assert_eq!(field(&items[0], "anchor"), Some(&Json::Str("guide".into())));
            assert_eq!(field(&items[2], "level"), Some(&Json::Num("3".into())));
        }
        other => panic!("toc json should be an array: {other:?}"),
    }

    // HTML input read from a file: existing ids survive.
    let dir = scratch_dir("toc");
    let page = dir.join("page.html");
    std::fs::write(&page, "<h2 id=\"keep\">Setup</h2><h2>Usage</h2>\n").expect("write page");
    let from_file = session.call(
        "toc",
        &format!(
            "{{\"path\":\"{}\",\"format\":\"html\"}}",
            json_escape(&as_str(&page))
        ),
    );
    assert!(!is_error(&from_file), "{from_file:?}");
    let markup = text_of(&from_file);
    assert!(markup.contains("href=\"#keep\""), "{markup}");
    assert!(markup.contains("href=\"#usage\""), "{markup}");
    let _ = std::fs::remove_dir_all(&dir);

    // Bad arguments stay inside the error envelope.
    let missing = session.call("toc", "{}");
    assert!(is_error(&missing), "{missing:?}");
    assert!(text_of(&missing).contains("(code 1)"), "{missing:?}");

    let refused = session.call("toc", "{\"input\":\"a,b\\n\",\"from\":\"csv\"}");
    assert!(is_error(&refused), "{refused:?}");
    assert!(text_of(&refused).contains("(code 2)"), "{refused:?}");

    let normalized = session.call(
        "normalize",
        &format!(
            "{{\"input\":\"{}\"}}",
            json_escape("#   Notes\n\n*  one   \n* two\n\n3. first\n7. second\n")
        ),
    );
    assert!(!is_error(&normalized), "{normalized:?}");
    assert_eq!(
        text_of(&normalized).trim_end(),
        "# Notes\n\n- one\n- two\n\n1. first\n2. second"
    );

    let no_input = session.call("normalize", "{}");
    assert!(is_error(&no_input), "{no_input:?}");
}

#[test]
fn document_syntaxes_and_streaming() {
    let mut session = Session::start();

    // reStructuredText arrives as a source.
    let rst = session.call(
        "convert",
        &format!(
            "{{\"input\":\"{}\",\"from\":\"rst\",\"to\":\"html\"}}",
            json_escape("Guide\n=====\n\n* one\n")
        ),
    );
    assert!(!is_error(&rst), "{rst:?}");
    assert!(text_of(&rst).contains("<h1>Guide</h1>"), "{rst:?}");

    // Headings become an OPML outline.
    let opml = session.call(
        "convert",
        "{\"input\":\"# A\\n\\n## B\\n\",\"from\":\"markdown\",\"to\":\"opml\"}",
    );
    assert!(!is_error(&opml), "{opml:?}");
    assert!(text_of(&opml).contains("<outline text=\"B\"/>"), "{opml:?}");

    // convert_file can stream an identity copy.
    let dir = scratch_dir("stream");
    let source = dir.join("notes.md");
    let copy = dir.join("copy.md");
    std::fs::write(&source, "# Title\n\nBody.\n").expect("write source");
    let result = session.call(
        "convert_file",
        &format!(
            "{{\"input_path\":\"{}\",\"output_path\":\"{}\",\"stream\":true}}",
            json_escape(&as_str(&source)),
            json_escape(&as_str(&copy))
        ),
    );
    assert!(!is_error(&result), "{result:?}");
    assert_eq!(
        std::fs::read_to_string(&copy).expect("read copy"),
        "# Title\n\nBody.\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn normalize_tool_takes_a_style() {
    let mut session = Session::start();
    let result = session.call(
        "normalize",
        "{\"input\":\"_soft_\\n\",\"emphasis\":\"underscore\"}",
    );
    assert!(!is_error(&result), "{result:?}");
    assert_eq!(text_of(&result), "_soft_\n");

    let converted = session.call(
        "convert",
        "{\"input\":\"_soft_\\n\",\"from\":\"md\",\"to\":\"html\"}",
    );
    assert!(
        text_of(&converted).contains("<em>soft</em>"),
        "{converted:?}"
    );

    let preset = session.call(
        "normalize",
        "{\"input\":\"_soft_\\n\",\"style\":\"underscore,tilde\"}",
    );
    assert!(!is_error(&preset), "{preset:?}");
    assert_eq!(text_of(&preset), "_soft_\n");

    let bad = session.call("normalize", "{\"input\":\"x\",\"fence\":\"pdf\"}");
    assert!(is_error(&bad), "{bad:?}");
    assert!(text_of(&bad).contains("(code 1)"), "{bad:?}");
}

#[test]
fn introspection_tools_describe_the_build() {
    let mut session = Session::start();

    let formats = session.call("formats", "{}");
    match parse_json(&text_of(&formats)).expect("formats JSON") {
        Json::Array(items) => assert_eq!(items.len(), docconvert::convert::FORMATS.len()),
        other => panic!("formats should be an array: {other:?}"),
    }

    let matrix = session.call("matrix", "{}");
    match parse_json(&text_of(&matrix)).expect("matrix JSON") {
        Json::Array(items) => assert_eq!(items.len(), docconvert::convert::MATRIX.len()),
        other => panic!("matrix should be an array: {other:?}"),
    }

    let capabilities = session.call("capabilities", "{}");
    let payload = parse_json(&text_of(&capabilities)).expect("capabilities JSON");
    assert_eq!(
        field(&payload, "version"),
        Some(&Json::Str(docconvert::VERSION.into()))
    );
    assert!(field(&payload, "matrix").is_some());
    assert!(field(&payload, "error_codes").is_some());
}
