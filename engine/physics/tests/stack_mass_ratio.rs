//! Card 8-39: how tall a stack of light boxes can carry a heavy top before it comes apart.
//!
//! The scene is a column of `boxes` unit boxes on a floor, the top one `ratio` times as dense as
//! the others, resting on the one below, shifted sideways by one of five small offsets (the result
//! at one offset is chaotic, so every case is run five times). After 600 steps (10 s) the farthest
//! sideways distance of any body from the axis of the column says what became of it:
//! settled (under 1 m), toppled (1 to 10 m; a column of eight boxes that falls over ends 5 to 7 m away) or
//! flung (over 10 m). Measured on 2026-11-20, Windows x86-64, sleeping off, in 2D and 3D.
//!
//! These tests pin what was seen at a few points; they are observations of the current solver, not
//! a promise. The region that settles is small: the ratio a stack carries falls with its height.

use base::math::{Vec2, Vec3};
use physics::core::{MAX_RELAX_ITERATIONS, MAX_SUB_STEPS, PhysicsError, SolverConfig};
use physics::{d2, d3};

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 600;
/// Steps of 30 Hz that make the same 10 s.
const SLOW_DT: f32 = 1.0 / 30.0;
const SLOW_STEPS: u32 = 300;
const OFFSETS: [f32; 5] = [0.0, 0.001, 0.01, 0.05, 0.2];
const SETTLED_BELOW: f32 = 1.0;
const FLUNG_ABOVE: f32 = 10.0;
/// The fastest body of any run in the scan was 98 m/s.
const SPEED_SEEN_BELOW: f32 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    Settled,
    Toppled,
    Flung,
}

#[derive(Clone, Copy, Debug)]
struct Seen {
    outcome: Outcome,
    peak_speed: f32,
}

#[derive(Clone, Copy)]
struct Stack {
    boxes: u8,
    ratio: f32,
    offset: f32,
    solver: SolverConfig,
    /// The length of a step and the number of steps run (10 s).
    dt: f32,
    steps: u32,
}

impl Stack {
    fn new(boxes: u8, ratio: f32, offset: f32) -> Self {
        Self {
            boxes,
            ratio,
            offset,
            solver: SolverConfig::DEFAULT,
            dt: DT,
            steps: STEPS,
        }
    }

    fn density(self, level: u8) -> f32 {
        if level + 1 == self.boxes {
            self.ratio
        } else {
            1.0
        }
    }

    fn x(self, level: u8) -> f32 {
        if level + 1 == self.boxes {
            self.offset
        } else {
            0.0
        }
    }
}

fn outcome(drift: f32) -> Outcome {
    if drift < SETTLED_BELOW {
        Outcome::Settled
    } else if drift <= FLUNG_ABOVE {
        Outcome::Toppled
    } else {
        Outcome::Flung
    }
}

fn run2(stack: Stack) -> Result<Seen, PhysicsError> {
    let mut world = d2::World::new(d2::WorldConfig {
        max_bodies: 64,
        gravity: d2::STANDARD_GRAVITY,
        max_pairs: 1024,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 1024,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: stack.solver,
        sleep: d2::SleepConfig::DISABLED,
    })?;
    world.create_body(&d2::BodyDesc::new(
        d2::BodyKind::Static,
        Vec2::new(0.0, -0.5),
        d2::Shape2::Box {
            half_extents: Vec2::new(200.0, 0.5),
        },
    ))?;
    for level in 0..stack.boxes {
        world.create_body(&d2::BodyDesc {
            density: stack.density(level),
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(stack.x(level), 0.5 + f32::from(level)),
                d2::Shape2::Box {
                    half_extents: Vec2::new(0.5, 0.5),
                },
            )
        })?;
    }
    let (mut drift, mut peak_speed) = (0.0_f32, 0.0_f32);
    for _ in 0..stack.steps {
        world.step(stack.dt)?;
        for (_, body) in world
            .iter()
            .filter(|(_, b)| b.kind() == d2::BodyKind::Dynamic)
        {
            drift = drift.max(body.position().x.abs());
            peak_speed = peak_speed.max(body.linear_velocity().length());
        }
    }
    Ok(Seen {
        outcome: outcome(drift),
        peak_speed,
    })
}

