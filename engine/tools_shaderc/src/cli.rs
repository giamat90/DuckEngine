//! The command line:
//! `shaderc [--tools DIR] [--backend glsl|spirv] <input.vert|input.frag>... -o <output.dshd>`.
//!
//! The backend is the kind of code the file carries: GLSL 450 text (the default, for the OpenGL
//! adapter) or SPIR-V (for the Vulkan adapter); a repeated `--backend` keeps the last one. The stage
//! of each input comes from its extension. Tool locations come from `--tools`, else the
//! environment (`DUCK_SHADER_TOOLS`, `DUCK_GLSLANG`, `DUCK_SPIRV_CROSS`), else the default folder.
//! The output file appears whole or not at all: it is written beside the target and renamed.

use crate::compiler::{CompileError, IShaderCompiler, Source};
use crate::external::ExternalCompiler;
use crate::pack::{pack, pack_spirv};
use crate::tools::ToolPaths;
use assets::dshd::{Backend, DshdError, Stage};
use core::fmt;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

/// Usage text printed on a wrong command line.
pub const USAGE: &str = "usage: shaderc [--tools DIR] [--backend glsl|spirv] \
<input.vert|input.frag>... -o <output.dshd>";

/// Why a run failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliError {
    /// The arguments were not understood.
    Usage,
    /// An input file name does not end in `.vert` or `.frag`.
    BadExtension(String),
    /// An input file could not be read (or is not text).
    Read(String, String),
    /// The output file could not be written.
    Write(String, String),
    /// A compile failed; the input name and the error.
    Compile(String, CompileError),
    /// The modules do not form a valid program.
    Pack(DshdError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => f.write_str(USAGE),
            Self::BadExtension(name) => write!(
                f,
                "cannot tell the stage of {name}: name it .vert (vertex) or .frag (fragment)"
            ),
            Self::Read(name, why) => write!(f, "cannot read the input file {name}: {why}"),
            Self::Write(name, why) => write!(f, "cannot write the output file {name}: {why}"),
            Self::Compile(name, e) => write!(f, "{name}: {e}"),
            Self::Pack(e) => write!(f, "cannot build the shader program: {e}"),
        }
    }
}

impl std::error::Error for CliError {}

/// Parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// Folder of the tools, when given.
    pub tools: Option<PathBuf>,
    /// Input files with their stage.
    pub inputs: Vec<(Stage, String)>,
    /// Destination path.
    pub output: String,
    /// The kind of code to write; GLSL unless `--backend` says otherwise.
    pub backend: Backend,
}

/// The stage a file name asks for, from its extension.
#[must_use]
pub fn stage_of(name: &str) -> Option<Stage> {
    let extension = Path::new(name).extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("vert") {
        Some(Stage::Vertex)
    } else if extension.eq_ignore_ascii_case("frag") {
        Some(Stage::Fragment)
    } else {
        None
    }
}

fn backend_of(name: &str) -> Option<Backend> {
    match name {
        "glsl" => Some(Backend::Glsl450),
        "spirv" => Some(Backend::SpirV),
        _ => None,
    }
}

/// Parses the arguments after the program name.
///
/// # Errors
/// [`CliError::Usage`] for an unknown flag, a missing value, an unknown backend, an input with an
/// unknown extension, no input or no output.
pub fn parse_args(args: &[String]) -> Result<Args, CliError> {
    let mut tools = None;
    let mut output = None;
    let mut backend = Backend::Glsl450;
    let mut inputs = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--tools" => tools = Some(PathBuf::from(rest.next().ok_or(CliError::Usage)?)),
            "-o" => output = Some(rest.next().ok_or(CliError::Usage)?.clone()),
            "--backend" => {
                backend = rest
                    .next()
                    .map(String::as_str)
                    .and_then(backend_of)
                    .ok_or(CliError::Usage)?;
            }
            flag if flag.starts_with('-') => return Err(CliError::Usage),
            name => {
                let stage =
                    stage_of(name).ok_or_else(|| CliError::BadExtension(name.to_string()))?;
                inputs.push((stage, name.to_string()));
            }
        }
    }
    match (output, inputs.is_empty()) {
        (Some(output), false) => Ok(Args {
            tools,
            inputs,
            output,
            backend,
        }),
        _ => Err(CliError::Usage),
    }
}

/// Writes `bytes` to a temporary file next to `path`, then renames it over `path`.
///
/// # Errors
/// The text of the failed operation. A failed write leaves no temporary file behind.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(format!(".tmp{}", std::process::id()));
    let temp = PathBuf::from(temp);
    let result = fs::write(&temp, bytes).and_then(|()| fs::rename(&temp, path));
    if let Err(e) = result {
        let _ = fs::remove_file(&temp);
        return Err(e.to_string());
    }
    Ok(())
}

fn read_source(name: &str) -> Result<String, CliError> {
    fs::read_to_string(name).map_err(|e| CliError::Read(name.to_string(), e.to_string()))
}

/// Compiles every input with `compile` (one call per file, in order).
fn compile_all<T>(
    inputs: &[(Stage, String)],
    compile: impl Fn(Source<'_>) -> Result<T, CompileError>,
) -> Result<Vec<T>, CliError> {
    let mut modules = Vec::with_capacity(inputs.len());
    for (stage, name) in inputs {
        let text = read_source(name)?;
        let source = Source {
            stage: *stage,
            text: &text,
        };
        modules.push(compile(source).map_err(|e| CliError::Compile(name.clone(), e))?);
    }
    Ok(modules)
}

/// Compiles the inputs with `compiler` and writes the `dshd` file. Nothing is written on failure.
///
/// # Errors
/// See [`CliError`].
pub fn build(compiler: &dyn IShaderCompiler, args: &Args) -> Result<(), CliError> {
    let bytes = match args.backend {
        Backend::Glsl450 => {
            pack(&compile_all(&args.inputs, |s| compiler.compile(s))?).map_err(CliError::Pack)?
        }
        Backend::SpirV => pack_spirv(&compile_all(&args.inputs, |s| compiler.compile_spirv(s))?)
            .map_err(CliError::Pack)?,
    };
    write_atomic(Path::new(&args.output), &bytes)
        .map_err(|why| CliError::Write(args.output.clone(), why))
}

/// The whole program: parse, find and check the tools, build. `env` reads one variable.
///
/// # Errors
/// See [`CliError`].
pub fn run(args: &[String], env: &dyn Fn(&str) -> Option<OsString>) -> Result<(), CliError> {
    let args = parse_args(args)?;
    let paths = match &args.tools {
        Some(root) => ToolPaths::from_root(root),
        None => ToolPaths::resolve(env),
    };
    let compiler =
        ExternalCompiler::new(&paths).map_err(|e| CliError::Compile("tools".to_string(), e))?;
    build(&compiler, &args)
}

#[cfg(test)]
mod tests;
