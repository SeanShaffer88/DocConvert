//! C ABI surface — everything declared in include/docconvert.h.
//! Conventions (house standard):
//! - `*mut c_char` results are caller-owned; free with `dc_free_string`.
//! - Failures return null / an ERR_* code + a thread-local last-error.
//! - Nothing unwinds across the FFI boundary.

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::convert;
use crate::{DC_OK, ERR_INTERNAL, ERR_INVALID_ARG, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH};

thread_local! {
    static LAST_ERROR: RefCell<(i32, CString)> =
        RefCell::new((0, CString::new("").unwrap()));
}

pub fn set_error(code: i32, msg: &str) {
    let cleaned: String = msg
        .chars()
        .map(|c| if c == '\0' { ' ' } else { c })
        .collect();
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = (
            code,
            CString::new(cleaned).unwrap_or_else(|_| CString::new("error").unwrap()),
        );
    });
}

fn clear_error() {
    set_error(DC_OK, "");
}

fn guard<T, F: FnOnce() -> T>(f: F) -> Result<T, (i32, String)> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Ok(v),
        Err(_) => Err((ERR_INTERNAL, "internal panic caught at FFI boundary".into())),
    }
}

fn req_str(p: *const c_char, name: &str) -> Result<String, (i32, String)> {
    if p.is_null() {
        return Err((ERR_INVALID_ARG, format!("{name} must not be null")));
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map(|s| s.to_string())
        .map_err(|_| (ERR_INVALID_ARG, format!("{name} must be valid UTF-8")))
}

fn opt_str(p: *const c_char) -> Result<Option<String>, (i32, String)> {
    if p.is_null() {
        Ok(None)
    } else {
        req_str(p, "format").map(Some)
    }
}

fn finish_str(run: Result<Result<String, (i32, String)>, (i32, String)>) -> *mut c_char {
    match run {
        Ok(Ok(s)) => {
            clear_error();
            match CString::new(s) {
                Ok(c) => c.into_raw(),
                Err(_) => {
                    set_error(ERR_INTERNAL, "result contained a NUL byte");
                    std::ptr::null_mut()
                }
            }
        }
        Ok(Err((code, msg))) | Err((code, msg)) => {
            set_error(code, &msg);
            std::ptr::null_mut()
        }
    }
}

fn finish_code(run: Result<Result<(), (i32, String)>, (i32, String)>) -> c_int {
    match run {
        Ok(Ok(())) => {
            clear_error();
            DC_OK
        }
        Ok(Err((code, msg))) | Err((code, msg)) => {
            set_error(code, &msg);
            code
        }
    }
}

#[no_mangle]
pub extern "C" fn dc_version() -> *const c_char {
    // Derived from the crate version, so `dc_version` cannot drift from
    // `VERSION_MINOR` / `VERSION_PATCH` again on a version bump.
    static VERSION_C: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();
    VERSION_C.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn dc_version_major() -> c_int {
    VERSION_MAJOR as c_int
}

#[no_mangle]
pub extern "C" fn dc_version_minor() -> c_int {
    VERSION_MINOR as c_int
}

#[no_mangle]
pub extern "C" fn dc_version_patch() -> c_int {
    VERSION_PATCH as c_int
}

#[no_mangle]
pub extern "C" fn dc_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().1.as_ptr())
}

#[no_mangle]
pub extern "C" fn dc_last_error_code() -> c_int {
    LAST_ERROR.with(|e| e.borrow().0) as c_int
}

/// # Safety (caller contract): `p` is null or a string this library
/// returned and has not been freed yet.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn dc_free_string(p: *mut c_char) {
    if !p.is_null() {
        unsafe {
            drop(CString::from_raw(p));
        }
    }
}

/// JSON list of the supported formats.
#[no_mangle]
pub extern "C" fn dc_formats_json() -> *mut c_char {
    finish_str(guard(|| Ok(convert::formats_json())))
}

fn convert_impl(
    input: *const c_char,
    from_format: *const c_char,
    to_format: *const c_char,
    options_json: *const c_char,
) -> *mut c_char {
    finish_str(guard(|| {
        let input = req_str(input, "input")?;
        let from = req_str(from_format, "from_format")?;
        let to = req_str(to_format, "to_format")?;
        let options = match opt_str(options_json)? {
            Some(text) => convert::Options::from_json(&text)?,
            None => convert::Options::default(),
        };
        convert::convert_with_options(&input, &from, &to, &options)
    }))
}

/// Convert text between formats ("markdown"/"md", "html", "csv",
/// "json", "text"/"txt"). Returns the converted string or null.
/// # Safety (caller contract): all pointers are valid C strings.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn dc_convert(
    input: *const c_char,
    from_format: *const c_char,
    to_format: *const c_char,
) -> *mut c_char {
    convert_impl(input, from_format, to_format, std::ptr::null())
}

