//! The command line front end, driven the way a script or a local AI
//! agent drives it: piped stdin/stdout, JSON introspection, silent
//! success, and exit codes callers can switch on.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use docconvert::convert::{formats_json, MATRIX};
use docconvert::json::{parse_json, Json};

fn run(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_docconvert"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn docconvert");
    {
        let mut pipe = child.stdin.take().expect("stdin");
        if let Some(text) = stdin {
            pipe.write_all(text.as_bytes()).expect("write stdin");
        }
    }
    child.wait_with_output().expect("wait for docconvert")
}

fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docconvert-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn as_str(path: &Path) -> &str {
    path.to_str().expect("UTF-8 scratch path")
}

#[test]
fn introspection_is_machine_readable() {
    let version = run(&["version"], None);
    assert!(version.status.success(), "{}", err(&version));
    assert_eq!(
        out(&version).trim(),
        format!("docconvert {}", docconvert::VERSION)
    );

    let formats = run(&["formats"], None);
    assert!(formats.status.success());
    let expected = Json::Array(
        docconvert::convert::FORMATS
            .iter()
            .map(|name| Json::Str(name.to_string()))
            .collect(),
    );
    assert_eq!(parse_json(out(&formats).trim()).unwrap(), expected);

    let matrix = run(&["matrix"], None);
    assert!(matrix.status.success());
    match parse_json(out(&matrix).trim()).unwrap() {
        Json::Array(pairs) => assert_eq!(pairs.len(), MATRIX.len()),
        other => panic!("matrix should be an array, got {other:?}"),
    }

    // One call tells an agent everything: formats, pairs, options, exits.
    let capabilities = run(&["capabilities"], None);
    assert!(capabilities.status.success());
    let text = out(&capabilities);
    assert_eq!(text.trim_end(), text.trim(), "should end with one newline");
    assert!(text.contains(&format!("\"version\":\"{}\"", docconvert::VERSION)));
    assert!(text.contains(&format!("\"formats\":{}", formats_json())));
    assert!(text.contains("\"front_matter\":[\"strip\",\"table\"]"));
    match parse_json(text.trim()).unwrap() {
        Json::Object(entries) => {
            assert_eq!(
                entries.len(),
                7,
                "name, version, formats, matrix, options, commands, error_codes"
            );
        }
        other => panic!("capabilities should be an object, got {other:?}"),
    }

    let help = run(&["--help"], None);
    assert!(help.status.success());
    assert!(out(&help).contains("capabilities"));

    // A bare invocation is the usage text, not a wait for Enter: only a
    // console that exists for this process alone (a double-click) pauses.
    let bare = run(&[], None);
    assert!(bare.status.success());
    assert!(out(&bare).contains("USAGE"));
    assert!(!out(&bare).contains("Press Enter to close"));
}

#[test]
fn converts_through_pipes() {
    let converted = run(
        &["convert", "-f", "md", "-t", "html"],
        Some("# Hi\n\n**bold**\n"),
    );
    assert!(converted.status.success(), "{}", err(&converted));
    assert_eq!(out(&converted), "<h1>Hi</h1>\n<p><strong>bold</strong></p>");
    assert!(err(&converted).is_empty(), "{}", err(&converted));

    // Explicit '-' means stdin/stdout, exactly like omitting both.
    let converted = run(
        &[
            "convert",
            "--from=md",
            "--to=markdown",
            "--input=-",
            "--output=-",
        ],
        Some("# Hi\n"),
    );
    assert!(converted.status.success(), "{}", err(&converted));
    assert_eq!(out(&converted), "# Hi\n");

    // Byte-faithful: CSV's CRLF line endings survive a round trip.
    let converted = run(
        &["convert", "-f", "csv", "-t", "text"],
        Some("a,b\r\n1,2\r\n"),
    );
    assert!(converted.status.success(), "{}", err(&converted));
    assert_eq!(out(&converted), "a,b\r\n1,2\r\n");
}

