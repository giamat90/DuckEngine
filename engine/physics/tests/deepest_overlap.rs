//! Card 8-53: `World::deepest_overlap` names the deepest contact of the last step, so a game can
//! see a squashed stack that no speed bound sees. It is read when asked: a step stores nothing.

use base::math::{Vec2, Vec3};
use physics::core::PhysicsError;
use physics::{d2, d3};

/// A step this short moves nothing visible, so the overlaps are the ones the bodies were made with.
const TINY: f32 = 1.0e-6;
const RADIUS: f32 = 0.5;

fn world2() -> Result<d2::World, PhysicsError> {
    d2::World::new(d2::WorldConfig {
        max_bodies: 8,
        gravity: Vec2::ZERO,
        max_pairs: 64,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: d2::SolverConfig::DEFAULT,
        sleep: d2::SleepConfig::DISABLED,
    })
}

fn world3() -> Result<d3::World, PhysicsError> {
    d3::World::new(d3::WorldConfig {
        max_bodies: 8,
        gravity: Vec3::ZERO,
        max_pairs: 64,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })
}

fn circles(xs: &[f32]) -> Result<(d2::World, Vec<d2::BodyHandle>), PhysicsError> {
    let mut world = world2()?;
    let mut handles = Vec::new();
    for &x in xs {
        handles.push(world.create_body(&d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::new(x, 0.0),
            d2::Shape2::Circle { radius: RADIUS },
        ))?);
    }
    Ok((world, handles))
}

fn spheres(xs: &[f32]) -> Result<(d3::World, Vec<d3::BodyHandle>), PhysicsError> {
    let mut world = world3()?;
    let mut handles = Vec::new();
    for &x in xs {
        handles.push(world.create_body(&d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            Vec3::new(x, 0.0, 0.0),
            d3::Shape3::Sphere { radius: RADIUS },
        ))?);
    }
    Ok((world, handles))
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

#[test]
fn nothing_overlaps_before_the_first_step_and_when_the_bodies_are_apart() -> Result<(), PhysicsError>
{
    let (mut world, _) = circles(&[0.0, 5.0])?;
    assert_eq!(world.deepest_overlap(), None);
    world.step(TINY)?;
    assert_eq!(world.deepest_overlap(), None);
    let (mut world, _) = spheres(&[0.0, 5.0])?;
    world.step(TINY)?;
    assert_eq!(world.deepest_overlap(), None);
    Ok(())
}

#[test]
fn touching_bodies_that_do_not_overlap_report_nothing() -> Result<(), PhysicsError> {
    // A gap of 0.01, inside the speculative margin of 0.02: a contact exists (positive
    // separation) but nothing overlaps.
    let (mut world, _) = circles(&[0.0, 1.01])?;
    world.step(TINY)?;
    assert!(!world.manifolds().as_slice().is_empty());
    assert_eq!(world.deepest_overlap(), None);
    let (mut world, _) = spheres(&[0.0, 1.01])?;
    world.step(TINY)?;
    assert!(!world.manifolds().as_slice().is_empty());
    assert_eq!(world.deepest_overlap(), None);
    Ok(())
}

#[test]
fn the_overlap_of_two_circles_is_the_depth_and_names_the_pair_in_2d() -> Result<(), PhysicsError> {
    let (mut world, handles) = circles(&[0.0, 0.9])?;
    world.step(TINY)?;
    let deepest = world.deepest_overlap().ok_or(PhysicsError::NotFinite)?;
    assert_eq!((deepest.first, deepest.second), (handles[0], handles[1]));
    assert!(near(deepest.depth, 0.1), "{}", deepest.depth);
    Ok(())
}

#[test]
fn the_overlap_of_two_spheres_is_the_depth_and_names_the_pair_in_3d() -> Result<(), PhysicsError> {
    let (mut world, handles) = spheres(&[0.0, 0.9])?;
    world.step(TINY)?;
    let deepest = world.deepest_overlap().ok_or(PhysicsError::NotFinite)?;
    assert_eq!((deepest.first, deepest.second), (handles[0], handles[1]));
    assert!(near(deepest.depth, 0.1), "{}", deepest.depth);
    Ok(())
}

#[test]
fn the_deepest_of_several_contacts_is_named_not_the_first() -> Result<(), PhysicsError> {
    // Overlaps of 0.1 (bodies 0 and 1) and 0.3 (bodies 1 and 2); bodies 0 and 2 are apart.
    let (mut world, handles) = circles(&[0.0, 0.9, 1.6])?;
    world.step(TINY)?;
    let deepest = world.deepest_overlap().ok_or(PhysicsError::NotFinite)?;
    assert_eq!((deepest.first, deepest.second), (handles[1], handles[2]));
    assert!(near(deepest.depth, 0.3), "{}", deepest.depth);
    let (mut world, handles) = spheres(&[0.0, 0.9, 1.6])?;
    world.step(TINY)?;
    let deepest = world.deepest_overlap().ok_or(PhysicsError::NotFinite)?;
    assert_eq!((deepest.first, deepest.second), (handles[1], handles[2]));
    assert!(near(deepest.depth, 0.3), "{}", deepest.depth);
    Ok(())
}

