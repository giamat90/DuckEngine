//! The 3D golden recorder (card 8-29): runs five scenes of the `D=3` instance on the scalar serial
//! reference and prints the state hash after chosen steps as machine-readable `GOLDEN|...` lines, in
//! the format of the 2D recorder of card 4-20. Nothing is stored here and the target compares
//! nothing: the lines are what a golden file of `gates/` would hold, compared on Windows and Linux
//! (D2) by `tools/golden-check` once the governance card 8-13 registers them. Run it with
//! `cargo test -p physics --features cross-platform-determinism --test golden3d -- --nocapture`.

use base::math::{Quat, Vec3};
use base::rng::Pcg32;
use physics::d3::{
    BallJoint3, BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    DistanceJoint3, HingeJoint3, Hull, Joint3, JointDesc, PhysicsError, STANDARD_GRAVITY, Shape3,
    SleepConfig, SolverConfig, World, WorldConfig,
};

/// The format of the lines; the tool refuses another number.
const FORMAT: u32 = 1;

/// Checkpoint stride of every scene, in steps.
const STRIDE: u32 = 10;

/// The fixed step: 60 Hz.
const DT: f32 = 1.0 / 60.0;

/// Room for the bodies, joints, pairs and manifolds of every scene.
const MAX_BODIES: u32 = 128;
const MAX_JOINTS: u32 = 32;
const CAPACITY: u32 = 4096;

/// Seed and stream of the generator of the pile.
const SEED: u64 = 7;
const STREAM: u64 = 21;

/// A scene: its name, its steps and the builder of its world.
struct Scene {
    name: &'static str,
    steps: u32,
    build: fn() -> Result<World, PhysicsError>,
}

fn scenes() -> [Scene; 5] {
    [
        Scene {
            name: "pyramid 3d",
            steps: 120,
            build: pyramid,
        },
        Scene {
            name: "sleeping tower 3d",
            steps: 300,
            build: tower,
        },
        Scene {
            name: "heavy on light 3d",
            steps: 120,
            build: heavy_on_light,
        },
        Scene {
            name: "mixed pile 3d",
            steps: 200,
            build: mixed_pile,
        },
        Scene {
            name: "joint chain 3d",
            steps: 180,
            build: joint_chain,
        },
    ]
}

fn world(sleep: SleepConfig) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: MAX_BODIES,
        gravity: STANDARD_GRAVITY,
        max_pairs: CAPACITY,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: CAPACITY,
        max_joints: MAX_JOINTS,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    })
}

fn cuboid(x: f32, y: f32, z: f32) -> Shape3 {
    Shape3::Box {
        half_extents: Vec3::new(x, y, z),
    }
}

/// A wide static floor whose top is at height 0.
fn floor(world: &mut World) -> Result<(), PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        cuboid(30.0, 1.0, 30.0),
    ))?;
    Ok(())
}

fn crate_at(world: &mut World, at: Vec3) -> Result<(), PhysicsError> {
    world.create_body(&BodyDesc::new(BodyKind::Dynamic, at, cuboid(0.5, 0.5, 0.5)))?;
    Ok(())
}

/// One row of a pyramid of unit crates, two crates deep, `count` crates wide.
fn pyramid_row(world: &mut World, row: u8, count: u8) -> Result<(), PhysicsError> {
    for i in 0..count {
        let dx = f32::from(i) - f32::from(count - 1) / 2.0;
        for depth in [-0.5, 0.5] {
            crate_at(world, Vec3::new(dx, 0.5 + f32::from(row), depth))?;
        }
    }
    Ok(())
}

/// Rows of unit crates, two deep, each row one crate shorter than the one under it.
fn pyramid() -> Result<World, PhysicsError> {
    const ROWS: u8 = 5;
    let mut w = world(SleepConfig::DISABLED)?;
    floor(&mut w)?;
    for row in 0..ROWS {
        pyramid_row(&mut w, row, ROWS - row)?;
    }
    Ok(w)
}

/// Eight crates one on the other with sleeping on, so the scene ends with a sleeping island.
fn tower() -> Result<World, PhysicsError> {
    const CRATES: u8 = 8;
    let mut w = world(SleepConfig::DEFAULT)?;
    floor(&mut w)?;
    for n in 0..CRATES {
        let at = Vec3::new(0.0, 0.5 + f32::from(n), 0.0);
        crate_at(&mut w, at)?;
    }
    Ok(w)
}

