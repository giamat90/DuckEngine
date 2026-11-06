//! The scenes of the harness: a world builder, a hook that runs before each step (body and joint
//! creation and destruction, a ball dropped on sleepers), and a step count. Every random choice
//! comes from `Pcg32` with a fixed seed, so a scene is the same on every call, run and platform.
//! The set is the one of the design note (section 5) plus a colour-overflow hub and a mix of
//! joints and contacts.

use base::math::Vec2;
use base::rng::Pcg32;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Joint2,
    JointDesc, PhysicsError, RevoluteJoint2, Rot2, STANDARD_GRAVITY, Shape2, SleepConfig,
    SolverConfig, World, WorldConfig,
};

/// Builds the world of a scene.
pub type Build = fn() -> Result<World, PhysicsError>;

/// Runs before step `n` (counted from 0): creates or destroys things, or does nothing.
pub type Events = fn(&mut World, u32) -> Result<(), PhysicsError>;

/// A scene of the harness.
pub struct Scene {
    pub name: &'static str,
    pub steps: u32,
    pub build: Build,
    pub events: Option<Events>,
}

/// Room for every pair and manifold the scenes make.
const CAPACITY: u32 = 4096;

/// Bodies and joints a world of the harness can hold.
const MAX_BODIES: u32 = 160;
const MAX_JOINTS: u32 = 32;

/// Half extent of the boxes of the pyramid and the row, and the rows of the pyramid.
const HALF: f32 = 0.4;
const PYRAMID_ROWS: u8 = 8;

/// Boxes in the row, the gap between them, and the speed of the first one.
const ROW_BOXES: u8 = 24;
const ROW_GAP: f32 = 0.05;
const ROW_PUSH: f32 = 6.0;

/// Links of a whip and their half extents (0.5 m long, 0.1 m thick).
const LINK_HALF: Vec2 = Vec2::new(0.25, 0.05);

/// Spokes of the hub: more small balls touching one ball than there are colours.
const SPOKES: u8 = 30;

/// Bodies dropped in the scatter scenes and the mix, and the width of the area they fall in.
const SCATTER_BODIES: u32 = 40;
const MIX_BODIES: u32 = 14;
const SCATTER_WIDTH: f32 = 8.0;
const SCATTER_HEIGHT: f32 = 4.0;

/// The seeds of the scatter scenes, and the stream every generator uses.
const SEEDS: [u64; 3] = [3, 11, 2024];
const STREAM: u64 = 21;

/// Steps before the ball is dropped on the sleepers (they sleep long before), and the step count.
pub const DROP_STEP: u32 = 70;
const SLEEP_WAKE_STEPS: u32 = 160;

/// Steps at which the churn scene destroys and creates things.
const CHURN_STEPS: u32 = 60;

fn config(sleep: SleepConfig, gravity: Vec2) -> WorldConfig {
    WorldConfig {
        max_bodies: MAX_BODIES,
        gravity,
        max_pairs: CAPACITY,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: CAPACITY,
        max_joints: MAX_JOINTS,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    }
}

fn static_box(world: &mut World, at: Vec2, half: Vec2) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        at,
        Shape2::Box { half_extents: half },
    ))
}

/// A world with a 100 m wide floor whose top face is the line `y = 0`.
fn floor_world(sleep: SleepConfig) -> Result<World, PhysicsError> {
    let mut world = World::new(config(sleep, STANDARD_GRAVITY))?;
    static_box(&mut world, Vec2::new(0.0, -1.0), Vec2::new(50.0, 1.0))?;
    Ok(world)
}

fn crate_at(world: &mut World, at: Vec2, half: f32) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        at,
        Shape2::Box {
            half_extents: Vec2::new(half, half),
        },
    ))
}

fn place_pyramid(world: &mut World, rows: u8, x: f32) -> Result<(), PhysicsError> {
    for row in 0..rows {
        let count = rows - row;
        for i in 0..count {
            let dx = f32::from(i) - f32::from(count - 1) / 2.0;
            let at = Vec2::new(x + dx * 2.0 * HALF, HALF + f32::from(row) * 2.0 * HALF);
            crate_at(world, at, HALF)?;
        }
    }
    Ok(())
}

