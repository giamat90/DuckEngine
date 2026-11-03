//! The OpenGL 4.5 functions the render device calls: Direct State Access objects, state and
//! draws, declared by hand and looked up in the context (card 3-10; the context's own handful
//! is in `gl.rs`).
//!
//! [`Api`] is a table of function pointers plus safe wrappers. A wrapper is only sound when the
//! context that loaded the table is current on the calling thread; [`Api`] is private to the
//! crate and only [`GlContext`](crate::GlContext) (not `Send`, current on its thread from the
//! moment it is made) hands out a reference. Object names are plain numbers: a wrong name only
//! raises a GL error flag. Every wrapper that passes a pointer derives the size from the slice
//! it takes, never from a second argument.
#![expect(
    unsafe_code,
    reason = "OpenGL is a C interface; every unsafe block has a SAFETY comment"
)]

use core::ffi::{CStr, c_void};
use core::ptr::{null, without_provenance};

const TEXTURE_2D: u32 = 0x0DE1;
const UNIFORM_BUFFER: u32 = 0x8A11;
const DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const COLOR_ATTACHMENT0: u32 = 0x8CE0;
const DEPTH_ATTACHMENT: u32 = 0x8D00;
const NONE: u32 = 0;
const COLOR: u32 = 0x1800;
const DEPTH: u32 = 0x1801;
const DYNAMIC_STORAGE_BIT: u32 = 0x0100;
const COMPILE_STATUS: u32 = 0x8B81;
const LINK_STATUS: u32 = 0x8B82;
const TRIANGLES: u32 = 0x0004;
const BLEND: u32 = 0x0BE2;
const DEPTH_TEST: u32 = 0x0B71;
const CULL_FACE: u32 = 0x0B44;
const SRC_ALPHA: u32 = 0x0302;
const ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
const ONE: u32 = 1;
const LESS_OR_EQUAL: u32 = 0x0203;
const UNPACK_ALIGNMENT: u32 = 0x0CF5;
const PACK_ALIGNMENT: u32 = 0x0D05;
const TEXTURE_MAX_LEVEL: u32 = 0x813D;
/// Longest shader or program info log kept, in bytes.
const LOG_LIMIT: usize = 1024;
/// `GL_MAX_TEXTURE_SIZE` and `GL_UNIFORM_BUFFER_OFFSET_ALIGNMENT`.
pub const MAX_TEXTURE_SIZE: u32 = 0x0D33;
pub const UNIFORM_BUFFER_OFFSET_ALIGNMENT: u32 = 0x8A34;

