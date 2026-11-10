//! The `dshd` v0 shader program format and its reader (card 3-11; the shader asset of
//! `wiki/architecture/render-port-v0.md` sections 7 and 8, decided by ADR-0009).
//!
//! A file holds one or two shader modules (a vertex and a fragment module) for one backend, with
//! the engine's own small reflection record, so runtime code never sees the reflection format of a
//! compiler. The backend is GLSL 450 text (for the OpenGL adapter) or SPIR-V (for the Vulkan
//! adapter, card 6-4). All integers are little-endian.
//!
//! Header (8 bytes):
//!
//! | offset | size | field |
//! |---|---|---|
//! | 0 | 4 | magic `DSHD` |
//! | 4 | 2 | version (0) |
//! | 6 | 1 | backend (0 = GLSL 450 text, 1 = SPIR-V) |
//! | 7 | 1 | module count (1 or 2) |
//!
//! Then, per module, a 16-byte record, `input_count` entries of 2 bytes, and the code:
//!
//! | offset | size | field |
//! |---|---|---|
//! | 0 | 1 | stage (0 = vertex, 1 = fragment) |
//! | 1 | 1 | vertex input count (0 to 16; always 0 for a fragment module) |
//! | 2 | 1 | uniform block binding (255 = none) |
//! | 3 | 1 | texture binding (255 = none) |
//! | 4 | 4 | uniform block size in bytes (0 when there is no block) |
//! | 8 | 4 | code length in bytes (1 up to 1 MiB) |
//! | 12 | 4 | reserved, zero |
//!
//! An input entry is the shader input location and its float component count (1 to 4), in strictly
//! increasing location order. The entry point of every module is `main`. For backend 0 the code is
//! UTF-8 text. For backend 1 the code is a SPIR-V binary: a whole number of 32-bit words, at least
//! the 5-word header, starting with the magic `0x07230203` in little-endian byte order; anything
//! else is refused before a driver could see it. The file length must equal the header plus all
//! modules exactly. The reader borrows the bytes and never allocates.

use core::fmt;

/// Size of the fixed header in bytes.
pub const HEADER_LEN: usize = 8;
/// Size of one module record in bytes.
pub const RECORD_LEN: usize = 16;
/// Value of a binding field that means "none".
pub const NO_BINDING: u8 = 255;
/// Most vertex inputs a module can declare.
pub const MAX_INPUTS: usize = 16;
/// Longest code accepted, in bytes.
pub const MAX_CODE_LEN: usize = 1 << 20;

const MAGIC: [u8; 4] = *b"DSHD";
const VERSION: u16 = 0;
const BACKEND_GLSL_450: u8 = 0;
const BACKEND_SPIRV: u8 = 1;
/// The first word of every SPIR-V binary, read as little-endian bytes.
const SPIRV_MAGIC: [u8; 4] = 0x0723_0203u32.to_le_bytes();
/// Bytes of a SPIR-V word.
const SPIRV_WORD: usize = 4;
/// Shortest SPIR-V binary: the five header words.
const SPIRV_MIN_LEN: usize = 5 * SPIRV_WORD;
const MAX_MODULES: usize = 2;
const INPUT_LEN: usize = 2;
const MAX_COMPONENTS: u8 = 4;

/// Why a file was refused, or an encode call was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DshdError {
    /// Fewer bytes than the header, a record or its code need (also a too small output buffer).
    Truncated,
    /// The first four bytes are not `DSHD`.
    BadMagic,
    /// A version this reader does not know.
    UnsupportedVersion(u16),
    /// A backend this reader does not know.
    UnsupportedBackend(u8),
    /// The module count is not 1 or 2.
    BadModuleCount,
    /// A stage byte this reader does not know.
    BadStage(u8),
    /// Two modules of the same stage.
    DuplicateStage,
    /// Too many inputs, a fragment module with inputs, a bad component count or an unordered list.
    BadInputs,
    /// A binding and a size that do not belong together, or the "none" value used as a binding.
    BadBinding,
    /// A module without code.
    EmptyCode,
    /// Code longer than [`MAX_CODE_LEN`].
    CodeTooLong,
    /// GLSL code that is not UTF-8.
    BadCode,
    /// SPIR-V code that is not whole words, is shorter than the SPIR-V header, or lacks the magic.
    BadSpirv,
    /// A reserved field is not zero.
    ReservedNonZero,
    /// Bytes remain after the last module.
    TrailingData,
}

