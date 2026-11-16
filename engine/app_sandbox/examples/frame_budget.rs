//! Frame times of the sandbox against the direct physics sample (card 7-13).
//!
//! Run in release mode: `cargo run --release -p app_sandbox --example frame_budget`. It draws the
//! same arena twice, once through the ECS and the glue crates (`app_sandbox`) and once straight
//! from the physics world (`app_physics_sample`), at one physics step per frame, on the Null
//! device and, when a window and an OpenGL 4.5 driver exist, on the OpenGL device. It also times
//! the physics step alone and the world transform update of 83 entities. Times are CPU time per
//! frame in microseconds; the stream is 960 frames long, so the "stream" rows count a growing
//! number of bodies and the "steady" rows the full 80.

use app_physics_sample::controls::keys::DEBUG;
use app_physics_sample::session::Session as Direct;
use app_physics_sample::simulation::{DEFAULT_SEED, STEP_TICKS, STREAM_LIMIT, Simulation};
use app_sandbox::Error;
use app_sandbox::measure::Summary;
use app_sandbox::renderer::{Renderer, Target};
use app_sandbox::{Draw, Requests, Sandbox};
use app_sprite_sample::GOLDEN_SHADER;
use app_sprite_sample::app::{HEIGHT, Opened, WIDTH};
use assets::dshd::Program;
use ecs::World;
use platform::window::{Action, Event, WindowId};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::null::NullDevice;
use render::scene::RenderScene;
use scene::{Transform2D, TransformUpdater, set_parent};
use std::time::Instant;

/// Frames of a run: the stream (960 frames), a warm-up and a steady stretch.
const FRAMES: usize = 1500;
/// The frames that count as the stream.
const STREAM: std::ops::Range<usize> = 0..960;
/// The frames that count as the steady state.
const STEADY: std::ops::Range<usize> = 1100..FRAMES;
/// How many times a whole run is repeated, to see how much the figures move; the order of the
/// two sides alternates.
const REPEATS: usize = 3;
/// Entities of the transform update: the arena (3) and the full stream (80).
const ENTITIES: usize = 83;
/// Repetitions of one transform update, so that the clock can see it.
const UPDATES: usize = 2000;
/// Nanoseconds in a microsecond.
const NS_PER_US: u64 = 1000;
/// Nanoseconds in a tenth of a microsecond.
const NS_PER_TENTH: u64 = 100;

type Failure = Box<dyn std::error::Error>;

/// Wraps the render phase of the sandbox to time it.
struct Timed<'a> {
    inner: Target<'a>,
    nanos: u64,
}

impl Draw for Timed<'_> {
    fn draw(&mut self, scene: &RenderScene) -> Result<(), Error> {
        let start = Instant::now();
        let done = self.inner.draw(scene);
        self.nanos += nanos_since(start);
        done
    }
}

fn nanos_since(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Frame times of the direct sample on `device`. The sample starts with its debug drawing on
/// (outlines and contacts as thin sprites); the sandbox has none, so the comparison that matters
/// is the one with `debug` false, where the key that toggles it is pressed once first.
fn direct(
    device: &mut dyn IRenderDevice,
    program: &Program<'_>,
    surface: render::ids::SurfaceId,
    debug: bool,
) -> Result<Vec<u64>, Failure> {
    let mut session = Direct::new(device, program, surface, (WIDTH, HEIGHT), DEFAULT_SEED)?;
    if !debug {
        session.apply(&Event::Key {
            window: WindowId::from_raw(0, 0),
            scancode: DEBUG,
            action: Action::Press,
            mods: 0,
        });
    }
    let mut times = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let start = Instant::now();
        session.frame(device, STEP_TICKS)?;
        times.push(nanos_since(start));
    }
    Ok(times)
}

/// Frame times of the sandbox on `device`: the whole frame, and the render phase of it.
fn sandbox(
    device: &mut dyn IRenderDevice,
    program: &Program<'_>,
    surface: render::ids::SurfaceId,
) -> Result<(Vec<u64>, Vec<u64>), Failure> {
    let mut renderer = Renderer::new(device, program, surface, (WIDTH, HEIGHT))?;
    let mut sandbox = Sandbox::new(renderer.textures(), DEFAULT_SEED, (WIDTH, HEIGHT))?;
    let (mut total, mut render) = (Vec::with_capacity(FRAMES), Vec::with_capacity(FRAMES));
    for _ in 0..FRAMES {
        let mut draw = Timed {
            inner: Target {
                device,
                renderer: &mut renderer,
            },
            nanos: 0,
        };
        let start = Instant::now();
        sandbox.frame(Requests::default(), STEP_TICKS, &mut draw)?;
        total.push(nanos_since(start));
        render.push(draw.nanos);
    }
    Ok((total, render))
}

