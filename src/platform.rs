//! Platform layer for DocConvert: deliberately thin — the parser,
//! extractor, and writer are pure Rust; only file I/O error mapping
//! lives here.

use std::io::{Read, Write};

use crate::{CResult, ERR_ACCESS_DENIED, ERR_NOT_FOUND, ERR_PLATFORM};

/// The chunk size for [`copy_file`] — small enough to stay off the heap
/// and large enough to keep syscalls rare.
const COPY_BUFFER: usize = 64 * 1024;

/// Whether the console this process is attached to was created for it
/// alone — the fingerprint of a double-click in Explorer, where the
/// window disappears the instant the program exits.
///
/// A shell keeps its own process attached, so `docconvert help` pasted in
/// PowerShell (or any other terminal, including Windows Terminal, whose
/// host process is attached too) reports `false`. Only Windows can tell;
/// elsewhere this is always `false`, which is why the notice it drives is
/// a Windows nicety rather than part of the contract.
pub fn console_is_fresh() -> bool {
    #[cfg(windows)]
    {
        // Declared here, not pulled in as a dependency: one function is
        // all this crate needs from kernel32, and the crate has none.
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        }
        // Two slots are enough: one process means "nobody else is using
        // this console"; anything more fills both and reports the real
        // count, which is all we compare against.
        let mut attached = [0u32; 2];
        let count = unsafe { GetConsoleProcessList(attached.as_mut_ptr(), attached.len() as u32) };
        count == 1 && attached[0] == std::process::id()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn read_file(path: &str) -> CResult<Vec<u8>> {
    std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => (ERR_NOT_FOUND, format!("file not found: '{path}'")),
        std::io::ErrorKind::PermissionDenied => (
            ERR_ACCESS_DENIED,
            format!("permission denied reading '{path}'"),
        ),
        _ => (ERR_PLATFORM, format!("cannot read '{path}': {e}")),
    })
}

/// Copy one file to another in fixed-size chunks, so an identity
/// conversion never holds the document in memory. Returns the bytes
/// copied.
pub fn copy_file(input: &str, output: &str) -> CResult<u64> {
    let mut source = std::fs::File::open(input).map_err(|e| open_error(input, &e))?;
    let mut target = std::fs::File::create(output).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => (
            ERR_ACCESS_DENIED,
            format!("permission denied writing '{output}'"),
        ),
        _ => (ERR_PLATFORM, format!("cannot write '{output}': {e}")),
    })?;
    let mut buffer = vec![0u8; COPY_BUFFER];
    let mut total = 0u64;
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|e| (ERR_PLATFORM, format!("cannot read '{input}': {e}")))?;
        if read == 0 {
            break;
        }
        target
            .write_all(&buffer[..read])
            .map_err(|e| (ERR_PLATFORM, format!("cannot write '{output}': {e}")))?;
        total += read as u64;
    }
    target
        .flush()
        .map_err(|e| (ERR_PLATFORM, format!("cannot write '{output}': {e}")))?;
    Ok(total)
}

fn open_error(path: &str, error: &std::io::Error) -> (i32, String) {
    match error.kind() {
        std::io::ErrorKind::NotFound => (ERR_NOT_FOUND, format!("file not found: '{path}'")),
        std::io::ErrorKind::PermissionDenied => (
            ERR_ACCESS_DENIED,
            format!("permission denied reading '{path}'"),
        ),
        _ => (ERR_PLATFORM, format!("cannot read '{path}': {error}")),
    }
}

pub fn write_file(path: &str, bytes: &[u8]) -> CResult<()> {
    std::fs::write(path, bytes).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => (
            ERR_ACCESS_DENIED,
            format!("permission denied writing '{path}'"),
        ),
        _ => (ERR_PLATFORM, format!("cannot write '{path}': {e}")),
    })
}

/// True when nobody typed a command: no arguments *and* a console that
/// exists only because the exe was double-clicked. Both halves matter —
/// a script runs with no arguments too, and must never be made to wait.
pub fn double_clicked(args: &[String]) -> bool {
    args.is_empty() && console_is_fresh()
}

/// What the CLI says when it is double-clicked: there is no window, so
/// the only useful thing it can do is explain itself and stay open.
pub fn double_click_notice() -> String {
    format!(
        "DocConvert {} — a command-line converter. There is no window: it
reads files, writes files, and prints to the terminal.

Open a terminal in this folder (Shift-right-click the folder, then
\"Open PowerShell window here\"), and run one of:

  .\\docconvert.exe help                           the full command list
  .\\docconvert.exe capabilities                   what this build converts
  .\\docconvert.exe convert notes.md notes.html    a file conversion
  .\\docconvert.exe convert -f md -t html < notes.md > notes.html

Press Enter to close this window.
",
        crate::VERSION
    )
}

/// The same courtesy for the MCP server, which is not meant to be
/// started by hand at all.
pub fn mcp_double_click_notice() -> String {
    format!(
"docconvert-mcp {} — a Model Context Protocol server. It is not a
program you run yourself: an MCP client (an AI app) launches it and
speaks JSON-RPC on stdin/stdout, so a double-click has nothing to do.

Register it in the client's configuration instead, for example:

  {{\"mcpServers\": {{\"docconvert\": {{\"command\": \"C:\\\\path\\\\to\\\\docconvert-mcp.exe\"}}}}}}

Press Enter to close this window.
",
        crate::VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_command_is_never_a_double_click() {
        assert!(!double_clicked(&["help".to_string()]));
        assert!(!double_clicked(&[
            "convert".to_string(),
            "notes.md".to_string()
        ]));
        // No arguments still needs a console that exists for us alone; a
        // test runs under a shell, so this is false on every platform.
        assert!(!double_clicked(&[]));
    }

    #[test]
    fn notices_say_what_to_do_next() {
        let cli = double_click_notice();
        let first = format!(
            "DocConvert {} — a command-line converter. There is no window: it",
            crate::VERSION
        );
        assert_eq!(cli.lines().next(), Some(first.as_str()), "{cli}");
        for command in [
            "  .\\docconvert.exe help",
            "  .\\docconvert.exe capabilities",
            "  .\\docconvert.exe convert notes.md notes.html",
            "  .\\docconvert.exe convert -f md -t html < notes.md > notes.html",
        ] {
            assert!(
                cli.lines().any(|line| line.starts_with(command)),
                "missing {command:?} in {cli}"
            );
        }
        assert!(cli.contains("Open PowerShell window here"), "{cli}");
        assert!(
            cli.ends_with("Press Enter to close this window.\n"),
            "{cli}"
        );

        let server = mcp_double_click_notice();
        let first = format!(
            "docconvert-mcp {} — a Model Context Protocol server.",
            crate::VERSION
        );
        assert!(server.starts_with(&first), "{server}");
        assert!(
            server.lines().any(|line| line.trim()
                == r#"{"mcpServers": {"docconvert": {"command": "C:\\path\\to\\docconvert-mcp.exe"}}}"#),
            "{server}"
        );
        assert!(
            server.ends_with("Press Enter to close this window.\n"),
            "{server}"
        );
    }
}
