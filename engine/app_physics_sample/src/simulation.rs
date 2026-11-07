//! The simulation: a walled arena, a stream of falling boxes and discs, and the fixed-step
//! accumulator that turns frame times into whole physics steps. Pure code over the physics world;
//! no device and no window. The step runs on the calling thread (the world's default serial
//! execution; the sample does not use the job pool).

use crate::error::Error;
use base::math::Vec2;
use base::rng::Pcg32;
use frame::FixedStep;
use physics::d2::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Rot2,
    STANDARD_GRAVITY, Shape2, SleepConfig, SolverConfig, World, WorldConfig,
};

/// The seed of the stream in the window and in the headless run.
pub const DEFAULT_SEED: u64 = 24;
/// Length of a physics step in ticks; a tick is a nanosecond (60 steps a second).
pub const STEP_TICKS: u64 = 16_666_667;
// (1/60 s is 16 666 666.67 ns; the 0.33 ns rounding is far below anything the accumulator shows.)
/// Most steps one frame may run: after a stall the time beyond it is dropped, not caught up.
pub const MAX_STEPS: u32 = 4;
/// The step handed to the world, in seconds.
pub const DT: f32 = 1.0 / 60.0;
/// A body joins the stream every this many steps.
pub const SPAWN_EVERY: u64 = 12;
/// Bodies the stream drops in all; clicks add more up to the world's capacity.
pub const STREAM_LIMIT: u32 = 80;
/// Half the width of the inside of the arena, in metres.
pub const HALF_WIDTH: f32 = 18.0;
/// Height of the top of the floor, in metres.
pub const FLOOR_TOP: f32 = -10.0;
/// Height the stream drops bodies from, in metres.
pub const SPAWN_HEIGHT: f32 = 10.0;
/// The stream drops bodies at a random x within this distance of the middle, in metres.
pub const SPAWN_SPREAD: f32 = 15.0;
/// Highest a click can place a body, in metres.
pub const CEILING: f32 = 11.0;
/// Bodies the world holds: the floor, two walls and the falling ones.
pub const MAX_BODIES: u32 = 128;
/// Pairs and manifolds the world holds.
const CONTACTS: u32 = 2048;
/// Half the thickness of the floor and the walls, in metres.
const WALL_HALF: f32 = 0.5;
/// Half the height of a wall, in metres.
const WALL_HALF_HEIGHT: f32 = 12.0;
/// Distance a click keeps from the walls and the floor, in metres.
const MARGIN: f32 = 1.0;
/// Smallest half side of a box and smallest radius of a disc, in metres.
const MIN_HALF: f32 = 0.3;
/// Most a half side adds to [`MIN_HALF`], in metres.
const EXTRA_HALF: f32 = 0.3;
/// Most a radius adds to [`MIN_HALF`], in metres.
const EXTRA_RADIUS: f32 = 0.25;
/// The stream and the clicks use this stream number of the generator.
const RNG_STREAM: u64 = 24;

fn config() -> WorldConfig {
    WorldConfig {
        max_bodies: MAX_BODIES,
        gravity: STANDARD_GRAVITY,
        max_pairs: CONTACTS,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: CONTACTS,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    }
}

fn wall(at: Vec2, half: Vec2) -> BodyDesc {
    BodyDesc::new(BodyKind::Static, at, Shape2::Box { half_extents: half })
}

/// A world holding the floor and the two walls of the arena.
///
/// # Errors
/// The physics world's error.
pub fn build_world() -> Result<World, Error> {
    let mut world = World::new(config())?;
    let side = HALF_WIDTH + WALL_HALF;
    world.create_body(&wall(
        Vec2::new(0.0, FLOOR_TOP - WALL_HALF),
        Vec2::new(side + WALL_HALF, WALL_HALF),
    ))?;
    for x in [-side, side] {
        world.create_body(&wall(
            Vec2::new(x, FLOOR_TOP + WALL_HALF_HEIGHT),
            Vec2::new(WALL_HALF, WALL_HALF_HEIGHT),
        ))?;
    }
    Ok(world)
}

/// What falls: a seeded generator that picks shape, size and tilt.
struct Spawner {
    rng: Pcg32,
}

impl Spawner {
    fn new(seed: u64) -> Self {
        Self {
            rng: Pcg32::new(seed, RNG_STREAM),
        }
    }