/// Frame times of the physics world alone: the stream and one step per frame, nothing drawn.
fn physics_only() -> Result<Vec<u64>, Failure> {
    let mut simulation = Simulation::new(DEFAULT_SEED)?;
    let mut times = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let start = Instant::now();
        simulation.advance(STEP_TICKS)?;
        times.push(nanos_since(start));
    }
    Ok(times)
}

/// Time of one world transform update of a flat scene of [`ENTITIES`] entities, nanoseconds.
fn transform_update() -> Result<u64, Failure> {
    let mut world = World::new();
    let root = world.spawn()?;
    world.insert(root, Transform2D::IDENTITY)?;
    for _ in 1..ENTITIES {
        let entity = world.spawn()?;
        world.insert(entity, Transform2D::IDENTITY)?;
        set_parent(&mut world, entity, root)?;
    }
    let mut updater = TransformUpdater::new();
    updater.update(&mut world)?;
    let start = Instant::now();
    for _ in 0..UPDATES {
        updater.update(&mut world)?;
    }
    Ok(nanos_since(start) / UPDATES as u64)
}

/// `nanos` as microseconds with one decimal ("12.3"), without going through a float.
fn micros(nanos: u64) -> String {
    format!("{}.{}", nanos / NS_PER_US, nanos % NS_PER_US / NS_PER_TENTH)
}

/// One row: the summary of the frames in `range` of `times`.
fn row(label: &str, times: &[u64], range: std::ops::Range<usize>) {
    let mut part = times[range.clone()].to_vec();
    if let Some(s) = Summary::of(&mut part) {
        println!(
            "  {label:<22} {:>9} {:>9} {:>9} {:>9} {:>9}",
            micros(s.mean),
            micros(s.median),
            micros(s.p95),
            micros(s.p99),
            micros(s.max)
        );
    }
}

fn header(title: &str) {
    println!(
        "\n{title}\n  {:<22} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "(microseconds)", "mean", "median", "p95", "p99", "max"
    );
}

/// All the rows of one run on `device`. The direct sample and the sandbox run one after the other
/// in the same process; `sandbox_first` swaps them, so that a warming or throttling CPU does not
/// favour the same side in every run.
fn report(
    name: &str,
    device: &mut dyn IRenderDevice,
    surface: render::ids::SurfaceId,
    sandbox_first: bool,
) -> Result<(), Failure> {
    let program = Program::parse(GOLDEN_SHADER)?;
    let early = if sandbox_first {
        Some(sandbox(device, &program, surface)?)
    } else {
        None
    };
    let shipped = direct(device, &program, surface, true)?;
    let plain = direct(device, &program, surface, false)?;
    let (total, render) = match early {
        Some(done) => done,
        None => sandbox(device, &program, surface)?,
    };
    for (title, range) in [("stream", STREAM), ("steady", STEADY)] {
        header(&format!("{name}, {title} frames"));
        row("direct, debug on", &shipped, range.clone());
        row("direct, debug off", &plain, range.clone());
        row("sandbox", &total, range.clone());
        row("sandbox render phase", &render, range);
    }
    Ok(())
}

fn on_null(sandbox_first: bool) -> Result<(), Failure> {
    let mut device = NullDevice::new();
    let surface = device.create_surface(SurfaceDesc {
        native_token: 1,
        width: WIDTH,
        height: HEIGHT,
    })?;
    report("Null device", &mut device, surface, sandbox_first)
}

fn on_gl() -> Result<(), Failure> {
    const GL_RUNS: usize = 2;
    let mut opened = match Opened::open("DuckEngine 7-13: frame budget") {
        Ok(opened) => opened,
        Err(error) => {
            println!("\nOpenGL device: not run ({error})");
            return Ok(());
        }
    };
    let surface = opened.device.create_surface(SurfaceDesc {
        native_token: opened.token.0,
        width: WIDTH,
        height: HEIGHT,
    })?;
    for run in 0..GL_RUNS {
        println!(
            "
=== OpenGL run {} ===",
            run + 1
        );
        report(
            "OpenGL 4.5 device",
            &mut opened.device,
            surface,
            run % 2 == 1,
        )?;
    }
    Ok(())
}

fn main() -> Result<(), Failure> {
    println!(
        "frame_budget: {} frames per run, {REPEATS} runs, {} {}, {} stream bodies",
        FRAMES,
        std::env::consts::OS,
        std::env::consts::ARCH,
        STREAM_LIMIT
    );
    for run in 1..=REPEATS {
        println!("\n=== run {run} ===");
        on_null(run % 2 == 0)?;
        let physics = physics_only()?;
        header("Physics world alone (stream, then steady)");
        row("stream frames", &physics, STREAM);
        row("steady frames", &physics, STEADY);
        println!(
            "\nTransform update of {ENTITIES} entities: {} us",
            micros(transform_update()?)
        );
    }
    on_gl()
}