function_table! {
    /// The OpenGL 4.5 core functions of the render device.
    pub(crate) struct Api {
        create_buffers: unsafe extern "system" fn(i32, *mut u32) = c"glCreateBuffers",
        named_buffer_storage: unsafe extern "system" fn(u32, isize, *const c_void, u32) = c"glNamedBufferStorage",
        named_buffer_sub_data: unsafe extern "system" fn(u32, isize, isize, *const c_void) = c"glNamedBufferSubData",
        delete_buffers: unsafe extern "system" fn(i32, *const u32) = c"glDeleteBuffers",
        create_textures: unsafe extern "system" fn(u32, i32, *mut u32) = c"glCreateTextures",
        texture_storage_2d: unsafe extern "system" fn(u32, i32, u32, i32, i32) = c"glTextureStorage2D",
        texture_sub_image_2d: unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void) = c"glTextureSubImage2D",
        get_texture_image: unsafe extern "system" fn(u32, i32, u32, u32, i32, *mut c_void) = c"glGetTextureImage",
        delete_textures: unsafe extern "system" fn(i32, *const u32) = c"glDeleteTextures",
        create_samplers: unsafe extern "system" fn(i32, *mut u32) = c"glCreateSamplers",
        sampler_parameter_i: unsafe extern "system" fn(u32, u32, i32) = c"glSamplerParameteri",
        delete_samplers: unsafe extern "system" fn(i32, *const u32) = c"glDeleteSamplers",
        create_shader: unsafe extern "system" fn(u32) -> u32 = c"glCreateShader",
        shader_source: unsafe extern "system" fn(u32, i32, *const *const u8, *const i32) = c"glShaderSource",
        compile_shader: unsafe extern "system" fn(u32) = c"glCompileShader",
        get_shader_iv: unsafe extern "system" fn(u32, u32, *mut i32) = c"glGetShaderiv",
        delete_shader: unsafe extern "system" fn(u32) = c"glDeleteShader",
        create_program: unsafe extern "system" fn() -> u32 = c"glCreateProgram",
        attach_shader: unsafe extern "system" fn(u32, u32) = c"glAttachShader",
        detach_shader: unsafe extern "system" fn(u32, u32) = c"glDetachShader",
        link_program: unsafe extern "system" fn(u32) = c"glLinkProgram",
        get_program_iv: unsafe extern "system" fn(u32, u32, *mut i32) = c"glGetProgramiv",
        delete_program: unsafe extern "system" fn(u32) = c"glDeleteProgram",
        use_program: unsafe extern "system" fn(u32) = c"glUseProgram",
        create_vertex_arrays: unsafe extern "system" fn(i32, *mut u32) = c"glCreateVertexArrays",
        enable_vertex_array_attrib: unsafe extern "system" fn(u32, u32) = c"glEnableVertexArrayAttrib",
        vertex_array_attrib_format: unsafe extern "system" fn(u32, u32, i32, u32, u8, u32) = c"glVertexArrayAttribFormat",
        vertex_array_attrib_binding: unsafe extern "system" fn(u32, u32, u32) = c"glVertexArrayAttribBinding",
        vertex_array_vertex_buffer: unsafe extern "system" fn(u32, u32, u32, isize, i32) = c"glVertexArrayVertexBuffer",
        vertex_array_element_buffer: unsafe extern "system" fn(u32, u32) = c"glVertexArrayElementBuffer",
        bind_vertex_array: unsafe extern "system" fn(u32) = c"glBindVertexArray",
        delete_vertex_arrays: unsafe extern "system" fn(i32, *const u32) = c"glDeleteVertexArrays",
        bind_buffer_base: unsafe extern "system" fn(u32, u32, u32) = c"glBindBufferBase",
        bind_texture_unit: unsafe extern "system" fn(u32, u32) = c"glBindTextureUnit",
        bind_sampler: unsafe extern "system" fn(u32, u32) = c"glBindSampler",
        create_framebuffers: unsafe extern "system" fn(i32, *mut u32) = c"glCreateFramebuffers",
        named_framebuffer_texture: unsafe extern "system" fn(u32, u32, u32, i32) = c"glNamedFramebufferTexture",
        named_framebuffer_draw_buffer: unsafe extern "system" fn(u32, u32) = c"glNamedFramebufferDrawBuffer",
        check_named_framebuffer_status: unsafe extern "system" fn(u32, u32) -> u32 = c"glCheckNamedFramebufferStatus",
        bind_framebuffer: unsafe extern "system" fn(u32, u32) = c"glBindFramebuffer",
        delete_framebuffers: unsafe extern "system" fn(i32, *const u32) = c"glDeleteFramebuffers",
        clear_named_framebuffer_fv: unsafe extern "system" fn(u32, u32, i32, *const f32) = c"glClearNamedFramebufferfv",
        enable: unsafe extern "system" fn(u32) = c"glEnable",
        disable: unsafe extern "system" fn(u32) = c"glDisable",
        blend_func_separate: unsafe extern "system" fn(u32, u32, u32, u32) = c"glBlendFuncSeparate",
        depth_mask: unsafe extern "system" fn(u8) = c"glDepthMask",
        depth_func: unsafe extern "system" fn(u32) = c"glDepthFunc",
        pixel_store_i: unsafe extern "system" fn(u32, i32) = c"glPixelStorei",
        draw_arrays_instanced: unsafe extern "system" fn(u32, i32, i32, i32) = c"glDrawArraysInstanced",
        draw_elements_instanced: unsafe extern "system" fn(u32, i32, u32, *const c_void, i32) = c"glDrawElementsInstanced",
        get_shader_info_log: unsafe extern "system" fn(u32, i32, *mut i32, *mut u8) = c"glGetShaderInfoLog",
        get_program_info_log: unsafe extern "system" fn(u32, i32, *mut i32, *mut u8) = c"glGetProgramInfoLog",
        texture_parameter_i: unsafe extern "system" fn(u32, u32, i32) = c"glTextureParameteri",
        get_integer_v: unsafe extern "system" fn(u32, *mut i32) = c"glGetIntegerv",
    }
}