/// `dc_convert` with an options object as JSON, e.g.
/// `{"front_matter":"table"}`. A null or empty string means defaults.
/// # Safety (caller contract): pointers are null or valid C strings.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn dc_convert_opts(
    input: *const c_char,
    from_format: *const c_char,
    to_format: *const c_char,
    options_json: *const c_char,
) -> *mut c_char {
    convert_impl(input, from_format, to_format, options_json)
}

/// Convert a file. Formats come from the explicit arguments when
/// given (non-null), else from the file extensions. Returns DC_OK
/// or an ERR_* code.
/// # Safety (caller contract): pointers are null or valid C strings.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn dc_convert_file(
    in_path: *const c_char,
    out_path: *const c_char,
    from_format: *const c_char,
    to_format: *const c_char,
) -> c_int {
    convert_file_impl(in_path, out_path, from_format, to_format, std::ptr::null())
}

/// `dc_convert_file` with an options object as JSON.
/// # Safety (caller contract): pointers are null or valid C strings.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn dc_convert_file_opts(
    in_path: *const c_char,
    out_path: *const c_char,
    from_format: *const c_char,
    to_format: *const c_char,
    options_json: *const c_char,
) -> c_int {
    convert_file_impl(in_path, out_path, from_format, to_format, options_json)
}

fn convert_file_impl(
    in_path: *const c_char,
    out_path: *const c_char,
    from_format: *const c_char,
    to_format: *const c_char,
    options_json: *const c_char,
) -> c_int {
    finish_code(guard(|| {
        let in_path = req_str(in_path, "in_path")?;
        let out_path = req_str(out_path, "out_path")?;
        let from = match opt_str(from_format)? {
            Some(f) => f,
            None => convert::format_for_path(&in_path).ok_or((
                ERR_INVALID_ARG,
                format!("cannot detect the input format from '{in_path}' — pass it explicitly"),
            ))?,
        };
        let to = match opt_str(to_format)? {
            Some(f) => f,
            None => convert::format_for_path(&out_path).ok_or((
                ERR_INVALID_ARG,
                format!("cannot detect the output format from '{out_path}' — pass it explicitly"),
            ))?,
        };
        let bytes = crate::platform::read_file(&in_path)?;
        let text = String::from_utf8(bytes).map_err(|_| {
            (
                crate::ERR_CORRUPT,
                format!("'{in_path}' is not valid UTF-8 text"),
            )
        })?;
        let options = match opt_str(options_json)? {
            Some(text) => convert::Options::from_json(&text)?,
            None => convert::Options::default(),
        };
        let converted = convert::convert_with_options(&text, &from, &to, &options)?;
        crate::platform::write_file(&out_path, converted.as_bytes())
    }))
}

/// JSON list of the `{from,to}` pairs this build supports — generated
/// from `convert::MATRIX`, the same table `convert` dispatches from.
#[no_mangle]
pub extern "C" fn dc_matrix_json() -> *mut c_char {
    finish_str(guard(|| Ok(convert::matrix_json())))
}

/// `dc_toc_json(input, from_format)` — the headings of a Markdown or
/// HTML document as `[{"level":2,"text":"…","anchor":"…"}]`, the same
/// anchors `{"heading_ids":true}` writes into Markdown→HTML output.
#[no_mangle]
pub extern "C" fn dc_toc_json(input: *const c_char, from_format: *const c_char) -> *mut c_char {
    finish_str(guard(|| {
        let input = req_str(input, "input")?;
        let from = req_str(from_format, "from_format")?;
        let entries = crate::toc::entries(&input, &from)?;
        Ok(crate::toc::to_json(&entries))
    }))
}

