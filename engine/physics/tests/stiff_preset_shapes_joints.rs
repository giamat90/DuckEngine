//! Card 8-60: the stiff-contact preset (`SolverConfig::STIFF_CONTACTS`, card 8-54) on what card
//! 8-57 pinned only for boxes and 2D joints: heavy tops that are spheres, capsules, hulls and 2D
//! circles, a column of hulls, a stack with the speed clamp on, and 3D joints (hanging chains,
//! whips, a bridge, a door with limits).
//!
//! Stacks: a column of unit boxes (or hull prisms) on a floor with a heavy top of `ratio` times
//! the density, shifted sideways by one of five small offsets (every case runs five times: the
//! result at one offset is chaotic); after 600 steps of 1/60 s a drift of the farthest body from
//! the axis under 1 m is "settled". Joints: the largest distance between the two anchors of a
//! joint over 600 steps. Measured on 2026-11-21, Windows x86-64 (the file also passes on Ubuntu
//! x86-64 under WSL2). These are observations of the current solver, not a promise: the exact counts of chaotic scenes may differ on a machine with other floating-point hardware (the D2 feature is off), and a count that moves is a finding to report, not a bound to loosen.

use base::dmath::{atan2, sin_cos};
use base::math::{Vec2, Vec3};
use physics::core::PhysicsError;
use physics::{d2, d3};

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 600;
const OFFSETS: [f32; 5] = [0.0, 0.001, 0.01, 0.05, 0.2];
const SETTLED_BELOW: f32 = 1.0;
const MAX_BODIES: u32 = 64;
const MAX_CONTACTS: u32 = 2048;
const FLOOR_HALF: f32 = 200.0;
/// Height of the pin of a chain, a whip or a door: a chain of twenty links hangs clear of the floor.
const PIN_HEIGHT: f32 = 25.0;
const DEFAULT: d3::SolverConfig = d3::SolverConfig::DEFAULT;
const STIFF: d3::SolverConfig = d3::SolverConfig::STIFF_CONTACTS;

fn world3(solver: d3::SolverConfig, joints: u32) -> Result<d3::World, PhysicsError> {
    let mut world = d3::World::new(d3::WorldConfig {
        max_bodies: MAX_BODIES,
        gravity: d3::STANDARD_GRAVITY,
        max_pairs: MAX_CONTACTS,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: MAX_CONTACTS,
        max_joints: joints,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver,
        sleep: d3::SleepConfig::DISABLED,
    })?;
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(FLOOR_HALF, 0.5, FLOOR_HALF),
        },
    ))?;
    Ok(world)
}

/// A twelve-sided prism of height one and radius half, as a hull.
fn prism() -> Result<d3::Shape3, PhysicsError> {
    const SIDES: u8 = 12;
    let mut points = Vec::new();
    for side in 0..SIDES {
        let turn = std::f32::consts::TAU * f32::from(side) / f32::from(SIDES);
        let (sin, cos) = sin_cos(turn);
        for y in [-0.5, 0.5] {
            points.push(Vec3::new(0.5 * cos, y, 0.5 * sin));
        }
    }
    Ok(d3::Shape3::Hull(d3::Hull::new(&points)?))
}

fn unit_box() -> d3::Shape3 {
    d3::Shape3::Box {
        half_extents: Vec3::splat(0.5),
    }
}

/// The kind of heavy top, with its shape and the height of its centre above its base.
#[derive(Clone, Copy)]
enum Top {
    Sphere,
    UprightCapsule,
    Hull,
}

impl Top {
    fn shape(self) -> Result<(d3::Shape3, f32), PhysicsError> {
        Ok(match self {
            Self::Sphere => (d3::Shape3::Sphere { radius: 0.5 }, 0.5),
            Self::UprightCapsule => (
                d3::Shape3::Capsule {
                    half_height: 0.25,
                    radius: 0.5,
                },
                0.75,
            ),
            Self::Hull => (prism()?, 0.5),
        })
    }
}

/// The larger of two drifts; `f32::max` ignores NaN, so a value that is not finite counts as the
/// worst drift.
fn worst(drift: f32, value: f32) -> f32 {
    if value.is_finite() {
        drift.max(value)
    } else {
        f32::INFINITY
    }
}

