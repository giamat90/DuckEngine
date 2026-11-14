//! The Null shader compiler: no tools, no GPU. It accepts any non-empty text and returns it
//! unchanged with an empty reflection (as SPIR-V: a bare header). Used by tests of code that needs an
//! [`IShaderCompiler`].

use crate::compiler::{
    CompileError, CompiledModule, CompiledSpirv, IShaderCompiler, Reflection, Source,
};

/// The five header words of an empty SPIR-V module: magic, version 1.0, no generator, bound 1,
/// schema 0.
const SPIRV_HEADER: [u32; 5] = [0x0723_0203, 0x0001_0000, 0, 1, 0];

/// A compiler that does no compiling.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullCompiler;

impl IShaderCompiler for NullCompiler {
    fn compile(&self, source: Source<'_>) -> Result<CompiledModule, CompileError> {
        if source.text.trim().is_empty() {
            return Err(CompileError::EmptySource);
        }
        Ok(CompiledModule {
            stage: source.stage,
            code: source.text.to_string(),
            reflection: Reflection::default(),
        })
    }

    fn compile_spirv(&self, source: Source<'_>) -> Result<CompiledSpirv, CompileError> {
        if source.text.trim().is_empty() {
            return Err(CompileError::EmptySource);
        }
        Ok(CompiledSpirv {
            stage: source.stage,
            code: SPIRV_HEADER.iter().flat_map(|w| w.to_le_bytes()).collect(),
            reflection: Reflection::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract;

    #[test]
    fn passes_the_shared_contract() {
        contract::shared(&NullCompiler);
    }

    #[test]
    fn passes_the_shared_spirv_contract() {
        contract::shared_spirv(&NullCompiler);
    }

    #[test]
    fn its_spirv_is_a_bare_header_with_an_empty_reflection() {
        let source = Source {
            stage: assets::dshd::Stage::Fragment,
            text: "void main() {}\n",
        };
        let module = NullCompiler.compile_spirv(source);
        assert!(module.is_ok_and(|m| m.code.len() == 20
            && m.reflection == Reflection::default()
            && m.stage == assets::dshd::Stage::Fragment));
    }

    #[test]
    fn returns_the_text_with_an_empty_reflection() {
        let source = Source {
            stage: assets::dshd::Stage::Fragment,
            text: "void main() {}\n",
        };
        let module = NullCompiler.compile(source);
        assert!(module.is_ok_and(|m| m.code == "void main() {}\n"
            && m.reflection == Reflection::default()
            && m.stage == assets::dshd::Stage::Fragment));
    }
}
