//! The sandbox world: the ECS world, the physics world, the audio engine and the one `frame`
//! call that runs the nine phases of the frame plan over them.

use crate::components::Motion;
use crate::error::Error;
use crate::spawn::{
    SOUND_RATE, body_desc, impact_gain, impact_sound, inside_arena, listener_half_width, scenery,
    sprite_size, stream_x,
};
use app_physics_sample::mapping::{PIXELS_PER_METRE, Textures};
use app_physics_sample::simulation::{
    DT, MAX_STEPS, SPAWN_EVERY, SPAWN_HEIGHT, STEP_TICKS, STREAM_LIMIT, build_world,
};
use audio::device::AudioRenderer;
use audio::engine::{AudioEngine, AudioProcessor, EngineConfig, SoundHandle, engine};
use audio::format::{ChannelLayout, StreamFormat};
use base::math::Vec2;
use base::rng::Pcg32;
use ecs::{Entity, World};
use frame::{FixedStep, FramePlan, Phase};
use physics::d2::{SolverExecution, World as PhysicsWorld};
use render::camera::OrthographicCamera;
use render::scene::RenderScene;
use scene::{Transform2D, TransformUpdater};
use sync_audio::{CueQueue, Listener, SoundCue, sync_audio};
use sync_physics::{PhysicsBody, attach_body, pull_transforms, push_forces};
use sync_render::{SpriteRenderer, extract_sprites};

/// The stream number of the random generator.
const RNG_STREAM: u64 = 24;
/// A change of velocity from one step to the next that counts as an impact, metres per second.
/// Gravity changes the velocity by 0.16 m/s in a step, so only a contact reaches this.
pub const IMPACT_SPEED: f32 = 3.0;
/// Cues the queue holds between two frames.
const CUE_CAPACITY: usize = 64;
/// Sprites one scene holds.
const SPRITE_CAPACITY: usize = 256;
/// Sounds, voices and engine commands.
const ENGINE: EngineConfig = EngineConfig {
    max_voices: 32,
    max_sounds: 4,
    command_capacity: 128,
};
/// Frames the mix advances in one video frame (a sixtieth of a second).
const MIX_FRAMES: u32 = 800;
/// The tint of the floor and the walls.
const SLATE: [u8; 4] = [70, 90, 130, 255];
/// The tint of a disc.
const ORANGE: [u8; 4] = [240, 140, 80, 255];

/// What the user asked for in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Requests {
    /// Start the arena again.
    pub reset: bool,
    /// Drop a body here (metres).
    pub spawn: Option<Vec2>,
}

/// What one frame did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FrameReport {
    /// Physics steps run.
    pub steps: u32,
    /// Bodies dropped (the stream and the clicks).
    pub spawned: u32,
    /// Impacts found.
    pub impacts: u32,
    /// Cues the queue refused because it was full.
    pub refused: u32,
    /// Sprites extracted.
    pub extracted: u32,
    /// Cues played.
    pub played: u32,
    /// Cues the audio engine refused.
    pub failed: u32,
}

/// Where the render phase sends the scene.
pub trait Draw {
    /// Draws one frame of `scene`.
    ///
    /// # Errors
    /// Whatever the render side reports.
    fn draw(&mut self, scene: &RenderScene) -> Result<(), Error>;
}

/// The camera of a view: one world unit is a metre, drawn [`PIXELS_PER_METRE`] pixels wide.
fn camera_for(view: (u32, u32)) -> Result<OrthographicCamera, Error> {
    OrthographicCamera::pixel_perfect(view.0, view.1, Vec2::ZERO, PIXELS_PER_METRE)
        .ok_or(Error::NoView)
}

/// The sandbox.
pub struct Sandbox {
    textures: Textures,
    seed: u64,
    view: (u32, u32),
    world: World,
    physics: PhysicsWorld,
    execution: SolverExecution,
    clock: FixedStep,
    updater: TransformUpdater,
    scene: RenderScene,
    bodies: Vec<Entity>,
    rng: Pcg32,
    steps: u64,
    streamed: u32,
    engine: AudioEngine,
    processor: AudioProcessor,
    cues: CueQueue,
    sound: SoundHandle,
    mix: Vec<f32>,
    peak: f32,
}

