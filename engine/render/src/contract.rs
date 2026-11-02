//! The generic contract suite of the render port: one function takes the device under test, so the
//! Null device and every adapter run the same checks (design: `wiki/architecture/render-port-v0.md`
//! section 9). A device is accepted when the suite passes unchanged. Checks that need a rasteriser
//! (a triangle's pixels) are not here: the Null device cannot pass them, they belong to the image
//! test of the GPU adapters.

use crate::desc::{
    BufferDesc, BufferUsage, PassDesc, Rect, RenderTarget, SubmitStats, SurfaceDesc, TextureDesc,
    TextureFormat,
};
use crate::device::IRenderDevice;
use crate::error::RenderError;
use crate::ids::{SurfaceId, TextureId};
use crate::stream::CommandStream;

const INVALID_HANDLE: Result<(), RenderError> = Err(RenderError::InvalidHandle);
const INVALID_DESC: Result<(), RenderError> = Err(RenderError::InvalidDescription);

/// Runs every check of the port contract against `device`. A device that draws asynchronously must
/// make `read_back` wait for the work submitted before it.
/// `surface` describes a surface the device
/// can create (for a GPU device, a real window token).
///
/// # Panics
/// When the device breaks the contract (this is test code).
pub fn render_device_contract(device: &mut dyn IRenderDevice, surface: SurfaceDesc) {
    caps_contract(device);
    buffer_contract(device);
    texture_contract(device);
    surface_contract(device, surface);
    clear_contract(device, surface);
    stream_rules_contract(device, surface);
}

fn rgba(width: u32, height: u32) -> TextureDesc {
    TextureDesc {
        width,
        height,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }
}

fn make_texture(device: &mut dyn IRenderDevice, width: u32, height: u32) -> TextureId {
    let made = device.create_texture(rgba(width, height));
    assert!(
        made.is_ok(),
        "create_texture {width}x{height} failed: {made:?}"
    );
    made.unwrap_or_else(|_| unreachable!())
}

fn make_surface(device: &mut dyn IRenderDevice, desc: SurfaceDesc) -> SurfaceId {
    let made = device.create_surface(desc);
    assert!(made.is_ok(), "create_surface failed: {made:?}");
    made.unwrap_or_else(|_| unreachable!())
}

/// Submits `stream` in a fresh frame on `surface` and ends the frame.
fn submit_in_frame(
    device: &mut dyn IRenderDevice,
    surface: SurfaceId,
    stream: &CommandStream,
) -> Result<SubmitStats, RenderError> {
    let frame = device.begin_frame(surface)?;
    let result = device.submit(frame, stream);
    assert_eq!(device.end_frame(frame), Ok(()), "end_frame after submit");
    result
}

fn caps_contract(device: &dyn IRenderDevice) {
    let caps = device.caps();
    assert!(
        caps.max_texture_size >= 1,
        "max_texture_size must be at least 1"
    );
    assert!(
        caps.uniform_buffer_alignment >= 1,
        "uniform alignment must be at least 1"
    );
    assert!(
        caps.index_u16 || caps.index_u32,
        "a device must support some index type"
    );
    assert!(!caps.device_name.is_empty(), "the device must name itself");
}

fn buffer_contract(device: &mut dyn IRenderDevice) {
    let desc = BufferDesc {
        size: 16,
        usage: BufferUsage::Vertex,
    };
    let buffer = device.create_buffer(desc);
    assert!(buffer.is_ok(), "create_buffer failed: {buffer:?}");
    let buffer = buffer.unwrap_or_else(|_| unreachable!());
    assert_eq!(
        device.write_buffer(buffer, 0, &[0; 16]),
        Ok(()),
        "write the whole buffer"
    );
    assert_eq!(
        device.write_buffer(buffer, 8, &[0; 9]),
        INVALID_DESC,
        "write past the end"
    );
    let empty = BufferDesc {
        size: 0,
        usage: BufferUsage::Index,
    };
    assert_eq!(
        device.create_buffer(empty).map(|_| ()),
        INVALID_DESC,
        "zero-size buffer"
    );
    device.destroy_buffer(buffer);
    let reused = device.create_buffer(desc);
    assert!(
        reused.is_ok(),
        "create_buffer after destroy failed: {reused:?}"
    );
    assert_ne!(
        reused,
        Ok(buffer),
        "a reused slot must get a new generation"
    );
    assert_eq!(
        device.write_buffer(buffer, 0, &[0]),
        INVALID_HANDLE,
        "write to a destroyed buffer"
    );
    device.destroy_buffer(buffer);
    if let Ok(reused) = reused {
        device.destroy_buffer(reused);
    }
}

fn texture_contract(device: &mut dyn IRenderDevice) {
    let texture = make_texture(device, 4, 2);
    let whole = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 2,
    };
    let pixels: Vec<u8> = (0..32).collect();
    assert_eq!(
        device.write_texture(texture, whole, &pixels),
        Ok(()),
        "upload"
    );
    let mut out = vec![0; 32];
    assert_eq!(device.read_back(texture, &mut out), Ok(()), "read back");
    assert_eq!(out, pixels, "upload then read back must be equal");
    assert_eq!(
        device.write_texture(texture, whole, &pixels[..31]),
        INVALID_DESC,
        "short upload"
    );
    assert_eq!(
        device.read_back(texture, &mut out[..31]),
        INVALID_DESC,
        "short read-back buffer"
    );
    let outside = Rect {
        x: 3,
        y: 0,
        width: 2,
        height: 1,
    };
    assert_eq!(
        device.write_texture(texture, outside, &[0; 8]),
        INVALID_DESC,
        "region outside"
    );
    assert_eq!(
        device.create_texture(rgba(0, 4)).map(|_| ()),
        INVALID_DESC,
        "zero-width texture"
    );
    device.destroy_texture(texture);
    assert_eq!(
        device.read_back(texture, &mut out),
        INVALID_HANDLE,
        "read back a destroyed texture"
    );
    assert_eq!(
        device.write_texture(texture, whole, &pixels),
        INVALID_HANDLE,
        "write a destroyed texture"
    );
}

