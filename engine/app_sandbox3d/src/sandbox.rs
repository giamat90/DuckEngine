//! The sandbox world: the ECS world, the physics world, the scene and the one `frame` call that
//! runs the nine phases of the frame plan over them.

use crate::controls::{Piece, Requests};
use crate::error::Error;
use crate::mesh::{BOX, CAPSULE, SCENERY_FIRST, SPHERE};
use crate::orbit::OrbitCamera;
use crate::spawn::{self, Made, drop_point, piece, scenery, stream_piece};
use base::math::{Quat, Vec3};
use base::rng::Pcg32;
use ecs::World;
use frame::{FixedStep, FramePlan, Phase};
use physics::d3::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, STANDARD_GRAVITY,
    Shape3, SleepConfig, SolverConfig, SolverExecution, World as PhysicsWorld, WorldConfig,
};
use render::ids::TextureId;
use render::lit::{DirectionalLight, Material};
use render::mesh_scene::{MeshRef, MeshScene};
use scene::{Transform3D, TransformUpdater3D};
use sync_physics::d3::{attach_body, pull_transforms, push_forces};
use sync_render::d3::{MeshRenderer, extract_meshes};

/// Nanoseconds in one physics step (a sixtieth of a second).
pub const STEP_TICKS: u64 = 16_666_667;
/// The step handed to the physics world, in seconds.
pub const DT: f32 = 1.0 / 60.0;
/// Most steps one frame may run: after a stall the time beyond it is dropped, not caught up.
pub const MAX_STEPS: u32 = 4;
/// Pieces the box holds at most.
pub const MAX_PIECES: usize = 128;
/// A piece joins the stream every this many steps.
pub const SPAWN_EVERY: u64 = 20;
/// Pieces the stream drops before it stops.
pub const STREAM_LIMIT: u32 = 40;
/// Pairs and manifolds the physics world holds.
const CONTACTS: u32 = 4096;
/// The colour of the floor and the walls.
const SLATE: [f32; 4] = [0.28, 0.35, 0.5, 1.0];
/// The colours of the three pieces.
const COLOURS: [[f32; 4]; 3] = [
    [0.95, 0.55, 0.3, 1.0],
    [0.4, 0.7, 0.95, 1.0],
    [0.5, 0.85, 0.45, 1.0],
];
/// Where the camera starts: the target, yaw, pitch and distance.
const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 1.5, 0.0);
const CAMERA_YAW: f32 = 0.6;
const CAMERA_PITCH: f32 = 0.55;
const CAMERA_DISTANCE: f32 = 18.0;

/// What one frame did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FrameReport {
    /// Physics steps run.
    pub steps: u32,
    /// Pieces dropped (the stream and the user).
    pub spawned: u32,
    /// Mesh instances extracted for the frame.
    pub extracted: u32,
}

/// Where the render phase sends the scene.
pub trait Draw {
    /// Draws one frame of `scene`.
    ///
    /// # Errors
    /// Whatever the render side reports.
    fn draw(&mut self, scene: &MeshScene) -> Result<(), Error>;
}

/// The light of the box: from above and to one side, a little ambient.
fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(-0.4, -1.0, -0.3),
        colour: [1.0, 0.96, 0.9],
        ambient: [0.22, 0.24, 0.28],
    }
}

/// The aspect of a view.
fn aspect_of(view: (u32, u32)) -> Result<f32, Error> {
    if view.0 == 0 || view.1 == 0 {
        return Err(Error::NoView);
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "window sizes are far below 2^24, so the f32 ratio is exact enough for a lens"
    )]
    Ok(view.0 as f32 / view.1 as f32)
}

/// The mesh number of a piece.
fn mesh_of(piece: Piece) -> u32 {
    match piece {
        Piece::Sphere => SPHERE,
        Piece::Box => BOX,
        Piece::Capsule => CAPSULE,
    }
}

/// The colour of a piece.
fn colour_of(piece: Piece) -> [f32; 4] {
    COLOURS[mesh_of(piece) as usize]
}

/// An empty physics world: gravity down, room for the box and its pieces.
fn build_physics() -> Result<PhysicsWorld, Error> {
    let walls = u32::try_from(scenery().len()).unwrap_or(u32::MAX);
    let max_bodies = walls + u32::try_from(MAX_PIECES).unwrap_or(u32::MAX);
    Ok(PhysicsWorld::new(WorldConfig {
        max_bodies,
        gravity: STANDARD_GRAVITY,
        max_pairs: CONTACTS,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: CONTACTS,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })?)
}

/// The sandbox.
pub struct Sandbox {
    texture: TextureId,
    seed: u64,
    view: (u32, u32),
    world: World,
    physics: PhysicsWorld,
    execution: SolverExecution,
    clock: FixedStep,
    updater: TransformUpdater3D,
    scene: MeshScene,
    orbit: OrbitCamera,
    rng: Pcg32,
    steps: u64,
    streamed: u32,
    pieces: usize,
}

