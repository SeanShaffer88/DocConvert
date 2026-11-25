//! `docconvert-mcp` — DocConvert as a Model Context Protocol server
//! over stdio, so a local AI client can call the conversion matrix as
//! tools.
//!
//! Clients spawn this with no arguments and speak newline-delimited
//! JSON-RPC on stdin/stdout; `serve` owns that loop. Anything meant
//! for a human goes to stderr, never into the protocol stream.

use std::io::{self, BufReader, BufWriter};
use std::process::ExitCode;

const USAGE: &str = "\
docconvert-mcp — Model Context Protocol server (stdio, JSON-RPC 2.0).

An MCP client launches it with no arguments and speaks the protocol on
stdin/stdout; there is nothing to start manually.

Register it with your client, for example:

  {
    \"mcpServers\": {
      \"docconvert\": { \"command\": \"C:\\\\path\\\\to\\\\docconvert-mcp.exe\" }
    }
  }

Tools: convert, convert_file, front_matter, formats, matrix,
capabilities.

  docconvert-mcp --help      this text (stderr-free, stdout)
  docconvert-mcp --version   version string
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("docconvert-mcp {}", docconvert::VERSION);
        return ExitCode::SUCCESS;
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    match docconvert::mcp::serve(BufReader::new(stdin.lock()), BufWriter::new(stdout.lock())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("docconvert-mcp: {error}");
            ExitCode::from(1)
        }
    }
}
