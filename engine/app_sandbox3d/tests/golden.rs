//! The checked-in lit shader assets (GLSL and SPIR-V) are what the `shaderc` tool makes today
//! from `shaders/lit.vert` and `shaders/lit.frag`. The check runs the tool in this process.
//!
//! It needs the shader tools (glslang, SPIRV-Cross); without them it prints `not run: tool not set
//! up` and passes. `DUCK_REQUIRE_SHADER_TOOLS=1` makes that a failure.

use app_sandbox3d::renderer::{LIT_GLSL, LIT_SPIRV};
use std::path::{Path, PathBuf};
use tools_shaderc::cli;
use tools_shaderc::tools::{ENV_REQUIRE, ENV_ROOT, ToolPaths, is_required};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn crate_file(relative: &str) -> PathBuf {
    Path::new(ROOT).join(relative)
}

/// The tools folder, or `None` after saying why not (or a panic when the tools are required).
fn tools_root() -> Option<PathBuf> {
    let root = std::env::var_os(ENV_ROOT)
        .map_or_else(|| crate_file("../../target/shader-spike"), PathBuf::from);
    let paths = ToolPaths::from_root(&root);
    if paths.glslang.is_file() && paths.spirv_cross.is_file() {
        return Some(root);
    }
    let required = std::env::var(ENV_REQUIRE).ok();
    assert!(
        !is_required(required.as_deref()),
        "the shader tools are required but not found under {}",
        root.display()
    );
    println!("not run: tool not set up. The lit assets were NOT compared with the tool output");
    None
}

/// The asset the tool makes from the lit sources for `backend` (`glsl` or `spirv`).
fn made(root: &Path, backend: &str) -> Vec<u8> {
    let out = std::env::temp_dir().join(format!(
        "duck-lit-golden-{}-{backend}.dshd",
        std::process::id()
    ));
    let args: Vec<String> = [
        "--tools",
        &root.to_string_lossy(),
        "--backend",
        backend,
        &crate_file("shaders/lit.vert").to_string_lossy(),
        &crate_file("shaders/lit.frag").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let result = cli::run(&args, &|name| std::env::var_os(name));
    assert!(result.is_ok(), "{result:?}");
    let bytes = std::fs::read(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    bytes
}

#[test]
fn the_lit_assets_are_what_the_tool_makes_today() {
    let Some(root) = tools_root() else { return };
    for (backend, golden) in [("glsl", LIT_GLSL), ("spirv", LIT_SPIRV)] {
        let bytes = made(&root, backend);
        assert!(!bytes.is_empty());
        assert!(
            bytes == golden,
            "regenerate the {backend} lit asset (assets/) and review the diff"
        );
    }
}