#[test]
fn front_matter_on_the_command_line() {
    let doc = "---\ntitle: Notes\ntags: [a, b]\n---\n\n# Body\n";

    // Default: stripped.
    let stripped = run(&["convert", "-f", "md", "-t", "html"], Some(doc));
    assert!(stripped.status.success(), "{}", err(&stripped));
    assert_eq!(out(&stripped), "<h1>Body</h1>");

    // Opted in via the convenience flag, and via a raw options object.
    for extra in [
        vec!["--front-matter", "table"],
        vec!["--options", "{\"front_matter\":\"table\"}"],
    ] {
        let mut args = vec!["convert", "-f", "md", "-t", "html"];
        args.extend(extra);
        let kept = run(&args, Some(doc));
        assert!(kept.status.success(), "{}", err(&kept));
        assert!(out(&kept).contains("<td>Notes</td>"), "{}", out(&kept));
    }

    // The metadata alone, as JSON.
    let meta = run(&["frontmatter"], Some(doc));
    assert!(meta.status.success(), "{}", err(&meta));
    assert_eq!(
        out(&meta).trim(),
        "{\"title\":\"Notes\",\"tags\":[\"a\",\"b\"]}"
    );

    let none = run(&["frontmatter"], Some("# no metadata\n"));
    assert_eq!(out(&none).trim(), "{}");
}

#[test]
fn table_of_contents_and_normalization() {
    let doc = "# Guide\n\n## Install\n\n### Windows\n\n## Run\n";

    let markdown = run(&["toc"], Some(doc));
    assert!(markdown.status.success(), "{}", err(&markdown));
    assert_eq!(
        out(&markdown).trim_end(),
        "- [Guide](#guide)\n  - [Install](#install)\n    - [Windows](#windows)\n  - [Run](#run)"
    );

    // `-t` is the same switch as `--format`.
    let json = run(&["toc", "--format", "json"], Some(doc));
    assert!(json.status.success(), "{}", err(&json));
    assert_eq!(
        out(&json).trim_end(),
        "[{\"level\":1,\"text\":\"Guide\",\"anchor\":\"guide\"},\
         {\"level\":2,\"text\":\"Install\",\"anchor\":\"install\"},\
         {\"level\":3,\"text\":\"Windows\",\"anchor\":\"windows\"},\
         {\"level\":2,\"text\":\"Run\",\"anchor\":\"run\"}]"
    );

    // HTML input keeps the ids the document already carries.
    let page = "<h1>Top</h1><h2 id=\"keep-me\">Setup</h2><h2>Usage</h2>";
    let html = run(&["toc", "--from", "html", "-t", "html"], Some(page));
    assert!(html.status.success(), "{}", err(&html));
    let markup = out(&html);
    assert!(
        markup.contains("<a href=\"#keep-me\">Setup</a>"),
        "{markup}"
    );
    assert!(markup.contains("<a href=\"#usage\">Usage</a>"), "{markup}");
    assert_eq!(markup.matches("<ul>").count(), 2, "{markup}");
    assert_eq!(markup.matches("</ul>").count(), 2, "{markup}");

    // A document without headings has an empty table of contents.
    let empty = run(&["toc"], Some("just a paragraph\n"));
    assert!(empty.status.success(), "{}", err(&empty));
    assert_eq!(out(&empty), "");

    // Sources that carry no headings are refused, not guessed at.
    let refused = run(&["toc", "--from", "csv"], Some("a,b\n1,2\n"));
    assert_eq!(refused.status.code(), Some(2), "{}", err(&refused));
    assert!(
        err(&refused).starts_with("docconvert: "),
        "{}",
        err(&refused)
    );

    // Markdown in, canonical Markdown out — and normalizing twice is a
    // no-op, so the pass is stable.
    let messy = "#   Notes\n\n*  one   \n* two\n\n3. first\n7. second\n";
    let once = run(&["normalize"], Some(messy));
    assert!(once.status.success(), "{}", err(&once));
    assert_eq!(
        out(&once).trim_end(),
        "# Notes\n\n- one\n- two\n\n1. first\n2. second"
    );
    let twice_input = out(&once);
    let twice = run(&["normalize"], Some(twice_input.as_str()));
    assert_eq!(out(&twice), out(&once));
}