/// The farthest sideways distance of any dynamic body of the 3D world from the axis.
fn drift3(world: &d3::World) -> f32 {
    let mut drift = 0.0_f32;
    for (_, body) in world
        .iter()
        .filter(|(_, b)| b.kind() == d3::BodyKind::Dynamic)
    {
        let at = body.position();
        let sideways = (at.x * at.x + at.z * at.z).sqrt();
        drift = worst(drift, sideways);
    }
    drift
}

fn run_steps3(world: &mut d3::World) -> Result<f32, PhysicsError> {
    let mut drift = 0.0_f32;
    for _ in 0..STEPS {
        world.step(DT)?;
        drift = drift.max(drift3(world));
    }
    Ok(drift)
}

/// `boxes` unit boxes with a heavy top of `top` on them, the top shifted by `offset`.
fn box_column_with_top(
    solver: d3::SolverConfig,
    top: Top,
    boxes: u8,
    ratio: f32,
    offset: f32,
) -> Result<f32, PhysicsError> {
    let mut world = world3(solver, 0)?;
    for level in 0..boxes {
        world.create_body(&d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            Vec3::new(0.0, 0.5 + f32::from(level), 0.0),
            unit_box(),
        ))?;
    }
    let (shape, lift) = top.shape()?;
    world.create_body(&d3::BodyDesc {
        density: ratio,
        ..d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            Vec3::new(offset, f32::from(boxes) + lift, 0.0),
            shape,
        )
    })?;
    run_steps3(&mut world)
}

/// A column of `boxes` hull prisms, the top one `ratio` times as dense and shifted by `offset`.
fn hull_column(
    solver: d3::SolverConfig,
    boxes: u8,
    ratio: f32,
    offset: f32,
) -> Result<f32, PhysicsError> {
    let mut world = world3(solver, 0)?;
    for level in 0..boxes {
        let top = level + 1 == boxes;
        world.create_body(&d3::BodyDesc {
            density: if top { ratio } else { 1.0 },
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(if top { offset } else { 0.0 }, 0.5 + f32::from(level), 0.0),
                prism()?,
            )
        })?;
    }
    run_steps3(&mut world)
}

/// Eight unit boxes with a top of `ratio`, with `SpeedPolicy::Clamp` at 500 m/s and 50 rad/s.
fn clamped_column(
    solver: d3::SolverConfig,
    boxes: u8,
    ratio: f32,
    offset: f32,
) -> Result<f32, PhysicsError> {
    let mut world = world3(solver, 0)?;
    world.set_speed_policy(d3::SpeedPolicy::Clamp {
        linear: 500.0,
        angular: 50.0,
    })?;
    for level in 0..boxes {
        let top = level + 1 == boxes;
        world.create_body(&d3::BodyDesc {
            density: if top { ratio } else { 1.0 },
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(if top { offset } else { 0.0 }, 0.5 + f32::from(level), 0.0),
                unit_box(),
            )
        })?;
    }
    run_steps3(&mut world)
}

/// `boxes` 2D boxes with a circle of `ratio` times the density on top, shifted by `offset`.
fn circle_top(
    solver: d3::SolverConfig,
    boxes: u8,
    ratio: f32,
    offset: f32,
) -> Result<f32, PhysicsError> {
    let mut world = d2::World::new(d2::WorldConfig {
        max_bodies: MAX_BODIES,
        gravity: d2::STANDARD_GRAVITY,
        max_pairs: MAX_CONTACTS,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: MAX_CONTACTS,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver,
        sleep: d2::SleepConfig::DISABLED,
    })?;
    world.create_body(&d2::BodyDesc::new(
        d2::BodyKind::Static,
        Vec2::new(0.0, -0.5),
        d2::Shape2::Box {
            half_extents: Vec2::new(FLOOR_HALF, 0.5),
        },
    ))?;
    for level in 0..boxes {
        world.create_body(&d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::new(0.0, 0.5 + f32::from(level)),
            d2::Shape2::Box {
                half_extents: Vec2::new(0.5, 0.5),
            },
        ))?;
    }
    world.create_body(&d2::BodyDesc {
        density: ratio,
        ..d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::new(offset, f32::from(boxes) + 0.5),
            d2::Shape2::Circle { radius: 0.5 },
        )
    })?;
    let mut drift = 0.0_f32;
    for _ in 0..STEPS {
        world.step(DT)?;
        for (_, body) in world
            .iter()
            .filter(|(_, b)| b.kind() == d2::BodyKind::Dynamic)
        {
            let x = body.position().x.abs();
            drift = worst(drift, x);
        }
    }
    Ok(drift)
}

