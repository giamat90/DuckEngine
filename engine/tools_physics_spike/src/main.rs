//! Throw-away harness of the ADR-0018 spike (card 4-2): the same stacking scene in a locked-axis
//! 3D world (A), a dimension-generic core at D=2 (B) and a native 2D world (C), plus the D=3
//! instance of B. Prints a markdown table: bytes per body, wall time per step on this machine,
//! and how far the stack settled. Report only; not part of the engine.
//!
//! `tools_physics_spike [--steps N] [--reps R] [--rows N] [--column N]`. Build with `--release`.

#[cfg(feature = "variant-b")]
mod dims;
#[cfg(feature = "variant-b3")]
mod dims3;
#[cfg(any(feature = "variant-b", feature = "variant-b3"))]
mod generic;
#[cfg(any(feature = "variant-a", feature = "variant-b3"))]
mod math3;
mod narrow2d;
mod scene;

#[cfg(feature = "variant-a")]
mod locked3d;
#[cfg(feature = "variant-c")]
mod native2d;

use scene::{BoxDef, SceneKind, Sim, build};
use std::time::Instant;

const DEFAULT_STEPS: usize = 300;
const DEFAULT_REPS: usize = 15;
const DEFAULT_ROWS: u16 = 10;
const DEFAULT_COLUMN: u16 = 10;
const NANOS_PER_MICRO: f64 = 1000.0;

/// How settled the scene is after the warm-up.
struct Settle {
    max_penetration: f32,
    max_speed: f32,
    max_spin: f32,
    max_drift: f32,
    contacts: usize,
}

/// A count as `f64` (exact below 2^32).
fn as_f64(count: usize) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples.get(samples.len() / 2).copied().unwrap_or(f64::NAN)
}

/// Runs `steps` steps and reports the state at the end. Penetration is the worst of the last
/// step's contacts.
fn settle<S: Sim>(defs: &[BoxDef], steps: usize) -> Settle {
    let mut world = S::from_scene(defs);
    for _ in 0..steps {
        world.step();
    }
    let mut out = Settle {
        max_penetration: world.max_penetration(),
        max_speed: 0.0,
        max_spin: 0.0,
        max_drift: 0.0,
        contacts: world.contact_count(),
    };
    for (i, def) in defs.iter().enumerate().skip(1) {
        let [x, y] = world.pos_xy(i);
        out.max_drift = out.max_drift.max((x - def.x).abs().max((y - def.y).abs()));
        out.max_speed = out.max_speed.max(world.linear_speed(i));
        out.max_spin = out.max_spin.max(world.angular_speed(i));
    }
    out
}

/// Median, minimum and maximum over `reps` repetitions of the mean step time in microseconds,
/// measured after `steps` warm-up steps.
fn time_steps<S: Sim>(defs: &[BoxDef], steps: usize, reps: usize) -> [f64; 3] {
    let mut samples = Vec::with_capacity(reps);
    for _ in 0..reps {
        let mut world = S::from_scene(defs);
        for _ in 0..steps {
            world.step();
        }
        let start = Instant::now();
        for _ in 0..steps {
            world.step();
        }
        let nanos = start.elapsed().as_secs_f64() * 1e9;
        samples.push(nanos / as_f64(steps) / NANOS_PER_MICRO);
    }
    let low = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let high = samples.iter().copied().fold(0.0, f64::max);
    [median(&mut samples), low, high]
}

fn final_hash<S: Sim>(defs: &[BoxDef], steps: usize) -> u64 {
    let mut world = S::from_scene(defs);
    for _ in 0..steps {
        world.step();
    }
    world.state_hash()
}

fn deterministic<S: Sim>(defs: &[BoxDef], steps: usize) -> bool {
    final_hash::<S>(defs, steps) == final_hash::<S>(defs, steps)
}

fn report<S: Sim>(name: &str, defs: &[BoxDef], steps: usize, reps: usize) {
    let s = settle::<S>(defs, steps);
    let [med, low, high] = time_steps::<S>(defs, steps, reps);
    let same = deterministic::<S>(defs, steps);
    println!(
        "| {name} | {} | {} | {med:.1} ({low:.1} to {high:.1}) | {} | {:.4} | {:.4} | {:.4} | {:.4} | {same} |",
        S::body_bytes(),
        S::contact_bytes(),
        s.contacts,
        s.max_penetration,
        s.max_speed,
        s.max_spin,
        s.max_drift,
    );
}

