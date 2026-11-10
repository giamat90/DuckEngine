//! SPIR-V modules as the adapter takes them: a check of the words the driver will be handed, and
//! the adapter's own numbering of descriptor sets.
//!
//! The shader files carry the sets and bindings their source declared, and the sources of the
//! engine write every resource at `set = 0` (a uniform block and a texture both at binding 0
//! is allowed in OpenGL, where they are two namespaces, and not in one Vulkan set). The adapter
//! therefore owns the set numbers: uniform blocks go to [`SET_UNIFORMS`], sampled textures to
//! [`SET_TEXTURES`], and the binding number stays what the source wrote, which is the slot the
//! stream binds (as on OpenGL). The module is changed in the copy this file makes, never in the
//! asset.

/// The first word of every SPIR-V module.
const MAGIC: u32 = 0x0723_0203;
/// Words of the module header; the first instruction follows.
const HEADER_WORDS: usize = 5;

/// Descriptor set of the uniform blocks.
pub const SET_UNIFORMS: u32 = 0;
/// Descriptor set of the sampled textures.
pub const SET_TEXTURES: u32 = 1;
/// Bindings per set: the port's slots.
pub const BINDINGS: u32 = 8;

const OP_VARIABLE: u32 = 59;
const OP_DECORATE: u32 = 71;
const DECORATION_BINDING: u32 = 33;
const DECORATION_DESCRIPTOR_SET: u32 = 34;
const CLASS_UNIFORM_CONSTANT: u32 = 0;
const CLASS_UNIFORM: u32 = 2;
const CLASS_STORAGE_BUFFER: u32 = 12;

/// Why a blob is not a module the adapter takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpirvError {
    /// The length is not a multiple of four bytes.
    NotWords,
    /// Fewer words than the header.
    TooShort,
    /// The first word is not the SPIR-V magic (or the module is big-endian).
    BadMagic,
    /// An instruction has no words, or runs past the end.
    Malformed,
    /// A resource the fixed binding scheme has no place for: a storage buffer, or a binding
    /// beyond the port's slots.
    UnsupportedResource,
}

/// The little-endian words of `code`, checked for length and magic.
///
/// # Errors
/// [`SpirvError::NotWords`], [`SpirvError::TooShort`] or [`SpirvError::BadMagic`].
pub fn words(code: &[u8]) -> Result<Vec<u32>, SpirvError> {
    if !code.len().is_multiple_of(4) {
        return Err(SpirvError::NotWords);
    }
    let words: Vec<u32> = code
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    if words.len() < HEADER_WORDS {
        return Err(SpirvError::TooShort);
    }
    if words[0] != MAGIC {
        return Err(SpirvError::BadMagic);
    }
    Ok(words)
}

/// What an instruction is: its opcode and the words of its operands.
struct Instruction<'a> {
    opcode: u32,
    operands: &'a [u32],
}

/// The instructions after the header, or the error of the first one that does not fit.
fn instructions(words: &[u32]) -> Result<Vec<(usize, Instruction<'_>)>, SpirvError> {
    let mut found = Vec::new();
    let mut at = HEADER_WORDS;
    while at < words.len() {
        let head = words[at];
        let length = (head >> 16) as usize;
        if length == 0 || at + length > words.len() {
            return Err(SpirvError::Malformed);
        }
        found.push((
            at,
            Instruction {
                opcode: head & 0xFFFF,
                operands: &words[at + 1..at + length],
            },
        ));
        at += length;
    }
    Ok(found)
}

/// The storage class of each variable, sorted by result id, so a lookup is a binary search.
fn classes(found: &[(usize, Instruction<'_>)]) -> Vec<(u32, u32)> {
    let mut classes: Vec<(u32, u32)> = found
        .iter()
        .filter(|(_, instruction)| instruction.opcode == OP_VARIABLE)
        .filter_map(|(_, instruction)| {
            Some((*instruction.operands.get(1)?, *instruction.operands.get(2)?))
        })
        .collect();
    classes.sort_unstable();
    classes
}

fn class_of(classes: &[(u32, u32)], id: u32) -> Option<u32> {
    let at = classes
        .binary_search_by_key(&id, |(known, _)| *known)
        .ok()?;
    Some(classes[at].1)
}

/// Puts every uniform block in [`SET_UNIFORMS`] and every sampled texture in [`SET_TEXTURES`].
///
/// # Errors
/// [`SpirvError::Malformed`] for a broken instruction list; [`SpirvError::UnsupportedResource`]
/// for a storage buffer or a binding number of [`BINDINGS`] or more.
pub fn retarget(words: &mut [u32]) -> Result<(), SpirvError> {
    let mut edits = Vec::new();
    let found = instructions(words)?;
    let classes = classes(&found);
    for (at, instruction) in found {
        if instruction.opcode != OP_DECORATE || instruction.operands.len() < 3 {
            continue;
        }
        let (id, decoration, value) = (
            instruction.operands[0],
            instruction.operands[1],
            instruction.operands[2],
        );
        if decoration == DECORATION_BINDING && value >= BINDINGS {
            return Err(SpirvError::UnsupportedResource);
        }
        if decoration != DECORATION_DESCRIPTOR_SET {
            continue;
        }
        let set = match class_of(&classes, id) {
            Some(CLASS_UNIFORM) => SET_UNIFORMS,
            Some(CLASS_UNIFORM_CONSTANT) => SET_TEXTURES,
            Some(CLASS_STORAGE_BUFFER) => return Err(SpirvError::UnsupportedResource),
            _ => continue,
        };
        edits.push((at + 3, set));
    }
    for (at, set) in edits {
        words[at] = set;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
