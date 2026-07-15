#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    use std::{env, path::PathBuf, process::Command};

    println!("cargo:rerun-if-changed=src/bindings.h");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    let output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("failed to run xcrun while locating the macOS SDK");
    if !output.status.success() {
        panic!(
            "xcrun could not locate the macOS SDK (status {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let sdk_path =
        String::from_utf8(output.stdout).expect("xcrun returned a non-UTF-8 macOS SDK path");
    let sdk_path = sdk_path.trim_end();
    assert!(
        !sdk_path.is_empty(),
        "xcrun returned an empty macOS SDK path"
    );

    let bindings = bindgen::Builder::default()
        .header("src/bindings.h")
        .clang_arg("-isysroot")
        .clang_arg(sdk_path)
        .clang_arg("-xobjective-c")
        .allowlist_type("CMItemIndex")
        .allowlist_type("CMSampleTimingInfo")
        .allowlist_type("CMVideoCodecType")
        .allowlist_type("VTEncodeInfoFlags")
        .allowlist_function("CMTimeMake")
        .allowlist_var("kCVPixelFormatType_.*")
        .allowlist_var("kCVReturn.*")
        .allowlist_var("VTEncodeInfoFlags_.*")
        .allowlist_var("kCMVideoCodecType_.*")
        .allowlist_var("kCMTime.*")
        .allowlist_var("kCMSampleAttachmentKey_.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .layout_tests(false)
        .generate()
        .expect("unable to generate bindings");

    let out_path = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not set OUT_DIR"));
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("couldn't write dispatch bindings");
}