impl Sandbox {
    /// A sandbox for a view of `view` pixels, with the stream seeded by `seed`.
    ///
    /// # Errors
    /// [`Error::NoView`] for a view with a zero side, or the error of whatever cannot be built.
    pub fn new(textures: Textures, seed: u64, view: (u32, u32)) -> Result<Self, Error> {
        let format = StreamFormat {
            sample_rate: SOUND_RATE,
            layout: ChannelLayout::Stereo,
            max_block_frames: MIX_FRAMES.next_power_of_two(),
        };
        let (mut engine, processor) = engine(format, ENGINE)?;
        let sound = engine.register_sound(impact_sound()?)?;
        let mut sandbox = Self {
            textures,
            seed,
            view,
            world: World::new(),
            physics: build_world()?,
            execution: SolverExecution::SERIAL,
            clock: FixedStep::new(STEP_TICKS, MAX_STEPS)
                .map_err(|e| Error::Render(format!("{e:?}")))?,
            updater: TransformUpdater::new(),
            scene: RenderScene::new(SPRITE_CAPACITY, camera_for(view)?)
                .map_err(|e| Error::Render(format!("{e:?}")))?,
            bodies: Vec::with_capacity(128),
            rng: Pcg32::new(seed, RNG_STREAM),
            steps: 0,
            streamed: 0,
            engine,
            processor,
            cues: CueQueue::new(CUE_CAPACITY),
            sound,
            mix: vec![0.0; MIX_FRAMES as usize * 2],
            peak: 0.0,
        };
        sandbox.populate()?;
        Ok(sandbox)
    }

    /// The arena: the floor and the two walls as sprites. Their bodies are in the physics world
    /// that `build_world` made; a static body never moves, so it has no component for the glue.
    fn populate(&mut self) -> Result<(), Error> {
        for (at, half) in scenery() {
            let entity = self.world.spawn()?;
            self.world.insert(entity, Transform2D::from_position(at))?;
            let mut sprite = SpriteRenderer::new(self.textures.white, half * 2.0);
            sprite.tint = SLATE;
            self.world.insert(entity, sprite)?;
        }
        Ok(())
    }

    /// Starts the arena again from the seed; the audio engine and its sound stay.
    fn restart(&mut self) -> Result<(), Error> {
        self.world = World::new();
        self.physics = build_world()?;
        self.physics.set_execution(self.execution.clone());
        self.bodies.clear();
        self.rng = Pcg32::new(self.seed, RNG_STREAM);
        self.steps = 0;
        self.streamed = 0;
        self.populate()
    }

    /// Drops a body at `at` (metres), moved inside the arena; false when the world is full.
    fn spawn_body(&mut self, at: Vec2) -> Result<bool, Error> {
        if self.physics.len() >= self.physics.capacity() {
            return Ok(false);
        }
        let desc = body_desc(&mut self.rng, inside_arena(at));
        // A shape without a sprite is refused before anything is made, so no body is left undrawn.
        let Some((extent, is_disc)) = sprite_size(&desc.shape) else {
            return Ok(false);
        };
        let entity = self.world.spawn()?;
        attach_body(&mut self.world, &mut self.physics, entity, &desc)?;
        let (texture, tint) = if is_disc {
            (self.textures.disc, ORANGE)
        } else {
            (self.textures.checker, [255; 4])
        };
        let mut sprite = SpriteRenderer::new(texture, extent);
        sprite.tint = tint;
        sprite.layer = 1;
        self.world.insert(entity, sprite)?;
        self.world.insert(entity, Motion::default())?;
        self.bodies.push(entity);
        Ok(true)
    }

    /// The stream: a body joins every [`SPAWN_EVERY`] steps until [`STREAM_LIMIT`] have.
    fn stream(&mut self) -> Result<u32, Error> {
        if !self.steps.is_multiple_of(SPAWN_EVERY) || self.streamed >= STREAM_LIMIT {
            return Ok(0);
        }
        let x = stream_x(&mut self.rng);
        let spawned = u32::from(self.spawn_body(Vec2::new(x, SPAWN_HEIGHT))?);
        self.streamed += spawned;
        Ok(spawned)
    }

    /// How hard the body of `entity` was hit in the last step, and its velocity noted for the
    /// next; `None` for an entity without a body.
    fn jolt_of(&mut self, entity: Entity) -> Option<f32> {
        let handle = self.world.get::<PhysicsBody>(entity)?.handle();
        let velocity = self.physics.body(handle).ok()?.linear_velocity();
        let motion = self.world.get_mut::<Motion>(entity)?;
        let jolt = (velocity - motion.velocity).length();
        motion.velocity = velocity;
        Some(jolt)
    }

    /// The cue of the impact the body at `index` took in the last step, if it took one.
    fn impact_cue(&mut self, index: usize) -> Option<SoundCue> {
        let entity = self.bodies.get(index).copied()?;
        let jolt = self.jolt_of(entity).filter(|jolt| *jolt >= IMPACT_SPEED)?;
        let mut cue = SoundCue::new(self.sound).at(entity);
        cue.gain = impact_gain(jolt);
        Some(cue)
    }

