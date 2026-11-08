//! Scene builders shared by the stability scenes. Everything returns a `Result`, because the lints
//! deny `unwrap` and `expect`. A scene is a world plus the handles its checks follow.
//!
//! The chain and bridge builders follow the ones of `tests/joints` (card 4-16). They are written
//! again here because two integration test binaries cannot share a module without each one
//! reporting the helpers it does not use as dead code, and no `allow` is permitted.

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Joint2,
    JointDesc, JointHandle, PhysicsError, RevoluteJoint2, Rot2, STANDARD_GRAVITY, Shape2,
    SleepConfig, SolverConfig, World, WorldConfig,
};

/// Half extent of a box of workload W1: 0.5 m boxes.
pub const W1_HALF: f32 = 0.25;

/// Half extents of a chain link: 0.5 m long, 0.1 m thick (as in the chain scenes of card 4-16).
pub const LINK_HALF: Vec2 = Vec2::new(0.25, 0.05);

/// Room for every pair and manifold the scenes make.
const PAIR_CAPACITY: u32 = 16_384;

/// A world and the handles the checks of a scene follow.
pub struct Scene {
    pub world: World,
    /// The dynamic bodies whose motion the scene is judged by.
    pub tracked: Vec<BodyHandle>,
    /// The point a chain or bridge of the scene hangs from (the origin when nothing does).
    pub hang: Vec2,
}

/// The configuration of every scene.
pub fn config(max_bodies: u32, max_joints: u32, sleep: SleepConfig) -> WorldConfig {
    WorldConfig {
        max_bodies,
        gravity: STANDARD_GRAVITY,
        max_pairs: PAIR_CAPACITY,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: PAIR_CAPACITY,
        max_joints,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    }
}

/// A world with a static floor whose top face is the line `y = 0`, 100 m wide.
pub fn world_with_floor(
    max_bodies: u32,
    max_joints: u32,
    sleep: SleepConfig,
) -> Result<World, PhysicsError> {
    let mut world = World::new(config(max_bodies, max_joints, sleep))?;
    static_box(&mut world, Vec2::new(0.0, -1.0), Vec2::new(50.0, 1.0))?;
    Ok(world)
}

pub fn static_box(world: &mut World, at: Vec2, half: Vec2) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        at,
        Shape2::Box { half_extents: half },
    ))
}

/// A static circle of radius 0.1: a point to hang things from.
pub fn pin(world: &mut World, at: Vec2) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        at,
        Shape2::Circle { radius: 0.1 },
    ))
}

/// A dynamic box of `half` extents and `density` at `at`.
pub fn dynamic_box(
    world: &mut World,
    at: Vec2,
    half: Vec2,
    density: f32,
) -> Result<BodyHandle, PhysicsError> {
    let mut desc = BodyDesc::new(BodyKind::Dynamic, at, Shape2::Box { half_extents: half });
    desc.density = density;
    world.create_body(&desc)
}

/// Angular damping of a circle: a stand-in for rolling resistance. Without it a circle that rolls
/// on a flat floor coasts at a speed too high to fall asleep (0.016 m/s and 0.054 rad/s were
/// measured after 10 s). Added after the first sleeping run of the mixed scene.
pub const BALL_ANGULAR_DAMPING: f32 = 1.0;

/// A dynamic circle of `radius` and `density` at `at`.
pub fn dynamic_ball(
    world: &mut World,
    at: Vec2,
    radius: f32,
    density: f32,
) -> Result<BodyHandle, PhysicsError> {
    let mut desc = BodyDesc::new(BodyKind::Dynamic, at, Shape2::Circle { radius });
    desc.density = density;
    desc.angular_damping = BALL_ANGULAR_DAMPING;
    world.create_body(&desc)
}

/// A pyramid of `rows` rows of boxes of half extent `half` standing on the floor, `rows` boxes in
/// the bottom row: the shape of workload W1 (which has about 1,000 boxes of 0.5 m).
pub fn pyramid(rows: u8, half: f32, sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    pyramid_with(rows, half, sleep, SolverConfig::DEFAULT)
}

/// [`pyramid`] with a chosen solver configuration.
pub fn pyramid_with(
    rows: u8,
    half: f32,
    sleep: SleepConfig,
    solver: SolverConfig,
) -> Result<Scene, PhysicsError> {
    let count = u32::from(rows) * (u32::from(rows) + 1) / 2;
    let mut cfg = config(count + 2, 0, sleep);
    cfg.solver = solver;
    let mut world = World::new(cfg)?;
    static_box(&mut world, Vec2::new(0.0, -1.0), Vec2::new(50.0, 1.0))?;
    let mut tracked = Vec::new();
    place_pyramid(&mut world, &mut tracked, rows, half, 0.0)?;
    Ok(Scene {
        world,
        tracked,
        hang: Vec2::ZERO,
    })
}

