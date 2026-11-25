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
                "name, version, formats, matrix, options, commands, exit_codes"
            );
        }
        other => panic!("capabilities should be an object, got {other:?}"),
    }

    let help = run(&["--help"], None);
    assert!(help.status.success());
    assert!(out(&help).contains("capabilities"));
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
    let unsupported = run(&["convert", "-f", "markdown", "-t", "csv"], Some("x"));
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