fn pyramid() -> Result<World, PhysicsError> {
    let mut world = floor_world(SleepConfig::DISABLED)?;
    place_pyramid(&mut world, PYRAMID_ROWS, 0.0)?;
    Ok(world)
}

/// Boxes side by side on the floor; the first is thrown at the rest and spins.
fn row_of_boxes() -> Result<World, PhysicsError> {
    let mut world = floor_world(SleepConfig::DISABLED)?;
    let pitch = 2.0 * HALF + ROW_GAP;
    for i in 0..ROW_BOXES {
        let at = Vec2::new(f32::from(i) * pitch, HALF);
        let handle = crate_at(&mut world, at, HALF)?;
        if i == 0 {
            world.set_linear_velocity(handle, Vec2::new(ROW_PUSH, 0.0))?;
            world.set_angular_velocity(handle, 1.0)?;
        }
    }
    Ok(world)
}

/// A value in `[low, high)`.
fn range(rng: &mut Pcg32, low: f32, high: f32) -> f32 {
    low + (high - low) * rng.next_f32()
}

/// A random dynamic body at `at`: a circle or a box of random size, turn, density and friction.
fn random_body(rng: &mut Pcg32, at: Vec2) -> BodyDesc {
    let shape = if rng.next_u32().is_multiple_of(2) {
        Shape2::Circle {
            radius: range(rng, 0.2, 0.45),
        }
    } else {
        Shape2::Box {
            half_extents: Vec2::new(range(rng, 0.2, 0.45), range(rng, 0.2, 0.45)),
        }
    };
    let mut desc = BodyDesc::new(BodyKind::Dynamic, at, shape);
    desc.rotation = Rot2::from_angle(range(rng, 0.0, std::f32::consts::PI));
    desc.density = range(rng, 0.5, 20.0);
    desc.friction = range(rng, 0.2, 1.0);
    desc
}

/// `count` random bodies dropped from random places between two walls.
fn scatter_into(world: &mut World, rng: &mut Pcg32, count: u32) -> Result<(), PhysicsError> {
    for _ in 0..count {
        let at = Vec2::new(
            range(rng, -SCATTER_WIDTH / 2.0, SCATTER_WIDTH / 2.0),
            range(rng, 0.6, SCATTER_HEIGHT),
        );
        world.create_body(&random_body(rng, at))?;
    }
    Ok(())
}

fn walls(world: &mut World) -> Result<(), PhysicsError> {
    let half = Vec2::new(0.25, SCATTER_HEIGHT / 2.0);
    for side in [-1.0, 1.0] {
        let x = side * (SCATTER_WIDTH / 2.0 + half.x);
        static_box(world, Vec2::new(x, half.y), half)?;
    }
    Ok(())
}

fn scatter(seed: u64) -> Result<World, PhysicsError> {
    let mut world = floor_world(SleepConfig::DEFAULT)?;
    walls(&mut world)?;
    let mut rng = Pcg32::new(seed, STREAM);
    scatter_into(&mut world, &mut rng, SCATTER_BODIES)?;
    Ok(world)
}

/// A whip of `links` links, one hinge each, released horizontally from a pin at `top`. Returns
/// the links, first link first.
fn whip(world: &mut World, top: Vec2, links: u8) -> Result<Vec<BodyHandle>, PhysicsError> {
    let pin = world.create_body(&BodyDesc::new(
        BodyKind::Static,
        top,
        Shape2::Circle { radius: 0.1 },
    ))?;
    let mut previous = (pin, Vec2::ZERO);
    let mut made = Vec::new();
    for i in 0..links {
        let centre = top + Vec2::new(0.5 * f32::from(i) + LINK_HALF.x, 0.0);
        let link = world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            centre,
            Shape2::Box {
                half_extents: LINK_HALF,
            },
        ))?;
        let near = Vec2::new(-LINK_HALF.x, 0.0);
        let joint = RevoluteJoint2::new(previous.1, near);
        world.create_joint(&JointDesc::new(previous.0, link, Joint2::Revolute(joint)))?;
        previous = (link, Vec2::new(LINK_HALF.x, 0.0));
        made.push(link);
    }
    Ok(made)
}

/// Links of the chain scene and of the mix.
const CHAIN_LINKS: u8 = 12;
const MIX_LINKS: u8 = 8;

