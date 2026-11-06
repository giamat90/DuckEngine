//! Workload W1 of `docs/budgets/reference-machine-and-workloads.md`: a pyramid of 0.5 m boxes on a
//! static floor (density 1, friction 0.6, restitution 0 are the defaults of `BodyDesc`), gravity
//! 9.80665 m/s2, fixed step 1/60 s, the solver's default 4 sub-steps and 1 relax pass (the
//! equivalent of the "10 velocity and 4 position iterations" of the W1 text, owner choice O9), one
//! thread unless a scheduler is given.
//!
//! A pyramid of `n` rows has `n (n + 1) / 2` boxes, so exactly 1,000 is not a pyramid: the full
//! scene has 44 rows, 990 boxes (the nearest below; 45 rows have 1,035).

use crate::counters::WorkCounters;
use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    PhysicsError, STANDARD_GRAVITY, Shape2, SleepConfig, SolverConfig, SolverExecution, World,
    WorldConfig,
};
use std::time::Instant;

/// Rows of the full W1 pyramid: 990 boxes.
pub const W1_ROWS: u8 = 44;

/// Steps of workload W1.
pub const W1_STEPS: u32 = 300;

/// The fixed step: 60 Hz.
pub const DT: f32 = 1.0 / 60.0;

/// Half extent of a box: boxes of 0.5 m.
pub const HALF: f32 = 0.25;

/// "No box moved more than 0.05 m from its rest position" (W1), in metres.
pub const DRIFT_BOUND: f32 = 0.05;

/// The step from which the settled drift is measured: the contacts compress in the first second.
pub const SETTLED_FROM: u32 = 100;

/// A box at rest is slower than this, in metres per second (the value of the stability scenes).
pub const REST_SPEED: f32 = 0.02;

/// Pair and manifold capacity per box: a box touches at most 8 neighbours in the grown boxes of
/// the broadphase, each pair counted once.
const PAIRS_PER_BOX: u32 = 16;

/// Half width of the floor, wide enough for the bottom row of the full pyramid (22 m).
const FLOOR_HALF_WIDTH: f32 = 50.0;

/// Nanoseconds in a second.
pub const NANOS_PER_SECOND: f64 = 1e9;

/// Boxes of a pyramid of `rows` rows.
#[must_use]
pub fn boxes_in(rows: u8) -> u32 {
    u32::from(rows) * (u32::from(rows) + 1) / 2
}

/// The world of a pyramid and the boxes it is judged by.
pub struct PyramidScene {
    world: World,
    tracked: Vec<BodyHandle>,
    sub_steps: u32,
}

impl PyramidScene {
    /// A pyramid of `rows` rows, `rows` boxes in the bottom row, resting on the floor, with
    /// sleeping on or off, solved as `execution` says.
    ///
    /// # Errors
    /// Any [`PhysicsError`] the world raises while it is built.
    pub fn new(
        rows: u8,
        execution: &SolverExecution,
        sleeping: bool,
    ) -> Result<Self, PhysicsError> {
        let boxes = boxes_in(rows);
        let capacity = boxes * PAIRS_PER_BOX;
        let config = WorldConfig {
            max_bodies: boxes + 1,
            gravity: STANDARD_GRAVITY,
            max_pairs: capacity,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: capacity,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: if sleeping {
                SleepConfig::DEFAULT
            } else {
                SleepConfig::DISABLED
            },
        };
        let mut world = World::new(config)?;
        world.create_body(&BodyDesc::new(
            BodyKind::Static,
            Vec2::new(0.0, -1.0),
            Shape2::Box {
                half_extents: Vec2::new(FLOOR_HALF_WIDTH, 1.0),
            },
        ))?;
        let tracked = place_boxes(&mut world, rows)?;
        world.set_execution(execution.clone());
        Ok(Self {
            world,
            tracked,
            sub_steps: SolverConfig::DEFAULT.sub_steps,
        })
    }

