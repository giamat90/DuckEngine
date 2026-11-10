//! The contract suite of [`IShaderCompiler`]: what every implementation must do. `shared` holds
//! the rules for all of them (Null included); `real` adds the rules only a real compiler can meet.
//! Each implementation's own test module calls these.

use crate::compiler::{CompileError, IShaderCompiler, Source};
use assets::dshd::{Stage, UniformBlock, VertexInput};

/// The sprite vertex shader (also the source of the checked-in golden asset).
pub const SPRITE_VERT: &str = include_str!("../shaders/sprite.vert");
/// The sprite fragment shader.
pub const SPRITE_FRAG: &str = include_str!("../shaders/sprite.frag");
/// A fragment shader that does not compile.
pub const BROKEN_FRAG: &str = include_str!("../shaders/broken.frag");

fn vertex(text: &str) -> Source<'_> {
    Source {
        stage: Stage::Vertex,
        text,
    }
}

/// Rules for every implementation.
///
/// # Panics
/// When a rule is broken; that is the test failing.
pub fn shared(compiler: &dyn IShaderCompiler) {
    assert_eq!(
        compiler.compile(vertex("")).err(),
        Some(CompileError::EmptySource)
    );
    assert_eq!(
        compiler.compile(vertex("  \n\t ")).err(),
        Some(CompileError::EmptySource)
    );
    let first = compiler.compile(vertex(SPRITE_VERT));
    let second = compiler.compile(vertex(SPRITE_VERT));
    assert!(first.is_ok(), "a valid vertex shader compiles");
    assert_eq!(first, second, "compiling twice gives the same result");
    let module = first.ok();
    assert!(module.is_some_and(|m| m.stage == Stage::Vertex && !m.code.is_empty()));
}

/// Rules only a real compiler can meet.
///
/// # Panics
/// When a rule is broken; that is the test failing.
pub fn real(compiler: &dyn IShaderCompiler) {
    shared(compiler);
    let broken = compiler.compile(Source {
        stage: Stage::Fragment,
        text: BROKEN_FRAG,
    });
    assert!(matches!(broken, Err(CompileError::Diagnostics { .. })));
    let vert = compiler.compile(vertex(SPRITE_VERT));
    let reflection = vert.map(|m| m.reflection).unwrap_or_default();
    assert_eq!(
        reflection.uniform,
        Some(UniformBlock {
            binding: 0,
            size: 64
        })
    );
    assert_eq!(
        reflection.inputs,
        vec![
            VertexInput {
                location: 0,
                components: 2
            },
            VertexInput {
                location: 1,
                components: 2
            },
            VertexInput {
                location: 2,
                components: 4
            },
        ]
    );
    let frag = compiler.compile(Source {
        stage: Stage::Fragment,
        text: SPRITE_FRAG,
    });
    let reflection = frag.map(|m| m.reflection).unwrap_or_default();
    assert_eq!(reflection.texture_binding, Some(0));
    assert_eq!(reflection.uniform, None);
    assert!(reflection.inputs.is_empty());
}

/// Rules for the SPIR-V output of every implementation: the same checks on empty text, the same
/// answer twice, and a binary that is whole words starting with the SPIR-V magic.
///
/// # Panics
/// When a rule is broken; that is the test failing.
pub fn shared_spirv(compiler: &dyn IShaderCompiler) {
    assert_eq!(
        compiler.compile_spirv(vertex("")).err(),
        Some(CompileError::EmptySource)
    );
    assert_eq!(
        compiler.compile_spirv(vertex("  \n\t ")).err(),
        Some(CompileError::EmptySource)
    );
    let first = compiler.compile_spirv(vertex(SPRITE_VERT));
    let second = compiler.compile_spirv(vertex(SPRITE_VERT));
    assert!(first.is_ok(), "a valid vertex shader compiles");
    assert_eq!(first, second, "compiling twice gives the same result");
    let module = first.ok();
    assert!(module.is_some_and(|m| m.stage == Stage::Vertex
        && m.code.len().is_multiple_of(4)
        && m.code.starts_with(&0x0723_0203u32.to_le_bytes())));
}

/// Rules only a real compiler can meet for the SPIR-V output: broken source is rejected, and the
/// reflection is the one the GLSL output carries (both come from the same SPIR-V).
///
/// # Panics
/// When a rule is broken; that is the test failing.
pub fn real_spirv(compiler: &dyn IShaderCompiler) {
    shared_spirv(compiler);
    let broken = compiler.compile_spirv(Source {
        stage: Stage::Fragment,
        text: BROKEN_FRAG,
    });
    assert!(matches!(broken, Err(CompileError::Diagnostics { .. })));
    for (stage, text) in [(Stage::Vertex, SPRITE_VERT), (Stage::Fragment, SPRITE_FRAG)] {
        let source = Source { stage, text };
        let glsl = compiler.compile(source).map(|m| m.reflection);
        let spirv = compiler.compile_spirv(source).map(|m| m.reflection);
        assert!(glsl.is_ok(), "{glsl:?}");
        assert_eq!(glsl, spirv, "one reflection for both outputs ({stage:?})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{CompiledModule, CompiledSpirv, Reflection};
    use crate::null::NullCompiler;

    /// Accepts everything, including empty text.
    struct Careless;

    impl IShaderCompiler for Careless {
        fn compile(&self, source: Source<'_>) -> Result<CompiledModule, CompileError> {
            Ok(CompiledModule {
                stage: source.stage,
                code: source.text.to_string(),
                reflection: Reflection::default(),
            })
        }

        fn compile_spirv(&self, source: Source<'_>) -> Result<CompiledSpirv, CompileError> {
            Ok(CompiledSpirv {
                stage: source.stage,
                code: source.text.as_bytes().to_vec(),
                reflection: Reflection::default(),
            })
        }
    }

    #[test]
    #[should_panic(expected = "assertion")]
    fn shared_rejects_a_compiler_that_accepts_empty_text() {
        shared(&Careless);
    }

    #[test]
    #[should_panic(expected = "assertion")]
    fn shared_spirv_rejects_a_compiler_that_accepts_empty_text() {
        shared_spirv(&Careless);
    }

    /// Returns a SPIR-V binary that has the right size but not the magic.
    struct NoMagic;

    impl IShaderCompiler for NoMagic {
        fn compile(&self, source: Source<'_>) -> Result<CompiledModule, CompileError> {
            NullCompiler.compile(source)
        }

        fn compile_spirv(&self, source: Source<'_>) -> Result<CompiledSpirv, CompileError> {
            let good = NullCompiler.compile_spirv(source)?;
            Ok(CompiledSpirv {
                code: vec![0; good.code.len()],
                ..good
            })
        }
    }

    #[test]
    #[should_panic(expected = "assertion")]
    fn shared_spirv_rejects_a_binary_without_the_magic() {
        shared_spirv(&NoMagic);
    }

    #[test]
    #[should_panic(expected = "assertion")]
    fn real_spirv_rejects_a_compiler_that_accepts_broken_source() {
        real_spirv(&NullCompiler);
    }

    #[test]
    #[should_panic(expected = "assertion")]
    fn real_rejects_a_compiler_that_accepts_broken_source() {
        real(&NullCompiler);
    }
}