    /// Queues the cue of the body at `index`: (impacts, refused), (0, 0) when it took none.
    fn queue_impact(&mut self, index: usize) -> (u32, u32) {
        self.impact_cue(index)
            .map_or((0, 0), |cue| (1, u32::from(self.cues.push(cue).is_err())))
    }

    /// Turns the impacts of the last step into cues; returns the impacts found and the cues the
    /// queue refused.
    fn find_impacts(&mut self) -> (u32, u32) {
        let (mut impacts, mut refused) = (0, 0);
        for index in 0..self.bodies.len() {
            let (found, turned_away) = self.queue_impact(index);
            impacts += found;
            refused += turned_away;
        }
        (impacts, refused)
    }

    /// Runs one phase of the frame plan.
    fn run_phase(
        &mut self,
        phase: Phase,
        requests: Requests,
        draw: &mut dyn Draw,
        report: &mut FrameReport,
    ) -> Result<(), Error> {
        match phase {
            Phase::Input => {}
            Phase::Gameplay => self.gameplay(requests, report)?,
            Phase::SyncPhysicsIn => {
                report.spawned += self.stream()?;
                push_forces(&mut self.world, &mut self.physics)?;
            }
            Phase::PhysicsStep => {
                self.physics.step(DT)?;
                self.steps += 1;
            }
            Phase::SyncPhysicsOut => {
                pull_transforms(&mut self.world, &self.physics)?;
                let (impacts, refused) = self.find_impacts();
                report.impacts += impacts;
                report.refused += refused;
            }
            Phase::SceneUpdate => {
                self.updater.update(&mut self.world)?;
            }
            Phase::SyncRender => {
                report.extracted = extract_sprites(&mut self.world, &mut self.scene)?.extracted;
            }
            Phase::Render => draw.draw(&self.scene)?,
            Phase::SyncAudio => self.audio(report),
        }
        Ok(())
    }

    /// The requests of the frame: a reset, then a click.
    fn gameplay(&mut self, requests: Requests, report: &mut FrameReport) -> Result<(), Error> {
        if requests.reset {
            self.restart()?;
        }
        if let Some(at) = requests.spawn {
            report.spawned += u32::from(self.spawn_body(at)?);
        }
        Ok(())
    }

    /// Plays the cues and mixes the sixtieth of a second they fall in, standing in for the audio
    /// device (the processor is pulled here, so ended voices are freed on the next call).
    fn audio(&mut self, report: &mut FrameReport) {
        let listener = Listener {
            x: 0.0,
            half_width: listener_half_width(self.view.0),
        };
        let synced = sync_audio(&self.world, &mut self.engine, &mut self.cues, &listener);
        report.played = synced.played;
        report.failed = synced.failed;
        self.processor.render(&mut self.mix, MIX_FRAMES);
        self.peak = self.mix.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
    }

    /// Runs one frame: the physics steps `frame_ticks` nanoseconds amount to, in the nine phases
    /// of the frame plan, and the render phase draws through `draw`.
    ///
    /// # Errors
    /// The error of the first phase that fails.
    pub fn frame(
        &mut self,
        requests: Requests,
        frame_ticks: u64,
        draw: &mut dyn Draw,
    ) -> Result<FrameReport, Error> {
        let steps = self.clock.advance(frame_ticks).count;
        let mut report = FrameReport {
            steps,
            ..FrameReport::default()
        };
        for phase in FramePlan::new(steps) {
            self.run_phase(phase, requests, draw, &mut report)?;
        }
        Ok(report)
    }

    /// Follows a window resize: the camera gets the new size.
    ///
    /// # Errors
    /// [`Error::NoView`] for a zero side; nothing changes then.
    pub fn resize(&mut self, view: (u32, u32)) -> Result<(), Error> {
        self.scene.set_camera(camera_for(view)?);
        self.view = view;
        Ok(())
    }

    /// Dynamic bodies alive.
    #[must_use]
    pub fn bodies(&self) -> usize {
        self.bodies.len()
    }

    /// Physics steps run since the start or the last reset.
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// A hash of the state of the world after the last frame: the physics state.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        self.physics.state_hash()
    }

    /// The loudest sample of the last audio block.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// How the physics solver runs; it changes the speed, never a bit of the result.
    pub fn set_execution(&mut self, execution: SolverExecution) {
        self.physics.set_execution(execution.clone());
        self.execution = execution;
    }

    /// The scene of the last frame.
    #[must_use]
    pub fn scene(&self) -> &RenderScene {
        &self.scene
    }
}