/// How many of the five offsets of `run` settle.
fn settled(run: impl Fn(f32) -> Result<f32, PhysicsError>) -> Result<usize, PhysicsError> {
    let mut count = 0;
    for offset in OFFSETS {
        count += usize::from(run(offset)? < SETTLED_BELOW);
    }
    Ok(count)
}

#[test]
fn a_heavy_sphere_on_boxes_settles_more_often_with_the_preset() -> Result<(), PhysicsError> {
    // Default 3 of 5 on three boxes at ratio 10 and 0 of 5 on eight boxes at ratio 50; the preset
    // 5 of 5 and 3 of 5.
    let light = |solver| settled(|o| box_column_with_top(solver, Top::Sphere, 3, 10.0, o));
    assert_eq!((light(DEFAULT)?, light(STIFF)?), (3, 5));
    let tall = |solver| settled(|o| box_column_with_top(solver, Top::Sphere, 8, 50.0, o));
    assert_eq!(tall(DEFAULT)?, 0);
    assert!(tall(STIFF)? >= 2, "{}", tall(STIFF)?);
    Ok(())
}

#[test]
fn a_heavy_hull_on_eight_boxes_settles_only_with_the_preset() -> Result<(), PhysicsError> {
    let tall = |solver| settled(|o| box_column_with_top(solver, Top::Hull, 8, 50.0, o));
    assert_eq!((tall(DEFAULT)?, tall(STIFF)?), (0, 5));
    Ok(())
}

#[test]
fn an_upright_capsule_on_boxes_rolls_off_with_either_solver() -> Result<(), PhysicsError> {
    // The preset is for the weight of the top, not for a top that has no flat base.
    for solver in [DEFAULT, STIFF] {
        let run = |o| box_column_with_top(solver, Top::UprightCapsule, 3, 10.0, o);
        assert_eq!(settled(run)?, 0);
    }
    Ok(())
}

#[test]
fn a_column_of_hull_prisms_settles_to_a_top_of_five_and_the_preset_carries_a_top_of_fifty()
-> Result<(), PhysicsError> {
    for solver in [DEFAULT, STIFF] {
        assert_eq!(settled(|o| hull_column(solver, 8, 5.0, o))?, 5);
    }
    assert_eq!(settled(|o| hull_column(DEFAULT, 8, 50.0, o))?, 0);
    let preset = settled(|o| hull_column(STIFF, 8, 50.0, o))?;
    assert!(preset >= 3, "{preset}");
    Ok(())
}

#[test]
fn a_heavy_circle_on_boxes_in_2d_settles_on_three_and_five_boxes_with_the_preset()
-> Result<(), PhysicsError> {
    let three = |solver| settled(|o| circle_top(solver, 3, 10.0, o));
    assert_eq!((three(DEFAULT)? < 5, three(STIFF)?), (true, 5));
    let five = |solver| settled(|o| circle_top(solver, 5, 10.0, o));
    assert_eq!(five(DEFAULT)?, 0);
    assert!(five(STIFF)? >= 3, "{}", five(STIFF)?);
    Ok(())
}

#[test]
fn the_preset_still_carries_a_heavy_top_with_the_speed_clamp_on() -> Result<(), PhysicsError> {
    assert_eq!(settled(|o| clamped_column(DEFAULT, 8, 50.0, o))?, 0);
    assert_eq!(settled(|o| clamped_column(STIFF, 8, 50.0, o))?, 5);
    Ok(())
}

// ----- 3D joints -----

/// What a joint scene came to: the largest distance between the two anchors of any joint, the end
/// (or middle) body's position, and whether it is finite.
struct Held {
    gap: f32,
    at: Vec3,
}

type Link = (d3::BodyHandle, Vec3, d3::BodyHandle, Vec3);

