//! `docconvert` — the command line in front of the library, built for
//! scripts and local AI agents: one JSON call (`capabilities`) to learn
//! everything the build can do, stdin/stdout piping, no prompts, and
//! exit codes that mirror the `DC_*` error codes in include/docconvert.h.
//!
//! Deliberately dependency-free: the argument parser below is the
//! whole of it, so the exe stays one small self-contained binary.

use std::io::{Read, Write};
use std::process::ExitCode;

use docconvert::convert::{self, FrontMatterMode, Options};
use docconvert::platform;
use docconvert::{error_message, CResult, ERR_CORRUPT, ERR_INVALID_ARG, ERR_PLATFORM};

const USAGE: &str = r#"docconvert — document conversion on the command line.
Markdown, HTML, CSV, JSON and plain text; the same matrix as the DLL.

USAGE
  docconvert <command> [arguments]

COMMANDS
  convert [flags] [INPUT] [OUTPUT]  convert a document (default: stdin to stdout)
  frontmatter [INPUT]               print YAML front matter as JSON
  toc [flags] [INPUT]               table of contents (markdown, html or json)
  normalize [INPUT] [OUTPUT]        canonical Markdown (style flags below)
  formats                           JSON array of format names
  matrix                            JSON array of supported {from,to} pairs
  capabilities                      everything an agent needs, as one JSON object
  version                           version string
  help                              this text

CONVERT FLAGS
  -f, --from <fmt>        input format (default: from the INPUT extension)
  -t, --to <fmt>          output format (default: from the OUTPUT extension)
  -i, --input <path>      input file ('-' or omitted = stdin)
  -o, --output <path>     output file ('-' or omitted = stdout)
      --front-matter <m>  strip (default) or table
      --heading-ids       id="slug" anchors on md->html headings
      --options <json>    options object, e.g. {"front_matter":"table"}
      --stream        copy identity conversions in chunks (files only)
  Flags also take --flag=value. Aliases: md, mkd, htm, xhtml, tsv, tab,
  txt, plain, rst, rest, adoc, asciidoc, opml.

NORMALIZE FLAGS
      --emphasis <s>      asterisk (default) or underscore
      --fence <s>         backtick (default) or tilde

EXAMPLES
  docconvert convert -f md -t html README.md README.html
  cat notes.md | docconvert convert -f md -t text
  docconvert convert --from=md --to=html --front-matter=table post.md
  docconvert convert -f md -t html --heading-ids --options '{"heading_ids":true}' post.md
  docconvert toc --format html README.md
  docconvert normalize notes.md
  docconvert convert -f rst -t html notes.rst notes.html
  docconvert convert -f md -t opml README.md outline.opml
  docconvert capabilities

EXIT CODES
  0 ok                       1 invalid argument
  2 conversion not supported 3 file not found
  4 access denied            5 platform error
  6 internal error           7 input failed to parse
A successful conversion writes only the document itself — nothing is
added or reformatted. Errors go to stderr as "docconvert: <message>".
"#;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("docconvert: {}", error_message(&message));
            ExitCode::from(code.clamp(0, 255) as u8)
        }
    }
}

fn run(args: &[String]) -> CResult<()> {
    if platform::double_clicked(args) {
        return pause(&platform::double_click_notice());
    }
    if args
        .iter()
        .any(|arg| arg == "-h" || arg == "--help" || arg == "help")
    {
        return print_stdout(USAGE);
    }
    let command = match args.first() {
        Some(command) => command.as_str(),
        None => return print_stdout(USAGE),
    };
    let rest = &args[1..];
    match command {
        "-V" | "--version" | "version" => {
            print_stdout(&format!("docconvert {}\n", docconvert::VERSION))
        }
        "formats" => print_stdout(&with_newline(convert::formats_json())),
        "matrix" => print_stdout(&with_newline(convert::matrix_json())),
        "capabilities" => print_stdout(&with_newline(capabilities())),
        "frontmatter" => cmd_frontmatter(rest),
        "toc" => cmd_toc(rest),
        "normalize" => cmd_normalize(rest),
        "convert" => cmd_convert(rest),
        other => Err(invalid(format!(
            "unknown command '{other}' (try 'docconvert help')"
        ))),
    }
}

