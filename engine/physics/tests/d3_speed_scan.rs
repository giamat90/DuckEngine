//! Cards 8-37 and 8-41: how fast do 3D scenes get, and does the bound see the absurd ones?
//!
//! `MAX_SPEED` and `MAX_SPIN` were chosen at 1e5 (card 8-25) and measured by card 8-37, which found
//! chains with an absurd mass ratio ending at thousands of radians per second with every step `Ok`;
//! card 8-41 adopted the bounds proposed in `wiki/spikes/speed-bounds-scan.md`. These tests run the
//! scenes of that scan and check, with wide margins so that a machine with other rounding still
//! passes, that the scenes a game makes stay far below the bounds and that the absurd ones are
//! now refused.

use base::math::Vec3;
use physics::core::PhysicsError;
use physics::d3::{
    BallJoint3, BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN,
    DEFAULT_SPECULATIVE_MARGIN, DistanceJoint3, Joint3, JointDesc, MAX_GIVEN_SPIN, MAX_SPEED,
    MAX_SPIN, STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
};

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 600;

fn world() -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 64,
        gravity: STANDARD_GRAVITY,
        max_pairs: 1024,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 1024,
        max_joints: 16,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DISABLED,
    })
}

fn ground(w: &mut World) -> Result<BodyHandle, PhysicsError> {
    w.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        Shape3::Box {
            half_extents: Vec3::new(200.0, 0.5, 200.0),
        },
    ))
}

fn cube(w: &mut World, at: Vec3, half: f32, density: f32) -> Result<BodyHandle, PhysicsError> {
    w.create_body(&BodyDesc {
        density,
        ..BodyDesc::new(
            BodyKind::Dynamic,
            at,
            Shape3::Box {
                half_extents: Vec3::new(half, half, half),
            },
        )
    })
}

fn norm(v: Vec3) -> f64 {
    let (x, y, z) = (f64::from(v.x), f64::from(v.y), f64::from(v.z));
    (x * x + y * y + z * z).sqrt()
}

/// Peak of one run: the largest linear and angular speed of any dynamic body after any step, and
/// whether and when a step was refused.
fn finite_or_infinite(x: f64) -> f64 {
    if x.is_finite() { x } else { f64::INFINITY }
}

struct Peak {
    linear: f64,
    angular: f64,
    refused: Option<(u32, PhysicsError)>,
}

fn run(mut w: World) -> Peak {
    let mut p = Peak {
        linear: 0.0,
        angular: 0.0,
        refused: None,
    };
    for step in 0..STEPS {
        if let Err(e) = w.step(DT) {
            p.refused = Some((step, e));
            break;
        }
        for (_, b) in w.iter().filter(|(_, b)| b.kind() == BodyKind::Dynamic) {
            // A number that is not a number counts as infinitely fast (`max` would skip it).
            p.linear = p.linear.max(finite_or_infinite(norm(b.linear_velocity())));
            p.angular = p
                .angular
                .max(finite_or_infinite(norm(b.angular_velocity())));
        }
    }
    p
}

fn drop_scene(height: f32, spin: f32) -> Result<World, PhysicsError> {
    let mut w = world()?;
    ground(&mut w)?;
    w.create_body(&BodyDesc {
        angular_velocity: Vec3::new(spin, 0.0, 0.0),
        ..BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(0.0, height, 0.0),
            Shape3::Box {
                half_extents: Vec3::new(0.5, 0.5, 0.5),
            },
        )
    })?;
    Ok(w)
}

/// Eight boxes one on the other, the top one `ratio` times as dense as the others.
fn stack_scene(ratio: f32) -> Result<World, PhysicsError> {
    let mut w = world()?;
    ground(&mut w)?;
    for i in 0..8_u8 {
        let top = i == 7;
        let y = 0.5 + f32::from(i) + if top { 0.3 } else { 0.0 };
        cube(
            &mut w,
            Vec3::new(0.0, y, 0.0),
            0.5,
            if top { ratio } else { 1.0 },
        )?;
    }
    Ok(w)
}

/// A small sphere shot at five boxes from 8 m away.
fn bullet_scene(speed: f32) -> Result<World, PhysicsError> {
    let mut w = world()?;
    ground(&mut w)?;
    for i in 0..5_u8 {
        cube(&mut w, Vec3::new(0.0, 0.5 + f32::from(i), 0.0), 0.5, 1.0)?;
    }
    w.create_body(&BodyDesc {
        linear_velocity: Vec3::new(-speed, 2.0, 0.0),
        ..BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(8.0, 2.0, 0.1),
            Shape3::Sphere { radius: 0.1 },
        )
    })?;
    Ok(w)
}

