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
    static VERSION_C: &[u8] = b"0.1.0\0";
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
    finish_str(guard(|| {
        Ok(format!(
            "[{}]",
            convert::FORMATS
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(",")
        ))
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
    finish_str(guard(|| {
        let input = req_str(input, "input")?;
        let from = req_str(from_format, "from_format")?;
        let to = req_str(to_format, "to_format")?;
        convert::convert(&input, &from, &to)
    }))
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
        let converted = convert::convert(&text, &from, &to)?;
        crate::platform::write_file(&out_path, converted.as_bytes())
    }))
}

/// JSON list of the {from,to} pairs supported in v0.1.
#[no_mangle]
pub extern "C" fn dc_matrix_json() -> *mut c_char {
    finish_str(guard(|| {
        let pairs: &[(&str, &str)] = &[
            ("markdown", "html"),
            ("markdown", "text"),
            ("html", "markdown"),
            ("html", "text"),
            ("text", "html"),
            ("text", "markdown"),
            ("csv", "json"),
            ("json", "csv"),
            ("csv", "markdown"),
            ("csv", "html"),
            ("csv", "text"),
            ("json", "text"),
            ("json", "html"),
            ("json", "markdown"),
        ];
        let items: Vec<String> = pairs
            .iter()
            .map(|(f, t)| format!("{{\"from\":\"{f}\",\"to\":\"{t}\"}}"))
            .collect();
        Ok(format!("[{}]", items.join(",")))
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

        let bad = dc_convert(
            cs("x").as_ptr(),
            cs("markdown").as_ptr(),
            cs("csv").as_ptr(),
        );
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
}