fn run_scene(title: &str, kind: SceneKind, steps: usize, reps: usize) {
    let defs = build(kind);
    println!(
        "\n### {title}: {} dynamic bodies, {steps} warm-up steps, median of {reps} reps\n",
        defs.len() - 1
    );
    println!(
        "| variant | bytes/body | bytes/contact | us/step median (min to max) | contacts | max penetration m | max speed m/s | max spin rad/s | max drift m | same hash twice |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    #[cfg(feature = "variant-a")]
    report::<locked3d::World>("A locked-axis 3D", &defs, steps, reps);
    #[cfg(feature = "variant-b")]
    report::<generic::World<dims::D2>>("B generic, D=2", &defs, steps, reps);
    #[cfg(feature = "variant-b3")]
    report::<generic::World<dims3::D3>>("B generic, D=3 (locked scene)", &defs, steps, reps);
    #[cfg(feature = "variant-c")]
    report::<native2d::World>("C native 2D", &defs, steps, reps);
}

fn arg(args: &[String], name: &str, default: usize) -> usize {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let steps = arg(&args, "--steps", DEFAULT_STEPS);
    let reps = arg(&args, "--reps", DEFAULT_REPS);
    let rows =
        u16::try_from(arg(&args, "--rows", usize::from(DEFAULT_ROWS))).unwrap_or(DEFAULT_ROWS);
    let column = u16::try_from(arg(&args, "--column", usize::from(DEFAULT_COLUMN)))
        .unwrap_or(DEFAULT_COLUMN);
    println!("wall time on the dev machine, release build, f32, fixed step 1/60 s");
    run_scene("pyramid", SceneKind::Pyramid(rows), steps, reps);
    run_scene("column", SceneKind::Column(column), steps, reps);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTLE_STEPS: usize = 300;
    const MAX_PENETRATION: f32 = 0.05;
    const MAX_SPEED: f32 = 0.05;
    const MAX_DRIFT: f32 = 0.1;

    fn stable<S: Sim>(kind: SceneKind) -> bool {
        let defs = build(kind);
        let s = settle::<S>(&defs, SETTLE_STEPS);
        s.max_penetration < MAX_PENETRATION
            && s.max_speed < MAX_SPEED
            && s.max_drift < MAX_DRIFT
            && s.contacts > 0
    }

    #[test]
    fn column_and_pyramid_settle_in_every_variant() {
        for kind in [SceneKind::Column(10), SceneKind::Pyramid(8)] {
            #[cfg(feature = "variant-a")]
            assert!(stable::<locked3d::World>(kind), "A {kind:?}");
            #[cfg(feature = "variant-b")]
            assert!(stable::<generic::World<dims::D2>>(kind), "B2 {kind:?}");
            #[cfg(feature = "variant-b3")]
            assert!(stable::<generic::World<dims3::D3>>(kind), "B3 {kind:?}");
            #[cfg(feature = "variant-c")]
            assert!(stable::<native2d::World>(kind), "C {kind:?}");
        }
    }

    #[test]
    fn same_scene_twice_gives_the_same_hash() {
        let defs = build(SceneKind::Pyramid(6));
        #[cfg(feature = "variant-a")]
        assert!(deterministic::<locked3d::World>(&defs, 120));
        #[cfg(feature = "variant-b")]
        assert!(deterministic::<generic::World<dims::D2>>(&defs, 120));
        #[cfg(feature = "variant-b3")]
        assert!(deterministic::<generic::World<dims3::D3>>(&defs, 120));
        #[cfg(feature = "variant-c")]
        assert!(deterministic::<native2d::World>(&defs, 120));
    }

    #[test]
    fn generic_d2_matches_native_2d_bit_for_bit() {
        #[cfg(all(feature = "variant-b", feature = "variant-c"))]
        {
            let defs = build(SceneKind::Pyramid(6));
            assert_eq!(
                final_hash::<generic::World<dims::D2>>(&defs, 120),
                final_hash::<native2d::World>(&defs, 120)
            );
        }
    }

    #[test]
    fn generic_d3_matches_locked_3d_bit_for_bit() {
        #[cfg(all(feature = "variant-a", feature = "variant-b3"))]
        {
            let defs = build(SceneKind::Pyramid(6));
            assert_eq!(
                final_hash::<locked3d::World>(&defs, 120),
                final_hash::<generic::World<dims3::D3>>(&defs, 120)
            );
        }
    }

    #[cfg(feature = "variant-b")]
    #[test]
    fn dropping_a_box_does_not_pass_through_the_ground() {
        let mut defs = build(SceneKind::Column(1));
        defs[1].y = 3.0;
        let mut world = generic::World::<dims::D2>::from_scene(&defs);
        for _ in 0..SETTLE_STEPS {
            world.step();
        }
        let [_, y] = world.pos_xy(1);
        assert!((y - 0.5).abs() < 0.02 && world.linear_speed(1) < MAX_SPEED);
    }
}