/// A chain of cubes hanging out sideways from a pin 10 m up, joined by balls or rods, the last one
/// `end_density` times as dense as the others.
fn chain_scene(links: u8, end_density: f32, rod: bool) -> Result<World, PhysicsError> {
    let mut w = world()?;
    let pin = w.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, 10.0, 0.0),
        Shape3::Sphere { radius: 0.1 },
    ))?;
    let mut prev = pin;
    for i in 0..links {
        let density = if i + 1 == links { end_density } else { 1.0 };
        let x = 0.8 * (f32::from(i) + 1.0);
        let link = cube(&mut w, Vec3::new(x, 10.0, 0.0), 0.15, density)?;
        let from = if i == 0 {
            Vec3::ZERO
        } else {
            Vec3::new(0.4, 0.0, 0.0)
        };
        let to = Vec3::new(-0.4, 0.0, 0.0);
        let joint = if rod {
            Joint3::Distance(DistanceJoint3::new(from, to, 0.8))
        } else {
            Joint3::Ball(BallJoint3::new(from, to))
        };
        w.create_joint(&JointDesc::new(prev, link, joint))?;
        prev = link;
    }
    Ok(w)
}

/// Every (rod or ball, link count, end density) of the chain scenes for two end densities.
fn chain_cases(ratios: [f32; 2]) -> Vec<(bool, u8, f32)> {
    [false, true]
        .into_iter()
        .flat_map(|rod| [3_u8, 6].into_iter().map(move |links| (rod, links)))
        .flat_map(|(rod, links)| ratios.into_iter().map(move |r| (rod, links, r)))
        .collect()
}

fn assert_healthy(name: &str, scene: Result<World, PhysicsError>) -> Result<(), PhysicsError> {
    let p = run(scene?);
    assert!(p.refused.is_none(), "{name}: {:?}", p.refused);
    assert!(
        f64::from(MAX_SPEED) / 2.0 > p.linear,
        "{name}: {} m/s",
        p.linear
    );
    assert!(
        f64::from(MAX_SPIN) / 2.0 > p.angular,
        "{name}: {} rad/s",
        p.angular
    );
    Ok(())
}

#[test]
fn scenes_a_game_makes_stay_far_below_the_proposed_bounds() -> Result<(), PhysicsError> {
    for h in [1.0_f32, 10.0, 100.0, 1000.0] {
        assert_healthy(&format!("drop from {h} m"), drop_scene(h, 0.0))?;
    }
    for spin in [10.0_f32, 100.0, 500.0] {
        assert_healthy(&format!("spin {spin} rad/s"), drop_scene(3.0, spin))?;
    }
    for ratio in [1.0_f32, 10.0, 100.0] {
        assert_healthy(&format!("stack x{ratio}"), stack_scene(ratio))?;
    }
    for speed in [50.0_f32, 300.0, 1000.0] {
        assert_healthy(&format!("bullet {speed} m/s"), bullet_scene(speed))?;
    }
    for (rod, links, ratio) in chain_cases([1.0, 100.0]) {
        let name = format!("chain rod={rod} links={links} x{ratio}");
        assert_healthy(&name, chain_scene(links, ratio, rod))?;
    }
    Ok(())
}

/// A heavy end 10 000 or a million times as dense as the links is far beyond the mass ratio the
/// joints document. Before card 8-41 the step did not notice (every step was `Ok` with spins of
/// thousands of radians per second, tens of turns per step); with the bound at 2000 rad/s it
/// refuses the scene as `Diverged`, within the first seconds.
#[test]
fn a_chain_with_an_absurd_mass_ratio_is_refused() -> Result<(), PhysicsError> {
    for (rod, links, ratio) in chain_cases([1.0e4, 1.0e6]) {
        let p = run(chain_scene(links, ratio, rod)?);
        let name = format!("chain rod={rod} links={links} x{ratio}");
        assert!(
            matches!(p.refused, Some((_, PhysicsError::Diverged))),
            "{name}: {:?}",
            p.refused
        );
    }
    Ok(())
}

/// A body made to spin at the largest allowed speed and dropped no longer throws itself far: the
/// spin is a fiftieth of what it was, and the speed it ends with is below the speed bound.
#[test]
fn a_body_spun_at_the_largest_given_spin_ends_below_the_speed_bound() -> Result<(), PhysicsError> {
    let p = run(drop_scene(3.0, MAX_GIVEN_SPIN)?);
    assert!(p.refused.is_none(), "{:?}", p.refused);
    assert!(f64::from(MAX_SPEED) / 2.0 > p.linear, "{} m/s", p.linear);
    Ok(())
}