/// A crate twenty times as dense on four light ones.
fn heavy_on_light() -> Result<World, PhysicsError> {
    const HEAVY_DENSITY: f32 = 20.0;
    let mut w = world(SleepConfig::DISABLED)?;
    floor(&mut w)?;
    for n in 0..4_u8 {
        let at = Vec3::new(0.0, 0.5 + f32::from(n), 0.0);
        crate_at(&mut w, at)?;
    }
    let mut heavy = BodyDesc::new(
        BodyKind::Dynamic,
        Vec3::new(0.0, 4.5, 0.0),
        cuboid(0.5, 0.5, 0.5),
    );
    heavy.density = HEAVY_DENSITY;
    w.create_body(&heavy)?;
    Ok(w)
}

/// The eight corners of a cube of half extent `half`, for a convex hull.
fn cube_hull(half: f32) -> Result<Shape3, PhysicsError> {
    let sign = |bits: u8, bit: u8| if bits & bit == 0 { -half } else { half };
    let corners: Vec<Vec3> = (0..8_u8)
        .map(|bits| Vec3::new(sign(bits, 1), sign(bits, 2), sign(bits, 4)))
        .collect();
    Ok(Shape3::Hull(Hull::new(&corners)?))
}

/// The shape of body `index` of the pile: sphere, box, capsule or hull in turn.
fn pile_shape(index: u8) -> Result<Shape3, PhysicsError> {
    const SIZE: f32 = 0.35;
    const CAPSULE_SHARE: f32 = 0.6;
    match index % 4 {
        0 => Ok(Shape3::Sphere { radius: SIZE }),
        1 => Ok(cuboid(SIZE, SIZE, SIZE)),
        2 => Ok(Shape3::Capsule {
            half_height: SIZE,
            radius: SIZE * CAPSULE_SHARE,
        }),
        _ => cube_hull(SIZE),
    }
}

/// Thirty-two bodies of four shapes dropped from a grid into a walled box, with a seeded tilt.
fn mixed_pile() -> Result<World, PhysicsError> {
    const BODIES: u8 = 32;
    const GRID: u8 = 4;
    const SPACING: f32 = 1.1;
    const DROP_BASE: f32 = 1.0;
    const DROP_LAYER: f32 = 1.2;
    const WALL: f32 = 2.5;
    const WALL_HEIGHT: f32 = 2.0;
    let mut w = world(SleepConfig::DISABLED)?;
    floor(&mut w)?;
    for side in [-1.0_f32, 1.0] {
        let along_x = Vec3::new(side * WALL, WALL_HEIGHT, 0.0);
        w.create_body(&BodyDesc::new(
            BodyKind::Static,
            along_x,
            cuboid(0.5, WALL_HEIGHT, WALL),
        ))?;
        let along_z = Vec3::new(0.0, WALL_HEIGHT, side * WALL);
        w.create_body(&BodyDesc::new(
            BodyKind::Static,
            along_z,
            cuboid(WALL, WALL_HEIGHT, 0.5),
        ))?;
    }
    let mut rng = Pcg32::new(SEED, STREAM);
    let per_layer = GRID * GRID;
    let grid = |n: u8| (f32::from(n) - f32::from(GRID - 1) / 2.0) * SPACING;
    for index in 0..BODIES {
        let cell = index % per_layer;
        let at = Vec3::new(
            grid(cell % GRID),
            DROP_BASE + f32::from(index / per_layer) * DROP_LAYER,
            grid(cell / GRID),
        );
        let mut desc = BodyDesc::new(BodyKind::Dynamic, at, pile_shape(index)?);
        desc.rotation = Quat::from_axis_angle(Vec3::new(1.0, 1.0, 0.0), rng.next_f32())
            .ok_or(PhysicsError::NotFinite)?;
        w.create_body(&desc)?;
    }
    Ok(w)
}

/// A fixed anchor, then spheres joined by a ball joint, a hinge and a rod, released sideways.
fn joint_chain() -> Result<World, PhysicsError> {
    const HEIGHT: f32 = 8.0;
    const LINK: f32 = 0.8;
    const RADIUS: f32 = 0.25;
    let mut w = world(SleepConfig::DISABLED)?;
    let anchor = w.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, HEIGHT, 0.0),
        cuboid(0.2, 0.2, 0.2),
    ))?;
    let reach = Vec3::new(LINK / 2.0, 0.0, 0.0);
    let back = Vec3::new(-LINK / 2.0, 0.0, 0.0);
    let mut last = anchor;
    for n in 1..=6_u8 {
        let at = Vec3::new(f32::from(n) * LINK, HEIGHT, 0.0);
        let next = w.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            at,
            Shape3::Sphere { radius: RADIUS },
        ))?;
        let joint = match n {
            3 => Joint3::Hinge(HingeJoint3::new(reach, back, Vec3::Z, Vec3::Z)),
            5 => Joint3::Distance(DistanceJoint3::new(reach, back, LINK)),
            _ => Joint3::Ball(BallJoint3::new(reach, back)),
        };
        w.create_joint(&JointDesc::new(last, next, joint))?;
        last = next;
    }
    Ok(w)
}

