//! Random scenes for the property tests, made from a seed with the engine's own `Pcg32` (no
//! external crate). A scene is a bin (a floor and two walls) with 3 to 10 bodies dropped from
//! cells of a grid, so none overlaps at the start, and on every second seed a chain of 2 to 8 links:
//! hanging in the middle gap of the bin, or (one in four) a whip pinned clear of it. The same seed gives the same scene on every run and platform.

use super::scene::{
    BALL_ANGULAR_DAMPING, ChainPlan, Lie, Scene, chain, static_box, world_with_floor,
};
use base::math::Vec2;
use base::rng::Pcg32;
use physics::d2::{BodyDesc, BodyKind, PhysicsError, Rot2, Shape2, SleepConfig, World};

/// The stream of the generator's `Pcg32`.
const STREAM: u64 = 17;

/// Half the inside width of the bin, in metres.
const BIN_HALF: f32 = 4.5;

/// The height of the walls of the bin, in metres.
const WALL_HEIGHT: f32 = 8.0;

/// Columns of the grid the bodies are dropped from, the distance between columns and between rows
/// (a body is at most 0.64 m from its cell centre, plus a jitter of 0.15 m, so the gap of 0.62 m
/// between the two middle columns stays free for a hanging chain).
const COLUMNS: u32 = 4;
const COLUMN_GAP: f32 = 2.2;
const ROW_GAP: f32 = 1.7;
const FIRST_ROW: f32 = 0.9;
const JITTER: f32 = 0.15;

/// One chain in this many is released horizontally (a whip that keeps swinging); the others hang
/// and come to rest.
const WHIP_ONE_IN: u32 = 4;

/// Where a whip is pinned: clear of the bin on its right. It swings and touches nothing, so its
/// joints and the contacts of the bin never share an island.
const WHIP_TOP: Vec2 = Vec2::new(12.0, 6.0);

/// A hanging chain hangs from the middle of the bin, into the gap between the two middle columns
/// of the grid, with its lower end this high above the floor, in metres: falling bodies meet it, so
/// joints and contacts share a solve and an island.
const HANGING_BOTTOM: f32 = 1.0;

/// A value in `[low, high)`.
fn range(rng: &mut Pcg32, low: f32, high: f32) -> f32 {
    low + (high - low) * rng.next_f32()
}

/// A density between 0.5 and 5, or on every second draw between 5 and 50: two bodies of a pile can
/// differ in mass by a factor of 100 (area differences come on top).
fn random_density(rng: &mut Pcg32) -> f32 {
    if rng.next_u32().is_multiple_of(2) {
        range(rng, 0.5, 5.0)
    } else {
        range(rng, 5.0, 50.0)
    }
}

/// A random dynamic body at `at`: a circle or a box, of random size, turn, density and friction.
fn random_body(rng: &mut Pcg32, at: Vec2) -> BodyDesc {
    let shape = if rng.next_u32().is_multiple_of(2) {
        Shape2::Circle {
            radius: range(rng, 0.25, 0.5),
        }
    } else {
        Shape2::Box {
            half_extents: Vec2::new(range(rng, 0.2, 0.45), range(rng, 0.2, 0.45)),
        }
    };
    let mut desc = BodyDesc::new(BodyKind::Dynamic, at, shape);
    desc.rotation = Rot2::from_angle(range(rng, 0.0, std::f32::consts::FRAC_PI_2));
    desc.density = random_density(rng);
    desc.friction = range(rng, 0.2, 1.0);
    desc.angular_damping = BALL_ANGULAR_DAMPING;
    desc
}

/// The bin: two walls beside the floor of the world.
fn build_bin(world: &mut World) -> Result<(), PhysicsError> {
    let half = Vec2::new(0.25, WALL_HEIGHT / 2.0);
    for side in [-1.0, 1.0] {
        let x = side * (BIN_HALF + half.x);
        static_box(world, Vec2::new(x, half.y), half)?;
    }
    Ok(())
}

/// The scene of `seed` with the sleep configuration `sleep`. Tracked: every dynamic body.
pub fn generate(seed: u64, sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    generate_with_bin(seed, sleep).map(|(scene, _)| scene)
}

/// As [`generate`], with the number of bodies dropped in the bin (the first ones of `tracked`; the
/// links of the whip come after them).
pub fn generate_with_bin(seed: u64, sleep: SleepConfig) -> Result<(Scene, usize), PhysicsError> {
    let mut rng = Pcg32::new(seed, STREAM);
    let mut world = world_with_floor(40, 16, sleep)?;
    build_bin(&mut world)?;
    let count = 3 + rng.next_u32() % 8;
    let mut tracked = Vec::new();
    for i in 0..count {
        let column = f32::from(u8::try_from(i % COLUMNS).unwrap_or(0));
        let row = f32::from(u8::try_from(i / COLUMNS).unwrap_or(0));
        let at = Vec2::new(
            -1.5 * COLUMN_GAP + COLUMN_GAP * column + range(&mut rng, -JITTER, JITTER),
            FIRST_ROW + ROW_GAP * row + range(&mut rng, -JITTER, JITTER),
        );
        let desc = random_body(&mut rng, at);
        tracked.push(world.create_body(&desc)?);
    }
    let in_bin = tracked.len();
    if rng.next_u32().is_multiple_of(2) {
        let whip = rng.next_u32().is_multiple_of(WHIP_ONE_IN);
        let links = u8::try_from(2 + rng.next_u32() % 7).unwrap_or(2);
        let hanging_top = Vec2::new(0.0, HANGING_BOTTOM + 0.5 * f32::from(links));
        let (top, lie) = if whip {
            (WHIP_TOP, Lie::Horizontal)
        } else {
            (hanging_top, Lie::Hanging)
        };
        let plan = ChainPlan {
            top,
            links,
            lie,
            last_density: range(&mut rng, 0.5, 5.0),
        };
        tracked.extend(chain(&mut world, plan)?);
    }
    let scene = Scene {
        world,
        tracked,
        hang: Vec2::ZERO,
    };
    Ok((scene, in_bin))
}