    /// The world, for the checks of a test.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Positions of the boxes in creation order (bottom row first).
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`] cannot happen; the type says it may.
    pub fn positions(&self) -> Result<Vec<Vec2>, PhysicsError> {
        self.tracked
            .iter()
            .map(|&handle| Ok(self.world.body(handle)?.position()))
            .collect()
    }

    /// The fastest box now, in metres per second.
    ///
    /// # Errors
    /// As [`positions`](Self::positions).
    pub fn fastest(&self) -> Result<f32, PhysicsError> {
        let mut worst = 0.0_f32;
        for &handle in &self.tracked {
            worst = worst.max(self.world.body(handle)?.linear_velocity().length());
        }
        Ok(worst)
    }
}

/// The boxes of a pyramid centred on x = 0, bottom row first, standing on the line y = 0.
fn place_boxes(world: &mut World, rows: u8) -> Result<Vec<BodyHandle>, PhysicsError> {
    let mut tracked = Vec::new();
    for row in 0..rows {
        let count = rows - row;
        for i in 0..count {
            let dx = f32::from(i) - f32::from(count - 1) / 2.0;
            let at = Vec2::new(dx * 2.0 * HALF, HALF + f32::from(row) * 2.0 * HALF);
            let half_extents = Vec2::new(HALF, HALF);
            tracked.push(world.create_body(&BodyDesc::new(
                BodyKind::Dynamic,
                at,
                Shape2::Box { half_extents },
            ))?);
        }
    }
    Ok(tracked)
}

/// The largest distance any of `now` is from its place in `before`, in metres.
fn largest_move(before: &[Vec2], now: &[Vec2]) -> f32 {
    before
        .iter()
        .zip(now)
        .map(|(a, b)| (*b - *a).length())
        .fold(0.0, f32::max)
}

/// What a run of the scene found and cost.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneRun {
    /// Work done.
    pub counters: WorkCounters,
    /// The farthest any box ended from where it started, in metres (W1 reads this against 0.05).
    pub max_drift: f32,
    /// The farthest any box ended from where it was at [`SETTLED_FROM`], in metres (0 when the run
    /// is shorter): the motion that is not the initial compression.
    pub settled_drift: f32,
    /// The fastest box after the last step, in metres per second.
    pub final_speed: f32,
    /// The state hash of the final world (the replay hash).
    pub hash: u64,
    /// False when the pair list or the manifold list overflowed, so work was dropped.
    pub complete: bool,
    /// Wall time of each step in nanoseconds.
    pub step_ns: Vec<f64>,
}

impl SceneRun {
    /// True when no box moved more than [`DRIFT_BOUND`] from its start and the boxes are at rest.
    #[must_use]
    pub fn is_standing(&self) -> bool {
        self.max_drift < DRIFT_BOUND && self.final_speed < REST_SPEED
    }
}

/// Steps the serial scalar pyramid `steps` times with no timing and no thread, the work an
/// instruction counter watches; returns the work counters and the final state hash.
///
/// # Errors
/// Any [`PhysicsError`] of the build or of a step.
pub fn icount_workload(rows: u8, steps: u32) -> Result<(WorkCounters, u64), PhysicsError> {
    let mut scene = PyramidScene::new(rows, &SolverExecution::SERIAL, false)?;
    let mut counters = WorkCounters::default();
    for _ in 0..steps {
        scene.world.step(DT)?;
        counters.record(&scene.world, scene.sub_steps);
    }
    Ok((counters, scene.world.state_hash()))
}

/// Builds the pyramid and steps it `steps` times, timing each step.
///
/// # Errors
/// Any [`PhysicsError`] of the build or of a step.
pub fn run_scene(
    rows: u8,
    steps: u32,
    execution: &SolverExecution,
    sleeping: bool,
) -> Result<SceneRun, PhysicsError> {
    let mut scene = PyramidScene::new(rows, execution, sleeping)?;
    let start = scene.positions()?;
    let mut settled = Vec::new();
    let mut counters = WorkCounters::default();
    let mut step_ns = Vec::with_capacity(steps as usize);
    for step in 1..=steps {
        let began = Instant::now();
        scene.world.step(DT)?;
        step_ns.push(began.elapsed().as_secs_f64() * NANOS_PER_SECOND);
        counters.record(&scene.world, scene.sub_steps);
        if step == SETTLED_FROM {
            settled = scene.positions()?;
        }
    }
    let end = scene.positions()?;
    Ok(SceneRun {
        counters,
        max_drift: largest_move(&start, &end),
        settled_drift: largest_move(&settled, &end),
        final_speed: scene.fastest()?,
        hash: scene.world.state_hash(),
        complete: scene.world.pairs().is_complete() && scene.world.manifolds().is_complete(),
        step_ns,
    })
}
