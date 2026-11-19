//! The 3D stability, determinism and benchmark scenes (card 8-12): a small catalogue of scenes of
//! the 3D physics world, all with the defaults of `BodyDesc`, gravity 9.80665 m/s2, a fixed step of
//! 1/60 s and the solver's default settings. Each scene says how it is judged: a *standing* scene
//! (a tower, a pyramid, a heavy box on light ones) must not move after it has settled, a
//! *contained* scene (a pile dropped into a walled box) must stay in the box and come to rest, and
//! the *hanging* chain must keep its links together while it swings. Random sizes and tilts come
//! from a seeded generator, so a scene is the same on every run.

use base::math::{Quat, Vec3};
use base::rng::Pcg32;
use physics::d3::{
    BallJoint3, BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN,
    DEFAULT_SPECULATIVE_MARGIN, Joint3, JointDesc, PhysicsError, STANDARD_GRAVITY, Shape3,
    SleepConfig, SolverConfig, SolverExecution, World, WorldConfig,
};
use std::time::Instant;

pub use crate::scene::{DT, NANOS_PER_SECOND, REST_SPEED, SETTLED_FROM};

/// "No body moved more than this after settling", in metres.
pub const STANDING_DRIFT: f32 = crate::scene::DRIFT_BOUND;

/// No body of a sound scene is faster than this, in metres per second.
pub const SPEED_LIMIT: f32 = 30.0;

/// Distance between two links of the chain, in metres.
pub const CHAIN_LINK: f32 = 0.8;

/// Half the inner width and depth of the box a pile is dropped into, in metres.
pub const CONTAINER_HALF: f32 = 3.0;

/// How far past the wall a body may be before it counts as escaped, in metres.
const ESCAPE_MARGIN: f32 = 0.5;

/// Height above which a body has left the box of a pile, in metres.
const CEILING: f32 = 2.0 * WALL_HALF_HEIGHT;

/// Half the height of a wall, in metres.
const WALL_HALF_HEIGHT: f32 = 3.0;

/// Half the thickness of the floor and the walls, in metres.
const WALL_HALF: f32 = 0.5;

/// Half side of the boxes of the towers and pyramids, in metres.
const HALF: f32 = 0.25;

/// Height of the anchor of the chain, in metres.
const ANCHOR_HEIGHT: f32 = 8.0;

/// Angular damping of the bodies of a pile, per second: the stand-in for rolling resistance, which
/// the solver does not model, so that a sphere on a flat floor comes to rest.
const PILE_ROLL_DAMPING: f32 = 2.0;

/// The random stream of the piles.
const RNG_STREAM: u64 = 812;

/// Pair and manifold capacity per body.
const PAIRS_PER_BODY: u32 = 32;

/// Side of the grid a pile is dropped from: a layer has `GRID * GRID` bodies.
const GRID: u8 = 4;

/// Spacing of the drop grid, in metres.
const GRID_SPACING: f32 = 1.2;

/// Height of the first layer of a pile, and between layers, in metres.
const DROP_BASE: f32 = 0.6;
const DROP_LAYER: f32 = 0.9;

/// Random sideways offset of a dropped body, in metres.
const DROP_JITTER: f32 = 0.2;

/// Smallest size of a pile body (radius or half side) and the random extra, in metres.
const PILE_SIZE: f32 = 0.25;
const PILE_SIZE_SPREAD: f32 = 0.1;

/// Radius of a pile capsule as a share of its half height.
const CAPSULE_SHARE: f32 = 0.8;

/// Shapes of a mixed pile, in turn: sphere, box, capsule.
const MIXED_SHAPES: u32 = 3;

/// Half side of the fixed anchor of the chain and radius of a link, in metres.
const ANCHOR_HALF: f32 = 0.1;
const LINK_RADIUS: f32 = 0.3;

/// Half width of the floor and half its thickness, in metres.
const FLOOR_HALF_WIDTH: f32 = 50.0;
const FLOOR_HALF_THICKNESS: f32 = 1.0;

/// Spare body slots for the fixed scenery.
const SCENERY_SLOTS: u32 = 8;

/// Densities of the tower scenes: normal, and the heavy box on top.
const NORMAL_DENSITY: f32 = 1.0;
const HEAVY_DENSITY: f32 = 20.0;

/// Seeds of the piles' generator.
const RNG_SEED: u64 = 12;

/// Layers of the pyramid, and boxes of the towers.
const PYRAMID_LAYERS: u8 = 6;

/// Layers of the large bench pyramid: 1,015 boxes, the counterpart of the 990 of W1.
const LARGE_LAYERS: u8 = 14;
const TALL_TOWER: u8 = 10;
const HEAVY_TOWER: u8 = 5;

