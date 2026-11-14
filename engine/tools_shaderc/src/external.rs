//! The real compiler: glslang (GLSL to SPIR-V) then SPIRV-Cross (SPIR-V to GLSL 450 and to the JSON
//! reflection), both run as child processes with argument vectors (ADR-0009). The SPIR-V glslang
//! makes is also an output of its own (`compile_spirv`, card 6-4): the Vulkan adapter takes it as
//! it is, and its reflection comes from the same SPIR-V as the GLSL output's.
//!
//! Versions are checked once, in [`ExternalCompiler::new`]. Every compile uses its own scratch
//! directory that is removed afterwards.
//!
//! **Trust.** The version check catches a wrong build; it is not a hash check and does not prove
//! the files are the ones that were published. The tools run with the caller's rights; see
//! [`crate::process`]. Clip space and Y direction are not set in the SPIRV-Cross arguments: the
//! output keeps GLSL conventions, and depth 0..1 relies on `glClipControl` in the GL device.

use crate::compiler::{
    CompileError, CompiledModule, CompiledSpirv, IShaderCompiler, Reflection, Source,
};
use crate::process::{self, ToolError, ToolRun, WorkDir};
use crate::reflect;
use crate::tools::{self, ToolPaths};
use assets::dshd::Stage;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long one tool run may take.
const TIME_LIMIT: Duration = Duration::from_secs(60);

/// glslang plus SPIRV-Cross.
#[derive(Clone, Debug)]
pub struct ExternalCompiler {
    paths: ToolPaths,
}

fn tool_error(e: &ToolError) -> CompileError {
    CompileError::Tool(e.to_string())
}

fn io_error(e: &std::io::Error) -> CompileError {
    CompileError::Io(e.to_string())
}

fn extension(stage: Stage) -> &'static str {
    match stage {
        Stage::Vertex => "vert",
        Stage::Fragment => "frag",
    }
}

fn version_of(
    program: &Path,
    flag: &str,
    check: fn(&str) -> Result<(), String>,
) -> Result<(), CompileError> {
    let work = WorkDir::create().map_err(|e| io_error(&e))?;
    let args = [flag.into()];
    let run = process::run(program, &args, work.path(), "version", TIME_LIMIT)
        .map_err(|e| tool_error(&e))?;
    check(&run.output).map_err(CompileError::Tool)
}

impl ExternalCompiler {
    /// Checks that both tools start and have the pinned versions.
    ///
    /// # Errors
    /// [`CompileError::Tool`] naming the tool and the problem.
    pub fn new(paths: &ToolPaths) -> Result<Self, CompileError> {
        // Children run in a scratch directory, so a relative tool path would no longer resolve.
        let paths = ToolPaths {
            glslang: tools::absolutize(&paths.glslang),
            spirv_cross: tools::absolutize(&paths.spirv_cross),
        };
        version_of(&paths.glslang, "--version", tools::check_glslang)?;
        version_of(&paths.spirv_cross, "--revision", tools::check_spirv_cross)?;
        Ok(Self { paths })
    }
}

fn require_success(run: &ToolRun, tool: &'static str) -> Result<(), CompileError> {
    if run.success {
        Ok(())
    } else {
        Err(CompileError::Diagnostics {
            tool,
            text: run.output.clone(),
        })
    }
}

/// The most a result file (SPIR-V excluded) may hold: the `dshd` code limit and the JSON limit.
const MAX_RESULT_LEN: usize = 1 << 20;
/// The most SPIR-V a result may hold: the `dshd` code limit.
const MAX_SPIRV_LEN: usize = assets::dshd::MAX_CODE_LEN;

fn read_text(path: &Path) -> Result<String, CompileError> {
    let bytes = process::read_bounded(path, MAX_RESULT_LEN).map_err(|e| tool_error(&e))?;
    Ok(tools::normalize_newlines(&String::from_utf8_lossy(&bytes)))
}

impl ExternalCompiler {
    /// Runs glslang on `source` inside `dir`; returns the path of the SPIR-V file it wrote.
    fn to_spirv(&self, dir: &Path, source: Source<'_>) -> Result<PathBuf, CompileError> {
        let input = dir.join(format!("in.{}", extension(source.stage)));
        let spirv = dir.join("out.spv");
        fs::write(&input, source.text).map_err(|e| io_error(&e))?;
        let glslang = process::run(
            &self.paths.glslang,
            &tools::glslang_args(&input, &spirv),
            dir,
            "glslang",
            TIME_LIMIT,
        )
        .map_err(|e| tool_error(&e))?;
        require_success(&glslang, "glslang")?;
        Ok(spirv)
    }

    /// Runs SPIRV-Cross for the reflection of `spirv` and reads it.
    fn reflect(&self, dir: &Path, spirv: &Path, stage: Stage) -> Result<Reflection, CompileError> {
        let json = dir.join("out.json");
        let to_json = process::run(
            &self.paths.spirv_cross,
            &tools::cross_reflect_args(spirv, &json),
            dir,
            "cross-reflect",
            TIME_LIMIT,
        )
        .map_err(|e| tool_error(&e))?;
        require_success(&to_json, "spirv-cross")?;
        Ok(reflect::parse(&read_text(&json)?, stage)?)
    }
}

impl IShaderCompiler for ExternalCompiler {
    fn compile_spirv(&self, source: Source<'_>) -> Result<CompiledSpirv, CompileError> {
        if source.text.trim().is_empty() {
            return Err(CompileError::EmptySource);
        }
        let work = WorkDir::create().map_err(|e| io_error(&e))?;
        let spirv = self.to_spirv(work.path(), source)?;
        let reflection = self.reflect(work.path(), &spirv, source.stage)?;
        let code = process::read_bounded(&spirv, MAX_SPIRV_LEN).map_err(|e| tool_error(&e))?;
        Ok(CompiledSpirv {
            stage: source.stage,
            code,
            reflection,
        })
    }

    fn compile(&self, source: Source<'_>) -> Result<CompiledModule, CompileError> {
        if source.text.trim().is_empty() {
            return Err(CompileError::EmptySource);
        }
        let work = WorkDir::create().map_err(|e| io_error(&e))?;
        let dir = work.path();
        let glsl = dir.join("out.glsl");
        let spirv = self.to_spirv(dir, source)?;

        let to_glsl = process::run(
            &self.paths.spirv_cross,
            &tools::cross_glsl_args(&spirv, &glsl),
            dir,
            "cross-glsl",
            TIME_LIMIT,
        )
        .map_err(|e| tool_error(&e))?;
        require_success(&to_glsl, "spirv-cross")?;

        let reflection = self.reflect(dir, &spirv, source.stage)?;
        Ok(CompiledModule {
            stage: source.stage,
            code: read_text(&glsl)?,
            reflection,
        })
    }
}

#[cfg(test)]
mod tests;
