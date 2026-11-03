//! What the port's descriptions mean in OpenGL: enum values and sizes, as plain data.
//!
//! No GL call lives here, so the tables are unit-tested without a driver.

use render::desc::{AddressMode, Filter, IndexType, ShaderStage, TextureFormat, VertexFormat};
use render::error::RenderError;

// Texture formats.
const RGBA8: u32 = 0x8058;
const DEPTH_COMPONENT32F: u32 = 0x8CAC;
const RGBA: u32 = 0x1908;
const BGRA: u32 = 0x80E1;
const DEPTH_COMPONENT: u32 = 0x1902;
// Data types.
pub(crate) const UNSIGNED_BYTE: u32 = 0x1401;
pub(crate) const UNSIGNED_SHORT: u32 = 0x1403;
pub(crate) const UNSIGNED_INT: u32 = 0x1405;
pub(crate) const FLOAT: u32 = 0x1406;
// Sampler parameters.
pub(crate) const TEXTURE_MIN_FILTER: u32 = 0x2801;
pub(crate) const TEXTURE_MAG_FILTER: u32 = 0x2800;
pub(crate) const TEXTURE_WRAP_S: u32 = 0x2802;
pub(crate) const TEXTURE_WRAP_T: u32 = 0x2803;
const NEAREST: u32 = 0x2600;
const LINEAR: u32 = 0x2601;
const NEAREST_MIPMAP_NEAREST: u32 = 0x2700;
const LINEAR_MIPMAP_LINEAR: u32 = 0x2703;
const CLAMP_TO_EDGE: u32 = 0x812F;
const REPEAT: u32 = 0x2901;
// Shader stages.
const VERTEX_SHADER: u32 = 0x8B31;
const FRAGMENT_SHADER: u32 = 0x8B30;

/// How a texture format is stored on the GPU and how bytes cross the API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextureLayout {
    /// The sized internal format (`glTextureStorage2D`).
    pub internal: u32,
    /// The client pixel format (`glTextureSubImage2D`, `glGetTextureImage`).
    pub format: u32,
    /// The client component type.
    pub data_type: u32,
}

/// BGRA is stored as RGBA and exchanged as BGRA, so the bytes a caller writes are the bytes it
/// reads back, like the Null device keeps them.
pub(crate) fn texture_layout(format: TextureFormat) -> TextureLayout {
    match format {
        TextureFormat::Rgba8 => TextureLayout {
            internal: RGBA8,
            format: RGBA,
            data_type: UNSIGNED_BYTE,
        },
        TextureFormat::Bgra8 => TextureLayout {
            internal: RGBA8,
            format: BGRA,
            data_type: UNSIGNED_BYTE,
        },
        TextureFormat::Depth32F => TextureLayout {
            internal: DEPTH_COMPONENT32F,
            format: DEPTH_COMPONENT,
            data_type: FLOAT,
        },
    }
}

/// One vertex attribute as `glVertexArrayAttribFormat` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AttributeLayout {
    /// Components, 1 to 4.
    pub components: i32,
    /// The component type.
    pub data_type: u32,
    /// Whether integers are normalised to 0..1.
    pub normalized: bool,
}

pub(crate) fn attribute_layout(format: VertexFormat) -> AttributeLayout {
    let (components, data_type, normalized) = match format {
        VertexFormat::Float2 => (2, FLOAT, false),
        VertexFormat::Float3 => (3, FLOAT, false),
        VertexFormat::Float4 => (4, FLOAT, false),
        VertexFormat::Unorm8x4 => (4, UNSIGNED_BYTE, true),
    };
    AttributeLayout {
        components,
        data_type,
        normalized,
    }
}

/// The minification filter. A sampler does not know its texture, and a texture with one level
/// has no other level to blend, so the mipmapped modes are safe for every texture.
pub(crate) fn min_filter(filter: Filter) -> u32 {
    match filter {
        Filter::Nearest => NEAREST_MIPMAP_NEAREST,
        Filter::Linear => LINEAR_MIPMAP_LINEAR,
    }
}

