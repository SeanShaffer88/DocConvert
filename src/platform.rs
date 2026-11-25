//! Platform layer for DocConvert: deliberately thin — the parser,
//! extractor, and writer are pure Rust; only file I/O error mapping
//! lives here.

use std::io::{Read, Write};

use crate::{CResult, ERR_ACCESS_DENIED, ERR_NOT_FOUND, ERR_PLATFORM};

/// The chunk size for [`copy_file`] — small enough to stay off the heap
/// and large enough to keep syscalls rare.
const COPY_BUFFER: usize = 64 * 1024;

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
