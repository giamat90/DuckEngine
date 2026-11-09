//! Compiles the C shim, and miniaudio with it when the pinned header can be had. Downloads the
//! official header once into a per-user cache and checks its SHA-256; a header of the user's own
//! (`DUCK_MINIAUDIO_HEADER`) is checked against the same hash. A problem with the header is a
//! warning, not a failed build: the shim is then built without miniaudio and the adapter reports
//! "no device". The compiler is the one thing the build needs: without a C compiler the `cc` crate
//! stops it.

use adapter_runtime_libs::obtain::SystemTools;
use adapter_runtime_libs::plan::{CACHE_VARIABLE, OFFLINE_VARIABLE, cache_root};
use adapter_runtime_libs::source::{SourceFile, obtain_source, parse_source, verify_source};
use std::path::{Path, PathBuf};

const MANIFEST: &str = include_str!("source-file.txt");
const SHIM: &str = "csrc/duck_miniaudio.c";

fn main() {
    println!("cargo:rerun-if-changed=source-file.txt");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={SHIM}");
    for name in [
        CACHE_VARIABLE,
        OFFLINE_VARIABLE,
        "DUCK_MINIAUDIO_HEADER",
        "LOCALAPPDATA",
        "XDG_CACHE_HOME",
        "HOME",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let include_dir = match header_folder(&out_dir, &target_os) {
        Ok(folder) => Some(folder),
        Err(problem) => {
            println!(
                "cargo:warning=miniaudio is not built in ({problem}); the audio device will report no device (see THIRD_PARTY.md)"
            );
            None
        }
    };
    // The tests check that the shim reports what the build did.
    let compiled = u8::from(include_dir.is_some());
    println!("cargo:rustc-env=DUCK_MINIAUDIO_COMPILED={compiled}");
    let mut build = cc::Build::new();
    build.file(SHIM).warnings(false);
    if let Some(folder) = &include_dir {
        build.include(folder).define("DUCK_HAVE_MINIAUDIO", None);
    }
    build.compile("duck_miniaudio");
    if target_os == "linux" {
        for lib in ["dl", "pthread", "m"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}

/// The folder that holds a checked `miniaudio.h`.
fn header_folder(out_dir: &Path, target_os: &str) -> Result<PathBuf, String> {
    let source = parse_source(MANIFEST).map_err(|e| format!("the manifest is not valid: {e}"))?;
    let lookup = |name: &str| std::env::var(name).ok();
    if let Some(own) = source
        .override_variable
        .as_deref()
        .and_then(&lookup)
        .filter(|value| !value.is_empty())
    {
        return own_header(Path::new(&own), &source, out_dir);
    }
    let offline = lookup(OFFLINE_VARIABLE).is_some_and(|v| !v.is_empty() && v != "0");
    let cache = cache_root(&lookup, out_dir, target_os);
    std::fs::create_dir_all(&cache).map_err(|e| format!("the cache folder cannot be made: {e}"))?;
    let path = obtain_source(&source, &cache, &SystemTools, offline).map_err(|e| e.to_string())?;
    stage(&path, &source, out_dir)
}

/// A header of the user's own.
fn own_header(path: &Path, source: &SourceFile, out_dir: &Path) -> Result<PathBuf, String> {
    verify_source(path, &source.sha256).map_err(|e| e.to_string())?;
    stage(path, source, out_dir)
}

/// Copies the header under its pinned name into the build folder, where the shim's
/// `#include "miniaudio.h"` finds it, and checks the copy: what the compiler reads is what was
/// hashed, whatever happens to the original afterwards.
fn stage(path: &Path, source: &SourceFile, out_dir: &Path) -> Result<PathBuf, String> {
    let folder = out_dir.join("header");
    std::fs::create_dir_all(&folder).map_err(|e| format!("no work folder: {e}"))?;
    let copy = folder.join(&source.file);
    std::fs::copy(path, &copy).map_err(|e| format!("the header cannot be copied: {e}"))?;
    verify_source(&copy, &source.sha256).map_err(|e| e.to_string())?;
    Ok(folder)
}
