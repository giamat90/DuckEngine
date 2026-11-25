//! Card 8-48: the collision scenes that `collision_speed_edge.rs` (card 8-47) did not try:
//! chains of three and four bodies, boxes, and contacts that carry spin. Measured on 2026-11-20 and
//! pinned here in 2D and 3D, as observations of a solver that resolves all contacts of a step
//! together (not as promises).
//!
//! What was found: a chain of spheres does not amplify a speed (the peak stays below twice the
//! starting speed, for three and four links); boxes are healthy up to the speeds and spins of a
//! normal scene, and are refused only at the top of the given range, where a box that spins at the
//! largest given spin hits a much lighter one at rest, or a short chain of boxes moves at the largest
//! given speed.

use base::math::{Vec2, Vec3};
use physics::core::PhysicsError;
use physics::{d2, d3};

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 40;
/// Neighbours of radius or half side 0.5 whose centres are 0.9 apart overlap a little, so a
/// contact exists in the first step (the engine has no continuous collision detection).
const SPACING: f32 = 0.9;

#[derive(Clone, Copy, PartialEq)]
enum Form {
    Round,
    Boxy,
}

/// A row of `links` bodies, the first moving right at `speed` with `spin`, each next one
/// `ratio` times lighter and at rest; all elastic.
#[derive(Clone, Copy)]
struct Row {
    form: Form,
    links: u32,
    ratio: f32,
    speed: f32,
    spin: f32,
}

/// What a run shows: the largest linear and angular speed seen after a successful step, and the
/// step that was refused, if any.
struct Seen {
    linear: f32,
    angular: f32,
    refused: Option<(u32, PhysicsError)>,
}

/// The density of the first, heaviest body: `ratio` to the power of `links - 1`, by repeated
/// multiplication (the power functions of `f32` are banned in this crate).
fn heaviest(row: Row) -> f32 {
    (1..row.links).fold(1.0, |density, _| density * row.ratio)
}

fn row2(row: Row) -> Result<d2::World, PhysicsError> {
    let mut world = d2::World::new(d2::WorldConfig {
        max_bodies: 8,
        gravity: Vec2::ZERO,
        max_pairs: 64,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: d2::SolverConfig::DEFAULT,
        sleep: d2::SleepConfig::DISABLED,
    })?;
    let (mut density, mut x) = (heaviest(row), 0.0);
    for link in 0..row.links {
        let first = link == 0;
        let shape = match row.form {
            Form::Round => d2::Shape2::Circle { radius: 0.5 },
            Form::Boxy => d2::Shape2::Box {
                half_extents: Vec2::new(0.5, 0.5),
            },
        };
        world.create_body(&d2::BodyDesc {
            linear_velocity: Vec2::new(if first { row.speed } else { 0.0 }, 0.0),
            angular_velocity: if first { row.spin } else { 0.0 },
            density,
            restitution: 1.0,
            ..d2::BodyDesc::new(d2::BodyKind::Dynamic, Vec2::new(x, 0.0), shape)
        })?;
        density /= row.ratio;
        x += SPACING;
    }
    Ok(world)
}

fn row3(row: Row) -> Result<d3::World, PhysicsError> {
    let mut world = d3::World::new(d3::WorldConfig {
        max_bodies: 8,
        gravity: Vec3::ZERO,
        max_pairs: 64,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })?;
    let (mut density, mut x) = (heaviest(row), 0.0);
    for link in 0..row.links {
        let first = link == 0;
        let shape = match row.form {
            Form::Round => d3::Shape3::Sphere { radius: 0.5 },
            Form::Boxy => d3::Shape3::Box {
                half_extents: Vec3::new(0.5, 0.5, 0.5),
            },
        };
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(if first { row.speed } else { 0.0 }, 0.0, 0.0),
            angular_velocity: Vec3::new(0.0, 0.0, if first { row.spin } else { 0.0 }),
            density,
            restitution: 1.0,
            ..d3::BodyDesc::new(d3::BodyKind::Dynamic, Vec3::new(x, 0.0, 0.0), shape)
        })?;
        density /= row.ratio;
        x += SPACING;
    }
    Ok(world)
}

fn run2(row: Row) -> Result<Seen, PhysicsError> {
    let mut world = row2(row)?;
    let mut seen = Seen {
        linear: 0.0,
        angular: 0.0,
        refused: None,
    };
    for step in 0..STEPS {
        if let Err(error) = world.step(DT) {
            seen.refused = Some((step, error));
            break;
        }
        for (_, body) in world.iter() {
            seen.linear = seen.linear.max(body.linear_velocity().length());
            seen.angular = seen.angular.max(body.angular_velocity().abs());
        }
    }
    Ok(seen)
}

fn run3(row: Row) -> Result<Seen, PhysicsError> {
    let mut world = row3(row)?;
    let mut seen = Seen {
        linear: 0.0,
        angular: 0.0,
        refused: None,
    };
    for step in 0..STEPS {
        if let Err(error) = world.step(DT) {
            seen.refused = Some((step, error));
            break;
        }
        for (_, body) in world.iter() {
            seen.linear = seen.linear.max(body.linear_velocity().length());
            seen.angular = seen.angular.max(body.angular_velocity().length());
        }
    }
    Ok(seen)
}

/// The same row in 2D and in 3D.
fn both(row: Row) -> Result<[Seen; 2], PhysicsError> {
    Ok([run2(row)?, run3(row)?])
}

fn row(form: Form, links: u32, ratio: f32, speed: f32) -> Row {
    Row {
        form,
        links,
        ratio,
        speed,
        spin: 0.0,
    }
}

