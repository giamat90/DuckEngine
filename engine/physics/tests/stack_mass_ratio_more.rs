//! Card 8-55: the stack scenes that `stack_mass_ratio.rs` (card 8-39) did not try: the heavy box in
//! the middle or at the bottom, stacks above eight, sleeping on, and a heavy top dropped from a
//! height.
//!
//! The scene is a column of unit boxes on a floor; one box (the heavy one) is `ratio` times as
//! dense as the others and is shifted sideways by one of five small offsets (the result at one
//! offset is chaotic, so every case is run five times). After 600 steps of 1/60 s the farthest
//! sideways distance of any body from the axis says what became of it: settled (under 1 m),
//! toppled (1 to 10 m) or flung (over 10 m). Measured on 2026-11-20, Windows x86-64, in 2D and 3D,
//! with the default solver and with `SolverConfig::STIFF_CONTACTS` (card 8-54).
//!
//! These tests pin what was seen at a few points with margin (all five offsets or none, except the
//! dropped top, which pins a comparison); they are observations of the current solver, not a promise.

use base::math::{Vec2, Vec3};
use physics::core::{PhysicsError, SolverConfig};
use physics::{d2, d3};

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 600;
const OFFSETS: [f32; 5] = [0.0, 0.001, 0.01, 0.05, 0.2];
const SETTLED_BELOW: f32 = 1.0;
/// Bodies, pairs and manifolds the world has room for, and the half width of the floor.
const MAX_BODIES: u32 = 64;
const MAX_CONTACTS: u32 = 2048;
const FLOOR_HALF: f32 = 200.0;

#[derive(Clone, Copy)]
struct Scene {
    boxes: u8,
    /// Level (from 0 at the bottom) of the heavy box.
    heavy: u8,
    ratio: f32,
    offset: f32,
    solver: SolverConfig,
    sleeping: bool,
    /// Extra height of the heavy box above its resting place (only meaningful for the top box).
    drop: f32,
}

impl Scene {
    fn new(boxes: u8, heavy: u8, ratio: f32, solver: SolverConfig) -> Self {
        assert!(heavy < boxes, "the heavy box is one of the boxes");
        Self {
            boxes,
            heavy,
            ratio,
            offset: 0.0,
            solver,
            sleeping: false,
            drop: 0.0,
        }
    }

    fn density(self, level: u8) -> f32 {
        if level == self.heavy { self.ratio } else { 1.0 }
    }

    /// Where the centre of the box at `level` starts: (sideways, height).
    fn start(self, level: u8) -> (f32, f32) {
        let height = 0.5 + f32::from(level);
        if level == self.heavy {
            (self.offset, height + self.drop)
        } else {
            (0.0, height)
        }
    }
}

/// What a run came to: the farthest drift and how many dynamic bodies sleep at the end.
struct Seen {
    drift: f32,
    asleep: usize,
    dynamic: usize,
}

fn run2(scene: Scene) -> Result<Seen, PhysicsError> {
    let sleep = if scene.sleeping {
        d2::SleepConfig::DEFAULT
    } else {
        d2::SleepConfig::DISABLED
    };
    let mut world = d2::World::new(d2::WorldConfig {
        max_bodies: MAX_BODIES,
        gravity: d2::STANDARD_GRAVITY,
        max_pairs: MAX_CONTACTS,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: MAX_CONTACTS,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: scene.solver,
        sleep,
    })?;
    world.create_body(&d2::BodyDesc::new(
        d2::BodyKind::Static,
        Vec2::new(0.0, -0.5),
        d2::Shape2::Box {
            half_extents: Vec2::new(FLOOR_HALF, 0.5),
        },
    ))?;
    for level in 0..scene.boxes {
        let (x, y) = scene.start(level);
        world.create_body(&d2::BodyDesc {
            density: scene.density(level),
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(x, y),
                d2::Shape2::Box {
                    half_extents: Vec2::new(0.5, 0.5),
                },
            )
        })?;
    }
    let mut drift = 0.0_f32;
    for _ in 0..STEPS {
        world.step(DT)?;
        for (_, body) in world
            .iter()
            .filter(|(_, b)| b.kind() == d2::BodyKind::Dynamic)
        {
            drift = drift.max(body.position().x.abs());
        }
    }
    let asleep: Vec<bool> = world
        .iter()
        .filter(|(_, b)| b.kind() == d2::BodyKind::Dynamic)
        .map(|(_, b)| b.is_asleep())
        .collect();
    Ok(Seen {
        drift,
        asleep: asleep.iter().filter(|&&a| a).count(),
        dynamic: asleep.len(),
    })
}