impl Sandbox {
    /// A sandbox for a view of `view` pixels whose pieces are drawn with `texture`, with the
    /// stream seeded by `seed`.
    ///
    /// # Errors
    /// [`Error::NoView`] for a view with a zero side, or the error of whatever cannot be built.
    pub fn new(texture: TextureId, seed: u64, view: (u32, u32)) -> Result<Self, Error> {
        let orbit = OrbitCamera::new(CAMERA_TARGET, CAMERA_YAW, CAMERA_PITCH, CAMERA_DISTANCE);
        let scene = MeshScene::new(
            scenery().len() + MAX_PIECES,
            orbit.camera(aspect_of(view)?),
            sun(),
        )
        .map_err(|e| Error::Render(format!("{e:?}")))?;
        let mut sandbox = Self {
            texture,
            seed,
            view,
            world: World::new(),
            physics: build_physics()?,
            execution: SolverExecution::SERIAL,
            clock: FixedStep::new(STEP_TICKS, MAX_STEPS)
                .map_err(|e| Error::Render(format!("{e:?}")))?,
            updater: TransformUpdater3D::new(),
            scene,
            orbit,
            rng: Pcg32::new(seed, spawn::RNG_STREAM),
            steps: 0,
            streamed: 0,
            pieces: 0,
        };
        sandbox.populate()?;
        Ok(sandbox)
    }

    /// The box: the floor and the four walls, each a static body and an entity to draw. A static
    /// body never moves, so it has no component for the glue.
    fn populate(&mut self) -> Result<(), Error> {
        for (index, block) in (0..).zip(scenery()) {
            self.physics.create_body(&BodyDesc::new(
                BodyKind::Static,
                block.position,
                Shape3::Box {
                    half_extents: block.half,
                },
            ))?;
            let entity = self.world.spawn()?;
            self.world.insert(
                entity,
                Transform3D {
                    position: block.position,
                    rotation: Quat::IDENTITY,
                    scale: block.half,
                },
            )?;
            let mut renderer = MeshRenderer::new(MeshRef(SCENERY_FIRST + index), self.texture);
            renderer.material = Material { colour: SLATE };
            self.world.insert(entity, renderer)?;
        }
        Ok(())
    }

    /// Starts the box again from the seed.
    fn restart(&mut self) -> Result<(), Error> {
        self.world = World::new();
        self.physics = build_physics()?;
        self.physics.set_execution(self.execution.clone());
        self.rng = Pcg32::new(self.seed, spawn::RNG_STREAM);
        self.steps = 0;
        self.streamed = 0;
        self.pieces = 0;
        self.populate()
    }

    /// Drops a piece of `kind` at `at`; false when the box is full.
    fn spawn_piece(&mut self, kind: Piece, at: Vec3) -> Result<bool, Error> {
        if self.pieces >= MAX_PIECES {
            return Ok(false);
        }
        let Made { desc, scale } = piece(&mut self.rng, kind, at);
        let entity = self.world.spawn()?;
        self.world.insert(
            entity,
            Transform3D {
                scale,
                ..Transform3D::IDENTITY
            },
        )?;
        attach_body(&mut self.world, &mut self.physics, entity, &desc)?;
        let mut renderer = MeshRenderer::new(MeshRef(mesh_of(kind)), self.texture);
        renderer.material = Material {
            colour: colour_of(kind),
        };
        self.world.insert(entity, renderer)?;
        self.pieces += 1;
        Ok(true)
    }

    /// The stream: a piece joins every [`SPAWN_EVERY`] steps until [`STREAM_LIMIT`] have.
    fn stream(&mut self) -> Result<u32, Error> {
        if !self.steps.is_multiple_of(SPAWN_EVERY) || self.streamed >= STREAM_LIMIT {
            return Ok(0);
        }
        let kind = stream_piece(&mut self.rng);
        let at = drop_point(&mut self.rng);
        let spawned = u32::from(self.spawn_piece(kind, at)?);
        self.streamed += spawned;
        Ok(spawned)
    }

    /// The requests of the frame: a reset, a drop, then the camera.
    fn gameplay(&mut self, requests: Requests, report: &mut FrameReport) -> Result<(), Error> {
        if requests.reset {
            self.restart()?;
        }
        if let Some(kind) = requests.drop {
            let at = drop_point(&mut self.rng);
            report.spawned += u32::from(self.spawn_piece(kind, at)?);
        }
        self.orbit.drag(requests.orbit.x, requests.orbit.y);
        self.orbit.zoom(requests.zoom);
        self.scene
            .set_camera(self.orbit.camera(aspect_of(self.view)?));
        Ok(())
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
            Phase::Input | Phase::SyncAudio => {}
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
            }
            Phase::SceneUpdate => {
                self.updater.update(&mut self.world)?;
            }
            Phase::SyncRender => {
                report.extracted = extract_meshes(&mut self.world, &mut self.scene)?.extracted;
            }
            Phase::Render => draw.draw(&self.scene)?,
        }
        Ok(())
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

    /// Follows a window resize: the lens gets the new aspect.
    ///
    /// # Errors
    /// [`Error::NoView`] for a zero side; nothing changes then.
    pub fn resize(&mut self, view: (u32, u32)) -> Result<(), Error> {
        let aspect = aspect_of(view)?;
        self.scene.set_camera(self.orbit.camera(aspect));
        self.view = view;
        Ok(())
    }

    /// Pieces alive.
    #[must_use]
    pub fn pieces(&self) -> usize {
        self.pieces
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

    /// How the physics solver runs; it changes the speed, never a bit of the result.
    pub fn set_execution(&mut self, execution: SolverExecution) {
        self.physics.set_execution(execution.clone());
        self.execution = execution;
    }

    /// The orbit camera.
    #[must_use]
    pub fn orbit(&self) -> &OrbitCamera {
        &self.orbit
    }

    /// The scene of the last frame.
    #[must_use]
    pub fn scene(&self) -> &MeshScene {
        &self.scene
    }
}