fn surface_contract(device: &mut dyn IRenderDevice, desc: SurfaceDesc) {
    let surface = make_surface(device, desc);
    assert_eq!(
        device.resize_surface(surface, desc.width, desc.height),
        Ok(()),
        "resize"
    );
    assert_eq!(
        device.resize_surface(surface, 0, desc.height),
        INVALID_DESC,
        "resize to zero"
    );
    let frame = device.begin_frame(surface);
    assert!(frame.is_ok(), "begin_frame failed: {frame:?}");
    let frame = frame.unwrap_or_else(|_| unreachable!());
    assert!(device.begin_frame(surface).is_err(), "one frame at a time");
    assert_eq!(device.end_frame(frame), Ok(()), "end_frame");
    assert_eq!(device.end_frame(frame), INVALID_HANDLE, "a frame ends once");
    device.destroy_surface(surface);
    assert_eq!(
        device.begin_frame(surface).map(|_| ()),
        INVALID_HANDLE,
        "frame on a destroyed surface"
    );
    assert_eq!(
        device.resize_surface(surface, 4, 4),
        INVALID_HANDLE,
        "resize a destroyed surface"
    );
}

fn clear_contract(device: &mut dyn IRenderDevice, desc: SurfaceDesc) {
    let surface = make_surface(device, desc);
    let target = make_texture(device, 4, 4);
    for (color, texel) in [
        ([1.0, 0.0, 0.0, 1.0], [255, 0, 0, 255]),
        ([0.0, 1.0, 0.0, 1.0], [0, 255, 0, 255]),
    ] {
        let mut stream = CommandStream::new();
        let pass = PassDesc {
            target: RenderTarget::Texture(target),
            clear_color: Some(color),
            clear_depth: None,
        };
        stream.begin_pass(pass);
        stream.end_pass();
        assert_eq!(
            submit_in_frame(device, surface, &stream).map(|stats| stats.draws),
            Ok(0),
            "clear pass"
        );
        let mut out = vec![0; 64];
        assert_eq!(
            device.read_back(target, &mut out),
            Ok(()),
            "read back the cleared texture"
        );
        assert!(
            out.chunks_exact(4).all(|px| px == texel),
            "every texel must be {texel:?}, got {out:?}"
        );
    }
    let mut rejected = CommandStream::new();
    let blue = [0.0, 0.0, 1.0, 1.0];
    rejected.begin_pass(PassDesc {
        target: RenderTarget::Texture(target),
        clear_color: Some(blue),
        clear_depth: None,
    });
    let result = submit_in_frame(device, surface, &rejected);
    assert_eq!(result.map(|_| ()), INVALID_DESC, "unclosed clear pass");
    let mut out = vec![0; 64];
    assert_eq!(
        device.read_back(target, &mut out),
        Ok(()),
        "read back after a rejected stream"
    );
    assert!(
        out.chunks_exact(4).all(|px| px == [0, 255, 0, 255]),
        "a rejected stream must change nothing"
    );
    device.destroy_texture(target);
    device.destroy_surface(surface);
}

fn stream_rules_contract(device: &mut dyn IRenderDevice, desc: SurfaceDesc) {
    let surface = make_surface(device, desc);
    let backbuffer = PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: None,
        clear_depth: None,
    };
    let mut unclosed = CommandStream::new();
    unclosed.begin_pass(backbuffer);
    let mut end_alone = CommandStream::new();
    end_alone.end_pass();
    let mut no_pipeline = CommandStream::new();
    no_pipeline.begin_pass(backbuffer);
    no_pipeline.draw(3, 1);
    no_pipeline.end_pass();
    for (name, stream) in [
        ("unclosed pass", &unclosed),
        ("end without begin", &end_alone),
        ("draw without pipeline", &no_pipeline),
    ] {
        let result = submit_in_frame(device, surface, stream);
        assert_eq!(result.map(|_| ()), INVALID_DESC, "{name}");
    }
    let buffer = device.create_buffer(BufferDesc {
        size: 16,
        usage: BufferUsage::Uniform,
    });
    assert!(buffer.is_ok(), "create_buffer failed: {buffer:?}");
    let buffer = buffer.unwrap_or_else(|_| unreachable!());
    let mut bad_slot = CommandStream::new();
    bad_slot.begin_pass(backbuffer);
    bad_slot.bind_uniforms(u32::MAX, buffer);
    bad_slot.end_pass();
    assert_eq!(
        submit_in_frame(device, surface, &bad_slot).map(|_| ()),
        INVALID_DESC,
        "uniform slot out of range"
    );
    device.destroy_buffer(buffer);
    let mut stale = CommandStream::new();
    stale.begin_pass(backbuffer);
    stale.bind_uniforms(0, buffer);
    stale.end_pass();
    assert_eq!(
        submit_in_frame(device, surface, &stale).map(|_| ()),
        INVALID_HANDLE,
        "stale buffer in a stream"
    );
    let frame = device.begin_frame(surface);
    assert!(frame.is_ok(), "begin_frame failed: {frame:?}");
    let frame = frame.unwrap_or_else(|_| unreachable!());
    assert_eq!(device.end_frame(frame), Ok(()));
    assert_eq!(
        device.submit(frame, &CommandStream::new()).map(|_| ()),
        INVALID_HANDLE,
        "submit to an ended frame"
    );
    device.destroy_surface(surface);
}

#[cfg(test)]
mod tests;
