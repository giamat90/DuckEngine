//! The shader compiler port, `IShaderCompiler`: GLSL 450 source in, one backend module (GLSL 450
//! text for OpenGL, or a SPIR-V binary for Vulkan) and the engine's reflection record out.
//!
//! The port is declared here, by the only code that needs it, a build-time tool at layer 4. The
//! runtime (`render`, the adapters) never sees a compiler: it reads the `dshd` file this tool writes
//! (ADR-0009, `wiki/architecture/render-port-v0.md` section 7). Implementations: [`crate::null`]
//! and [`crate::external`] (glslang plus SPIRV-Cross). A contract suite in [`crate::contract`]
//! holds what every implementation must do.

use crate::reflect::ReflectError;
use assets::dshd::{Stage, UniformBlock, VertexInput};
use core::fmt;

/// What the engine needs to know about a module besides its code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reflection {
    /// The one uniform block, if the module has one.
    pub uniform: Option<UniformBlock>,
    /// Binding (texture unit) of the one combined sampler, if the module has one.
    pub texture_binding: Option<u8>,
    /// Vertex inputs in location order (always empty for a fragment module).
    pub inputs: Vec<VertexInput>,
}

/// One shader stage as text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source<'a> {
    /// The stage the text is written for.
    pub stage: Stage,
    /// GLSL 450 text with Vulkan-style bindings (ADR-0009).
    pub text: &'a str,
}

/// The result for one stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledModule {
    /// The stage that was compiled.
    pub stage: Stage,
    /// The backend text (GLSL 450 for OpenGL), `\n` line ends, entry point `main`.
    pub code: String,
    /// The reflection record.
    pub reflection: Reflection,
}

/// The SPIR-V result for one stage (card 6-4): the binary glslang made, with the same reflection
/// record as the GLSL output of the same source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledSpirv {
    /// The stage that was compiled.
    pub stage: Stage,
    /// The SPIR-V binary: whole little-endian words, entry point `main`.
    pub code: Vec<u8>,
    /// The reflection record.
    pub reflection: Reflection,
}

/// Why a compile failed. Every variant carries what a person needs to fix it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    /// The source text is empty.
    EmptySource,
    /// A tool is missing, has the wrong version, could not be started or ran out of time.
    Tool(String),
    /// The compiler rejected the source; the text is the tool's own diagnostics.
    Diagnostics {
        /// Which tool produced them.
        tool: &'static str,
        /// The diagnostics as printed.
        text: String,
    },
    /// The reflection record could not be made.
    Reflect(ReflectError),
    /// A file or directory operation failed.
    Io(String),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySource => f.write_str("the shader source is empty"),
            Self::Tool(text) => write!(f, "tool problem: {text}"),
            Self::Diagnostics { tool, text } => write!(f, "{tool} rejected the source:\n{text}"),
            Self::Reflect(e) => write!(f, "reflection: {e}"),
            Self::Io(text) => write!(f, "i/o: {text}"),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<ReflectError> for CompileError {
    fn from(e: ReflectError) -> Self {
        Self::Reflect(e)
    }
}

/// A compiler from source text to a backend module. Implementations do not keep state between
/// calls that changes a result.
pub trait IShaderCompiler {
    /// Compiles one stage.
    ///
    /// # Errors
    /// [`CompileError::EmptySource`] for an empty text, [`CompileError::Diagnostics`] for source the
    /// compiler rejects, the other variants for tool and file problems. Never panics.
    fn compile(&self, source: Source<'_>) -> Result<CompiledModule, CompileError>;

    /// Compiles one stage to a SPIR-V binary, for the Vulkan adapter. The same source and the same
    /// errors as [`IShaderCompiler::compile`]; the reflection equals the one of the GLSL output.
    ///
    /// # Errors
    /// As [`IShaderCompiler::compile`].
    fn compile_spirv(&self, source: Source<'_>) -> Result<CompiledSpirv, CompileError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_its_own_message() {
        let all = [
            CompileError::EmptySource,
            CompileError::Tool("t".to_string()),
            CompileError::Diagnostics {
                tool: "glslang",
                text: "bad".to_string(),
            },
            CompileError::Reflect(ReflectError::Unsupported("x")),
            CompileError::Io("disk".to_string()),
        ];
        let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
        for (i, a) in texts.iter().enumerate() {
            assert!(!a.is_empty());
            assert!(texts[i + 1..].iter().all(|b| a != b));
        }
        assert!(texts[2].contains("glslang") && texts[2].contains("bad"));
        assert!(texts[1].contains('t') && texts[4].contains("disk"));
        assert_eq!(
            CompileError::from(ReflectError::Unsupported("x")),
            CompileError::Reflect(ReflectError::Unsupported("x"))
        );
    }
}