/// Whether the hash after `step` (counted from 1) is a checkpoint: the first step, every
/// `stride`-th step and the last one.
fn is_checkpoint(step: u32, stride: u32, steps: u32) -> bool {
    step == 1 || step.is_multiple_of(stride) || step == steps
}

/// The checkpoint lines of one scene, stepped on the scalar serial reference.
fn lines(scene: &Scene) -> Result<Vec<String>, PhysicsError> {
    let mut world = (scene.build)()?;
    let mut out = vec![format!(
        "GOLDEN|scene|{}|{}|{STRIDE}",
        scene.name, scene.steps
    )];
    for step in 1..=scene.steps {
        world.step(DT)?;
        if is_checkpoint(step, STRIDE, scene.steps) {
            out.push(format!(
                "GOLDEN|hash|{}|{step}|{:016x}",
                scene.name,
                world.state_hash()
            ));
        }
    }
    Ok(out)
}

/// The operating system of the build, from `cfg`.
fn platform_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

/// The CPU family of the build.
fn platform_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "other"
    }
}

/// The header: the format, the feature, the platform and whether the build fuses operations.
fn header() -> Vec<String> {
    vec![
        format!("GOLDEN|format|{FORMAT}"),
        format!(
            "GOLDEN|feature|cross-platform-determinism|{}",
            if cfg!(feature = "cross-platform-determinism") {
                "on"
            } else {
                "off"
            }
        ),
        format!("GOLDEN|platform|{}|{}", platform_os(), platform_arch()),
        format!("GOLDEN|fma|{}", cfg!(target_feature = "fma")),
    ]
}

/// Every golden line of a run, ending with the count of the lines before it.
fn golden_lines() -> Result<Vec<String>, PhysicsError> {
    let mut out = header();
    for scene in &scenes() {
        out.extend(lines(scene)?);
    }
    out.push(format!("GOLDEN|end|{}", out.len()));
    Ok(out)
}

/// The test the CI job would run with `--nocapture`: prints the lines for the tool.
#[test]
fn print_the_golden_lines() -> Result<(), PhysicsError> {
    // The test harness prints "test <name> ... " without a newline: start the lines on a new one.
    println!();
    for line in golden_lines()? {
        println!("{line}");
    }
    Ok(())
}

/// The same lines twice in one process: a recorder that printed something unstable would never
/// match a stored file.
#[test]
fn the_golden_lines_are_the_same_twice() -> Result<(), PhysicsError> {
    assert_eq!(golden_lines()?, golden_lines()?);
    Ok(())
}

#[test]
fn the_golden_lines_cover_every_scene_with_the_first_and_the_last_step() -> Result<(), PhysicsError>
{
    let all = golden_lines()?;
    let count = |kind: &str| {
        let prefix = format!("GOLDEN|{kind}|");
        all.iter().filter(|line| line.starts_with(&prefix)).count()
    };
    assert_eq!(count("scene"), scenes().len());
    assert_eq!(count("end"), 1);
    assert_eq!(all.last(), Some(&format!("GOLDEN|end|{}", all.len() - 1)));
    for scene in scenes() {
        let first = format!("GOLDEN|hash|{}|1|", scene.name);
        let last = format!("GOLDEN|hash|{}|{}|", scene.name, scene.steps);
        for prefix in [first, last] {
            assert!(all.iter().any(|line| line.starts_with(&prefix)), "{prefix}");
        }
    }
    Ok(())
}

/// A scene that did nothing would hash the same at every step; every scene must move.
#[test]
fn every_scene_changes_its_hash_over_time() -> Result<(), PhysicsError> {
    for scene in scenes() {
        let hashes: Vec<String> = lines(&scene)?
            .into_iter()
            .filter(|line| line.starts_with("GOLDEN|hash|"))
            .collect();
        let first = hashes.first().and_then(|line| line.rsplit('|').next());
        let last = hashes.last().and_then(|line| line.rsplit('|').next());
        assert_ne!(first, last, "{}", scene.name);
    }
    Ok(())
}

#[test]
fn golden_checkpoints_are_the_first_step_every_stride_and_the_last() {
    let picked: Vec<u32> = (1..=12).filter(|&s| is_checkpoint(s, 5, 12)).collect();
    assert_eq!(picked, [1, 5, 10, 12]);
    let picked: Vec<u32> = (1..=10).filter(|&s| is_checkpoint(s, 5, 10)).collect();
    assert_eq!(picked, [1, 5, 10]);
}