pub(crate) fn mag_filter(filter: Filter) -> u32 {
    match filter {
        Filter::Nearest => NEAREST,
        Filter::Linear => LINEAR,
    }
}

pub(crate) fn wrap_mode(mode: AddressMode) -> u32 {
    match mode {
        AddressMode::Clamp => CLAMP_TO_EDGE,
        AddressMode::Repeat => REPEAT,
    }
}

pub(crate) fn shader_kind(stage: ShaderStage) -> u32 {
    match stage {
        ShaderStage::Vertex => VERTEX_SHADER,
        ShaderStage::Fragment => FRAGMENT_SHADER,
    }
}

pub(crate) fn index_type(index_type: IndexType) -> u32 {
    match index_type {
        IndexType::U16 => UNSIGNED_SHORT,
        IndexType::U32 => UNSIGNED_INT,
    }
}

/// The entry point of every GL shader module: GLSL has no other.
pub(crate) const ENTRY: &str = "main";

const OUT_OF_MEMORY: u32 = 0x0505;

/// What an error flag from `glGetError` (0 is none) means to the port: the driver ran out of
/// memory, or the device did something the driver refused (any other flag).
pub(crate) fn error_from_gl(code: u32) -> Result<(), RenderError> {
    match code {
        0 => Ok(()),
        OUT_OF_MEMORY => Err(RenderError::OutOfMemory),
        _ => Err(RenderError::Unsupported),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_formats_are_rgba8_inside_and_differ_in_the_client_order() {
        let rgba = texture_layout(TextureFormat::Rgba8);
        let bgra = texture_layout(TextureFormat::Bgra8);
        assert_eq!(rgba.internal, bgra.internal);
        assert_eq!((rgba.format, bgra.format), (0x1908, 0x80E1));
        assert_eq!(rgba.data_type, UNSIGNED_BYTE);
        assert_eq!(bgra.data_type, UNSIGNED_BYTE);
    }

    #[test]
    fn depth_is_a_32_bit_float() {
        let depth = texture_layout(TextureFormat::Depth32F);
        assert_eq!(depth.internal, 0x8CAC);
        assert_eq!(depth.format, 0x1902);
        assert_eq!(depth.data_type, FLOAT);
    }

    #[test]
    fn attributes_follow_their_formats() {
        let float3 = attribute_layout(VertexFormat::Float3);
        assert_eq!((float3.components, float3.data_type), (3, FLOAT));
        assert!(!float3.normalized);
        assert_eq!(attribute_layout(VertexFormat::Float2).components, 2);
        assert_eq!(attribute_layout(VertexFormat::Float4).components, 4);
        let colour = attribute_layout(VertexFormat::Unorm8x4);
        assert_eq!((colour.components, colour.data_type), (4, UNSIGNED_BYTE));
        assert!(colour.normalized);
    }

    #[test]
    fn filters_and_wrap_modes_map_one_to_one() {
        assert_eq!(min_filter(Filter::Nearest), 0x2700);
        assert_eq!(min_filter(Filter::Linear), 0x2703);
        assert_eq!(mag_filter(Filter::Nearest), 0x2600);
        assert_eq!(mag_filter(Filter::Linear), 0x2601);
        assert_eq!(wrap_mode(AddressMode::Clamp), 0x812F);
        assert_eq!(wrap_mode(AddressMode::Repeat), 0x2901);
    }

    #[test]
    fn gl_error_flags_become_port_errors() {
        assert_eq!(error_from_gl(0), Ok(()));
        assert_eq!(error_from_gl(0x0505), Err(RenderError::OutOfMemory));
        assert_eq!(error_from_gl(0x0502), Err(RenderError::Unsupported));
        assert_eq!(error_from_gl(0x0500), Err(RenderError::Unsupported));
    }

    #[test]
    fn stages_and_index_types_map_to_their_enums() {
        assert_eq!(shader_kind(ShaderStage::Vertex), 0x8B31);
        assert_eq!(shader_kind(ShaderStage::Fragment), 0x8B30);
        assert_eq!(index_type(IndexType::U16), UNSIGNED_SHORT);
        assert_eq!(index_type(IndexType::U32), UNSIGNED_INT);
    }
}