    /// A random x for the stream, within [`SPAWN_SPREAD`] of the middle.
    fn stream_x(&mut self) -> f32 {
        (self.rng.next_f32() - 0.5) * 2.0 * SPAWN_SPREAD
    }

    /// A box or a disc at `at`, of a random size and tilt.
    fn body(&mut self, at: Vec2) -> BodyDesc {
        let circle = self.rng.next_u32() % 2 == 1;
        let shape = if circle {
            Shape2::Circle {
                radius: MIN_HALF + EXTRA_RADIUS * self.rng.next_f32(),
            }
        } else {
            Shape2::Box {
                half_extents: Vec2::new(
                    MIN_HALF + EXTRA_HALF * self.rng.next_f32(),
                    MIN_HALF + EXTRA_HALF * self.rng.next_f32(),
                ),
            }
        };
        let mut desc = BodyDesc::new(BodyKind::Dynamic, at, shape);
        desc.rotation = Rot2::from_angle(self.rng.next_f32() * std::f32::consts::TAU);
        desc
    }
}

/// The arena, its stream and its clock.
pub struct Simulation {
    world: World,
    spawner: Spawner,
    clock: FixedStep,
    seed: u64,
    steps: u64,
    streamed: u32,
}

impl Simulation {
    /// A fresh arena; the same `seed` gives the same run.
    ///
    /// # Errors
    /// The physics world's error, or the accumulator's.
    pub fn new(seed: u64) -> Result<Self, Error> {
        Ok(Self {
            world: build_world()?,
            spawner: Spawner::new(seed),
            clock: FixedStep::new(STEP_TICKS, MAX_STEPS)?,
            seed,
            steps: 0,
            streamed: 0,
        })
    }

    /// Starts over with the seed the simulation was built with.
    ///
    /// # Errors
    /// As [`Simulation::new`].
    pub fn reset(&mut self) -> Result<(), Error> {
        *self = Self::new(self.seed)?;
        Ok(())
    }