/// The distance between the world positions of the two anchors of one joint.
fn anchor_gap(world: &d3::World, (a, anchor_a, b, anchor_b): Link) -> Result<f32, PhysicsError> {
    let (first, second) = (world.body(a)?, world.body(b)?);
    let at_a = first.position() + first.rotation().rotate(anchor_a);
    let at_b = second.position() + second.rotation().rotate(anchor_b);
    Ok((at_a - at_b).length())
}

fn run_joints(
    world: &mut d3::World,
    links: &[Link],
    watched: d3::BodyHandle,
) -> Result<Held, PhysicsError> {
    let mut gap = 0.0_f32;
    for _ in 0..STEPS {
        world.step(DT)?;
        for &link in links {
            gap = gap.max(anchor_gap(world, link)?);
        }
    }
    Ok(Held {
        gap,
        at: world.body(watched)?.position(),
    })
}

fn pin(world: &mut d3::World, height: f32) -> Result<d3::BodyHandle, PhysicsError> {
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, height, 0.0),
        d3::Shape3::Sphere { radius: 0.1 },
    ))
}

/// A chain of `count` links from the pin: hanging (links of 1 m, end link `heavy` times as dense)
/// or, when `thrown`, laid out sideways and spinning (the whip of the joint tests).
fn chain(
    solver: d3::SolverConfig,
    count: u8,
    heavy: f32,
    thrown: bool,
    hinged: bool,
) -> Result<Held, PhysicsError> {
    let mut world = world3(solver, 64)?;
    let mut previous = pin(&mut world, PIN_HEIGHT)?;
    let mut links = Vec::new();
    let reach = if thrown {
        Vec3::new(0.5, 0.0, 0.0)
    } else {
        Vec3::new(0.0, -0.5, 0.0)
    };
    for link in 0..count {
        let along = f32::from(link);
        let at = if thrown {
            Vec3::new(0.5 + along, PIN_HEIGHT, 0.0)
        } else {
            Vec3::new(0.0, PIN_HEIGHT - 0.5 - along, 0.0)
        };
        let half = if thrown {
            Vec3::new(0.5, 0.1, 0.1)
        } else {
            Vec3::new(0.1, 0.5, 0.1)
        };
        let mut desc = d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            at,
            d3::Shape3::Box { half_extents: half },
        );
        if link + 1 == count {
            desc.density = heavy;
        }
        if thrown {
            desc.angular_velocity = Vec3::new(5.0, 3.0, 8.0);
        }
        let next = world.create_body(&desc)?;
        let anchor_a = if link == 0 { Vec3::ZERO } else { reach };
        let joint = if hinged {
            d3::Joint3::Hinge(d3::HingeJoint3::new(anchor_a, -reach, Vec3::Z, Vec3::Z))
        } else {
            d3::Joint3::Ball(d3::BallJoint3::new(anchor_a, -reach))
        };
        world.create_joint(&d3::JointDesc::new(previous, next, joint))?;
        links.push((previous, anchor_a, next, -reach));
        previous = next;
    }
    run_joints(&mut world, &links, previous)
}

/// A bridge of `planks` planks of 1 m between two posts, hinged at the ends, with a box of
/// `load` times the density dropped on the middle (none for zero).
fn bridge(solver: d3::SolverConfig, planks: u8, load: f32) -> Result<Held, PhysicsError> {
    let mut world = world3(solver, 64)?;
    let post = |x| {
        d3::BodyDesc::new(
            d3::BodyKind::Static,
            Vec3::new(x, 5.0, 0.0),
            d3::Shape3::Box {
                half_extents: Vec3::splat(0.5),
            },
        )
    };
    let left = world.create_body(&post(-0.5))?;
    let right = world.create_body(&post(f32::from(planks) + 0.5))?;
    let hinge = |a, b| d3::Joint3::Hinge(d3::HingeJoint3::new(a, b, Vec3::Z, Vec3::Z));
    let (mut previous, mut anchor_a) = (left, Vec3::new(0.5, 0.4, 0.0));
    let (mut links, mut middle) = (Vec::new(), left);
    for index in 0..planks {
        let plank = world.create_body(&d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            Vec3::new(0.5 + f32::from(index), 5.4, 0.0),
            d3::Shape3::Box {
                half_extents: Vec3::new(0.5, 0.1, 0.5),
            },
        ))?;
        let anchor_b = Vec3::new(-0.5, 0.0, 0.0);
        world.create_joint(&d3::JointDesc::new(
            previous,
            plank,
            hinge(anchor_a, anchor_b),
        ))?;
        links.push((previous, anchor_a, plank, anchor_b));
        (previous, anchor_a) = (plank, Vec3::new(0.5, 0.0, 0.0));
        if index == planks / 2 {
            middle = plank;
        }
    }
    let anchor_b = Vec3::new(-0.5, 0.4, 0.0);
    world.create_joint(&d3::JointDesc::new(
        previous,
        right,
        hinge(anchor_a, anchor_b),
    ))?;
    links.push((previous, anchor_a, right, anchor_b));
    if load > 0.0 {
        world.create_body(&d3::BodyDesc {
            density: load,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(0.5 + f32::from(planks / 2), 6.0, 0.0),
                unit_box(),
            )
        })?;
    }
    run_joints(&mut world, &links, middle)
}