/// Everything a script needs to plan conversions, in one object: the
/// formats, the pairs, the options, and what each exit code means.
fn capabilities() -> String {
    let mut out = String::from("{\"name\":\"docconvert\",\"version\":\"");
    out.push_str(docconvert::VERSION);
    out.push_str("\",\"formats\":");
    out.push_str(&convert::formats_json());
    out.push_str(",\"matrix\":");
    out.push_str(&convert::matrix_json());
    out.push_str(
        ",\"options\":{\"front_matter\":[\"strip\",\"table\"],\
         \"heading_ids\":[true,false],\
         \"normalize\":{\"emphasis\":[\"asterisk\",\"underscore\"],\
         \"fence\":[\"backtick\",\"tilde\"]}}",
    );
    out.push_str(
        ",\"commands\":[\"convert\",\"frontmatter\",\"toc\",\"normalize\",\
         \"formats\",\"matrix\",\"capabilities\",\"version\",\"help\"]",
    );
    out.push_str(",\"error_codes\":");
    out.push_str(&convert::error_codes_json());
    out.push('}');
    out
}

fn with_newline(text: String) -> String {
    text + "\n"
}

/// `docconvert toc [--from fmt] [--format markdown|html|json] [INPUT]`
fn cmd_toc(args: &[String]) -> CResult<()> {
    let mut from: Option<String> = None;
    let mut format: Option<String> = None;
    let mut input: Option<String> = None;
    let mut index = 0usize;
    while index < args.len() {
        let raw = &args[index];
        index += 1;
        let (flag, inline_value) = split_flag(raw);
        match flag.as_str() {
            "-f" | "--from" | "--format" | "-t" | "--to" | "-i" | "--input" => {
                let value = match inline_value {
                    Some(value) => value,
                    None => {
                        let value = match args.get(index) {
                            Some(value) => value.clone(),
                            None => return Err(invalid(format!("{flag} needs a value"))),
                        };
                        index += 1;
                        value
                    }
                };
                match flag.as_str() {
                    "-f" | "--from" => from = Some(value),
                    "--format" | "-t" | "--to" => format = Some(value),
                    _ => input = Some(value),
                }
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(invalid(format!(
                    "unknown option '{flag}' (try 'docconvert help')"
                )));
            }
            _ => input = Some(flag.clone()),
        }
    }
    let from = match from {
        Some(from) => from,
        None => match input.as_deref() {
            Some(path) if path != "-" => {
                convert::format_for_path(path).unwrap_or_else(|| "markdown".to_string())
            }
            _ => "markdown".to_string(),
        },
    };
    let text = match input.as_deref() {
        Some(path) if path != "-" => read_file(path)?,
        _ => read_stdin()?,
    };
    let format = format.unwrap_or_else(|| "markdown".to_string());
    let rendered = docconvert::toc::render_from(&text, &from, &format)?;
    print_stdout(&rendered)
}

/// `docconvert normalize [INPUT] [OUTPUT]` — canonical Markdown.
fn cmd_normalize(args: &[String]) -> CResult<()> {
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut style = docconvert::markdown::Style::default();
    let mut index = 0usize;
    while index < args.len() {
        let raw = &args[index];
        index += 1;
        let (flag, inline_value) = split_flag(raw);
        match flag.as_str() {
            "--emphasis" | "--fence" | "-i" | "--input" | "-o" | "--output" => {
                let value = match inline_value {
                    Some(value) => value,
                    None => {
                        let value = match args.get(index) {
                            Some(value) => value.clone(),
                            None => return Err(invalid(format!("{flag} needs a value"))),
                        };
                        index += 1;
                        value
                    }
                };
                match flag.as_str() {
                    "-i" | "--input" => input = Some(value),
                    "-o" | "--output" => output = Some(value),
                    "--emphasis" => style.emphasis = docconvert::markdown::Emphasis::parse(&value)?,
                    _ => style.fence = docconvert::markdown::Fence::parse(&value)?,
                }
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(invalid(format!(
                    "unknown option '{flag}' (try 'docconvert help')"
                )));
            }
            _ => {
                if input.is_none() {
                    input = Some(flag.clone());
                } else if output.is_none() {
                    output = Some(flag.clone());
                } else {
                    return Err(invalid("normalize takes at most two paths"));
                }
            }
        }
    }
    let text = match input.as_deref() {
        Some(path) if path != "-" => read_file(path)?,
        _ => read_stdin()?,
    };
    let normalized = docconvert::markdown::normalize_with(&text, style)?;
    match output.as_deref() {
        Some(path) if path != "-" => platform::write_file(path, normalized.as_bytes()),
        _ => print_stdout(&normalized),
    }
}

