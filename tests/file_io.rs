//! The file API end to end: format detection by extension, explicit
//! formats, options, and the error codes callers switch on.

use std::ffi::{CStr, CString};
use std::path::PathBuf;

use docconvert::ffi::{
    dc_convert_file, dc_convert_file_opts, dc_convert_file_streaming, dc_last_error,
    dc_last_error_code, dc_version,
};
use docconvert::{DC_OK, ERR_INVALID_ARG, ERR_NOT_FOUND, ERR_NOT_SUPPORTED};

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docconvert-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn path(p: &std::path::Path) -> CString {
    cs(p.to_str().expect("UTF-8 scratch path"))
}

/// The shipped C header names the release in its banner, so a bump has to
/// reach it too — and the numeric parts cannot drift from the string.
#[test]
fn version_is_one_source_of_truth() {
    let header = include_str!("../include/docconvert.h");
    let banner = format!("DocConvert.dll v{}", docconvert::VERSION);
    assert!(
        header.contains(&banner),
        "header banner says: {}",
        &header[..60]
    );

    assert_eq!(
        format!(
            "{}.{}.{}",
            docconvert::VERSION_MAJOR,
            docconvert::VERSION_MINOR,
            docconvert::VERSION_PATCH
        ),
        docconvert::VERSION
    );
}

#[test]
fn version_is_reported() {
    let version = unsafe { CStr::from_ptr(dc_version()) }.to_string_lossy();
    assert_eq!(version, docconvert::VERSION);
    assert_eq!(
        format!(
            "{}.{}.{}",
            docconvert::VERSION_MAJOR,
            docconvert::VERSION_MINOR,
            docconvert::VERSION_PATCH
        ),
        docconvert::VERSION
    );
}

#[test]
fn converts_files_by_extension() {
    let dir = scratch_dir("by-extension");
    let md = dir.join("notes.md");
    let html = dir.join("notes.html");
    let back = dir.join("notes-copy.md");
    std::fs::write(&md, "# Hi\n\nSome **bold** text.\n").unwrap();

    let code = dc_convert_file(
        path(&md).as_ptr(),
        path(&html).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert_eq!(code, DC_OK);
    assert_eq!(
        std::fs::read_to_string(&html).unwrap(),
        "<h1>Hi</h1>\n<p>Some <strong>bold</strong> text.</p>"
    );

    let code = dc_convert_file(
        path(&html).as_ptr(),
        path(&back).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert_eq!(code, DC_OK);
    let round = std::fs::read_to_string(&back).unwrap();
    assert!(round.contains("# Hi"), "{round}");
    assert!(round.contains("**bold**"), "{round}");
}

#[test]
fn options_and_errors() {
    let dir = scratch_dir("options");
    let md = dir.join("post.md");
    let html = dir.join("post.html");
    std::fs::write(&md, "---\ntitle: Hello\n---\n\nBody\n").unwrap();

    let code = dc_convert_file_opts(
        path(&md).as_ptr(),
        path(&html).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
        cs("{\"front_matter\":\"table\"}").as_ptr(),
    );
    assert_eq!(code, DC_OK);
    let produced = std::fs::read_to_string(&html).unwrap();
    assert!(produced.contains("<td>Hello</td>"), "{produced}");

    // Missing input: DC_ERR_NOT_FOUND plus a thread-local message.
    let missing = dir.join("nope.md");
    let code = dc_convert_file(
        path(&missing).as_ptr(),
        path(&html).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert_eq!(code, ERR_NOT_FOUND);
    assert_eq!(dc_last_error_code(), ERR_NOT_FOUND);
    let message = unsafe { CStr::from_ptr(dc_last_error()) }
        .to_string_lossy()
        .into_owned();
    assert!(message.contains("nope.md"), "{message}");

    // An unsupported pair fails loudly and writes nothing. (Plain text
    // has no table to become, so text → csv is still not in the matrix.)
    let plain = dir.join("note.txt");
    std::fs::write(&plain, "just prose\n").unwrap();
    let out = dir.join("post.csv");
    let code = dc_convert_file(
        path(&plain).as_ptr(),
        path(&out).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert_eq!(code, ERR_NOT_SUPPORTED);
    assert!(!out.exists());

    // Unknown extension: INVALID_ARG, never a guess.
    let weird = dir.join("post.dat");
    let code = dc_convert_file(
        path(&md).as_ptr(),
        path(&weird).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert_eq!(code, ERR_INVALID_ARG);

    // Explicit formats rescue an unknown extension.
    let code = dc_convert_file(
        path(&md).as_ptr(),
        path(&weird).as_ptr(),
        cs("markdown").as_ptr(),
        cs("text").as_ptr(),
    );
    assert_eq!(code, DC_OK);
    assert!(std::fs::read_to_string(&weird).unwrap().contains("Body"));
}

#[test]
fn streaming_copies_identity_conversions_and_converts_the_rest() {
    let dir = scratch_dir("stream");
    let source = dir.join("notes.md");
    let copy = dir.join("copy.md");
    let html = dir.join("notes.html");
    let body = "# Title\n\nSome **bold** text.\n".repeat(200);
    std::fs::write(&source, &body).expect("write source");

    // markdown -> markdown is byte-faithful, so it streams.
    let written = dc_convert_file_streaming(
        path(&source).as_ptr(),
        path(&copy).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert_eq!(written, body.len() as i64);
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), body);

    // Anything else converts through the whole-buffer path.
    let written = dc_convert_file_streaming(
        path(&source).as_ptr(),
        path(&html).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
    );
    let out = std::fs::read_to_string(&html).unwrap();
    assert_eq!(written, out.len() as i64);
    assert!(out.starts_with("<h1>Title</h1>"), "{out}");

    // A missing input is the usual error code, not a crash.
    let missing = dir.join("nope.md");
    let code = dc_convert_file_streaming(
        path(&missing).as_ptr(),
        path(&copy).as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert_eq!(code, -1);
    assert_eq!(dc_last_error_code(), ERR_NOT_FOUND);
    let _ = std::fs::remove_dir_all(&dir);
}