fn run3(scene: Scene) -> Result<Seen, PhysicsError> {
    let sleep = if scene.sleeping {
        d3::SleepConfig::DEFAULT
    } else {
        d3::SleepConfig::DISABLED
    };
    let mut world = d3::World::new(d3::WorldConfig {
        max_bodies: MAX_BODIES,
        gravity: d3::STANDARD_GRAVITY,
        max_pairs: MAX_CONTACTS,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: MAX_CONTACTS,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: scene.solver,
        sleep,
    })?;
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(FLOOR_HALF, 0.5, FLOOR_HALF),
        },
    ))?;
    for level in 0..scene.boxes {
        let (x, y) = scene.start(level);
        world.create_body(&d3::BodyDesc {
            density: scene.density(level),
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(x, y, 0.0),
                d3::Shape3::Box {
                    half_extents: Vec3::new(0.5, 0.5, 0.5),
                },
            )
        })?;
    }
    let mut drift = 0.0_f32;
    for _ in 0..STEPS {
        world.step(DT)?;
        for (_, body) in world
            .iter()
            .filter(|(_, b)| b.kind() == d3::BodyKind::Dynamic)
        {
            let at = body.position();
            drift = drift.max((at.x * at.x + at.z * at.z).sqrt());
        }
    }
    let asleep: Vec<bool> = world
        .iter()
        .filter(|(_, b)| b.kind() == d3::BodyKind::Dynamic)
        .map(|(_, b)| b.is_asleep())
        .collect();
    Ok(Seen {
        drift,
        asleep: asleep.iter().filter(|&&a| a).count(),
        dynamic: asleep.len(),
    })
}

/// The five offsets of one scene, in the dimension `three`.
fn five(three: bool, scene: Scene) -> Result<Vec<Seen>, PhysicsError> {
    OFFSETS
        .iter()
        .map(|&offset| {
            let scene = Scene { offset, ..scene };
            if three { run3(scene) } else { run2(scene) }
        })
        .collect()
}

fn settled(seen: &[Seen]) -> usize {
    seen.iter().filter(|s| s.drift < SETTLED_BELOW).count()
}

/// How many of the five runs of `scene` settled, in 2D and in 3D.
fn settled_in_both(scene: Scene) -> Result<[usize; 2], PhysicsError> {
    Ok([settled(&five(false, scene)?), settled(&five(true, scene)?)])
}

const DEFAULT: SolverConfig = SolverConfig::DEFAULT;
const STIFF: SolverConfig = SolverConfig::STIFF_CONTACTS;

#[test]
fn a_heavy_box_at_the_bottom_of_a_stack_of_eight_settles_with_either_solver()
-> Result<(), PhysicsError> {
    for solver in [DEFAULT, STIFF] {
        let scene = Scene::new(8, 0, 100.0, solver);
        assert_eq!(settled_in_both(scene)?, [5, 5]);
    }
    Ok(())
}

#[test]
fn a_heavy_box_in_the_middle_of_eight_never_settles_at_the_default_and_always_with_the_preset()
-> Result<(), PhysicsError> {
    assert_eq!(settled_in_both(Scene::new(8, 4, 100.0, DEFAULT))?, [0, 0]);
    assert_eq!(settled_in_both(Scene::new(8, 4, 100.0, STIFF))?, [5, 5]);
    Ok(())
}

#[test]
fn the_preset_keeps_sixteen_boxes_with_a_top_of_five_standing_and_the_default_does_not()
-> Result<(), PhysicsError> {
    assert_eq!(settled_in_both(Scene::new(16, 15, 5.0, DEFAULT))?, [0, 0]);
    assert_eq!(settled_in_both(Scene::new(16, 15, 5.0, STIFF))?, [5, 5]);
    Ok(())
}

/// Every run settled and every dynamic body sleeps at its end.
fn assert_all_asleep(runs: &[Seen], boxes: u8) {
    for seen in runs {
        assert_eq!(seen.dynamic, usize::from(boxes), "every box was made");
        assert!(seen.drift < SETTLED_BELOW, "drift {}", seen.drift);
        assert_eq!(seen.asleep, seen.dynamic, "all asleep");
    }
}

#[test]
fn with_sleeping_on_a_stack_that_settles_falls_asleep_to_the_last_box() -> Result<(), PhysicsError>
{
    for solver in [DEFAULT, STIFF] {
        let scene = Scene {
            sleeping: true,
            ..Scene::new(8, 7, 5.0, solver)
        };
        for three in [false, true] {
            assert_all_asleep(&five(three, scene)?, scene.boxes);
        }
    }
    Ok(())
}

#[test]
fn a_heavy_top_dropped_from_two_metres_on_three_boxes_settles_less_often_with_the_preset_in_3d()
-> Result<(), PhysicsError> {
    // The finding of the card: the preset that keeps a resting stack standing is not better for a
    // heavy box that lands on it; in 3D it settled none of five where the default settled four. The
    // preset's runs drift 1.3 to 3.8 m (slid or toppled, not flung), the one default run that fails
    // about 4 m, against the 1 m line: a thin contrast, so the claim is relative, not a count.
    let dropped = |solver| Scene {
        drop: 2.0,
        ..Scene::new(3, 2, 10.0, solver)
    };
    let default = settled(&five(true, dropped(DEFAULT))?);
    let stiff = settled(&five(true, dropped(STIFF))?);
    assert!(default >= 3, "default {default}");
    assert!(stiff < default, "preset {stiff} against default {default}");
    Ok(())
}