/// Bodies of the sphere pile, the mixed pile and the chain.
const SPHERE_PILE: u8 = 60;
const MIXED_PILE: u8 = 90;
const CHAIN_LINKS: u8 = 12;

/// How a scene is judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judgement {
    /// Must not move after it has settled.
    Standing,
    /// Must stay in its box and come to rest.
    Contained,
    /// Must keep its links together while it swings.
    Hanging,
}

/// The scenes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene3 {
    /// Ten boxes one on the other.
    Tower,
    /// A square pyramid of boxes, six layers.
    Pyramid,
    /// Four light boxes with a box twenty times as dense on top.
    HeavyOnLight,
    /// Sixty spheres dropped into a walled box.
    SpherePile,
    /// Ninety spheres, boxes and capsules dropped into a walled box.
    MixedPile,
    /// Twelve spheres on ball joints, released horizontally from a fixed anchor.
    Chain,
    /// A square pyramid of 14 layers, 1,015 boxes: the benchmark scene, not in [`Self::ALL`].
    LargePyramid,
}

impl Scene3 {
    /// Every scene of the stability catalogue, in the order of the report. The large pyramid is
    /// not in it: it is a benchmark scene only.
    pub const ALL: [Self; 6] = [
        Self::Tower,
        Self::Pyramid,
        Self::HeavyOnLight,
        Self::SpherePile,
        Self::MixedPile,
        Self::Chain,
    ];

    /// The name the report and the tests use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tower => "tower",
            Self::Pyramid => "pyramid",
            Self::HeavyOnLight => "heavy-on-light",
            Self::SpherePile => "sphere-pile",
            Self::MixedPile => "mixed-pile",
            Self::Chain => "chain",
            Self::LargePyramid => "large-pyramid",
        }
    }

    /// Dynamic bodies in the scene (one number, used to build it and to test it).
    #[must_use]
    pub fn bodies(self) -> usize {
        usize::from(match self {
            Self::Tower => u16::from(TALL_TOWER),
            Self::Pyramid => pyramid_bodies(PYRAMID_LAYERS),
            Self::HeavyOnLight => u16::from(HEAVY_TOWER),
            Self::SpherePile => u16::from(SPHERE_PILE),
            Self::MixedPile => u16::from(MIXED_PILE),
            Self::Chain => u16::from(CHAIN_LINKS),
            Self::LargePyramid => pyramid_bodies(LARGE_LAYERS),
        })
    }

    /// How the scene is judged.
    #[must_use]
    pub const fn judgement(self) -> Judgement {
        match self {
            Self::Tower | Self::Pyramid | Self::HeavyOnLight | Self::LargePyramid => {
                Judgement::Standing
            }
            Self::SpherePile | Self::MixedPile => Judgement::Contained,
            Self::Chain => Judgement::Hanging,
        }
    }
}

/// A world under construction and the bodies it is judged by.
struct Builder {
    world: World,
    tracked: Vec<BodyHandle>,
}

impl Builder {
    fn new(bodies: u16, joints: u32, sleeping: bool) -> Result<Self, PhysicsError> {
        let capacity = u32::from(bodies) * PAIRS_PER_BODY;
        let world = World::new(WorldConfig {
            max_bodies: u32::from(bodies) + SCENERY_SLOTS,
            gravity: STANDARD_GRAVITY,
            max_pairs: capacity,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: capacity,
            max_joints: joints,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: if sleeping {
                SleepConfig::DEFAULT
            } else {
                SleepConfig::DISABLED
            },
        })?;
        Ok(Self {
            world,
            tracked: Vec::with_capacity(usize::from(bodies)),
        })
    }

    /// A static box centred at `at`.
    fn fixed(&mut self, at: Vec3, half: Vec3) -> Result<BodyHandle, PhysicsError> {
        self.world.create_body(&BodyDesc::new(
            BodyKind::Static,
            at,
            Shape3::Box { half_extents: half },
        ))
    }

    /// A dynamic body, tracked.
    fn add(&mut self, desc: &BodyDesc) -> Result<BodyHandle, PhysicsError> {
        let handle = self.world.create_body(desc)?;
        self.tracked.push(handle);
        Ok(handle)
    }

    /// The floor, whose top is at y = 0.
    fn floor(&mut self) -> Result<(), PhysicsError> {
        self.fixed(
            Vec3::new(0.0, -FLOOR_HALF_THICKNESS, 0.0),
            Vec3::new(FLOOR_HALF_WIDTH, FLOOR_HALF_THICKNESS, FLOOR_HALF_WIDTH),
        )?;
        Ok(())
    }

