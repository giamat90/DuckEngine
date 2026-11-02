//! Turning compiled modules into the bytes of a `dshd` file.

use crate::compiler::CompiledModule;
use assets::dshd::{DshdError, ModuleSpec, encode_into, encoded_len};

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

#[cfg(test)]
mod tests;