fn run3(stack: Stack) -> Result<Seen, PhysicsError> {
    let mut world = d3::World::new(d3::WorldConfig {
        max_bodies: 64,
        gravity: d3::STANDARD_GRAVITY,
        max_pairs: 1024,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 1024,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: stack.solver,
        sleep: d3::SleepConfig::DISABLED,
    })?;
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(200.0, 0.5, 200.0),
        },
    ))?;
    for level in 0..stack.boxes {
        world.create_body(&d3::BodyDesc {
            density: stack.density(level),
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(stack.x(level), 0.5 + f32::from(level), 0.0),
                d3::Shape3::Box {
                    half_extents: Vec3::new(0.5, 0.5, 0.5),
                },
            )
        })?;
    }
    let (mut drift, mut peak_speed) = (0.0_f32, 0.0_f32);
    for _ in 0..stack.steps {
        world.step(stack.dt)?;
        for (_, body) in world
            .iter()
            .filter(|(_, b)| b.kind() == d3::BodyKind::Dynamic)
        {
            let at = body.position();
            drift = drift.max((at.x * at.x + at.z * at.z).sqrt());
            peak_speed = peak_speed.max(body.linear_velocity().length());
        }
    }
    Ok(Seen {
        outcome: outcome(drift),
        peak_speed,
    })
}

/// The five offsets of one case, in the dimension `three`.
fn seen(three: bool, stack: Stack) -> Result<Vec<Seen>, PhysicsError> {
    OFFSETS
        .iter()
        .map(|&offset| {
            let stack = Stack { offset, ..stack };
            if three { run3(stack) } else { run2(stack) }
        })
        .collect()
}

fn count(seen: &[Seen], wanted: Outcome) -> usize {
    seen.iter().filter(|s| s.outcome == wanted).count()
}

/// Cases that settled at every offset, in 2D and in 3D (a finer scan of 41 offsets from 0 to 0.3 m
/// gave 41 of 41 for each): (boxes, ratio). The cells one ratio higher settle in most offsets only.
const SETTLES: [(u8, f32); 8] = [
    (2, 100.0),
    (3, 10.0),
    (4, 10.0),
    (5, 10.0),
    (6, 5.0),
    (8, 5.0),
    (2, 5.0),
    (8, 1.0),
];

fn assert_settles(three: bool) -> Result<(), PhysicsError> {
    for (boxes, ratio) in SETTLES {
        let all = seen(three, Stack::new(boxes, ratio, 0.0))?;
        assert_eq!(
            count(&all, Outcome::Settled),
            5,
            "{boxes} boxes, ratio {ratio}: {all:?}"
        );
    }
    Ok(())
}

#[test]
fn a_short_stack_or_a_small_ratio_settles_in_2d() -> Result<(), PhysicsError> {
    assert_settles(false)
}

#[test]
fn a_short_stack_or_a_small_ratio_settles_in_3d() -> Result<(), PhysicsError> {
    assert_settles(true)
}

/// Cases that never settled at any of the five offsets, in 2D and in 3D: (boxes, ratio).
const NEVER_SETTLES: [(u8, f32); 4] = [(6, 50.0), (6, 100.0), (8, 50.0), (8, 100.0)];

fn assert_never_settles(three: bool) -> Result<(), PhysicsError> {
    for (boxes, ratio) in NEVER_SETTLES {
        let all = seen(three, Stack::new(boxes, ratio, 0.0))?;
        assert_eq!(
            count(&all, Outcome::Settled),
            0,
            "{boxes} boxes, ratio {ratio}: {all:?}"
        );
    }
    Ok(())
}

#[test]
fn a_tall_stack_with_a_top_fifty_times_as_dense_comes_apart_in_2d() -> Result<(), PhysicsError> {
    assert_never_settles(false)
}

#[test]
fn a_tall_stack_with_a_top_fifty_times_as_dense_comes_apart_in_3d() -> Result<(), PhysicsError> {
    assert_never_settles(true)
}

#[test]
fn a_stack_of_eight_with_a_top_a_thousand_times_as_dense_flings_a_body_in_2d_and_3d()
-> Result<(), PhysicsError> {
    for three in [false, true] {
        let all = seen(three, Stack::new(8, 1000.0, 0.0))?;
        assert!(count(&all, Outcome::Flung) >= 1, "three {three}: {all:?}");
        // No speed bound sees it: the fastest body stays far below the bound, and a clamp set at
        // 500 m/s (card 8-50) would not fire.
        assert!(
            all.iter().all(|s| s.peak_speed < SPEED_SEEN_BELOW),
            "{all:?}"
        );
    }
    Ok(())
}

#[test]
fn the_most_sub_steps_and_relax_passes_alone_do_not_make_a_stack_of_eight_with_a_top_fifty_times_as_dense_settle()
-> Result<(), PhysicsError> {
    // Sub-steps and relax passes at their largest, with the default stiffness: the stiffness is the
    // lever, not the counts (card 8-54).
    let solver = SolverConfig {
        sub_steps: MAX_SUB_STEPS,
        relax_iterations: MAX_RELAX_ITERATIONS,
        ..SolverConfig::DEFAULT
    };
    for three in [false, true] {
        let all = seen(
            three,
            Stack {
                solver,
                ..Stack::new(8, 50.0, 0.0)
            },
        )?;
        assert_eq!(count(&all, Outcome::Settled), 0, "three {three}: {all:?}");
    }
    Ok(())
}