    /// The floor and four walls round a square of side `2 * CONTAINER_HALF`.
    fn container(&mut self) -> Result<(), PhysicsError> {
        let reach = CONTAINER_HALF + WALL_HALF;
        self.fixed(
            Vec3::new(0.0, -WALL_HALF, 0.0),
            Vec3::new(reach, WALL_HALF, reach),
        )?;
        for side in [-reach, reach] {
            let high = Vec3::new(WALL_HALF, WALL_HALF_HEIGHT, reach);
            self.fixed(Vec3::new(side, WALL_HALF_HEIGHT, 0.0), high)?;
            let wide = Vec3::new(reach, WALL_HALF_HEIGHT, WALL_HALF);
            self.fixed(Vec3::new(0.0, WALL_HALF_HEIGHT, side), wide)?;
        }
        Ok(())
    }
}

fn cube() -> Shape3 {
    Shape3::Box {
        half_extents: Vec3::new(HALF, HALF, HALF),
    }
}

/// `count` boxes one on the other at x = z = 0; the last has `top_density`.
fn tower(count: u8, top_density: f32, sleeping: bool) -> Result<Builder, PhysicsError> {
    let mut b = Builder::new(u16::from(count), 0, sleeping)?;
    b.floor()?;
    for level in 0..count {
        let at = Vec3::new(0.0, HALF + f32::from(level) * 2.0 * HALF, 0.0);
        let mut desc = BodyDesc::new(BodyKind::Dynamic, at, cube());
        if level + 1 == count {
            desc.density = top_density;
        }
        b.add(&desc)?;
    }
    Ok(b)
}

/// One layer of a pyramid: `side` by `side` boxes at height level `layer`.
fn pyramid_layer(b: &mut Builder, layer: u8, side: u8) -> Result<(), PhysicsError> {
    let centre = |n: u8| (f32::from(n) - f32::from(side - 1) / 2.0) * 2.0 * HALF;
    let height = HALF + f32::from(layer) * 2.0 * HALF;
    for i in 0..side {
        for j in 0..side {
            let at = Vec3::new(centre(i), height, centre(j));
            b.add(&BodyDesc::new(BodyKind::Dynamic, at, cube()))?;
        }
    }
    Ok(())
}

/// Boxes of a square pyramid of `layers` layers: the squares of 1 to `layers`.
fn pyramid_bodies(layers: u8) -> u16 {
    (1..=u16::from(layers)).map(|n| n * n).sum()
}

/// A square pyramid of `layers` layers: the bottom layer is `layers` boxes on a side.
fn pyramid(layers: u8, sleeping: bool) -> Result<Builder, PhysicsError> {
    let mut b = Builder::new(pyramid_bodies(layers), 0, sleeping)?;
    b.floor()?;
    for layer in 0..layers {
        pyramid_layer(&mut b, layer, layers - layer)?;
    }
    Ok(b)
}

/// Bias of the tilt axis upward, so a tilt is never about a horizontal axis only.
const TILT_UP: f32 = 0.1;

/// A random tilt.
fn tilt(rng: &mut Pcg32) -> Quat {
    let axis = Vec3::new(
        rng.next_f32() - 0.5,
        rng.next_f32() + TILT_UP,
        rng.next_f32() - 0.5,
    );
    let angle = rng.next_f32() * std::f32::consts::PI;
    axis.normalized()
        .and_then(|axis| Quat::from_axis_angle(axis, angle))
        .unwrap_or(Quat::IDENTITY)
}

/// The shape of body `index` of a pile: spheres only, or in turn a sphere, a box and a capsule.
fn pile_shape(rng: &mut Pcg32, index: u32, mixed: bool) -> Shape3 {
    let size = PILE_SIZE + PILE_SIZE_SPREAD * rng.next_f32();
    match if mixed { index % MIXED_SHAPES } else { 0 } {
        0 => Shape3::Sphere { radius: size },
        1 => Shape3::Box {
            half_extents: Vec3::new(size, size, size),
        },
        _ => Shape3::Capsule {
            half_height: size,
            radius: size * CAPSULE_SHARE,
        },
    }
}