/// The shape of a vertex attribute for the vertex array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AttribFormat {
    pub components: i32,
    pub data_type: u32,
    pub normalized: bool,
    pub offset: u32,
}

/// Storage of a texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextureStorage {
    pub levels: i32,
    pub internal: u32,
    pub width: i32,
    pub height: i32,
}

/// A rectangle of texels, in the `i32` OpenGL takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Client pixel format and type of a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Transfer {
    pub format: u32,
    pub data_type: u32,
}

/// What a draw takes besides the pipeline state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexedDraw {
    pub count: i32,
    pub index_type: u32,
    /// Byte offset of the first index inside the element buffer.
    pub offset: usize,
    pub instances: i32,
}

/// Which attachment of a framebuffer a texture goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Attachment {
    Color,
    Depth,
}

impl Api {
    /// Looks the functions up with `lookup`, the proc-address function of a context that is
    /// current on this thread; `Err` names the first function the driver lacks.
    pub fn load_current(lookup: impl FnMut(&CStr) -> *mut c_void) -> Result<Self, &'static CStr> {
        // SAFETY: OpenGL's proc-address functions return the function with the C signature of
        // the name that was asked for, and the declarations above follow the OpenGL 4.5 core
        // specification.
        unsafe { Self::load(lookup) }
    }

    /// Sets the state the device relies on: byte-packed transfers and no face culling.
    pub fn set_device_defaults(&self) {
        // SAFETY: a context is current; both names are valid pixel-store parameters and 1 is a
        // valid alignment; `CULL_FACE` is a valid capability.
        unsafe {
            (self.pixel_store_i)(UNPACK_ALIGNMENT, 1);
            (self.pixel_store_i)(PACK_ALIGNMENT, 1);
            (self.disable)(CULL_FACE);
        }
    }

    /// One integer state value, for example `MAX_TEXTURE_SIZE`.
    pub fn integer(&self, name: u32) -> i32 {
        let mut value = 0i32;
        // SAFETY: a context is current; the names read through this wrapper return exactly one
        // integer and `value` has room for one.
        unsafe { (self.get_integer_v)(name, &raw mut value) };
        value
    }

    // ---- buffers ----

    /// A new buffer of `size` bytes whose contents can be updated; 0 on failure.
    pub fn create_buffer(&self, size: usize) -> u32 {
        let Ok(size) = isize::try_from(size) else {
            return 0;
        };
        let mut name = 0u32;
        // SAFETY: a context is current; `name` has room for the one name requested; a null
        // data pointer asks for uninitialised storage of `size` bytes.
        unsafe {
            (self.create_buffers)(1, &raw mut name);
            (self.named_buffer_storage)(name, size, null(), DYNAMIC_STORAGE_BIT);
        }
        name
    }

    /// Copies `bytes` into the buffer at `offset`.
    pub fn buffer_sub_data(&self, buffer: u32, offset: usize, bytes: &[u8]) {
        let (Ok(offset), Ok(size)) = (isize::try_from(offset), isize::try_from(bytes.len())) else {
            return;
        };
        // SAFETY: a context is current; `bytes` is `size` readable bytes. A range outside the
        // buffer only raises an error flag.
        unsafe { (self.named_buffer_sub_data)(buffer, offset, size, bytes.as_ptr().cast()) };
    }

    pub fn delete_buffer(&self, buffer: u32) {
        // SAFETY: a context is current; one name is read from a live local.
        unsafe { (self.delete_buffers)(1, &raw const buffer) };
    }

    // ---- textures ----

    /// A new 2D texture with storage; 0 on failure.
    pub fn create_texture(&self, storage: TextureStorage) -> u32 {
        let mut name = 0u32;
        // SAFETY: a context is current; `name` has room for the one name requested; invalid
        // dimensions or formats only raise an error flag.
        unsafe {
            (self.create_textures)(TEXTURE_2D, 1, &raw mut name);
            (self.texture_storage_2d)(
                name,
                storage.levels,
                storage.internal,
                storage.width,
                storage.height,
            );
        }
        name
    }

    /// Limits sampling of `texture` to its `levels` mip levels (`GL_TEXTURE_MAX_LEVEL`).
    pub fn limit_levels(&self, texture: u32, levels: i32) {
        // SAFETY: a context is current; a wrong name or value only raises an error flag.
        unsafe { (self.texture_parameter_i)(texture, TEXTURE_MAX_LEVEL, levels.saturating_sub(1)) };
    }

    /// Writes `bytes` (tightly packed, four bytes per texel) into a region of level 0. `false`
    /// without a call when `bytes` is not exactly the region's size.
    pub fn texture_sub_image(
        &self,
        texture: u32,
        region: Region,
        transfer: Transfer,
        bytes: &[u8],
    ) -> bool {
        let texels = i64::from(region.width) * i64::from(region.height);
        let exact = region.width > 0
            && region.height > 0
            && i64::try_from(bytes.len()).is_ok_and(|len| len == texels * 4);
        if !exact {
            return false;
        }
        // SAFETY: a context is current; the check above shows `bytes` holds exactly
        // width x height texels of four bytes, which is what an unpack alignment of 1 with an
        // RGBA/BGRA/depth transfer of 8-bit or 32-bit components reads.
        unsafe {
            (self.texture_sub_image_2d)(
                texture,
                0,
                region.x,
                region.y,
                region.width,
                region.height,
                transfer.format,
                transfer.data_type,
                bytes.as_ptr().cast(),
            );
        }
        true
    }

    /// Reads level 0 into `out`; GL refuses (error flag) when `out` is too small.
    pub fn get_texture_image(&self, texture: u32, transfer: Transfer, out: &mut [u8]) {
        let Ok(size) = i32::try_from(out.len()) else {
            return;
        };
        // SAFETY: a context is current; `out` is `size` writable bytes and `size` is the buffer
        // size argument, so GL writes no more than that.
        unsafe {
            (self.get_texture_image)(
                texture,
                0,
                transfer.format,
                transfer.data_type,
                size,
                out.as_mut_ptr().cast(),
            );
        }
    }

    pub fn delete_texture(&self, texture: u32) {
        // SAFETY: a context is current; one name is read from a live local.
        unsafe { (self.delete_textures)(1, &raw const texture) };
    }

    // ---- samplers ----

    pub fn create_sampler(&self) -> u32 {
        let mut name = 0u32;
        // SAFETY: a context is current; `name` has room for the one name requested.
        unsafe { (self.create_samplers)(1, &raw mut name) };
        name
    }

    /// Sets an integer sampler parameter (a filter or a wrap mode enum).
    pub fn sampler_parameter(&self, sampler: u32, parameter: u32, value: u32) {
        let value = i32::try_from(value).unwrap_or(0);
        // SAFETY: a context is current; a wrong parameter or value only raises an error flag.
        unsafe { (self.sampler_parameter_i)(sampler, parameter, value) };
    }

    pub fn delete_sampler(&self, sampler: u32) {
        // SAFETY: a context is current; one name is read from a live local.
        unsafe { (self.delete_samplers)(1, &raw const sampler) };
    }

    // ---- shaders and programs ----

    /// Compiles GLSL `source` of the stage `kind`; the driver's info log when it does not.
    pub fn compile_shader(&self, kind: u32, source: &[u8]) -> Result<u32, String> {
        let length = i32::try_from(source.len()).map_err(|_| String::from("source too long"))?;
        let pointer = source.as_ptr();
        let mut status = 0i32;
        // SAFETY: a context is current; one string pointer and one length are read from live
        // locals and describe `source`, which GL copies during `glShaderSource`.
        let shader = unsafe {
            let shader = (self.create_shader)(kind);
            (self.shader_source)(shader, 1, &raw const pointer, &raw const length);
            (self.compile_shader)(shader);
            (self.get_shader_iv)(shader, COMPILE_STATUS, &raw mut status);
            shader
        };
        if status == 0 {
            let log = self.info_log(shader, false);
            self.delete_shader(shader);
            return Err(log);
        }
        Ok(shader)
    }

    /// The info log of a shader or program as text: at most `LOG_LIMIT` bytes, invalid UTF-8
    /// replaced.
    fn info_log(&self, object: u32, program: bool) -> String {
        let mut bytes = [0u8; LOG_LIMIT];
        let mut written = 0i32;
        let capacity = i32::try_from(LOG_LIMIT).unwrap_or(i32::MAX);
        let fetch = if program {
            self.get_program_info_log
        } else {
            self.get_shader_info_log
        };
        // SAFETY: a context is current; `bytes` has `capacity` writable bytes, which GL is told,
        // and `written` has room for the one length it returns.
        unsafe { fetch(object, capacity, &raw mut written, bytes.as_mut_ptr()) };
        let used = usize::try_from(written).unwrap_or(0).min(LOG_LIMIT);
        String::from_utf8_lossy(&bytes[..used])
            .trim_end()
            .to_owned()
    }

    pub fn delete_shader(&self, shader: u32) {
        // SAFETY: a context is current; a wrong name only raises an error flag.
        unsafe { (self.delete_shader)(shader) };
    }

    /// Links two compiled shaders into a program; the info log when linking fails. The shaders
    /// stay the caller's.
    pub fn link_program(&self, vertex: u32, fragment: u32) -> Result<u32, String> {
        let mut status = 0i32;
        // SAFETY: a context is current; `status` has room for the one integer asked for. The
        // shaders are detached again, so the program does not depend on their lifetime.
        let program = unsafe {
            let program = (self.create_program)();
            (self.attach_shader)(program, vertex);
            (self.attach_shader)(program, fragment);
            (self.link_program)(program);
            (self.get_program_iv)(program, LINK_STATUS, &raw mut status);
            (self.detach_shader)(program, vertex);
            (self.detach_shader)(program, fragment);
            program
        };
        if status == 0 {
            let log = self.info_log(program, true);
            self.delete_program(program);
            return Err(log);
        }
        Ok(program)
    }

    pub fn delete_program(&self, program: u32) {
        // SAFETY: a context is current; a wrong name only raises an error flag.
        unsafe { (self.delete_program)(program) };
    }

    pub fn use_program(&self, program: u32) {
        // SAFETY: a context is current; a wrong name only raises an error flag.
        unsafe { (self.use_program)(program) };
    }

    // ---- vertex arrays ----

    pub fn create_vertex_array(&self) -> u32 {
        let mut name = 0u32;
        // SAFETY: a context is current; `name` has room for the one name requested.
        unsafe { (self.create_vertex_arrays)(1, &raw mut name) };
        name
    }

    /// Enables the attribute at `location`, reading vertex buffer binding 0.
    pub fn vertex_array_attribute(&self, array: u32, location: u32, format: AttribFormat) {
        // SAFETY: a context is current; a wrong location or format only raises an error flag.
        unsafe {
            (self.enable_vertex_array_attrib)(array, location);
            (self.vertex_array_attrib_format)(
                array,
                location,
                format.components,
                format.data_type,
                u8::from(format.normalized),
                format.offset,
            );
            (self.vertex_array_attrib_binding)(array, location, 0);
        }
    }

    /// Attaches `buffer` to binding 0 of the array, starting `offset` bytes in.
    pub fn vertex_array_vertex_buffer(&self, array: u32, buffer: u32, offset: usize, stride: u32) {
        let (Ok(offset), Ok(stride)) = (isize::try_from(offset), i32::try_from(stride)) else {
            return;
        };
        // SAFETY: a context is current; a wrong name, offset or stride only raises an error flag.
        unsafe { (self.vertex_array_vertex_buffer)(array, 0, buffer, offset, stride) };
    }

    pub fn vertex_array_element_buffer(&self, array: u32, buffer: u32) {
        // SAFETY: a context is current; a wrong name only raises an error flag.
        unsafe { (self.vertex_array_element_buffer)(array, buffer) };
    }

    pub fn bind_vertex_array(&self, array: u32) {
        // SAFETY: a context is current; a wrong name only raises an error flag.
        unsafe { (self.bind_vertex_array)(array) };
    }

    pub fn delete_vertex_array(&self, array: u32) {
        // SAFETY: a context is current; one name is read from a live local.
        unsafe { (self.delete_vertex_arrays)(1, &raw const array) };
    }

    // ---- bindings ----

    pub fn bind_uniform_buffer(&self, slot: u32, buffer: u32) {
        // SAFETY: a context is current; a slot out of range only raises an error flag.
        unsafe { (self.bind_buffer_base)(UNIFORM_BUFFER, slot, buffer) };
    }

    pub fn bind_texture_and_sampler(&self, slot: u32, texture: u32, sampler: u32) {
        // SAFETY: a context is current; a slot out of range only raises an error flag.
        unsafe {
            (self.bind_texture_unit)(slot, texture);
            (self.bind_sampler)(slot, sampler);
        }
    }

    // ---- framebuffers ----

    /// A framebuffer drawing into `texture` level 0; `None` when it is not complete.
    pub fn framebuffer_for(&self, texture: u32, attachment: Attachment) -> Option<u32> {
        let mut name = 0u32;
        let (point, draw) = match attachment {
            Attachment::Color => (COLOR_ATTACHMENT0, COLOR_ATTACHMENT0),
            Attachment::Depth => (DEPTH_ATTACHMENT, NONE),
        };
        // SAFETY: a context is current; `name` has room for the one name requested.
        let status = unsafe {
            (self.create_framebuffers)(1, &raw mut name);
            (self.named_framebuffer_texture)(name, point, texture, 0);
            (self.named_framebuffer_draw_buffer)(name, draw);
            (self.check_named_framebuffer_status)(name, DRAW_FRAMEBUFFER)
        };
        if status == FRAMEBUFFER_COMPLETE {
            return Some(name);
        }
        self.delete_framebuffer(name);
        None
    }

    pub fn delete_framebuffer(&self, framebuffer: u32) {
        // SAFETY: a context is current; one name is read from a live local.
        unsafe { (self.delete_framebuffers)(1, &raw const framebuffer) };
    }

    /// Makes `framebuffer` (0 is the window's) the target of drawing.
    pub fn bind_draw_framebuffer(&self, framebuffer: u32) {
        // SAFETY: a context is current; a wrong name only raises an error flag.
        unsafe { (self.bind_framebuffer)(DRAW_FRAMEBUFFER, framebuffer) };
    }

    /// Clears colour attachment 0 of `framebuffer`.
    pub fn clear_colour(&self, framebuffer: u32, rgba: [f32; 4]) {
        // SAFETY: a context is current; the colour value is four floats and `rgba` has four.
        unsafe { (self.clear_named_framebuffer_fv)(framebuffer, COLOR, 0, rgba.as_ptr()) };
    }

    /// Clears the depth of `framebuffer` (depth writes must be on).
    pub fn clear_depth(&self, framebuffer: u32, depth: f32) {
        // SAFETY: a context is current; the depth value is one float and `depth` is one.
        unsafe {
            (self.depth_mask)(1);
            (self.clear_named_framebuffer_fv)(framebuffer, DEPTH, 0, &raw const depth);
        };
    }

    // ---- state ----

    /// Source-alpha blending, or none.
    pub fn set_blend(&self, on: bool) {
        // SAFETY: a context is current; the capability and blend factors are valid enums.
        unsafe {
            if on {
                (self.enable)(BLEND);
                (self.blend_func_separate)(
                    SRC_ALPHA,
                    ONE_MINUS_SRC_ALPHA,
                    ONE,
                    ONE_MINUS_SRC_ALPHA,
                );
            } else {
                (self.disable)(BLEND);
            }
        }
    }

    pub fn set_depth(&self, test: bool, write: bool) {
        // SAFETY: a context is current; the capability and function are valid enums.
        unsafe {
            if test {
                (self.enable)(DEPTH_TEST);
                (self.depth_func)(LESS_OR_EQUAL);
            } else {
                (self.disable)(DEPTH_TEST);
            }
            (self.depth_mask)(u8::from(write));
        }
    }

    // ---- draws ----

    pub fn draw_arrays(&self, vertices: i32, instances: i32) {
        // SAFETY: a context is current. Reads of vertex data go through the buffers attached to
        // the bound vertex array; a core profile has no client-side arrays, so a missing buffer
        // is an error flag, not a read of memory.
        unsafe { (self.draw_arrays_instanced)(TRIANGLES, 0, vertices, instances) };
    }

    pub fn draw_indexed(&self, draw: IndexedDraw) {
        // SAFETY: a context is current. In a core profile the indices argument is a byte offset
        // into the element buffer attached to the bound vertex array (there are no client-side
        // index arrays), so a null-provenance integer is the right argument and nothing is read
        // through it as a pointer.
        unsafe {
            (self.draw_elements_instanced)(
                TRIANGLES,
                draw.count,
                draw.index_type,
                without_provenance::<c_void>(draw.offset),
                draw.instances,
            );
        }
    }
}