#[test]
fn a_box_under_a_heavy_top_gives_the_same_result_a_second_time() -> Result<(), PhysicsError> {
    let first = run3(Stack::new(8, 100.0, 0.05))?;
    let second = run3(Stack::new(8, 100.0, 0.05))?;
    assert_eq!(first.outcome, second.outcome);
    assert_eq!(first.peak_speed.to_bits(), second.peak_speed.to_bits());
    Ok(())
}

/// Cases the stiff-contact preset (card 8-54) settles at every one of 41 offsets (0 to 0.3 m), in
/// 2D and in 3D, and that the default solver almost never does (0 to 3 of 41): (boxes, ratio).
const STIFF_SETTLES: [(u8, f32); 4] = [(8, 20.0), (8, 30.0), (5, 50.0), (5, 70.0)];

/// The cases run in 3D, which is much slower in the dev profile: the two that matter most.
const STIFF_SETTLES_3D: [(u8, f32); 2] = [(8, 30.0), (5, 70.0)];

fn assert_preset_settles(three: bool, cases: &[(u8, f32)]) -> Result<(), PhysicsError> {
    for &(boxes, ratio) in cases {
        let stack = Stack {
            solver: SolverConfig::STIFF_CONTACTS,
            ..Stack::new(boxes, ratio, 0.0)
        };
        let all = seen(three, stack)?;
        assert_eq!(
            count(&all, Outcome::Settled),
            5,
            "{boxes} boxes, ratio {ratio}: {all:?}"
        );
    }
    Ok(())
}

#[test]
fn the_stiff_contact_preset_keeps_a_tall_stack_with_a_heavy_top_standing_in_2d()
-> Result<(), PhysicsError> {
    assert_preset_settles(false, &STIFF_SETTLES)
}

#[test]
fn the_stiff_contact_preset_keeps_a_tall_stack_with_a_heavy_top_standing_in_3d()
-> Result<(), PhysicsError> {
    assert_preset_settles(true, &STIFF_SETTLES_3D)
}

#[test]
fn the_default_solver_settles_at_most_one_run_in_five_of_those_cases_so_the_preset_is_what_changed()
-> Result<(), PhysicsError> {
    for three in [false, true] {
        for (boxes, ratio) in STIFF_SETTLES_3D {
            let all = seen(three, Stack::new(boxes, ratio, 0.0))?;
            assert!(
                count(&all, Outcome::Settled) <= 1,
                "{boxes} boxes, ratio {ratio}, three {three}: {all:?}"
            );
        }
    }
    Ok(())
}

/// Cases run at steps of 30 Hz, where the stiffness of the preset is capped at 60 Hz instead of
/// 120 (card 8-57): (boxes, ratio). The preset settled all five offsets of each, in 2D and in 3D.
const SLOW_STEP_CASES: [(u8, f32); 4] = [(8, 20.0), (5, 50.0), (5, 70.0), (3, 100.0)];

/// The cases run in 3D, the slower dimension.
const SLOW_STEP_CASES_3D: [(u8, f32); 2] = [(8, 20.0), (5, 70.0)];

/// The two cases where the default solver settled at most one run in five at steps of 30 Hz.
const SLOW_STEP_CONTROLS: [(u8, f32); 2] = [(8, 20.0), (5, 70.0)];

fn slow(solver: SolverConfig, boxes: u8, ratio: f32) -> Stack {
    Stack {
        solver,
        dt: SLOW_DT,
        steps: SLOW_STEPS,
        ..Stack::new(boxes, ratio, 0.0)
    }
}

#[test]
fn the_stiff_contact_preset_still_settles_tall_stacks_at_steps_of_30_hz_in_2d_and_3d()
-> Result<(), PhysicsError> {
    for three in [false, true] {
        let cases: &[(u8, f32)] = if three {
            &SLOW_STEP_CASES_3D
        } else {
            &SLOW_STEP_CASES
        };
        for &(boxes, ratio) in cases {
            let all = seen(three, slow(SolverConfig::STIFF_CONTACTS, boxes, ratio))?;
            assert_eq!(
                count(&all, Outcome::Settled),
                5,
                "{boxes} boxes, ratio {ratio}, three {three}: {all:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn the_default_solver_settles_at_most_one_run_in_five_of_two_of_those_cases_at_steps_of_30_hz()
-> Result<(), PhysicsError> {
    for three in [false, true] {
        for (boxes, ratio) in SLOW_STEP_CONTROLS {
            let all = seen(three, slow(SolverConfig::DEFAULT, boxes, ratio))?;
            assert!(
                count(&all, Outcome::Settled) <= 1,
                "{boxes} boxes, ratio {ratio}, three {three}: {all:?}"
            );
        }
    }
    Ok(())
}