/// `count` bodies dropped from a grid, layer by layer, into a walled box.
fn pile(count: u8, mixed: bool, sleeping: bool) -> Result<Builder, PhysicsError> {
    let mut b = Builder::new(u16::from(count), 0, sleeping)?;
    b.container()?;
    let mut rng = Pcg32::new(RNG_SEED, RNG_STREAM);
    let per_layer = GRID * GRID;
    let grid = |n: u8| (f32::from(n) - f32::from(GRID - 1) / 2.0) * GRID_SPACING;
    for index in 0..count {
        let cell = index % per_layer;
        let jitter = (rng.next_f32() - 0.5) * DROP_JITTER;
        let layer = f32::from(index / per_layer);
        let at = Vec3::new(
            grid(cell % GRID) + jitter,
            DROP_BASE + layer * DROP_LAYER,
            grid(cell / GRID) - jitter,
        );
        let shape = pile_shape(&mut rng, u32::from(index), mixed);
        let mut desc = BodyDesc::new(BodyKind::Dynamic, at, shape);
        desc.rotation = tilt(&mut rng);
        desc.angular_damping = PILE_ROLL_DAMPING;
        b.add(&desc)?;
    }
    Ok(b)
}

/// The joints of the chain with the points they hold, for the error check.
struct Link {
    first: BodyHandle,
    second: BodyHandle,
    anchor_first: Vec3,
    anchor_second: Vec3,
}

/// A chain of `count` spheres released horizontally from a fixed anchor.
fn chain(count: u8, sleeping: bool) -> Result<(Builder, Vec<Link>), PhysicsError> {
    let mut b = Builder::new(u16::from(count), u32::from(count), sleeping)?;
    let anchor = b.fixed(
        Vec3::new(0.0, ANCHOR_HEIGHT, 0.0),
        Vec3::new(ANCHOR_HALF, ANCHOR_HALF, ANCHOR_HALF),
    )?;
    let (reach, back) = (
        Vec3::new(CHAIN_LINK / 2.0, 0.0, 0.0),
        Vec3::new(-CHAIN_LINK / 2.0, 0.0, 0.0),
    );
    let mut links = Vec::new();
    let mut last = anchor;
    for n in 1..=count {
        let at = Vec3::new(f32::from(n) * CHAIN_LINK, ANCHOR_HEIGHT, 0.0);
        let next = b.add(&BodyDesc::new(
            BodyKind::Dynamic,
            at,
            Shape3::Sphere {
                radius: LINK_RADIUS,
            },
        ))?;
        b.world.create_joint(&JointDesc::new(
            last,
            next,
            Joint3::Ball(BallJoint3::new(reach, back)),
        ))?;
        links.push(Link {
            first: last,
            second: next,
            anchor_first: reach,
            anchor_second: back,
        });
        last = next;
    }
    Ok((b, links))
}

/// The deterministic workload of the instruction-count gate (card 8-29): a square pyramid of
/// `layers` layers on the serial scalar solver, sleeping off, stepped `steps` times. Returns the
/// number of boxes and the state hash, so two builds can be told to have run the same thing.
///
/// # Errors
/// Any [`PhysicsError`] of the build or of a step.
pub fn icount_workload3(layers: u8, steps: u32) -> Result<(u16, u64), PhysicsError> {
    let mut builder = pyramid(layers, false)?;
    builder.world.set_execution(SolverExecution::SERIAL);
    for _ in 0..steps {
        builder.world.step(DT)?;
    }
    Ok((pyramid_bodies(layers), builder.world.state_hash()))
}

/// A scene of the catalogue, built and ready to step.
pub struct Scene3World {
    world: World,
    tracked: Vec<BodyHandle>,
    links: Vec<Link>,
}

impl Scene3World {
    /// Builds `scene`, solved as `execution` says, with sleeping on or off.
    ///
    /// # Errors
    /// Any [`PhysicsError`] the world raises while it is built.
    pub fn new(
        scene: Scene3,
        execution: &SolverExecution,
        sleeping: bool,
    ) -> Result<Self, PhysicsError> {
        let (mut builder, links) = match scene {
            Scene3::Tower => (tower(TALL_TOWER, NORMAL_DENSITY, sleeping)?, Vec::new()),
            Scene3::Pyramid => (pyramid(PYRAMID_LAYERS, sleeping)?, Vec::new()),
            Scene3::HeavyOnLight => (tower(HEAVY_TOWER, HEAVY_DENSITY, sleeping)?, Vec::new()),
            Scene3::SpherePile => (pile(SPHERE_PILE, false, sleeping)?, Vec::new()),
            Scene3::MixedPile => (pile(MIXED_PILE, true, sleeping)?, Vec::new()),
            Scene3::Chain => chain(CHAIN_LINKS, sleeping)?,
            Scene3::LargePyramid => (pyramid(LARGE_LAYERS, sleeping)?, Vec::new()),
        };
        builder.world.set_execution(execution.clone());
        Ok(Self {
            world: builder.world,
            tracked: builder.tracked,
            links,
        })
    }