impl fmt::Display for DshdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated"),
            Self::BadMagic => f.write_str("bad magic"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version {v}"),
            Self::UnsupportedBackend(b) => write!(f, "unsupported backend {b}"),
            Self::BadModuleCount => f.write_str("bad module count"),
            Self::BadStage(s) => write!(f, "bad stage {s}"),
            Self::DuplicateStage => f.write_str("two modules of one stage"),
            Self::BadInputs => f.write_str("bad vertex inputs"),
            Self::BadBinding => f.write_str("bad binding or block size"),
            Self::EmptyCode => f.write_str("empty code"),
            Self::CodeTooLong => f.write_str("code too long"),
            Self::BadCode => f.write_str("code is not UTF-8"),
            Self::BadSpirv => f.write_str("code is not a SPIR-V binary"),
            Self::ReservedNonZero => f.write_str("reserved field is not zero"),
            Self::TrailingData => f.write_str("trailing data"),
        }
    }
}

impl std::error::Error for DshdError {}

/// The kind of code a file carries (the backend byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// GLSL 450 text, for the OpenGL adapter.
    Glsl450,
    /// A SPIR-V binary, for the Vulkan adapter.
    SpirV,
}

impl Backend {
    fn to_byte(self) -> u8 {
        match self {
            Self::Glsl450 => BACKEND_GLSL_450,
            Self::SpirV => BACKEND_SPIRV,
        }
    }

    fn from_byte(byte: u8) -> Result<Self, DshdError> {
        match byte {
            BACKEND_GLSL_450 => Ok(Self::Glsl450),
            BACKEND_SPIRV => Ok(Self::SpirV),
            other => Err(DshdError::UnsupportedBackend(other)),
        }
    }
}

/// The pipeline stage of a module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Vertex stage.
    Vertex,
    /// Fragment stage.
    Fragment,
}

impl Stage {
    fn to_byte(self) -> u8 {
        match self {
            Self::Vertex => 0,
            Self::Fragment => 1,
        }
    }

    fn from_byte(byte: u8) -> Result<Self, DshdError> {
        match byte {
            0 => Ok(Self::Vertex),
            1 => Ok(Self::Fragment),
            other => Err(DshdError::BadStage(other)),
        }
    }
}

/// One vertex shader input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VertexInput {
    /// The `location` of the input.
    pub location: u8,
    /// Float components: 1 (`float`) to 4 (`vec4`).
    pub components: u8,
}

/// The uniform block of a module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UniformBlock {
    /// Binding point of the block.
    pub binding: u8,
    /// Size of the block in bytes (not zero).
    pub size: u32,
}

/// What the encoder needs to write one module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleSpec<'a> {
    /// Stage of the module.
    pub stage: Stage,
    /// Its uniform block, if it has one.
    pub uniform: Option<UniformBlock>,
    /// Binding (texture unit) of its sampled texture, if it has one.
    pub texture_binding: Option<u8>,
    /// Vertex inputs, in increasing location order (empty for a fragment module).
    pub inputs: &'a [VertexInput],
    /// The GLSL 450 text.
    pub code: &'a str,
}

/// What the encoder needs to write one SPIR-V module: like [`ModuleSpec`], with a binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpirvModuleSpec<'a> {
    /// Stage of the module.
    pub stage: Stage,
    /// Its uniform block, if it has one.
    pub uniform: Option<UniformBlock>,
    /// Binding of its sampled texture, if it has one.
    pub texture_binding: Option<u8>,
    /// Vertex inputs, in increasing location order (empty for a fragment module).
    pub inputs: &'a [VertexInput],
    /// The SPIR-V binary (whole little-endian words, see the module documentation).
    pub code: &'a [u8],
}

