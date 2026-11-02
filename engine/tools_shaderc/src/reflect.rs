//! From the `--reflect` JSON of SPIRV-Cross to the engine's own [`Reflection`] record (design:
//! `wiki/architecture/render-port-v0.md` section 7: runtime code never sees a compiler's reflection
//! format).
//!
//! Only what the OpenGL adapter needs is read: vertex inputs (float vectors), one uniform block in
//! set 0, one combined sampler in set 0. Anything else is refused with a reason instead of being
//! dropped, so a shader never loses a resource silently.

use crate::compiler::Reflection;
use crate::json::{Json, JsonError, parse as parse_json};
use assets::dshd::{MAX_INPUTS, NO_BINDING, Stage, UniformBlock, VertexInput};
use core::fmt;

/// Why a reflection record could not be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectError {
    /// The JSON text is not valid.
    Json(JsonError),
    /// The shader uses something the engine's record cannot hold; the text says what.
    Unsupported(&'static str),
    /// The JSON has a different shape than SPIRV-Cross writes; the text says where.
    Malformed(&'static str),
}

impl fmt::Display for ReflectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "{e}"),
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
            Self::Malformed(what) => write!(f, "malformed reflection: {what}"),
        }
    }
}

impl std::error::Error for ReflectError {}

impl From<JsonError> for ReflectError {
    fn from(e: JsonError) -> Self {
        Self::Json(e)
    }
}

/// Resource kinds the record cannot hold, with the message for each.
const REFUSED: [(&str, &str); 7] = [
    (
        "separate_images",
        "separate texture and sampler objects; write one combined sampler2D (SPIRV-Cross drops the bindings of separate ones in OpenGL GLSL)",
    ),
    (
        "separate_samplers",
        "separate texture and sampler objects; write one combined sampler2D",
    ),
    ("push_constants", "push constants"),
    ("ssbos", "storage buffers"),
    ("images", "storage images"),
    ("subpass_inputs", "subpass inputs"),
    ("atomic_counters", "atomic counters"),
];

/// The list under `key`. SPIRV-Cross leaves out a category that has no entries, so an absent key
/// is an empty list; a key that is present but not a list is malformed, never silently empty.
fn entries<'j>(root: &'j Json, key: &'static str) -> Result<&'j [Json], ReflectError> {
    match root.get(key) {
        None => Ok(&[]),
        Some(value) => value
            .as_array()
            .ok_or(ReflectError::Malformed("a resource list is not an array")),
    }
}

/// Arrays of resources (`"array": [n]`) are not supported.
fn not_an_array(entry: &Json) -> Result<(), ReflectError> {
    if entry.get("array").is_some() {
        Err(ReflectError::Unsupported(
            "arrays of resources or vertex inputs",
        ))
    } else {
        Ok(())
    }
}

fn small(value: Option<&Json>, what: &'static str) -> Result<u8, ReflectError> {
    let number = value
        .and_then(Json::as_u64)
        .ok_or(ReflectError::Malformed(what))?;
    u8::try_from(number).map_err(|_| ReflectError::Unsupported(what))
}

fn components(entry: &Json) -> Result<u8, ReflectError> {
    let kind = entry
        .get("type")
        .and_then(Json::as_str)
        .ok_or(ReflectError::Malformed("vertex input type"))?;
    match kind {
        "float" => Ok(1),
        "vec2" => Ok(2),
        "vec3" => Ok(3),
        "vec4" => Ok(4),
        _ => Err(ReflectError::Unsupported(
            "vertex input type other than float, vec2, vec3 or vec4",
        )),
    }
}

fn inputs(root: &Json) -> Result<Vec<VertexInput>, ReflectError> {
    let list = entries(root, "inputs")?;
    if list.len() > MAX_INPUTS {
        return Err(ReflectError::Unsupported("more than 16 vertex inputs"));
    }
    let mut out = list
        .iter()
        .map(|e| {
            not_an_array(e)?;
            Ok(VertexInput {
                location: small(e.get("location"), "vertex input location")?,
                components: components(e)?,
            })
        })
        .collect::<Result<Vec<_>, ReflectError>>()?;
    out.sort_by_key(|i| i.location);
    Ok(out)
}

/// The binding of the only entry of `key`, which must live in set 0 (a missing set means 0).
fn only_binding(
    root: &Json,
    key: &'static str,
    what: &'static str,
) -> Result<Option<(u8, Json)>, ReflectError> {
    let list = entries(root, key)?;
    let Some(entry) = list.first() else {
        return Ok(None);
    };
    if list.len() > 1 {
        return Err(ReflectError::Unsupported(what));
    }
    not_an_array(entry)?;
    if entry.get("set").map_or(Ok(0), |s| small(Some(s), what))? != 0 {
        return Err(ReflectError::Unsupported(what));
    }
    let binding = small(entry.get("binding"), what)?;
    if binding == NO_BINDING {
        return Err(ReflectError::Unsupported(what));
    }
    Ok(Some((binding, entry.clone())))
}

fn uniform(root: &Json) -> Result<Option<UniformBlock>, ReflectError> {
    let what = "more than one uniform block, a set other than 0, or a binding of 255 or more";
    let Some((binding, entry)) = only_binding(root, "ubos", what)? else {
        return Ok(None);
    };
    let size = entry
        .get("block_size")
        .and_then(Json::as_u64)
        .ok_or(ReflectError::Malformed("uniform block size"))?;
    let size = u32::try_from(size).map_err(|_| ReflectError::Unsupported("uniform block size"))?;
    if size == 0 {
        return Err(ReflectError::Unsupported("empty uniform block"));
    }
    Ok(Some(UniformBlock { binding, size }))
}

fn texture(root: &Json) -> Result<Option<u8>, ReflectError> {
    let what = "more than one sampler, a set other than 0, or a binding of 255 or more";
    Ok(only_binding(root, "textures", what)?.map(|(binding, _)| binding))
}

/// Builds the record for a module of `stage` from SPIRV-Cross `--reflect` output.
///
/// # Errors
/// [`ReflectError`]: invalid JSON, an unexpected shape, or a resource the record cannot hold.
pub fn parse(json: &str, stage: Stage) -> Result<Reflection, ReflectError> {
    let root = parse_json(json)?;
    if !matches!(root, Json::Object(_)) {
        return Err(ReflectError::Malformed("the top level is not an object"));
    }
    for (key, reason) in REFUSED {
        if !entries(&root, key)?.is_empty() {
            return Err(ReflectError::Unsupported(reason));
        }
    }
    Ok(Reflection {
        uniform: uniform(&root)?,
        texture_binding: texture(&root)?,
        inputs: match stage {
            Stage::Vertex => inputs(&root)?,
            Stage::Fragment => Vec::new(),
        },
    })
}

#[cfg(test)]
mod tests;