#[test]
fn heading_ids_are_opt_in() {
    let doc = "# Guide\n\n## Install\n";
    let plain = run(&["convert", "-f", "md", "-t", "html"], Some(doc));
    assert_eq!(out(&plain), "<h1>Guide</h1>\n<h2>Install</h2>");

    for extra in [
        vec!["--heading-ids"],
        vec!["--options", "{\"heading_ids\":true}"],
    ] {
        let mut args = vec!["convert", "-f", "md", "-t", "html"];
        args.extend(extra);
        let ids = run(&args, Some(doc));
        assert!(ids.status.success(), "{}", err(&ids));
        assert_eq!(
            out(&ids),
            "<h1 id=\"guide\">Guide</h1>\n<h2 id=\"install\">Install</h2>"
        );
    }

    // The anchors agree with the table of contents.
    let toc = run(&["toc"], Some(doc));
    assert_eq!(
        out(&toc).trim_end(),
        "- [Guide](#guide)\n  - [Install](#install)"
    );
}

#[test]
fn document_syntaxes_and_streaming_on_the_command_line() {
    // reStructuredText and AsciiDoc are sources; their headings are
    // ordinary Markdown headings afterwards.
    let rst = "Guide\n=====\n\nInstall\n-------\n\n* one\n* two\n";
    let html = run(&["convert", "-f", "rst", "-t", "html"], Some(rst));
    assert!(html.status.success(), "{}", err(&html));
    assert!(out(&html).contains("<h1>Guide</h1>"), "{}", out(&html));
    assert!(out(&html).contains("<li>one</li>"), "{}", out(&html));

    let adoc = "= Guide\n\n== Install\n";
    let toc = run(&["toc", "--from", "adoc"], Some(adoc));
    assert!(toc.status.success(), "{}", err(&toc));
    assert_eq!(
        out(&toc).trim_end(),
        "- [Guide](#guide)\n  - [Install](#install)"
    );

    // Headings become an OPML outline, and an outline becomes bullets.
    let opml = run(
        &["convert", "-f", "md", "-t", "opml"],
        Some("# A\n\n## B\n"),
    );
    assert!(opml.status.success(), "{}", err(&opml));
    let outline = out(&opml);
    assert!(outline.contains("<opml version=\"2.0\">"), "{outline}");
    let back = run(
        &["convert", "-f", "opml", "-t", "md"],
        Some(outline.as_str()),
    );
    assert_eq!(out(&back).trim_end(), "- A\n  - B");

    // --stream needs two paths; identity conversions are copied.
    let dir = scratch_dir("stream");
    let source = dir.join("notes.md");
    let copy = dir.join("copy.md");
    let body = "# Title\n\nBody text.\n";
    std::fs::write(&source, body).expect("write source");
    let streamed = run(
        &[
            "convert",
            "--stream",
            "-f",
            "md",
            "-t",
            "md",
            as_str(&source),
            as_str(&copy),
        ],
        None,
    );
    assert!(streamed.status.success(), "{}", err(&streamed));
    assert_eq!(std::fs::read_to_string(&copy).expect("read copy"), body);
    assert_eq!(out(&streamed), "");

    let no_paths = run(&["convert", "--stream", "-f", "md", "-t", "md"], Some(body));
    assert_eq!(no_paths.status.code(), Some(1), "{}", err(&no_paths));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn normalize_styles_and_streamed_blocks() {
    let doc = "_soft_ and __loud__\n\n```\ncode\n```\n";
    let plain = run(&["normalize"], Some(doc));
    assert_eq!(out(&plain), "*soft* and **loud**\n\n```\ncode\n```\n");

    let styled = run(
        &["normalize", "--emphasis", "underscore", "--fence=tilde"],
        Some(doc),
    );
    assert!(styled.status.success(), "{}", err(&styled));
    assert_eq!(out(&styled), "_soft_ and __loud__\n\n~~~\ncode\n~~~\n");

    let bad = run(&["normalize", "--fence", "pdf"], Some(doc));
    assert_eq!(bad.status.code(), Some(1), "{}", err(&bad));

    // Streaming: text and markdown both stream block by block, and the
    // result is exactly what the whole-document path produces.
    let dir = scratch_dir("blocks");
    let text = dir.join("notes.txt");
    let html = dir.join("notes.html");
    let markdown = dir.join("notes.md");
    let html_out = dir.join("notes.out.html");
    std::fs::write(&text, "one\ntwo\n\nthree\n").expect("write text");
    let streamed = run(
        &[
            "convert",
            "--stream",
            "-f",
            "text",
            "-t",
            "html",
            as_str(&text),
            as_str(&html_out),
        ],
        None,
    );
    assert!(streamed.status.success(), "{}", err(&streamed));
    assert_eq!(
        std::fs::read_to_string(&html_out).expect("read"),
        "<p>one<br>\ntwo</p>\n<p>three</p>\n"
    );

    std::fs::write(&markdown, "# Title\n\nBody\n\n- one\n- two\n").expect("write md");
    let streamed = run(
        &[
            "convert",
            "--stream",
            "-f",
            "md",
            "-t",
            "html",
            as_str(&markdown),
            as_str(&html),
        ],
        None,
    );
    assert!(streamed.status.success(), "{}", err(&streamed));
    assert_eq!(
        std::fs::read_to_string(&html).expect("read"),
        "<h1>Title</h1>\n<p>Body</p>\n<ul>\n<li>one</li>\n<li>two</li>\n</ul>"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn converts_files_and_stays_quiet() {
    let dir = scratch_dir("files");
    let md = dir.join("notes.md");
    let html = dir.join("notes.html");
    std::fs::write(&md, "# Hi\n\nSome **bold** text.\n").expect("write md");

    let converted = run(&["convert", as_str(&md), as_str(&html)], None);
    assert!(converted.status.success(), "{}", err(&converted));
    assert!(out(&converted).is_empty(), "file conversions are silent");
    assert_eq!(
        std::fs::read_to_string(&html).expect("read html"),
        "<h1>Hi</h1>\n<p>Some <strong>bold</strong> text.</p>"
    );

    // `frontmatter` also accepts a path.
    std::fs::write(&md, "---\ntitle: File\n---\n\nBody\n").expect("rewrite md");
    let meta = run(&["frontmatter", as_str(&md)], None);
    assert!(meta.status.success(), "{}", err(&meta));
    assert_eq!(out(&meta).trim(), "{\"title\":\"File\"}");
}

#[test]
fn exit_codes_mirror_the_dc_error_codes() {
    // 2: a pair the matrix does not contain.
    let unsupported = run(&["convert", "-f", "text", "-t", "csv"], Some("x"));
    assert_eq!(unsupported.status.code(), Some(2));
    assert!(
        err(&unsupported).contains("not supported"),
        "{}",
        err(&unsupported)
    );

    // 1: bad arguments — unknown format, command, or flag.
    for args in [
        vec!["convert", "-f", "nope", "-t", "html"],
        vec!["frobnicate"],
        vec!["convert", "--nope"],
        vec!["convert"],
    ] {
        let failed = run(&args, Some("x"));
        assert_eq!(failed.status.code(), Some(1), "args {args:?}");
        assert!(err(&failed).starts_with("docconvert: "), "{}", err(&failed));
    }

    // 3: the input file does not exist.
    let missing = run(
        &[
            "convert",
            "-f",
            "md",
            "-t",
            "html",
            "definitely-not-here.md",
        ],
        None,
    );
    assert_eq!(missing.status.code(), Some(3));

    // 7: input that does not parse.
    let corrupt = run(&["convert", "-f", "json", "-t", "csv"], Some("{not json}"));
    assert_eq!(corrupt.status.code(), Some(7));
}

#[test]
fn rejects_undetectable_formats() {
    // stdin without --from is an argument error, not a guess.
    let no_from = run(&["convert", "-t", "html"], Some("# Hi\n"));
    assert_eq!(no_from.status.code(), Some(1));
    assert!(err(&no_from).contains("--from"), "{}", err(&no_from));

    // stdout without --to likewise.
    let no_to = run(&["convert", "-f", "md"], Some("# Hi\n"));
    assert_eq!(no_to.status.code(), Some(1));
    assert!(err(&no_to).contains("--to"), "{}", err(&no_to));
}