    /// The world, for the checks of a test.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Steps the world once.
    ///
    /// # Errors
    /// Any [`PhysicsError`] of the step.
    pub fn step(&mut self) -> Result<(), PhysicsError> {
        self.world.step(DT)
    }

    /// The state hash.
    #[must_use]
    pub fn hash(&self) -> u64 {
        self.world.state_hash()
    }

    /// Positions of the dynamic bodies in creation order.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`] cannot happen; the type says it may.
    pub fn positions(&self) -> Result<Vec<Vec3>, PhysicsError> {
        self.tracked
            .iter()
            .map(|&handle| Ok(self.world.body(handle)?.position()))
            .collect()
    }

    /// The fastest dynamic body now, in metres per second.
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

    /// Dynamic bodies that left the box of a pile (or fell through the floor).
    ///
    /// # Errors
    /// As [`positions`](Self::positions).
    pub fn escaped(&self) -> Result<usize, PhysicsError> {
        let out = CONTAINER_HALF + ESCAPE_MARGIN;
        Ok(self
            .positions()?
            .iter()
            .filter(|p| p.x.abs() > out || p.z.abs() > out || p.y < -ESCAPE_MARGIN || p.y > CEILING)
            .count())
    }

    /// Dynamic bodies asleep now.
    ///
    /// # Errors
    /// As [`positions`](Self::positions).
    pub fn asleep(&self) -> Result<usize, PhysicsError> {
        let mut count = 0;
        for &handle in &self.tracked {
            count += usize::from(self.world.is_asleep(handle)?);
        }
        Ok(count)
    }

    /// The most any joint of the chain is pulled apart: the distance between the two points it
    /// holds together, in metres (zero for a scene without joints).
    ///
    /// # Errors
    /// As [`positions`](Self::positions).
    pub fn link_error(&self) -> Result<f32, PhysicsError> {
        let mut worst = 0.0_f32;
        for link in &self.links {
            let (a, b) = (self.world.body(link.first)?, self.world.body(link.second)?);
            let held_a = a.position() + a.rotation().rotate(link.anchor_first);
            let held_b = b.position() + b.rotation().rotate(link.anchor_second);
            worst = worst.max((held_a - held_b).length());
        }
        Ok(worst)
    }
}

/// What a run of a scene found and cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Run3 {
    /// The farthest any dynamic body ended from where it started, in metres.
    pub drift: f32,
    /// The farthest any body ended from where it was at [`SETTLED_FROM`], in metres (0 when the
    /// run is shorter).
    pub settled_drift: f32,
    /// The fastest body after the last step, in metres per second.
    pub final_speed: f32,
    /// The fastest any body was after any step, in metres per second.
    pub peak_speed: f32,
    /// Bodies outside the box of a pile after the last step.
    pub escaped: usize,
    /// Bodies asleep after the last step.
    pub asleep: usize,
    /// The state hash of the final world (the replay hash).
    pub hash: u64,
    /// False when the pair list or the manifold list overflowed, so work was dropped.
    pub complete: bool,
    /// Wall time of each step in nanoseconds.
    pub step_ns: Vec<f64>,
}

/// The largest distance any of `now` is from its place in `before`, in metres.
fn largest_move(before: &[Vec3], now: &[Vec3]) -> f32 {
    before
        .iter()
        .zip(now)
        .map(|(a, b)| (*b - *a).length())
        .fold(0.0, f32::max)
}

/// Builds `scene` and steps it `steps` times, timing each step.
///
/// # Errors
/// Any [`PhysicsError`] of the build or of a step.
pub fn run_scene3(
    scene: Scene3,
    steps: u32,
    execution: &SolverExecution,
    sleeping: bool,
) -> Result<Run3, PhysicsError> {
    let mut world = Scene3World::new(scene, execution, sleeping)?;
    let start = world.positions()?;
    let mut settled = Vec::new();
    let mut peak = 0.0_f32;
    let mut step_ns = Vec::with_capacity(steps as usize);
    for step in 1..=steps {
        let began = Instant::now();
        world.step()?;
        step_ns.push(began.elapsed().as_secs_f64() * NANOS_PER_SECOND);
        peak = peak.max(world.fastest()?);
        if step == SETTLED_FROM {
            settled = world.positions()?;
        }
    }
    let end = world.positions()?;
    Ok(Run3 {
        drift: largest_move(&start, &end),
        settled_drift: largest_move(&settled, &end),
        final_speed: world.fastest()?,
        peak_speed: peak,
        escaped: world.escaped()?,
        asleep: world.asleep()?,
        hash: world.hash(),
        complete: world.world().pairs().is_complete() && world.world().manifolds().is_complete(),
        step_ns,
    })
}