fn cmd_frontmatter(args: &[String]) -> CResult<()> {
    if args.len() > 1 {
        return Err(invalid("frontmatter takes at most one input path"));
    }
    let path = args.first().map(String::as_str);
    let option = path.filter(|flag| flag.starts_with('-') && *flag != "-");
    if let Some(flag) = option {
        return Err(invalid(format!("unknown option '{flag}'")));
    }
    let input = match path {
        Some(path) if path != "-" => read_file(path)?,
        _ => read_stdin()?,
    };
    let (front, _body) = docconvert::frontmatter::split(&input);
    let json = match front {
        Some(front) => front.to_json().to_compact(),
        None => "{}".to_string(),
    };
    print_stdout(&with_newline(json))
}

#[derive(Debug, Default)]
struct ConvertArgs {
    heading_ids: bool,
    stream: bool,
    input: Option<String>,
    output: Option<String>,
    from: Option<String>,
    to: Option<String>,
    options_json: Option<String>,
    front_matter: Option<String>,
}

fn cmd_convert(raw_args: &[String]) -> CResult<()> {
    let args = parse_convert_args(raw_args)?;
    let from = match args.from {
        Some(from) => from,
        None => match args.input.as_deref() {
            Some(path) if path != "-" => convert::format_for_path(path).ok_or_else(|| {
                invalid(format!(
                    "cannot detect the input format from '{path}' — pass --from"
                ))
            })?,
            _ => return Err(invalid("--from is required when reading stdin")),
        },
    };
    let to = match args.to {
        Some(to) => to,
        None => match args.output.as_deref() {
            Some(path) if path != "-" => convert::format_for_path(path).ok_or_else(|| {
                invalid(format!(
                    "cannot detect the output format from '{path}' — pass --to"
                ))
            })?,
            _ => return Err(invalid("--to is required when writing to stdout")),
        },
    };
    let mut options = match args.options_json {
        Some(json) => Options::from_json(&json)?,
        None => Options::default(),
    };
    if let Some(mode) = args.front_matter {
        options.front_matter = FrontMatterMode::parse(&mode)?;
    }
    if args.heading_ids {
        options.heading_ids = true;
    }
    if args.stream {
        // Streaming is a file-to-file path: identity conversions are
        // copied in chunks, everything else parses as usual.
        return match (args.input.as_deref(), args.output.as_deref()) {
            (Some(from_path), Some(to_path)) if from_path != "-" && to_path != "-" => {
                convert::convert_file_streaming(
                    from_path,
                    to_path,
                    Some(from.as_str()),
                    Some(to.as_str()),
                )
                .map(|_| ())
            }
            _ => Err(invalid("--stream needs an input and an output path")),
        };
    }
    let input = match args.input.as_deref() {
        Some(path) if path != "-" => read_file(path)?,
        _ => read_stdin()?,
    };
    let converted = convert::convert_with_options(&input, &from, &to, &options)?;
    match args.output.as_deref() {
        Some(path) if path != "-" => platform::write_file(path, converted.as_bytes()),
        _ => print_stdout(&converted),
    }
}