fn all_finite(at: Vec3) -> bool {
    at.x.is_finite() && at.y.is_finite() && at.z.is_finite()
}

#[test]
fn a_hanging_chain_of_twenty_links_with_a_heavy_end_holds_with_the_preset_as_with_the_default()
-> Result<(), PhysicsError> {
    // The end of 20 links hangs about 5 m above the floor.
    for solver in [DEFAULT, STIFF] {
        let held = chain(solver, 20, 100.0, false, false)?;
        assert!(held.gap < 0.05, "gap {}", held.gap);
        assert!(held.at.y < 5.5 && held.at.y > 4.5, "{:?}", held.at);
    }
    Ok(())
}

#[test]
fn a_whip_of_eight_links_stays_together_and_finite_with_the_preset() -> Result<(), PhysicsError> {
    // Measured, ball and hinge, end link 1 and 100 times as dense: gaps 0.010 to 0.092 with the
    // preset (0.018 to 0.086 with the default).
    for hinged in [false, true] {
        for heavy in [1.0, 100.0] {
            let held = chain(STIFF, 8, heavy, true, hinged)?;
            assert!(all_finite(held.at), "{:?}", held.at);
            assert!(
                held.gap < 0.15,
                "hinged {hinged}, end {heavy}: gap {}",
                held.gap
            );
        }
    }
    Ok(())
}

#[test]
fn a_bridge_of_planks_carries_a_heavy_box_with_the_preset_and_keeps_its_joints_close()
-> Result<(), PhysicsError> {
    // Measured, gap of the worst joint with a load of 100 times the density: six planks 0.58
    // (default) and 0.39 (preset); ten planks 0.20 (default) and 0.43 (preset: twice the
    // default, a finding of the card). The bounds are those of the preset.
    for (planks, bound) in [(6, 0.6), (10, 0.8)] {
        let held = bridge(STIFF, planks, 100.0)?;
        assert!(all_finite(held.at), "{:?}", held.at);
        assert!(held.gap < bound, "{planks} planks: gap {}", held.gap);
        assert!(held.at.y > 4.5, "the middle plank fell: {:?}", held.at);
    }
    let bare = bridge(STIFF, 6, 0.0)?;
    assert!(bare.gap < 0.02, "an empty bridge: gap {}", bare.gap);
    Ok(())
}

#[test]
fn a_hinge_door_with_limits_stops_at_its_limit_with_the_preset() -> Result<(), PhysicsError> {
    let mut world = world3(STIFF, 4)?;
    let post = pin(&mut world, PIN_HEIGHT)?;
    let door = world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Dynamic,
        Vec3::new(1.0, PIN_HEIGHT, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(1.0, 0.1, 0.5),
        },
    ))?;
    let mut hinge = d3::HingeJoint3::new(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0), Vec3::Z, Vec3::Z);
    hinge.limits = Some(d3::AngleLimits {
        lower: -1.0,
        upper: 0.5,
    });
    world.create_joint(&d3::JointDesc::new(post, door, d3::Joint3::Hinge(hinge)))?;
    let mut lowest = 0.0_f32;
    for _ in 0..STEPS {
        world.step(DT)?;
        let direction = world.body(door)?.rotation().rotate(Vec3::X);
        lowest = lowest.min(atan2(direction.y, direction.x));
    }
    assert!((lowest + 1.0).abs() < 0.02, "the door swung to {lowest}");
    Ok(())
}
