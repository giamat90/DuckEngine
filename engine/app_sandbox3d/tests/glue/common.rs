//! Shared set-up: a sandbox, a draw target that records, and scripted frames.

use app_sandbox3d::Error;
use app_sandbox3d::controls::Requests;
use app_sandbox3d::sandbox::{Draw, FrameReport, Sandbox};
use render::ids::TextureId;
use render::mesh_scene::{MeshInstance, MeshScene};

pub(crate) type Failure = Box<dyn std::error::Error>;

pub(crate) const VIEW: (u32, u32) = (800, 450);
/// Nanoseconds in one physics step (a sixtieth of a second).
pub(crate) const STEP_TICKS: u64 = 16_666_667;
/// Fixed blocks of the box: the floor and four walls.
pub(crate) const SCENERY: usize = 5;

/// Remembers what the render phase was handed.
#[derive(Default)]
pub(crate) struct Recorder {
    pub(crate) draws: u32,
    pub(crate) instances: Vec<MeshInstance>,
    pub(crate) aspect: f32,
}

impl Draw for Recorder {
    fn draw(&mut self, scene: &MeshScene) -> Result<(), Error> {
        self.draws += 1;
        self.instances = scene.instances().to_vec();
        self.aspect = scene.camera().aspect;
        Ok(())
    }
}

impl Recorder {
    /// The pieces that fell (every instance after the scenery), as their world positions.
    pub(crate) fn pieces(&self) -> Vec<[f32; 3]> {
        self.instances
            .iter()
            .skip(SCENERY)
            .map(|i| {
                let m = i.model.to_cols_array();
                [m[12], m[13], m[14]]
            })
            .collect()
    }
}

pub(crate) fn texture() -> TextureId {
    TextureId::from_raw(0, 1)
}

pub(crate) fn sandbox(seed: u64) -> Result<Sandbox, Failure> {
    Ok(Sandbox::new(texture(), seed, VIEW)?)
}

/// One frame of exactly one physics step.
pub(crate) fn step(
    sandbox: &mut Sandbox,
    requests: Requests,
    draw: &mut Recorder,
) -> Result<FrameReport, Failure> {
    Ok(sandbox.frame(requests, STEP_TICKS, draw)?)
}

/// `count` frames without requests.
pub(crate) fn run(sandbox: &mut Sandbox, count: u32, draw: &mut Recorder) -> Result<(), Failure> {
    for _ in 0..count {
        step(sandbox, Requests::default(), draw)?;
    }
    Ok(())
}

use app_sandbox3d::session::Session;
use assets::dshd::Program;
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::null::NullDevice;

/// A session on a Null device, ready to draw.
pub(crate) fn null_session(seed: u64) -> Result<(NullDevice, Session), Failure> {
    let mut device = NullDevice::new();
    let surface = device.create_surface(SurfaceDesc {
        native_token: 1,
        width: VIEW.0,
        height: VIEW.1,
    })?;
    let program = Program::parse(app_sandbox3d::renderer::LIT_GLSL)?;
    let session = Session::new(&mut device, &program, surface, VIEW, seed)?;
    Ok((device, session))
}

/// FNV-1a over bytes: a stable fingerprint for a golden stream.
pub(crate) fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