/// The fields both spec types share; the code as bytes.
struct Raw<'a> {
    stage: Stage,
    uniform: Option<UniformBlock>,
    texture_binding: Option<u8>,
    inputs: &'a [VertexInput],
    code: &'a [u8],
}

trait AsRaw {
    fn raw(&self) -> Raw<'_>;
}

impl AsRaw for ModuleSpec<'_> {
    fn raw(&self) -> Raw<'_> {
        Raw {
            stage: self.stage,
            uniform: self.uniform,
            texture_binding: self.texture_binding,
            inputs: self.inputs,
            code: self.code.as_bytes(),
        }
    }
}

impl AsRaw for SpirvModuleSpec<'_> {
    fn raw(&self) -> Raw<'_> {
        Raw {
            stage: self.stage,
            uniform: self.uniform,
            texture_binding: self.texture_binding,
            inputs: self.inputs,
            code: self.code,
        }
    }
}

fn check_inputs(spec: &Raw<'_>) -> Result<(), DshdError> {
    let fragment_has_inputs = spec.stage == Stage::Fragment && !spec.inputs.is_empty();
    let widths_ok = spec
        .inputs
        .iter()
        .all(|i| (1..=MAX_COMPONENTS).contains(&i.components));
    let ordered = spec
        .inputs
        .windows(2)
        .all(|w| w[0].location < w[1].location);
    if spec.inputs.len() > MAX_INPUTS || fragment_has_inputs || !widths_ok || !ordered {
        return Err(DshdError::BadInputs);
    }
    Ok(())
}

fn check_bindings(spec: &Raw<'_>) -> Result<(), DshdError> {
    let block_ok = spec
        .uniform
        .is_none_or(|u| u.binding != NO_BINDING && u.size > 0);
    let texture_ok = spec.texture_binding != Some(NO_BINDING);
    if block_ok && texture_ok {
        Ok(())
    } else {
        Err(DshdError::BadBinding)
    }
}

/// Whether `code` is a SPIR-V binary as far as the header goes: whole words, at least the five
/// header words, the magic first. The rest is the driver's to validate.
fn is_spirv(code: &[u8]) -> bool {
    code.len() >= SPIRV_MIN_LEN
        && code.len().is_multiple_of(SPIRV_WORD)
        && code.starts_with(&SPIRV_MAGIC)
}

fn check_code(code: &[u8], backend: Backend) -> Result<(), DshdError> {
    match code.len() {
        0 => Err(DshdError::EmptyCode),
        n if n > MAX_CODE_LEN => Err(DshdError::CodeTooLong),
        _ if backend == Backend::SpirV && !is_spirv(code) => Err(DshdError::BadSpirv),
        _ => Ok(()),
    }
}

fn check_module(spec: &Raw<'_>, backend: Backend) -> Result<(), DshdError> {
    check_inputs(spec)?;
    check_bindings(spec)?;
    check_code(spec.code, backend)
}

fn check_modules<T: AsRaw>(modules: &[T], backend: Backend) -> Result<(), DshdError> {
    if modules.is_empty() || modules.len() > MAX_MODULES {
        return Err(DshdError::BadModuleCount);
    }
    if let [first, second] = modules
        && first.raw().stage == second.raw().stage
    {
        return Err(DshdError::DuplicateStage);
    }
    modules
        .iter()
        .try_for_each(|m| check_module(&m.raw(), backend))
}

fn total_len<T: AsRaw>(modules: &[T], backend: Backend) -> Result<usize, DshdError> {
    check_modules(modules, backend)?;
    Ok(modules.iter().fold(HEADER_LEN, |sum, m| {
        let raw = m.raw();
        sum + RECORD_LEN + raw.inputs.len() * INPUT_LEN + raw.code.len()
    }))
}

