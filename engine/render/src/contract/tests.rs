use super::*;
use crate::caps::RenderCaps;
use crate::desc::{PipelineDesc, SamplerDesc, ShaderDesc};
use crate::ids::{BufferId, FrameId, PipelineId, SamplerId, ShaderId};
use crate::null::NullDevice;

const SURFACE: SurfaceDesc = SurfaceDesc {
    native_token: 1,
    width: 16,
    height: 16,
};

#[test]
fn the_null_device_passes_the_contract() {
    render_device_contract(&mut NullDevice::new(), SURFACE);
}

#[test]
fn the_contract_can_run_twice_on_one_device() {
    let mut device = NullDevice::with_capacity(8);
    render_device_contract(&mut device, SURFACE);
    render_device_contract(&mut device, SURFACE);
}

/// Which rule a [`Broken`] device breaks.
#[derive(Clone, Copy)]
enum Flaw {
    AcceptsStaleBuffer,
    AcceptsUnclosedPass,
    IgnoresClear,
    CorruptsUpload,
    AcceptsStaleFrame,
    AcceptsStaleInStream,
    UnnamedDevice,
    NoIndexTypes,
    AcceptsBadDepth,
}

/// A Null device wrapped so one rule is broken; the contract must notice.
struct Broken {
    inner: NullDevice,
    flaw: Flaw,
}

impl IRenderDevice for Broken {
    fn caps(&self) -> RenderCaps {
        let caps = self.inner.caps();
        match self.flaw {
            Flaw::UnnamedDevice => RenderCaps {
                device_name: "",
                ..caps
            },
            Flaw::NoIndexTypes => RenderCaps {
                index_u16: false,
                index_u32: false,
                ..caps
            },
            _ => caps,
        }
    }
    fn create_surface(&mut self, desc: SurfaceDesc) -> Result<SurfaceId, RenderError> {
        self.inner.create_surface(desc)
    }
    fn resize_surface(
        &mut self,
        surface: SurfaceId,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        self.inner.resize_surface(surface, width, height)
    }
    fn destroy_surface(&mut self, surface: SurfaceId) {
        self.inner.destroy_surface(surface);
    }
    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId, RenderError> {
        self.inner.create_buffer(desc)
    }
    fn create_texture(&mut self, desc: TextureDesc) -> Result<TextureId, RenderError> {
        self.inner.create_texture(desc)
    }
    fn create_sampler(&mut self, desc: SamplerDesc) -> Result<SamplerId, RenderError> {
        self.inner.create_sampler(desc)
    }
    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId, RenderError> {
        self.inner.create_shader(desc)
    }
    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> Result<PipelineId, RenderError> {
        self.inner.create_pipeline(desc)
    }
    fn write_buffer(
        &mut self,
        buffer: BufferId,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        let result = self.inner.write_buffer(buffer, offset, bytes);
        match (self.flaw, result) {
            (Flaw::AcceptsStaleBuffer, Err(RenderError::InvalidHandle)) => Ok(()),
            (_, other) => other,
        }
    }
    fn write_texture(
        &mut self,
        texture: TextureId,
        region: Rect,
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        let mut copy = bytes.to_vec();
        if matches!(self.flaw, Flaw::CorruptsUpload) {
            copy = copy.into_iter().map(|b| b.wrapping_add(1)).collect();
        }
        self.inner.write_texture(texture, region, &copy)
    }
    fn destroy_buffer(&mut self, buffer: BufferId) {
        self.inner.destroy_buffer(buffer);
    }
    fn destroy_texture(&mut self, texture: TextureId) {
        self.inner.destroy_texture(texture);
    }
    fn destroy_shader(&mut self, shader: ShaderId) {
        self.inner.destroy_shader(shader);
    }
    fn destroy_sampler(&mut self, sampler: SamplerId) {
        self.inner.destroy_sampler(sampler);
    }
    fn destroy_pipeline(&mut self, pipeline: PipelineId) {
        self.inner.destroy_pipeline(pipeline);
    }
    fn begin_frame(&mut self, surface: SurfaceId) -> Result<FrameId, RenderError> {
        self.inner.begin_frame(surface)
    }
    fn submit(
        &mut self,
        frame: FrameId,
        stream: &CommandStream,
    ) -> Result<SubmitStats, RenderError> {
        let result = self.inner.submit(frame, stream);
        match (self.flaw, result) {
            (Flaw::AcceptsUnclosedPass, Err(RenderError::InvalidDescription))
            | (Flaw::AcceptsStaleInStream, Err(RenderError::InvalidHandle)) => {
                Ok(SubmitStats::default())
            }
            (Flaw::AcceptsBadDepth, Err(RenderError::InvalidDescription)) if uses_depth(stream) => {
                Ok(SubmitStats::default())
            }
            (Flaw::IgnoresClear, Ok(stats)) => Ok(SubmitStats {
                commands: stats.commands,
                draws: 1,
            }),
            (_, other) => other,
        }
    }
    fn end_frame(&mut self, frame: FrameId) -> Result<(), RenderError> {
        let result = self.inner.end_frame(frame);
        match (self.flaw, result) {
            (Flaw::AcceptsStaleFrame, Err(RenderError::InvalidHandle)) => Ok(()),
            (_, other) => other,
        }
    }
    fn read_back(&mut self, texture: TextureId, out: &mut [u8]) -> Result<(), RenderError> {
        self.inner.read_back(texture, out)
    }
}

fn uses_depth(stream: &CommandStream) -> bool {
    stream
        .commands()
        .flatten()
        .any(|command| matches!(command, crate::stream::Command::BeginPass(p) if p.depth.is_some()))
}

fn run_broken(flaw: Flaw) {
    render_device_contract(
        &mut Broken {
            inner: NullDevice::new(),
            flaw,
        },
        SURFACE,
    );
}

#[test]
#[should_panic(expected = "write to a destroyed buffer")]
fn a_device_that_accepts_a_stale_buffer_fails_the_contract() {
    run_broken(Flaw::AcceptsStaleBuffer);
}

#[test]
#[should_panic(expected = "unclosed clear pass")]
fn a_device_that_accepts_an_unclosed_pass_fails_the_contract() {
    run_broken(Flaw::AcceptsUnclosedPass);
}

#[test]
#[should_panic(expected = "clear pass")]
fn a_device_that_miscounts_draws_fails_the_contract() {
    run_broken(Flaw::IgnoresClear);
}

#[test]
#[should_panic(expected = "upload then read back must be equal")]
fn a_device_that_corrupts_an_upload_fails_the_contract() {
    run_broken(Flaw::CorruptsUpload);
}

#[test]
#[should_panic(expected = "a frame ends once")]
fn a_device_that_ends_a_frame_twice_fails_the_contract() {
    run_broken(Flaw::AcceptsStaleFrame);
}

#[test]
#[should_panic(expected = "the device must name itself")]
fn a_device_without_a_name_fails_the_contract() {
    run_broken(Flaw::UnnamedDevice);
}

#[test]
#[should_panic(expected = "some index type")]
fn a_device_without_index_types_fails_the_contract() {
    run_broken(Flaw::NoIndexTypes);
}

#[test]
#[should_panic(expected = "stale buffer in a stream")]
fn a_device_that_accepts_a_stale_handle_in_a_stream_fails_the_contract() {
    run_broken(Flaw::AcceptsStaleInStream);
}

#[test]
#[should_panic(expected = "depth of another size")]
fn a_device_that_accepts_a_wrong_depth_attachment_fails_the_contract() {
    run_broken(Flaw::AcceptsBadDepth);
}