const CHAINS: [(u32, f32); 4] = [(3, 3.0), (3, 10.0), (4, 3.0), (4, 10.0)];

#[test]
fn a_chain_of_spheres_does_not_amplify_a_speed_beyond_a_two_body_bounce() -> Result<(), PhysicsError>
{
    let speed = d2::MAX_GIVEN_SPEED;
    for (links, ratio) in CHAINS {
        for seen in both(row(Form::Round, links, ratio, speed))? {
            assert!(seen.refused.is_none(), "{links} links, ratio {ratio}");
            assert!(
                seen.linear < 2.0 * speed,
                "{links} links, ratio {ratio}: {}",
                seen.linear
            );
        }
    }
    Ok(())
}

#[test]
fn chains_of_boxes_at_ordinary_fast_speeds_step_in_2d_and_3d() -> Result<(), PhysicsError> {
    for (links, ratio) in CHAINS {
        for seen in both(row(Form::Boxy, links, ratio, 1250.0))? {
            assert!(seen.refused.is_none(), "{links} links, ratio {ratio}");
        }
    }
    Ok(())
}

#[test]
fn a_chain_of_three_boxes_at_the_largest_given_speed_is_refused_at_the_second_step()
-> Result<(), PhysicsError> {
    for seen in both(row(Form::Boxy, 3, 3.0, d2::MAX_GIVEN_SPEED))? {
        assert_eq!(seen.refused, Some((1, PhysicsError::Diverged)));
        assert!(
            seen.linear < d2::MAX_SPEED,
            "the step before was inside the bound: {}",
            seen.linear
        );
    }
    Ok(())
}

#[test]
fn a_heavy_box_spinning_at_half_the_largest_given_spin_hits_a_light_one_and_steps()
-> Result<(), PhysicsError> {
    let spin = d2::MAX_GIVEN_SPIN / 2.0;
    for ratio in [100.0, 1000.0] {
        let hit = Row {
            spin,
            ..row(Form::Boxy, 2, ratio, 1000.0)
        };
        for seen in both(hit)? {
            assert!(seen.refused.is_none(), "ratio {ratio}");
        }
    }
    Ok(())
}

/// The other way round (a ratio below one makes each next body heavier): a light box spinning at
/// the largest given spin that hits a far heavier one at rest steps at every speed tried.
#[test]
fn a_light_box_spinning_at_the_largest_given_spin_hits_a_heavy_one_at_rest_and_steps()
-> Result<(), PhysicsError> {
    for speed in [500.0, 1000.0, 2500.0] {
        let hit = Row {
            spin: d2::MAX_GIVEN_SPIN,
            ..row(Form::Boxy, 2, 0.001, speed)
        };
        for seen in both(hit)? {
            assert!(seen.refused.is_none(), "speed {speed}");
        }
    }
    Ok(())
}

#[test]
fn a_heavy_box_spinning_at_the_largest_given_spin_hits_a_light_one_and_is_refused()
-> Result<(), PhysicsError> {
    let hit = Row {
        spin: d2::MAX_GIVEN_SPIN,
        ..row(Form::Boxy, 2, 1000.0, 1000.0)
    };
    for seen in both(hit)? {
        assert_eq!(
            seen.refused.map(|(_, error)| error),
            Some(PhysicsError::Diverged)
        );
    }
    Ok(())
}

#[test]
fn spheres_that_spin_at_the_largest_given_spin_do_not_gain_spin_from_a_hit()
-> Result<(), PhysicsError> {
    let hit = Row {
        spin: d2::MAX_GIVEN_SPIN,
        ..row(Form::Round, 2, 1000.0, 2500.0)
    };
    for seen in both(hit)? {
        assert!(seen.refused.is_none());
        let spin = d2::MAX_GIVEN_SPIN;
        assert!(
            seen.angular > 0.9 * spin && seen.angular < 1.01 * spin,
            "{}",
            seen.angular
        );
    }
    Ok(())
}

/// Which body is destroyed matters: the error names none. In the refused chain of three boxes the
/// fast first body is not the cure (the other two already carry the speed and meet), the middle
/// one is.
fn after_destroying2(which: usize) -> Result<Result<(), PhysicsError>, PhysicsError> {
    let mut world = row2(row(Form::Boxy, 3, 3.0, d2::MAX_GIVEN_SPEED))?;
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    let victim = world
        .iter()
        .map(|(handle, _)| handle)
        .nth(which)
        .ok_or(PhysicsError::StaleHandle)?;
    world.destroy_body(victim)?;
    Ok(world.step(DT))
}

fn after_destroying3(which: usize) -> Result<Result<(), PhysicsError>, PhysicsError> {
    let mut world = row3(row(Form::Boxy, 3, 3.0, d2::MAX_GIVEN_SPEED))?;
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    let victim = world
        .iter()
        .map(|(handle, _)| handle)
        .nth(which)
        .ok_or(PhysicsError::StaleHandle)?;
    world.destroy_body(victim)?;
    Ok(world.step(DT))
}

#[test]
fn destroying_a_body_of_the_offending_contact_lets_the_world_step_again() -> Result<(), PhysicsError>
{
    assert_eq!(after_destroying2(1)?, Ok(()));
    assert_eq!(after_destroying3(1)?, Ok(()));
    assert_eq!(after_destroying2(2)?, Ok(()));
    assert_eq!(after_destroying3(2)?, Ok(()));
    Ok(())
}

#[test]
fn destroying_the_fast_first_body_does_not_cure_the_refusal() -> Result<(), PhysicsError> {
    assert_eq!(after_destroying2(0)?, Err(PhysicsError::Diverged));
    assert_eq!(after_destroying3(0)?, Err(PhysicsError::Diverged));
    Ok(())
}