#[test]
fn the_report_follows_the_last_step_and_a_destroyed_body_leaves_it() -> Result<(), PhysicsError> {
    let (mut world, handles) = spheres(&[0.0, 0.9, 1.6])?;
    world.step(TINY)?;
    world.destroy_body(handles[2])?;
    // The manifolds are those of the last step until the next step (or `collide`) remakes them,
    // so the destroyed body is named until then; after the step it is gone.
    assert_eq!(
        world
            .deepest_overlap()
            .map(|found| (found.first, found.second)),
        Some((handles[1], handles[2]))
    );
    world.step(TINY)?;
    let deepest = world.deepest_overlap().ok_or(PhysicsError::NotFinite)?;
    assert_eq!((deepest.first, deepest.second), (handles[0], handles[1]));
    assert!(near(deepest.depth, 0.1), "{}", deepest.depth);
    Ok(())
}

/// A static floor and three unit boxes in a column, the top one of `top_density` (the others 1),
/// stepped for five seconds at 60 Hz with `solver`; the deepest overlap at the end and the speed of
/// the top box.
fn squashed_by(solver: d3::SolverConfig, top_density: f32) -> Result<(f32, f32), PhysicsError> {
    let mut world = d3::World::new(d3::WorldConfig {
        gravity: d3::STANDARD_GRAVITY,
        solver,
        ..world3_config()
    })?;
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(10.0, 0.5, 10.0),
        },
    ))?;
    let mut top = None;
    for (height, density) in [(0.5, 1.0), (1.5, 1.0), (2.5, top_density)] {
        top = Some(world.create_body(&d3::BodyDesc {
            density,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(0.0, height, 0.0),
                d3::Shape3::Box {
                    half_extents: Vec3::new(0.5, 0.5, 0.5),
                },
            )
        })?);
    }
    for _ in 0..300 {
        world.step(1.0 / 60.0)?;
    }
    let top = top.ok_or(PhysicsError::NotFinite)?;
    let speed = world.body(top)?.linear_velocity().length();
    let depth = world.deepest_overlap().map_or(0.0, |overlap| overlap.depth);
    Ok((depth, speed))
}

fn world3_config() -> d3::WorldConfig {
    d3::WorldConfig {
        max_bodies: 16,
        gravity: Vec3::ZERO,
        max_pairs: 64,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    }
}

#[test]
fn a_dense_top_squashes_a_stack_and_the_report_shows_how_much() -> Result<(), PhysicsError> {
    // Measured: a light top 2 mm; a top 200 times as dense 0.24 m with the default solver (a
    // quarter of a box) and 8.5 mm with the stiff preset. One measurement of this commit.
    let (light, _) = squashed_by(d3::SolverConfig::DEFAULT, 1.0)?;
    let (default, default_speed) = squashed_by(d3::SolverConfig::DEFAULT, 200.0)?;
    let (stiff, _) = squashed_by(d3::SolverConfig::STIFF_CONTACTS, 200.0)?;
    assert!(light < 0.005, "{light}");
    assert!(default > 0.1, "{default}");
    assert!(stiff < 0.02, "{stiff}");
    // No speed bound sees the squash: the top box is slow while it is deep in the stack.
    assert!(default_speed < 1.0, "{default_speed}");
    Ok(())
}

#[test]
fn equal_depths_name_the_first_pair_in_manifold_order() -> Result<(), PhysicsError> {
    let (mut world, handles) = circles(&[0.0, 0.9, 1.8])?;
    world.step(TINY)?;
    let deepest = world.deepest_overlap().ok_or(PhysicsError::NotFinite)?;
    assert_eq!((deepest.first, deepest.second), (handles[0], handles[1]));
    assert!(near(deepest.depth, 0.1), "{}", deepest.depth);
    Ok(())
}

#[test]
fn a_box_sunk_into_the_floor_reports_its_four_point_face_depth() -> Result<(), PhysicsError> {
    let mut world = world3()?;
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(5.0, 0.5, 5.0),
        },
    ))?;
    // The box (half side 0.5) has its centre 0.4 above the floor top: 0.1 deep.
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Dynamic,
        Vec3::new(0.0, 0.4, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        },
    ))?;
    world.step(TINY)?;
    let deepest = world.deepest_overlap().ok_or(PhysicsError::NotFinite)?;
    assert!(near(deepest.depth, 0.1), "{}", deepest.depth);
    Ok(())
}
