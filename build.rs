//! Embeds `assets/docconvert.ico` into the Windows binaries, so Explorer,
//! the taskbar and Alt-Tab show the product icon instead of the generic
//! console glyph.
//!
//! No resource compiler and no dependency: `build_support/win_icon.rs`
//! writes the `.rsrc` section itself and wraps it in a COFF object, which
//! the linker takes as an ordinary input file. Non-Windows targets skip all
//! of it — there is nothing to embed.

#![allow(dead_code)]

include!("build_support/win_icon.rs");

fn main() {
    println!("cargo:rerun-if-changed=assets/docconvert.ico");
    println!("cargo:rerun-if-changed=build_support/win_icon.rs");

    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }

    let icon_path = std::path::Path::new("assets/docconvert.ico");
    let bytes = match std::fs::read(icon_path) {
        Ok(bytes) => bytes,
        Err(error) => panic!("cannot read {}: {error}", icon_path.display()),
    };
    let icons = read_ico(&bytes)
        .unwrap_or_else(|error| panic!("{} is not a usable icon: {error}", icon_path.display()));
    let arch = Arch::from_target(&target)
        .unwrap_or_else(|| panic!("no icon resources defined for target '{target}'"));
    let object = coff_object(&icons, arch);

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let object_path = std::path::Path::new(&out_dir).join("docconvert-icon.obj");
    if let Err(error) = std::fs::write(&object_path, &object) {
        panic!("cannot write {}: {error}", object_path.display());
    }

    // The two executables and the DLL all wear the icon.
    let path = object_path.display();
    println!("cargo:rustc-link-arg-bins={path}");
    println!("cargo:rustc-link-arg-cdylib={path}");
}
