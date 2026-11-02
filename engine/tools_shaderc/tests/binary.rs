//! The `shaderc` program end to end, and the checked-in first shader asset.
//!
//! Tests that need the real tools print `not run: tool not set up` and pass without them;
//! `DUCK_REQUIRE_SHADER_TOOLS=1` makes them fail instead.

use assets::dshd::{Program, Stage, UniformBlock, VertexInput};
use std::path::{Path, PathBuf};
use std::process::Command;
use tools_shaderc::tools::{ENV_REQUIRE, ENV_ROOT, ToolPaths, is_required};

const SHADERC: &str = env!("CARGO_BIN_EXE_shaderc");
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn crate_file(relative: &str) -> PathBuf {
    Path::new(ROOT).join(relative)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("duck-shaderc-bin-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    dir
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
    println!("not run: tool not set up");
    None
}

#[test]
fn the_golden_sprite_asset_reads_and_describes_the_sprite_shader() {
    let bytes = std::fs::read(crate_file("golden/sprite.dshd")).unwrap_or_default();
    let program = Program::parse(&bytes);
    assert!(program.is_ok(), "{program:?}");
    let program = program.unwrap_or_else(|_| unreachable!());
    assert_eq!(program.module_count(), 2);
    let vertex = program.module_for(Stage::Vertex);
    assert!(vertex.is_some_and(|m| {
        m.uniform()
            == Some(UniformBlock {
                binding: 0,
                size: 64,
            })
            && m.inputs().collect::<Vec<_>>()
                == vec![
                    VertexInput {
                        location: 0,
                        components: 2,
                    },
                    VertexInput {
                        location: 1,
                        components: 2,
                    },
                    VertexInput {
                        location: 2,
                        components: 4,
                    },
                ]
            && m.code().starts_with("#version 450")
    }));
    let fragment = program.module_for(Stage::Fragment);
    assert!(fragment.is_some_and(|m| m.texture_binding() == Some(0)
        && m.uniform().is_none()
        && m.code().starts_with("#version 450")));
}

#[test]
fn the_golden_asset_is_what_the_tool_makes_today() {
    let Some(root) = tools_root() else {
        println!(
            "not run: tool not set up. golden/sprite.dshd was NOT compared with the tool output"
        );
        return;
    };
    let out = scratch("golden").join("sprite.dshd");
    let status = Command::new(SHADERC)
        .arg("--tools")
        .arg(&root)
        .arg(crate_file("shaders/sprite.vert"))
        .arg(crate_file("shaders/sprite.frag"))
        .arg("-o")
        .arg(&out)
        .status();
    assert!(status.is_ok_and(|s| s.success()));
    let made = std::fs::read(&out).unwrap_or_default();
    let golden = std::fs::read(crate_file("golden/sprite.dshd")).unwrap_or_default();
    assert!(!made.is_empty());
    assert!(
        made == golden,
        "regenerate golden/sprite.dshd and review the diff"
    );
}

#[test]
fn a_relative_tools_folder_works_from_another_directory() {
    let Some(root) = tools_root() else { return };
    let root = std::fs::canonicalize(root).unwrap_or_default();
    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else {
        panic!("the tools folder has no parent");
    };
    let dir = scratch("relative");
    let out = dir.join("o.dshd");
    let status = Command::new(SHADERC)
        .current_dir(parent)
        .arg("--tools")
        .arg(name)
        .arg(crate_file("shaders/sprite.vert"))
        .arg("-o")
        .arg(&out)
        .status();
    assert!(status.is_ok_and(|s| s.success()));
    assert!(out.is_file());
}

#[test]
fn a_wrong_command_line_prints_usage_and_fails() {
    let output = Command::new(SHADERC).arg("--nope").output();
    assert!(
        output.is_ok_and(|o| !o.status.success()
            && String::from_utf8_lossy(&o.stderr).contains("usage: shaderc"))
    );
}

#[test]
fn missing_tools_fail_with_a_message_and_write_nothing() {
    let dir = scratch("missing-tools");
    let out = dir.join("o.dshd");
    let output = Command::new(SHADERC)
        .arg("--tools")
        .arg(dir.join("no-tools"))
        .arg(crate_file("shaders/sprite.vert"))
        .arg("-o")
        .arg(&out)
        .output();
    assert!(output.is_ok_and(
        |o| !o.status.success() && String::from_utf8_lossy(&o.stderr).contains("tool problem")
    ));
    assert!(!out.exists());
}

#[test]
fn a_broken_shader_fails_with_diagnostics_and_writes_nothing() {
    let Some(root) = tools_root() else { return };
    let dir = scratch("broken");
    let out = dir.join("o.dshd");
    let output = Command::new(SHADERC)
        .arg("--tools")
        .arg(&root)
        .arg(crate_file("shaders/broken.frag"))
        .arg("-o")
        .arg(&out)
        .output();
    assert!(output.is_ok_and(|o| !o.status.success()
        && String::from_utf8_lossy(&o.stderr).contains("glslang rejected the source")));
    assert!(!out.exists());
}
