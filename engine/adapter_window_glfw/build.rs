//! Brings `glfw3.dll` next to the programs of this build (Windows), so that a program that opens a
//! window finds it. Downloads the official release once into a per-user cache, checks its SHA-256
//! twice, copies it. A problem is a warning, never a failed build: the engine then runs on the
//! Headless window system, as it always could.

use adapter_runtime_libs::obtain::SystemTools;
use adapter_runtime_libs::plan::{CACHE_VARIABLE, OFFLINE_VARIABLE, run};
use std::path::PathBuf;

const MANIFEST: &str = include_str!("runtime-lib.txt");

fn main() {
    println!("cargo:rerun-if-changed=runtime-lib.txt");
    println!("cargo:rerun-if-changed=build.rs");
    for name in [
        CACHE_VARIABLE,
        OFFLINE_VARIABLE,
        "DUCK_GLFW_LIB",
        "LOCALAPPDATA",
        "XDG_CACHE_HOME",
        "HOME",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let (Some(out_dir), Ok(target_os)) = (
        std::env::var_os("OUT_DIR").map(PathBuf::from),
        std::env::var("CARGO_CFG_TARGET_OS"),
    ) else {
        return;
    };
    let lookup = |name: &str| std::env::var(name).ok();
    let outcome = run(MANIFEST, &target_os, &out_dir, &lookup, &SystemTools);
    for file in &outcome.watch {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    for warning in &outcome.warnings {
        println!("cargo:warning={warning}");
    }
}