fn parse_convert_args(args: &[String]) -> CResult<ConvertArgs> {
    let mut parsed = ConvertArgs::default();
    let mut positionals: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let raw = &args[index];
        index += 1;
        if raw == "--" {
            positionals.extend(args[index..].iter().cloned());
            break;
        }
        let (flag, inline_value) = split_flag(raw);
        let takes_value = matches!(
            flag.as_str(),
            "-f" | "--from"
                | "-t"
                | "--to"
                | "-i"
                | "--input"
                | "-o"
                | "--output"
                | "--options"
                | "--front-matter"
        );
        if takes_value {
            let value = match inline_value {
                Some(value) => value,
                None => {
                    if index >= args.len() {
                        return Err(invalid(format!("{flag} needs a value")));
                    }
                    let value = args[index].clone();
                    index += 1;
                    value
                }
            };
            match flag.as_str() {
                "-f" | "--from" => parsed.from = Some(value),
                "-t" | "--to" => parsed.to = Some(value),
                "-o" | "--output" => parsed.output = Some(value),
                "--options" => parsed.options_json = Some(value),
                "--front-matter" => parsed.front_matter = Some(value),
                _ => parsed.input = Some(value),
            }
        } else if flag == "--heading-ids" {
            parsed.heading_ids = true;
        } else if flag == "--stream" {
            parsed.stream = true;
        } else if flag.starts_with('-') && flag != "-" {
            return Err(invalid(format!(
                "unknown option '{flag}' (try 'docconvert help')"
            )));
        } else {
            positionals.push(flag);
        }
    }
    let mut positionals = positionals.into_iter();
    if let Some(first) = positionals.next() {
        if parsed.input.is_some() {
            return Err(invalid("input given twice (positional and --input)"));
        }
        parsed.input = Some(first);
        if let Some(second) = positionals.next() {
            if parsed.output.is_some() {
                return Err(invalid("output given twice (positional and --output)"));
            }
            parsed.output = Some(second);
            if let Some(extra) = positionals.next() {
                return Err(invalid(format!("unexpected argument '{extra}'")));
            }
        }
    }
    Ok(parsed)
}

/// Split `--flag=value` (and `-f=value`) into its two halves.
fn split_flag(raw: &str) -> (String, Option<String>) {
    if raw.starts_with('-') && raw != "-" {
        if let Some(eq) = raw.find('=') {
            return (raw[..eq].to_string(), Some(raw[eq + 1..].to_string()));
        }
    }
    (raw.to_string(), None)
}

fn invalid(message: impl Into<String>) -> (i32, String) {
    (ERR_INVALID_ARG, message.into())
}

fn read_stdin() -> CResult<String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|e| (ERR_PLATFORM, format!("cannot read stdin: {e}")))?;
    String::from_utf8(bytes).map_err(|_| (ERR_CORRUPT, "stdin is not valid UTF-8".to_string()))
}

fn read_file(path: &str) -> CResult<String> {
    let bytes = platform::read_file(path)?;
    String::from_utf8(bytes).map_err(|_| (ERR_CORRUPT, format!("'{path}' is not valid UTF-8 text")))
}

/// Write the document to stdout exactly as produced. A closed pipe
/// (`docconvert … | head`) is a normal way for the reader to stop, not
/// an error worth a non-zero exit.
/// Print a notice meant for a human who double-clicked the exe, then
/// wait for Enter so the console window stays up long enough to read it.
fn pause(notice: &str) -> CResult<()> {
    print_stdout(notice)?;
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    Ok(())
}

fn print_stdout(text: &str) -> CResult<()> {
    let mut stdout = std::io::stdout();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|_| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err((ERR_PLATFORM, format!("cannot write to stdout: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_positionals() {
        let parsed = parse_convert_args(&args(&["-f", "md", "--to=html", "in.md", "out.html"]))
            .expect("parse");
        assert_eq!(parsed.from.as_deref(), Some("md"));
        assert_eq!(parsed.to.as_deref(), Some("html"));
        assert_eq!(parsed.input.as_deref(), Some("in.md"));
        assert_eq!(parsed.output.as_deref(), Some("out.html"));

        let parsed = parse_convert_args(&args(&["--input=-", "--options", "{}"])).expect("parse");
        assert_eq!(parsed.input.as_deref(), Some("-"));
        assert_eq!(parsed.options_json.as_deref(), Some("{}"));
    }

    #[test]
    fn rejects_nonsense() {
        assert!(parse_convert_args(&args(&["--nope"])).is_err());
        assert!(parse_convert_args(&args(&["-f"])).is_err());
        assert!(parse_convert_args(&args(&["a.md", "b.html", "c.txt"])).is_err());
        assert!(parse_convert_args(&args(&["-i", "a.md", "b.md"])).is_err());
        assert!(parse_convert_args(&args(&["-f", "md", "-f", "html"])).is_ok());
    }

    #[test]
    fn capabilities_is_valid_json_with_the_matrix() {
        let value = docconvert::json::parse_json(&capabilities()).expect("capabilities JSON");
        match value {
            docconvert::json::Json::Object(entries) => {
                assert!(entries.iter().any(|(key, _)| key == "matrix"));
                assert!(entries.iter().any(|(key, _)| key == "error_codes"));
            }
            other => panic!("capabilities should be an object, got {other:?}"),
        }
    }
}