fn chain() -> Result<World, PhysicsError> {
    let mut world = floor_world(SleepConfig::DISABLED)?;
    whip(&mut world, Vec2::new(0.0, 5.0), CHAIN_LINKS)?;
    Ok(world)
}

/// The whip sweeps through a heap of bodies below it, so hinges and contacts share the solve.
fn joints_and_contacts() -> Result<World, PhysicsError> {
    let mut world = floor_world(SleepConfig::DEFAULT)?;
    let top = Vec2::new(-3.0, 5.0);
    whip(&mut world, top, MIX_LINKS)?;
    let mut rng = Pcg32::new(7, STREAM);
    for _ in 0..MIX_BODIES {
        let at = Vec2::new(range(&mut rng, -3.5, 1.0), range(&mut rng, 0.6, 2.5));
        world.create_body(&random_body(&mut rng, at))?;
    }
    Ok(world)
}

/// Where the hub starts: it falls onto the floor and the spokes are squeezed.
const HUB_CENTRE: Vec2 = Vec2::new(-8.0, 3.0);

/// One large ball with more small balls touching it than there are colours, so part of its
/// constraints overflow into the serial set, next to a small pyramid that does not. The ball falls
/// onto the floor, so the contacts of the spokes with the floor share colours with the others.
fn hub() -> Result<World, PhysicsError> {
    let mut world = floor_world(SleepConfig::DISABLED)?;
    let centre = HUB_CENTRE;
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        centre,
        Shape2::Circle { radius: 1.0 },
    ))?;
    let step = std::f32::consts::TAU / f32::from(SPOKES);
    for k in 0..SPOKES {
        let spoke = Rot2::from_angle(step * f32::from(k)).rotate(Vec2::new(1.1, 0.0));
        let ball = world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            centre + spoke,
            Shape2::Circle { radius: 0.1 },
        ))?;
        world.set_linear_velocity(ball, spoke * -2.0)?;
    }
    place_pyramid(&mut world, 4, 8.0)?;
    Ok(world)
}

/// Two small pyramids that fall asleep; later a ball dropped on one of them wakes it.
fn sleepers() -> Result<World, PhysicsError> {
    let mut world = floor_world(SleepConfig::DEFAULT)?;
    place_pyramid(&mut world, 4, -6.0)?;
    place_pyramid(&mut world, 3, 6.0)?;
    Ok(world)
}

fn drop_ball(world: &mut World, step: u32) -> Result<(), PhysicsError> {
    if step == DROP_STEP {
        world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::new(-6.0, 9.0),
            Shape2::Circle { radius: 0.4 },
        ))?;
    }
    Ok(())
}

/// The n-th dynamic body in slot order.
fn nth_dynamic(world: &World, n: usize) -> Option<BodyHandle> {
    world
        .iter()
        .filter(|(_, body)| body.kind() == BodyKind::Dynamic)
        .nth(n)
        .map(|(handle, _)| handle)
}

/// A whip and a heap of bodies in which bodies and joints are made and destroyed between steps.
fn churn() -> Result<World, PhysicsError> {
    let mut world = floor_world(SleepConfig::DISABLED)?;
    walls(&mut world)?;
    whip(&mut world, Vec2::new(-6.0, 7.0), MIX_LINKS)?;
    let mut rng = Pcg32::new(5, STREAM);
    scatter_into(&mut world, &mut rng, MIX_BODIES)?;
    Ok(world)
}

/// Where the churn scene drops a new body.
const CHURN_DROP: Vec2 = Vec2::new(1.0, 6.0);

/// What the churn scene does between two steps.
#[derive(Clone, Copy)]
enum Action {
    /// Destroys the n-th dynamic body after the links of the whip (a body of the heap).
    DestroyHeapBody(usize),
    /// Makes a box; it takes the slot of a destroyed body with a higher generation.
    CreateCrate,
    /// Destroys the oldest joint (a hinge of the whip); its slot is free.
    DestroyOldestJoint,
    /// Destroys the n-th dynamic body of the whip, with the hinges it has.
    DestroyLink(usize),
    /// Hinges two bodies of the heap; the hinge takes a free joint slot.
    JoinHeapBodies,
}