/// The YAML front matter of a Markdown document as a JSON object
/// (`{}` when there is none or it is malformed). Mirrors
/// `dc_convert_opts`'s `front_matter` handling without converting.
#[no_mangle]
pub extern "C" fn dc_front_matter_json(input: *const c_char) -> *mut c_char {
    finish_str(guard(|| {
        let input = req_str(input, "input")?;
        let (front, _body) = crate::frontmatter::split(&input);
        Ok(match front {
            Some(front) => front.to_json().to_compact(),
            None => "{}".to_string(),
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn cs(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    #[test]
    fn ffi_conversions() {
        let p = dc_convert(
            cs("# Hi\n\nSome **bold** text.\n").as_ptr(),
            cs("md").as_ptr(),
            cs("html").as_ptr(),
        );
        assert!(!p.is_null());
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        dc_free_string(p);
        assert_eq!(s, "<h1>Hi</h1>\n<p>Some <strong>bold</strong> text.</p>");

        let bad = dc_convert(cs("x").as_ptr(), cs("text").as_ptr(), cs("csv").as_ptr());
        assert!(bad.is_null());
        assert_eq!(dc_last_error_code(), crate::ERR_NOT_SUPPORTED);

        let m = dc_matrix_json();
        let s = unsafe { CStr::from_ptr(m) }.to_string_lossy().into_owned();
        dc_free_string(m);
        assert!(
            s.contains("\"markdown\",\"to\":\"html\"") || s.contains("\"from\":\"markdown\""),
            "{s}"
        );
    }

    #[test]
    fn ffi_toc_json_reports_headings() {
        let p = dc_toc_json(
            cs("# Guide\n\n## Install & run\n").as_ptr(),
            cs("md").as_ptr(),
        );
        assert_eq!(
            take(p),
            "[{\"level\":1,\"text\":\"Guide\",\"anchor\":\"guide\"},\
             {\"level\":2,\"text\":\"Install & run\",\"anchor\":\"install-run\"}]"
        );

        // HTML keeps the ids the document already has.
        let html = dc_toc_json(
            cs("<h2 id=\"custom\">Setup</h2><h3>Usage</h3>").as_ptr(),
            cs("html").as_ptr(),
        );
        assert!(take(html).contains("\"anchor\":\"custom\""));

        // A source without headings is a not-supported error, not silence.
        let bad = dc_toc_json(cs("a,b\n1,2\n").as_ptr(), cs("csv").as_ptr());
        assert!(bad.is_null());
        assert_eq!(dc_last_error_code(), crate::ERR_NOT_SUPPORTED);

        let null_input = dc_toc_json(std::ptr::null(), cs("md").as_ptr());
        assert!(null_input.is_null());
        assert_eq!(dc_last_error_code(), crate::ERR_INVALID_ARG);
    }

    /// Own and free a returned string, failing loudly with the
    /// thread-local error when the call returned null.
    fn take(p: *mut c_char) -> String {
        assert!(!p.is_null());
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        dc_free_string(p);
        s
    }

    #[test]
    fn opts_and_front_matter() {
        let doc = cs("---\ntitle: Release\ndraft: true\n---\n# Body\n");
        let html = take(dc_convert(
            doc.as_ptr(),
            cs("md").as_ptr(),
            cs("html").as_ptr(),
        ));
        assert_eq!(html, "<h1>Body</h1>");
        let html = take(dc_convert_opts(
            doc.as_ptr(),
            cs("md").as_ptr(),
            cs("html").as_ptr(),
            cs("{\"front_matter\":\"table\"}").as_ptr(),
        ));
        assert!(html.contains("<td>Release</td>"), "{html}");
        // Unknown options fail with INVALID_ARG instead of being ignored.
        let bad = dc_convert_opts(
            doc.as_ptr(),
            cs("md").as_ptr(),
            cs("html").as_ptr(),
            cs("{\"nope\":1}").as_ptr(),
        );
        assert!(bad.is_null());
        assert_eq!(dc_last_error_code(), crate::ERR_INVALID_ARG);
        // Front matter is readable without converting anything.
        let json = take(dc_front_matter_json(doc.as_ptr()));
        assert_eq!(json, "{\"title\":\"Release\",\"draft\":true}");
        assert_eq!(take(dc_front_matter_json(cs("# Hi\n").as_ptr())), "{}");
    }

    #[test]
    fn matrix_json_matches_the_table() {
        let mut from_keys = Vec::new();
        let mut to_keys = Vec::new();
        match crate::json::parse_json(&take(dc_matrix_json())).unwrap() {
            crate::json::Json::Array(items) => {
                for item in items {
                    match item {
                        crate::json::Json::Object(entries) => {
                            for (key, value) in entries {
                                let text = match value {
                                    crate::json::Json::Str(s) => s,
                                    _ => panic!("matrix values must be strings"),
                                };
                                match key.as_str() {
                                    "from" => from_keys.push(text),
                                    "to" => to_keys.push(text),
                                    other => panic!("unexpected matrix key {other}"),
                                }
                            }
                        }
                        _ => panic!("matrix entries must be objects"),
                    }
                }
            }
            _ => panic!("matrix must be a JSON array"),
        }
        assert_eq!(from_keys.len(), convert::MATRIX.len());
        for (from, to) in from_keys.iter().zip(to_keys.iter()) {
            assert!(
                convert::MATRIX
                    .iter()
                    .any(|(f, t)| *f == from.as_str() && *t == to.as_str()),
                "{from}→{to} is not in convert::MATRIX"
            );
        }
    }
}
