//! The Null shader compiler: no tools, no GPU. It accepts any non-empty text and returns it
//! unchanged with an empty reflection. Used by tests of code that needs an [`IShaderCompiler`].

use crate::compiler::{CompileError, CompiledModule, IShaderCompiler, Reflection, Source};

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