/// The churn: `(step, action)`. Destroying and creating alternate so slots are reused, and two
/// bodies go in the same step (40 and 41 are neighbours) before two come back.
const CHURN: [(u32, Action); 9] = [
    (10, Action::DestroyHeapBody(2)),
    (14, Action::CreateCrate),
    (20, Action::DestroyOldestJoint),
    (24, Action::JoinHeapBodies),
    (30, Action::DestroyLink(3)),
    (40, Action::DestroyHeapBody(4)),
    (41, Action::DestroyHeapBody(6)),
    (44, Action::CreateCrate),
    (45, Action::CreateCrate),
];

fn act(world: &mut World, action: Action) -> Result<(), PhysicsError> {
    match action {
        Action::DestroyHeapBody(n) => destroy_nth(world, MIX_LINKS as usize + n),
        Action::CreateCrate => crate_at(world, CHURN_DROP, HALF).map(|_| ()),
        Action::DestroyOldestJoint => {
            let first = world.joints().first().map(|entry| entry.handle);
            first.map_or(Ok(()), |handle| world.destroy_joint(handle))
        }
        Action::DestroyLink(n) => destroy_nth(world, n),
        Action::JoinHeapBodies => join_heap_bodies(world),
    }
}

fn destroy_nth(world: &mut World, n: usize) -> Result<(), PhysicsError> {
    nth_dynamic(world, n).map_or(Ok(()), |handle| world.destroy_body(handle))
}

fn churn_events(world: &mut World, step: u32) -> Result<(), PhysicsError> {
    for (at, action) in CHURN {
        if at == step {
            act(world, action)?;
        }
    }
    Ok(())
}

fn join_heap_bodies(world: &mut World) -> Result<(), PhysicsError> {
    let (Some(a), Some(b)) = (
        nth_dynamic(world, MIX_LINKS as usize + 1),
        nth_dynamic(world, MIX_LINKS as usize + 3),
    ) else {
        return Ok(());
    };
    let hinge = RevoluteJoint2::new(Vec2::new(0.3, 0.0), Vec2::new(-0.3, 0.0));
    world
        .create_joint(&JointDesc::new(a, b, Joint2::Revolute(hinge)))
        .map(|_| ())
}

macro_rules! scatter_scene {
    ($name:literal, $seed:expr) => {
        Scene {
            name: $name,
            steps: SCATTER_STEPS,
            build: || scatter($seed),
            events: None,
        }
    };
}

const SCATTER_STEPS: u32 = 70;

/// The scenes of the matrix.
pub fn matrix() -> Vec<Scene> {
    vec![
        Scene {
            name: "pyramid",
            steps: 45,
            build: pyramid,
            events: None,
        },
        Scene {
            name: "row of boxes",
            steps: 60,
            build: row_of_boxes,
            events: None,
        },
        Scene {
            name: "chain",
            steps: 60,
            build: chain,
            events: None,
        },
        scatter_scene!("scatter seed 3", SEEDS[0]),
        scatter_scene!("scatter seed 11", SEEDS[1]),
        scatter_scene!("scatter seed 2024", SEEDS[2]),
        Scene {
            name: "sleep and wake",
            steps: SLEEP_WAKE_STEPS,
            build: sleepers,
            events: Some(drop_ball),
        },
        Scene {
            name: "creation and destruction",
            steps: CHURN_STEPS,
            build: churn,
            events: Some(churn_events),
        },
        Scene {
            name: "colour overflow hub",
            steps: 45,
            build: hub,
            events: None,
        },
        Scene {
            name: "joints and contacts",
            steps: 60,
            build: joints_and_contacts,
            events: None,
        },
    ]
}

/// The steps of the long runs: 10 s.
pub const LONG_STEPS: u32 = 600;

/// The scenes of the long runs: a scatter that comes to rest and falls asleep, and the whip that
/// never does.
pub fn long_runs() -> Vec<Scene> {
    vec![
        Scene {
            name: "scatter seed 3, long",
            steps: LONG_STEPS,
            build: || scatter(SEEDS[0]),
            events: None,
        },
        Scene {
            name: "joints and contacts, long",
            steps: LONG_STEPS,
            build: joints_and_contacts,
            events: None,
        },
    ]
}