    /// The physics world.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Physics steps run since the start.
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// The hash of the world's state.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        self.world.state_hash()
    }

    /// Adds `frame_ticks` to the accumulator and runs the whole steps it holds (at most
    /// [`MAX_STEPS`]); returns how many ran. The stream drops a body before the step it falls in.
    ///
    /// # Errors
    /// The physics world's error.
    pub fn advance(&mut self, frame_ticks: u64) -> Result<u32, Error> {
        let steps = self.clock.advance(frame_ticks);
        for _ in 0..steps.count {
            self.stream()?;
            self.world.step(DT)?;
            self.steps += 1;
        }
        Ok(steps.count)
    }

    /// Drops the next body of the stream when this step is a spawn step and the stream is not done.
    fn stream(&mut self) -> Result<(), Error> {
        if self.steps.is_multiple_of(SPAWN_EVERY) && self.streamed < STREAM_LIMIT {
            let at = Vec2::new(self.spawner.stream_x(), SPAWN_HEIGHT);
            self.streamed += u32::from(self.spawn_at(at)?);
        }
        Ok(())
    }

    /// Drops a body at `at` (metres), moved inside the arena; false when the world is full.
    ///
    /// # Errors
    /// The physics world's error.
    pub fn spawn_at(&mut self, at: Vec2) -> Result<bool, Error> {
        if self.world.len() >= self.world.capacity() {
            return Ok(false);
        }
        let inside = Vec2::new(
            at.x.clamp(-(HALF_WIDTH - MARGIN), HALF_WIDTH - MARGIN),
            at.y.clamp(FLOOR_TOP + MARGIN, CEILING),
        );
        let desc = self.spawner.body(inside);
        self.world.create_body(&desc)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use physics::d2::BodyHandle;

    fn sim() -> Simulation {
        Simulation::new(7).unwrap_or_else(|_| unreachable!())
    }

    fn run(sim: &mut Simulation, steps: u64) {
        for _ in 0..steps {
            assert_eq!(sim.advance(STEP_TICKS), Ok(1));
        }
    }

    fn dynamic(sim: &Simulation) -> Vec<(BodyHandle, Vec2)> {
        sim.world()
            .iter()
            .filter(|(_, b)| b.kind() == BodyKind::Dynamic)
            .map(|(h, b)| (h, b.position()))
            .collect()
    }

    #[test]
    fn the_arena_is_a_floor_and_two_walls() {
        let world = build_world().unwrap_or_else(|_| unreachable!());
        assert_eq!(world.len(), 3);
        assert!(world.iter().all(|(_, b)| b.kind() == BodyKind::Static));
    }

    #[test]
    fn a_frame_runs_the_whole_steps_it_holds_and_keeps_the_rest() {
        let mut sim = sim();
        assert_eq!(sim.advance(STEP_TICKS), Ok(1));
        assert_eq!(sim.advance(STEP_TICKS * 3 / 2), Ok(1));
        // The half step carried over completes with the next half.
        assert_eq!(sim.advance(STEP_TICKS / 2 + 1), Ok(1));
        assert_eq!(sim.advance(STEP_TICKS / 2), Ok(0));
        assert_eq!(sim.steps(), 3);
    }

    #[test]
    fn a_stall_runs_at_most_max_steps() {
        let mut sim = sim();
        assert_eq!(sim.advance(STEP_TICKS * 100), Ok(MAX_STEPS));
        assert_eq!(u64::from(MAX_STEPS), sim.steps());
    }

    #[test]
    fn the_stream_drops_a_body_every_spawn_period_and_stops_at_the_limit() {
        let mut sim = sim();
        run(&mut sim, 1);
        assert_eq!(dynamic(&sim).len(), 1);
        run(&mut sim, SPAWN_EVERY - 1);
        assert_eq!(dynamic(&sim).len(), 1);
        run(&mut sim, 1);
        assert_eq!(dynamic(&sim).len(), 2);
        run(&mut sim, SPAWN_EVERY * u64::from(STREAM_LIMIT));
        assert_eq!(dynamic(&sim).len(), STREAM_LIMIT as usize);
    }

    #[test]
    fn bodies_fall_and_stay_inside_the_arena() {
        let mut sim = sim();
        run(&mut sim, 30);
        let first = dynamic(&sim)[0].1;
        assert!(first.y < SPAWN_HEIGHT - 1.0, "{first:?}");
        run(&mut sim, 900);
        for (_, at) in dynamic(&sim) {
            assert!(at.x.abs() < HALF_WIDTH, "{at:?}");
            assert!(at.y > FLOOR_TOP && at.y < CEILING + 1.0, "{at:?}");
        }
    }

    #[test]
    fn a_click_places_a_body_inside_the_arena_whatever_the_position() {
        let mut sim = sim();
        assert_eq!(sim.spawn_at(Vec2::new(1000.0, 1000.0)), Ok(true));
        assert_eq!(sim.spawn_at(Vec2::new(-1000.0, -1000.0)), Ok(true));
        let placed = dynamic(&sim);
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].1, Vec2::new(HALF_WIDTH - MARGIN, CEILING));
        assert_eq!(
            placed[1].1,
            Vec2::new(-(HALF_WIDTH - MARGIN), FLOOR_TOP + MARGIN)
        );
    }

    #[test]
    fn a_full_world_refuses_a_click_without_an_error() {
        let mut sim = sim();
        let room = MAX_BODIES - 3;
        for _ in 0..room {
            assert_eq!(sim.spawn_at(Vec2::ZERO), Ok(true));
        }
        assert_eq!(sim.world().len(), MAX_BODIES as usize);
        assert_eq!(sim.spawn_at(Vec2::ZERO), Ok(false));
        // The stream meets the full world the same way.
        assert!(sim.advance(STEP_TICKS).is_ok());
    }

    #[test]
    fn the_same_seed_gives_the_same_run_and_another_seed_a_different_one() {
        let hash = |seed| {
            let mut sim = Simulation::new(seed).unwrap_or_else(|_| unreachable!());
            run(&mut sim, 200);
            sim.state_hash()
        };
        assert_eq!(hash(7), hash(7));
        assert_ne!(hash(7), hash(8));
    }

    #[test]
    fn a_reset_goes_back_to_the_start() {
        let fresh = sim().state_hash();
        let mut sim = sim();
        run(&mut sim, 50);
        assert_ne!(sim.state_hash(), fresh);
        assert_eq!(sim.reset(), Ok(()));
        assert_eq!(sim.state_hash(), fresh);
        assert_eq!(sim.steps(), 0);
        run(&mut sim, 1);
        assert_eq!(dynamic(&sim).len(), 1);
    }

    #[test]
    fn the_floor_and_walls_enclose_the_inside_of_the_arena() {
        let world = build_world().unwrap_or_else(|_| unreachable!());
        let mut shapes: Vec<(Vec2, Vec2)> = world
            .iter()
            .filter_map(|(_, b)| match b.shape() {
                Shape2::Box { half_extents } => Some((b.position(), half_extents)),
                _ => None,
            })
            .collect();
        assert_eq!(shapes.len(), 3);
        shapes.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
        let side = HALF_WIDTH + WALL_HALF;
        // Left wall, floor, right wall: their inner faces sit at the edge of the inside.
        let (left, floor, right) = (shapes[0], shapes[1], shapes[2]);
        assert!(
            (left.0.x + left.1.x - -HALF_WIDTH).abs() < 1.0e-4,
            "{left:?}"
        );
        assert!(
            (right.0.x - right.1.x - HALF_WIDTH).abs() < 1.0e-4,
            "{right:?}"
        );
        assert!(
            (floor.0.y + floor.1.y - FLOOR_TOP).abs() < 1.0e-4,
            "{floor:?}"
        );
        // The floor reaches the outer faces of both walls; the walls rise above the ceiling.
        assert!((floor.1.x - (side + WALL_HALF)).abs() < 1.0e-4, "{floor:?}");
        assert!(left.0.y + left.1.y > CEILING && right.0.y + right.1.y > CEILING);
        assert!((left.0.y - left.1.y - FLOOR_TOP).abs() < 1.0e-4, "{left:?}");
    }

    #[test]
    fn the_stream_drops_boxes_and_discs_of_the_sizes_it_promises() {
        let mut sim = sim();
        for _ in 0..STREAM_LIMIT {
            run(&mut sim, 1);
            // Freeze the picture right at the spawn: read the new body before it moves far.
            run(&mut sim, SPAWN_EVERY - 1);
        }
        let shapes: Vec<Shape2> = sim
            .world()
            .iter()
            .filter(|(_, b)| b.kind() == BodyKind::Dynamic)
            .map(|(_, b)| b.shape())
            .collect();
        let circles = shapes
            .iter()
            .filter(|s| is_disc_of_promised_size(s))
            .count();
        // Roughly half are discs.
        assert!((20..=60).contains(&circles), "{circles}");
    }

    /// True for a disc; panics when a body is outside the sizes the stream promises.
    fn is_disc_of_promised_size(shape: &Shape2) -> bool {
        match *shape {
            Shape2::Circle { radius } => {
                assert!(
                    (MIN_HALF..=MIN_HALF + EXTRA_RADIUS).contains(&radius),
                    "{radius}"
                );
                true
            }
            Shape2::Box { half_extents: h } => {
                assert!((MIN_HALF..=MIN_HALF + EXTRA_HALF).contains(&h.x), "{h:?}");
                assert!((MIN_HALF..=MIN_HALF + EXTRA_HALF).contains(&h.y), "{h:?}");
                false
            }
            _ => false,
        }
    }

    #[test]
    fn a_spawned_body_is_tilted_by_up_to_a_turn() {
        let mut sim = sim();
        for _ in 0..40 {
            assert_eq!(sim.spawn_at(Vec2::ZERO), Ok(true));
        }
        let angles: Vec<f32> = sim
            .world()
            .iter()
            .filter(|(_, b)| b.kind() == BodyKind::Dynamic)
            .map(|(_, b)| b.rotation().angle())
            .collect();
        assert!(angles.iter().any(|a| *a > 1.5) && angles.iter().any(|a| *a < -1.5));
    }

    #[test]
    fn the_stream_x_spreads_evenly_around_the_middle_within_the_spread() {
        const DRAWS: u16 = 400;
        let mut spawner = Spawner::new(3);
        let xs: Vec<f32> = (0..DRAWS).map(|_| spawner.stream_x()).collect();
        assert!(xs.iter().all(|x| x.abs() <= SPAWN_SPREAD), "{xs:?}");
        let (low, high) = xs
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
        assert!(
            low < -0.9 * SPAWN_SPREAD && high > 0.9 * SPAWN_SPREAD,
            "{low} {high}"
        );
        let mean = xs.iter().sum::<f32>() / f32::from(DRAWS);
        assert!(mean.abs() < 0.1 * SPAWN_SPREAD, "{mean}");
    }
}
