//! Tests that need the real glslang and SPIRV-Cross. Without them they print
//! `not run: tool not set up` and pass; with `DUCK_REQUIRE_SHADER_TOOLS=1` they fail instead.

use super::*;
use crate::contract;
use crate::tools::{ENV_REQUIRE, ENV_ROOT, is_required};
use std::path::PathBuf;

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

/// The compiler, or `None` after saying why not (or a panic when the tools are required).
fn compiler() -> Option<ExternalCompiler> {
    let env = |name: &str| std::env::var_os(name);
    let mut paths = ToolPaths::resolve(&env);
    if env(ENV_ROOT).is_none() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/shader-spike");
        paths = ToolPaths::from_root(&root);
    }
    match ExternalCompiler::new(&paths) {
        Ok(c) => Some(c),
        Err(e) => {
            let required = std::env::var(ENV_REQUIRE).ok();
            assert!(
                !is_required(required.as_deref()),
                "the shader tools are required: {e}"
            );
            println!("not run: tool not set up ({e})");
            None
        }
    }
}

#[test]
fn the_real_compiler_meets_the_contract() {
    if let Some(c) = compiler() {
        contract::real(&c);
    }
}

#[test]
fn the_real_compiler_meets_the_spirv_contract() {
    if let Some(c) = compiler() {
        contract::real_spirv(&c);
    }
}

#[test]
fn the_spirv_is_version_one_or_later_and_ends_in_whole_words() {
    if let Some(c) = compiler() {
        let module = must(c.compile_spirv(Source {
            stage: Stage::Fragment,
            text: contract::SPRITE_FRAG,
        }));
        let version = u32::from_le_bytes([
            module.code[4],
            module.code[5],
            module.code[6],
            module.code[7],
        ]);
        assert!(version >= 0x0001_0000, "{version:#x}");
        assert!(module.code.len() > 20 && module.code.len().is_multiple_of(4));
    }
}

#[test]
fn diagnostics_name_the_problem() {
    if let Some(c) = compiler() {
        let result = c.compile(Source {
            stage: Stage::Fragment,
            text: contract::BROKEN_FRAG,
        });
        assert!(
            matches!(&result, Err(CompileError::Diagnostics { tool: "glslang", text }) if text.contains("ERROR")),
            "{result:?}"
        );
    }
}

#[test]
fn output_has_line_feeds_only_and_the_main_entry() {
    if let Some(c) = compiler() {
        let module = c.compile(Source {
            stage: Stage::Vertex,
            text: contract::SPRITE_VERT,
        });
        assert!(module.is_ok_and(|m| !m.code.contains('\r') && m.code.contains("void main()")));
    }
}

#[test]
fn a_wrong_tool_version_is_refused() {
    // The test program stands in for a tool that answers with something else.
    let me = std::env::current_exe().unwrap_or_default();
    let result = ExternalCompiler::new(&ToolPaths {
        glslang: me.clone(),
        spirv_cross: me,
    });
    assert!(matches!(result, Err(CompileError::Tool(_))));
}

#[test]
fn result_files_are_bounded_before_they_are_read() {
    let dir = must(WorkDir::create());
    let big = dir.path().join("big.json");
    must(fs::write(&big, vec![b'a'; MAX_RESULT_LEN + 1]));
    assert!(matches!(read_text(&big), Err(CompileError::Tool(_))));
    let ok = dir.path().join("ok.json");
    must(fs::write(&ok, "a\r\nb"));
    assert_eq!(read_text(&ok), Ok("a\nb".to_string()));
}

#[test]
fn relative_tool_paths_are_made_absolute_before_use() {
    // A relative path to a missing tool: the error message must show an absolute path.
    let result = ExternalCompiler::new(&ToolPaths {
        glslang: PathBuf::from("rel/glslang"),
        spirv_cross: PathBuf::from("rel/cross"),
    });
    let text = result.err().map(|e| e.to_string()).unwrap_or_default();
    let cwd = must(std::env::current_dir());
    assert!(text.contains(&*cwd.to_string_lossy()), "{text}");
}

#[test]
fn a_missing_tool_is_a_tool_error() {
    let missing = PathBuf::from("definitely/not/here");
    let result = ExternalCompiler::new(&ToolPaths {
        glslang: missing.clone(),
        spirv_cross: missing,
    });
    assert!(matches!(result, Err(CompileError::Tool(_))));
}