/// Total file length for GLSL `modules`.
///
/// # Errors
/// The first problem found in the modules, see [`DshdError`].
pub fn encoded_len(modules: &[ModuleSpec<'_>]) -> Result<usize, DshdError> {
    total_len(modules, Backend::Glsl450)
}

/// Total file length for SPIR-V `modules`.
///
/// # Errors
/// The first problem found in the modules, see [`DshdError`]; a binary that is not SPIR-V is
/// [`DshdError::BadSpirv`].
pub fn spirv_encoded_len(modules: &[SpirvModuleSpec<'_>]) -> Result<usize, DshdError> {
    total_len(modules, Backend::SpirV)
}

fn record(spec: &Raw<'_>, code_len: u32) -> [u8; RECORD_LEN] {
    let mut r = [0u8; RECORD_LEN];
    r[0] = spec.stage.to_byte();
    r[1] = u8::try_from(spec.inputs.len()).unwrap_or(u8::MAX);
    r[2] = spec.uniform.map_or(NO_BINDING, |u| u.binding);
    r[3] = spec.texture_binding.unwrap_or(NO_BINDING);
    r[4..8].copy_from_slice(&spec.uniform.map_or(0, |u| u.size).to_le_bytes());
    r[8..12].copy_from_slice(&code_len.to_le_bytes());
    r
}

fn write_module<'o>(out: &'o mut [u8], spec: &Raw<'_>) -> Result<&'o mut [u8], DshdError> {
    let code_len = u32::try_from(spec.code.len()).map_err(|_| DshdError::CodeTooLong)?;
    let (head, rest) = out
        .split_at_mut_checked(RECORD_LEN)
        .ok_or(DshdError::Truncated)?;
    head.copy_from_slice(&record(spec, code_len));
    let (inputs, rest) = rest
        .split_at_mut_checked(spec.inputs.len() * INPUT_LEN)
        .ok_or(DshdError::Truncated)?;
    for (slot, input) in inputs.chunks_exact_mut(INPUT_LEN).zip(spec.inputs) {
        slot.copy_from_slice(&[input.location, input.components]);
    }
    let (code, rest) = rest
        .split_at_mut_checked(spec.code.len())
        .ok_or(DshdError::Truncated)?;
    code.copy_from_slice(spec.code);
    Ok(rest)
}

fn write_all<T: AsRaw>(
    out: &mut [u8],
    modules: &[T],
    backend: Backend,
) -> Result<usize, DshdError> {
    let total = total_len(modules, backend)?;
    let dst = out.get_mut(..total).ok_or(DshdError::Truncated)?;
    let (head, mut rest) = dst
        .split_at_mut_checked(HEADER_LEN)
        .ok_or(DshdError::Truncated)?;
    head[0..4].copy_from_slice(&MAGIC);
    head[4..6].copy_from_slice(&VERSION.to_le_bytes());
    head[6] = backend.to_byte();
    head[7] = u8::try_from(modules.len()).map_err(|_| DshdError::BadModuleCount)?;
    for module in modules {
        rest = write_module(rest, &module.raw())?;
    }
    Ok(total)
}

/// Writes the GLSL file for `modules` into `out`; returns the bytes written. Never allocates.
///
/// # Errors
/// The modules are checked like [`encoded_len`]; [`DshdError::Truncated`] when `out` is too small.
pub fn encode_into(out: &mut [u8], modules: &[ModuleSpec<'_>]) -> Result<usize, DshdError> {
    write_all(out, modules, Backend::Glsl450)
}

/// Writes the SPIR-V file for `modules` into `out`; returns the bytes written. Never allocates.
///
/// # Errors
/// The modules are checked like [`spirv_encoded_len`]; [`DshdError::Truncated`] when `out` is too
/// small.
pub fn encode_spirv_into(
    out: &mut [u8],
    modules: &[SpirvModuleSpec<'_>],
) -> Result<usize, DshdError> {
    write_all(out, modules, Backend::SpirV)
}

/// A validated view over a `dshd` file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program<'a> {
    backend: Backend,
    count: u8,
    data: &'a [u8],
}

/// One module of a [`Program`], borrowed from the file bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module<'a> {
    stage: Stage,
    uniform: Option<UniformBlock>,
    texture_binding: Option<u8>,
    inputs: &'a [u8],
    code: &'a [u8],
    backend: Backend,
    /// The code as text; empty for a SPIR-V module.
    text: &'a str,
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let raw = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(raw.try_into().ok()?))
}

fn binding(byte: u8) -> Option<u8> {
    (byte != NO_BINDING).then_some(byte)
}

/// Reads the module at the start of `data`; returns it and the bytes after it.
fn read_module(data: &[u8], backend: Backend) -> Result<(Module<'_>, &[u8]), DshdError> {
    let head = data
        .first_chunk::<RECORD_LEN>()
        .ok_or(DshdError::Truncated)?;
    let stage = Stage::from_byte(head[0])?;
    let input_count = usize::from(head[1]);
    let size = le_u32(head, 4).ok_or(DshdError::Truncated)?;
    let code_len = le_u32(head, 8).ok_or(DshdError::Truncated)?;
    if le_u32(head, 12) != Some(0) {
        return Err(DshdError::ReservedNonZero);
    }
    let code_len = usize::try_from(code_len).map_err(|_| DshdError::CodeTooLong)?;
    if input_count > MAX_INPUTS || (stage == Stage::Fragment && input_count > 0) {
        return Err(DshdError::BadInputs);
    }
    if code_len == 0 {
        return Err(DshdError::EmptyCode);
    }
    if code_len > MAX_CODE_LEN {
        return Err(DshdError::CodeTooLong);
    }
    let after = data.get(RECORD_LEN..).ok_or(DshdError::Truncated)?;
    let (inputs, after) = after
        .split_at_checked(input_count * INPUT_LEN)
        .ok_or(DshdError::Truncated)?;
    let (code, after) = after
        .split_at_checked(code_len)
        .ok_or(DshdError::Truncated)?;
    let text = match backend {
        Backend::Glsl450 => core::str::from_utf8(code).map_err(|_| DshdError::BadCode)?,
        Backend::SpirV => {
            if !is_spirv(code) {
                return Err(DshdError::BadSpirv);
            }
            ""
        }
    };
    let uniform = binding(head[2]).map(|b| UniformBlock { binding: b, size });
    let block_matches = uniform.is_some() == (size > 0);
    if !block_matches || !inputs_valid(inputs) {
        return Err(if block_matches {
            DshdError::BadInputs
        } else {
            DshdError::BadBinding
        });
    }
    let module = Module {
        stage,
        uniform,
        texture_binding: binding(head[3]),
        inputs,
        code,
        backend,
        text,
    };
    Ok((module, after))
}

fn inputs_valid(inputs: &[u8]) -> bool {
    let widths_ok = inputs
        .chunks_exact(INPUT_LEN)
        .all(|e| (1..=MAX_COMPONENTS).contains(&e[1]));
    let mut locations = inputs.chunks_exact(INPUT_LEN).map(|e| e[0]);
    let first = locations.next();
    let ordered = locations
        .try_fold(first, |prev, next| {
            prev.filter(|p| *p < next).map(|_| Some(next))
        })
        .is_some();
    widths_ok && ordered
}

/// Checks `count` modules in a row; returns the bytes after the last one.
fn walk(data: &[u8], count: u8, backend: Backend) -> Result<&[u8], DshdError> {
    let mut rest = data;
    let mut previous = None;
    for _ in 0..count {
        let (module, after) = read_module(rest, backend)?;
        if previous == Some(module.stage) {
            return Err(DshdError::DuplicateStage);
        }
        previous = Some(module.stage);
        rest = after;
    }
    Ok(rest)
}

impl<'a> Program<'a> {
    /// Validates `bytes` completely (header, every record, sizes against the length) and returns a
    /// view.
    ///
    /// # Errors
    /// The first problem found, see [`DshdError`]. Hostile input yields an error, never a panic.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, DshdError> {
        let head = bytes
            .first_chunk::<HEADER_LEN>()
            .ok_or(DshdError::Truncated)?;
        if head[0..4] != MAGIC {
            return Err(DshdError::BadMagic);
        }
        let version = u16::from_le_bytes([head[4], head[5]]);
        if version != VERSION {
            return Err(DshdError::UnsupportedVersion(version));
        }
        let backend = Backend::from_byte(head[6])?;
        let count = head[7];
        if count == 0 || usize::from(count) > MAX_MODULES {
            return Err(DshdError::BadModuleCount);
        }
        let data = bytes.get(HEADER_LEN..).ok_or(DshdError::Truncated)?;
        let rest = walk(data, count, backend)?;
        if rest.is_empty() {
            Ok(Self {
                backend,
                count,
                data,
            })
        } else {
            Err(DshdError::TrailingData)
        }
    }

    /// The kind of code the file carries.
    #[must_use]
    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// Number of modules (1 or 2).
    #[must_use]
    pub fn module_count(&self) -> usize {
        usize::from(self.count)
    }

    /// The module at `index` in file order, or `None` past the last one.
    #[must_use]
    pub fn module(&self, index: usize) -> Option<Module<'a>> {
        if index >= self.module_count() {
            return None;
        }
        let mut rest = self.data;
        for _ in 0..index {
            rest = read_module(rest, self.backend).ok()?.1;
        }
        read_module(rest, self.backend)
            .ok()
            .map(|(module, _)| module)
    }

    /// The module of `stage`, if the file has one.
    #[must_use]
    pub fn module_for(&self, stage: Stage) -> Option<Module<'a>> {
        (0..self.module_count())
            .filter_map(|i| self.module(i))
            .find(|m| m.stage == stage)
    }
}

