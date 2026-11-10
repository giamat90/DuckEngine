//! Turning compiled modules into the bytes of a `dshd` file.

use crate::compiler::{CompiledModule, CompiledSpirv};
use assets::dshd::{
    DshdError, ModuleSpec, SpirvModuleSpec, encode_into, encode_spirv_into, encoded_len,
    spirv_encoded_len,
};

/// Encodes the modules (one per stage, vertex and fragment) as a `dshd` v0 file.
///
/// # Errors
/// The [`DshdError`] of the encoder: no module, a duplicated stage, a missing binding, code too
/// long, and the like.
pub fn pack(modules: &[CompiledModule]) -> Result<Vec<u8>, DshdError> {
    let specs: Vec<ModuleSpec<'_>> = modules
        .iter()
        .map(|m| ModuleSpec {
            stage: m.stage,
            uniform: m.reflection.uniform,
            texture_binding: m.reflection.texture_binding,
            inputs: &m.reflection.inputs,
            code: &m.code,
        })
        .collect();
    let mut bytes = vec![0u8; encoded_len(&specs)?];
    let written = encode_into(&mut bytes, &specs)?;
    bytes.truncate(written);
    Ok(bytes)
}

/// Encodes the modules (one per stage) as a `dshd` v0 file with the SPIR-V backend.
///
/// # Errors
/// The [`DshdError`] of the encoder, and [`DshdError::BadSpirv`] for a binary that is not whole
/// words or lacks the SPIR-V magic.
pub fn pack_spirv(modules: &[CompiledSpirv]) -> Result<Vec<u8>, DshdError> {
    let specs: Vec<SpirvModuleSpec<'_>> = modules
        .iter()
        .map(|m| SpirvModuleSpec {
            stage: m.stage,
            uniform: m.reflection.uniform,
            texture_binding: m.reflection.texture_binding,
            inputs: &m.reflection.inputs,
            code: &m.code,
        })
        .collect();
    let mut bytes = vec![0u8; spirv_encoded_len(&specs)?];
    let written = encode_spirv_into(&mut bytes, &specs)?;
    bytes.truncate(written);
    Ok(bytes)
}

#[cfg(test)]
mod tests;
