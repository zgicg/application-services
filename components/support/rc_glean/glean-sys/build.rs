/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use bindgen;
use std::{env, ffi::OsString, path::PathBuf, process::{Command, Stdio }};
use std::collections::HashMap;

use serde::Deserialize;

fn env(name: &str) -> Option<OsString> {
    println!("cargo:rerun-if-env-changed={}", name);
    env::var_os(name)
}

fn env_str(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={}", name);
    env::var(name).ok()
}

const DEFAULT_ANDROID_NDK_API_VERSION: &str = "21";

fn main() {
    // Note: this has to be first!
    // XXX TODO: do we need this for things other than NSS?
    maybe_setup_ndk_clang_path();
    // We're going to build glean separately, as a system dependency,
    // as if we were building e.g. an external C library.
    let glean_manifest = locate_glean_manifest();
    // The glean-ffi package ships with a copy of glean.h, conveniently located
    // right next to its Cargo.toml. Find it locally on disk.
    let glean_header_file = glean_manifest
        .with_file_name("glean.h");
    //let glean_header_file = PathBuf::from("/home/rfk/INCLUDE/glean.h");
    println!("cargo:rerun-if-changed={}", glean_header_file.to_str().unwrap());
    // Ensure we have a libglean_ffi.so built for the target platform.
    let glean_dylib_file = build_glean_dylib(&glean_manifest);
    //let glean_dylib_file = PathBuf::from("/home/rfk/LIBS/libglean_ffi.so");
    println!(
        "cargo:rustc-link-search=native={}",
        glean_dylib_file.parent().unwrap().to_str().unwrap()
    );
    println!("cargo:rustc-link-lib=dylib=glean_ffi");
    let bindings = bindgen::Builder::default()
        .header("stdint.h")
        .header(glean_header_file.to_str().unwrap())
        // TODO: read this list from a config file or something, like we do in nss_sys.
        .whitelist_function("glean_get_version")
        .whitelist_function("glean_event_record")
        .whitelist_function("glean_new_event_metric")
        .whitelist_function("glean_destroy_event_metric")
        .whitelist_function("glean_new_timing_distribution_metric")
        .whitelist_function("glean_destroy_timing_distribution_metric")
        .whitelist_function("glean_timing_distribution_set_start")
        .whitelist_function("glean_timing_distribution_set_stop_and_accumulate")
        .whitelist_function("glean_timing_distribution_cancel")
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

fn locate_glean_manifest() -> PathBuf {
    let cargo = env::var("CARGO").unwrap();
    let output = Command::new(cargo)
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .output()
        .unwrap();
    if !output.status.success() {
        panic!("failed to load cargo metadata");
    }
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let metadata: Metadata = serde_json::from_str(stdout).unwrap();
    let glean_ffi_pkgs: Vec<_> = metadata
        .packages
        .iter()
        .filter(|p| p.name == "glean-ffi")
        .collect();
    if glean_ffi_pkgs.len() == 0 {
        panic!("failed to locate glean-ffi dependency metadata");
    }
    if glean_ffi_pkgs.len() > 1 {
        panic!("found multiple glean-ffi dependencies, this will not end well");
    }
    PathBuf::from(glean_ffi_pkgs[0].manifest_path.clone())
}

fn build_glean_dylib(glean_manifest: &PathBuf) -> PathBuf {
    let cargo = env::var("CARGO").unwrap();
    // We mostly want to inherit the build environment of the parent,
    // however some toolchains (e.g. rust-android-gradle) like to set
    // flags that force the SONAME to be the top-level name of the parent.
    // Overwrite to give it the correct name.
    let soname_pattern = regex::Regex::new(r"-soname,lib(.+)\.so").unwrap();
    let filtered_env: HashMap<String, String> = env::vars().filter(|(_, ref v)| {
        soname_pattern.is_match(v)
    }).map(|(ref k, ref v)| {
        (k.clone(), soname_pattern.replace_all(v, "-soname,libglean_ffi.so").into_owned())
    }).collect();
    // Build glean-ffi seperately as a stand-alone library.
    println!("cargo:rerun-if-changed={}", glean_manifest.to_str().unwrap());
    let output = Command::new(cargo)
        .arg("build")
        .args(if env::var("PROFILE").unwrap() == "release" { Some("--release") } else { None })
        .arg("--manifest-path")
        .arg(glean_manifest.to_str().unwrap())
        .arg("--target")
        .arg(env::var("TARGET").unwrap())
        .arg("--target-dir")
        .arg(PathBuf::from(env::var("OUT_DIR").unwrap()))
        .envs(&filtered_env)
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    if !output.status.success() {
        panic!("failed to build glean-ffi");
    }
    // Find the resulting `.so`, which is the only artifact we actually want.
    let mut built_dylib_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    built_dylib_path.push(env::var("TARGET").unwrap());
    built_dylib_path.push(env::var("PROFILE").unwrap());
    built_dylib_path.push("libglean_ffi.so");
    if ! built_dylib_path.exists() {
        panic!("failed to build {:?}", built_dylib_path);
    }
    // Copy it into a standalone directory, so that cargo won't be tempted to link
    // against any other artifacts from that build.
    let mut glean_dylib_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    glean_dylib_path.push("libglean_ffi.so");
    std::fs::copy(&built_dylib_path, &glean_dylib_path).unwrap();
    println!("cargo:rerun-if-changed={}", glean_dylib_path.to_str().unwrap());
    return glean_dylib_path;
}

#[derive(Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
}

#[derive(Clone, Deserialize)]
pub struct Package {
    pub name: String,
    pub manifest_path: String,
}

// Set the CLANG_PATH env variable to point to the right clang for the NDK in question.
// Note that this basically needs to be done first thing in main.
fn maybe_setup_ndk_clang_path() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").ok();
    if target_os.as_ref().map_or(false, |x| x == "android") {
        let mut buf = PathBuf::from(env("ANDROID_NDK_ROOT").unwrap());
        let ndk_api = env_str("ANDROID_NDK_API_VERSION")
            .unwrap_or(DEFAULT_ANDROID_NDK_API_VERSION.to_owned());

        if ndk_api.is_empty() {
            println!("cargo:warning=ANDROID_NDK_API_VERSION is unset. Trying unprefixed");
        }
        let mut target = env::var("TARGET").unwrap();
        if target == "armv7-linux-androideabi" {
            // See https://developer.android.com/ndk/guides/other_build_systems
            // for information on why this is weird and different (or at least,
            // confirmation that it's supposed to be that way...)
            target = "armv7a-linux-androideabi".to_owned();
        }
        for &path in &["toolchains", "llvm", "prebuilt", android_host_tag(), "bin"] {
            buf.push(path);
        }
        buf.push(format!("{}{}-clang", target, ndk_api));
        env::set_var("CLANG_PATH", buf);
    }
}

fn android_host_tag() -> &'static str {
    // cfg! target_os actually refers to the host environment in this case (build script).
    #[cfg(target_os = "macos")]
    return "darwin-x86_64";
    #[cfg(target_os = "linux")]
    return "linux-x86_64";
    #[cfg(target_os = "windows")]
    return "windows-x86_64";
}