impl<'a> Module<'a> {
    /// Stage of the module.
    #[must_use]
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Its uniform block, if it has one.
    #[must_use]
    pub fn uniform(&self) -> Option<UniformBlock> {
        self.uniform
    }

    /// Binding (texture unit) of its sampled texture, if it has one.
    #[must_use]
    pub fn texture_binding(&self) -> Option<u8> {
        self.texture_binding
    }

    /// The GLSL 450 text; the entry point is `main`. Empty for a SPIR-V module (see
    /// [`Module::spirv`]). Check [`Program::backend`] first: a SPIR-V file given to a GLSL
    /// consumer would otherwise hand it an empty shader.
    #[must_use]
    pub fn code(&self) -> &'a str {
        self.text
    }

    /// The SPIR-V binary (whole little-endian words, magic checked) of a SPIR-V module; `None` for
    /// a GLSL module. The entry point is `main`. The slice is only byte-aligned (it sits at an
    /// arbitrary offset in the file): never cast it to `*const u32`, copy it with
    /// [`Module::spirv_words`].
    #[must_use]
    pub fn spirv(&self) -> Option<&'a [u8]> {
        (self.backend == Backend::SpirV).then_some(self.code)
    }

    /// The SPIR-V binary as 32-bit words, for a driver that wants a `u32` array (the words are
    /// read from the bytes, so alignment does not matter); `None` for a GLSL module.
    #[must_use]
    pub fn spirv_words(self) -> Option<impl Iterator<Item = u32> + 'a> {
        self.spirv().map(|bytes| {
            bytes
                .chunks_exact(SPIRV_WORD)
                .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        })
    }

    /// Number of vertex inputs.
    #[must_use]
    pub fn input_count(&self) -> usize {
        self.inputs.len() / INPUT_LEN
    }

    /// The vertex inputs in location order.
    pub fn inputs(&self) -> impl Iterator<Item = VertexInput> + 'a {
        self.inputs.chunks_exact(INPUT_LEN).map(|e| VertexInput {
            location: e[0],
            components: e[1],
        })
    }
}

#[cfg(test)]
mod tests;