/// The boxes of a pyramid centred on `x`, pushed onto `tracked`.
pub fn place_pyramid(
    world: &mut World,
    tracked: &mut Vec<BodyHandle>,
    rows: u8,
    half: f32,
    x: f32,
) -> Result<(), PhysicsError> {
    for row in 0..rows {
        let count = rows - row;
        for i in 0..count {
            let dx = f32::from(i) - f32::from(count - 1) / 2.0;
            let at = Vec2::new(x + dx * 2.0 * half, half + f32::from(row) * 2.0 * half);
            tracked.push(dynamic_box(world, at, Vec2::new(half, half), 1.0)?);
        }
    }
    Ok(())
}

/// A column of boxes of half extent `half` on the floor, one per entry of `densities`, lowest
/// first.
pub fn column(densities: &[f32], half: f32, sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    let count = u32::try_from(densities.len()).unwrap_or(u32::MAX);
    let mut world = world_with_floor(count + 2, 0, sleep)?;
    let mut tracked = Vec::new();
    for (level, &density) in (0_u16..).zip(densities) {
        let at = Vec2::new(0.0, half + f32::from(level) * 2.0 * half);
        tracked.push(dynamic_box(&mut world, at, Vec2::new(half, half), density)?);
    }
    Ok(Scene {
        world,
        tracked,
        hang: Vec2::ZERO,
    })
}

/// `point` of the world as a point of the frame of `body`, relative to its centre.
fn local_point(world: &World, body: BodyHandle, point: Vec2) -> Result<Vec2, PhysicsError> {
    let body = world.body(body)?;
    let (cos, sin) = body.rotation().direction();
    let offset = point - body.position();
    Ok(Vec2::new(
        cos * offset.x + sin * offset.y,
        cos * offset.y - sin * offset.x,
    ))
}

/// A hinge at the world point `point`, whichever way the two bodies are placed.
pub fn hinge_at(
    world: &mut World,
    first: BodyHandle,
    second: BodyHandle,
    point: Vec2,
) -> Result<JointHandle, PhysicsError> {
    let anchor_a = local_point(world, first, point)?;
    let anchor_b = local_point(world, second, point)?;
    world.create_joint(&JointDesc::new(
        first,
        second,
        Joint2::Revolute(RevoluteJoint2::new(anchor_a, anchor_b)),
    ))
}

/// How a chain lies when it is made.
#[derive(Clone, Copy)]
pub enum Lie {
    /// Straight out to the right from the pin: released, it whips down.
    Horizontal,
    /// Straight down from the pin: at rest.
    Hanging,
}

/// The pin to hang a chain from and the number of links.
#[derive(Clone, Copy)]
pub struct ChainPlan {
    pub top: Vec2,
    pub links: u8,
    pub lie: Lie,
    /// The density of the last link; the others have density 1 (0.05 kg).
    pub last_density: f32,
}

/// A chain from a new pin at `plan.top`, one hinge per link. Returns the links, first link first.
pub fn chain(world: &mut World, plan: ChainPlan) -> Result<Vec<BodyHandle>, PhysicsError> {
    let mut previous = pin(world, plan.top)?;
    let mut links = Vec::new();
    for i in 0..plan.links {
        let density = if i + 1 == plan.links {
            plan.last_density
        } else {
            1.0
        };
        let along = 0.5 * f32::from(i);
        let (centre, rotation, joint_at) = match plan.lie {
            Lie::Horizontal => (
                plan.top + Vec2::new(along + LINK_HALF.x, 0.0),
                Rot2::IDENTITY,
                plan.top + Vec2::new(along, 0.0),
            ),
            Lie::Hanging => (
                plan.top - Vec2::new(0.0, along + LINK_HALF.x),
                Rot2::from_angle(std::f32::consts::FRAC_PI_2),
                plan.top - Vec2::new(0.0, along),
            ),
        };
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            centre,
            Shape2::Box {
                half_extents: LINK_HALF,
            },
        );
        desc.rotation = rotation;
        desc.density = density;
        let link = world.create_body(&desc)?;
        hinge_at(world, previous, link, joint_at)?;
        links.push(link);
        previous = link;
    }
    Ok(links)
}

/// A bridge of `planks` planks between two pins `span` metres apart at height `height`, laid along
/// a parabola that sags `sag` metres in the middle; a hinge at every corner of the line.
pub fn bridge(
    world: &mut World,
    span: f32,
    height: f32,
    planks: u8,
    sag: f32,
) -> Result<Vec<BodyHandle>, PhysicsError> {
    let left = pin(world, Vec2::new(0.0, height))?;
    let right = pin(world, Vec2::new(span, height))?;
    let corner = |i: u8| {
        let t = f32::from(i) / f32::from(planks);
        Vec2::new(span * t, height - sag * 4.0 * t * (1.0 - t))
    };
    let mut links: Vec<BodyHandle> = Vec::new();
    let mut previous = left;
    for i in 0..planks {
        let (from, to) = (corner(i), corner(i + 1));
        let along = to - from;
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            (from + to) * 0.5,
            Shape2::Box {
                half_extents: Vec2::new(along.length() * 0.5, LINK_HALF.y),
            },
        );
        desc.rotation = Rot2::from_angle(base::dmath::atan2(along.y, along.x));
        let plank = world.create_body(&desc)?;
        hinge_at(world, previous, plank, from)?;
        links.push(plank);
        previous = plank;
    }
    hinge_at(world, previous, right, corner(planks))?;
    Ok(links)
}
