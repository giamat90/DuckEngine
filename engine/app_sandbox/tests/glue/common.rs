//! Shared set-up: a sandbox, a draw target that records, and scripted frames.

use app_physics_sample::mapping::Textures;
use app_physics_sample::simulation::STEP_TICKS;
use app_sandbox::{Draw, Error, FrameReport, Requests, Sandbox};
use render::ids::TextureId;
use render::scene::RenderScene;

pub(crate) type Failure = Box<dyn std::error::Error>;

pub(crate) const VIEW: (u32, u32) = (800, 450);

/// Remembers what the render phase was handed.
#[derive(Default)]
pub(crate) struct Recorder {
    pub(crate) draws: u32,
    pub(crate) sprites: usize,
    /// The lowest y of a falling body (layer 1) in the last drawn scene; `f32::MAX` if none.
    pub(crate) lowest: f32,
}

impl Draw for Recorder {
    fn draw(&mut self, scene: &RenderScene) -> Result<(), Error> {
        self.draws += 1;
        self.sprites = scene.sprites().len();
        self.lowest = scene
            .sprites()
            .iter()
            .filter(|sprite| sprite.layer == 1)
            .map(|sprite| sprite.position.y)
            .fold(f32::MAX, f32::min);
        Ok(())
    }
}

pub(crate) fn textures() -> Textures {
    Textures {
        white: TextureId::from_raw(0, 1),
        checker: TextureId::from_raw(1, 1),
        disc: TextureId::from_raw(2, 1),
    }
}

pub(crate) fn sandbox(seed: u64) -> Result<Sandbox, Failure> {
    Ok(Sandbox::new(textures(), seed, VIEW)?)
}

/// One frame of exactly one physics step.
pub(crate) fn step(
    sandbox: &mut Sandbox,
    requests: Requests,
    draw: &mut Recorder,
) -> Result<FrameReport, Failure> {
    Ok(sandbox.frame(requests, STEP_TICKS, draw)?)
}

/// `count` frames without requests; the sum of the played cues.
pub(crate) fn run(sandbox: &mut Sandbox, count: u32, draw: &mut Recorder) -> Result<u32, Failure> {
    let mut played = 0;
    for _ in 0..count {
        played += step(sandbox, Requests::default(), draw)?.played;
    }
    Ok(played)
}

use app_sandbox::session::Session;
use app_sprite_sample::GOLDEN_SHADER;
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
    let program = Program::parse(GOLDEN_SHADER)?;
    let session = Session::new(&mut device, &program, surface, VIEW, seed)?;
    Ok((device, session))
}

/// FNV-1a over bytes: a stable fingerprint for a golden stream.
pub(crate) fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
