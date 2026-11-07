//! The narrowphase inside the world, through the public 2D API: manifolds appear for the pairs of
//! the pair list that touch, in pair order, with the right normal, depth, anchors and feature ids;
//! they stay stable between steps for a slowly moving box; the buffer reports overflow instead of
//! losing manifolds silently. (What the solver does with them is tested in `tests/solver`.)

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    FeatureId, PhysicsError, Rot2, STANDARD_GRAVITY, Shape2, SolverConfig, World, WorldConfig,
};

const DT: f32 = 1.0 / 60.0;

fn world(gravity: Vec2, max_manifolds: u32) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 32,
        gravity,
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DEFAULT,
    })
}

fn ground(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(10.0, 1.0),
        },
    ))
}

fn crate_at(x: f32, y: f32) -> BodyDesc {
    BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(x, y),
        Shape2::Box {
            half_extents: Vec2::new(0.5, 0.5),
        },
    )
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-5
}

#[test]
fn a_box_resting_overlapped_on_the_ground_gives_two_points_one_centimetre_deep()
-> Result<(), PhysicsError> {
    let mut world = world(Vec2::ZERO, 16)?;
    let floor = ground(&mut world)?;
    let rest = world.create_body(&crate_at(0.0, 0.49))?;
    world.find_pairs();
    world.collide();
    let [entry] = world.manifolds().as_slice() else {
        panic!("one manifold expected: {:?}", world.manifolds().as_slice());
    };
    assert_eq!((entry.first, entry.second), (floor, rest));
    let m = entry.manifold;
    assert_eq!(m.normal, Vec2::new(0.0, 1.0));
    assert_eq!(m.points().len(), 2);
    for p in m.points() {
        assert!(near(p.separation, -0.01), "{p:?}");
        assert!(near(p.point.y, -0.005), "{p:?}");
        // The anchors are relative to the centres: the ground's at (0, -1), the box's at (0, 0.49).
        assert!(near(p.anchor_a.y, p.point.y + 1.0) && near(p.anchor_a.x, p.point.x));
        assert!(near(p.anchor_b.y, p.point.y - 0.49) && near(p.anchor_b.x, p.point.x));
    }
    let xs: Vec<f32> = m.points().iter().map(|p| p.point.x).collect();
    assert_eq!(xs, vec![-0.5, 0.5]);
    assert_ne!(m.points()[0].id, m.points()[1].id);
    assert!(world.manifolds().is_complete());
    Ok(())
}

#[test]
fn a_box_falling_onto_the_ground_gets_a_manifold_only_when_it_is_within_the_margin()
-> Result<(), PhysicsError> {
    let mut world = world(STANDARD_GRAVITY, 16)?;
    ground(&mut world)?;
    world.create_body(&crate_at(0.0, 0.55))?;
    let mut pair_without_contact = false;
    let mut first_contact = None;
    for step in 0..60 {
        world.step(DT)?;
        pair_without_contact |= !world.pairs().is_empty() && world.manifolds().is_empty();
        if first_contact.is_none() && !world.manifolds().is_empty() {
            first_contact = Some(step);
            let m = world.manifolds().as_slice()[0].manifold;
            assert_eq!(m.normal, Vec2::new(0.0, 1.0));
            assert_eq!(m.points().len(), 2);
            assert!(
                m.points()
                    .iter()
                    .all(|p| p.separation <= DEFAULT_SPECULATIVE_MARGIN && p.separation > -0.05),
                "{:?}",
                m.points()
            );
        }
    }
    assert!(
        pair_without_contact,
        "the fat boxes pair up before the shapes are near"
    );
    assert!(
        first_contact.is_some_and(|step| step > 0),
        "{first_contact:?}"
    );
    // Since card 4-13 the solver stops it: it rests on the ground (before, it fell through).
    let (_, body) = world.iter().nth(1).ok_or(PhysicsError::StaleHandle)?;
    assert!(
        (body.position().y - 0.5).abs() < 0.01,
        "{:?}",
        body.position()
    );
    Ok(())
}

#[test]
fn feature_ids_stay_the_same_while_a_slightly_turned_box_slides_on_the_ground()
-> Result<(), PhysicsError> {
    // Under gravity and without friction, so the solver keeps it on the ground and does not stop it.
    let mut world = world(STANDARD_GRAVITY, 16)?;
    ground(&mut world)?;
    let mut desc = crate_at(-2.0, 0.495);
    desc.rotation = Rot2::from_cos_sin(1000.0, 3.0).unwrap_or(Rot2::IDENTITY);
    desc.linear_velocity = Vec2::new(0.4, 0.0);
    desc.angular_velocity = 0.002;
    desc.friction = 0.0;
    world.create_body(&desc)?;
    let mut first: Option<Vec<FeatureId>> = None;
    let mut last_x = f32::NEG_INFINITY;
    for step in 0..400 {
        world.step(DT)?;
        let [entry] = world.manifolds().as_slice() else {
            panic!("step {step}: one manifold expected");
        };
        let ids: Vec<FeatureId> = entry.manifold.points().iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), 2, "step {step}");
        assert_eq!(
            first.get_or_insert_with(|| ids.clone()),
            &ids,
            "step {step}"
        );
        let x = entry.manifold.points()[0].point.x;
        assert!(x > last_x, "step {step}: the contact follows the box");
        last_x = x;
    }
    Ok(())
}

fn row_of_circles(world: &mut World, count: u8) -> Result<(), PhysicsError> {
    for i in 0..count {
        let desc = BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::new(0.9 * f32::from(i), 0.0),
            Shape2::Circle { radius: 0.5 },
        );
        world.create_body(&desc)?;
    }
    Ok(())
}

#[test]
fn manifolds_past_the_capacity_are_counted_and_the_stored_ones_are_a_prefix()
-> Result<(), PhysicsError> {
    let mut full = world(Vec2::ZERO, 16)?;
    let mut small = world(Vec2::ZERO, 3)?;
    row_of_circles(&mut full, 8)?;
    row_of_circles(&mut small, 8)?;
    for world in [&mut full, &mut small] {
        world.find_pairs();
        world.collide();
    }
    // Seven neighbouring pairs touch; the pairs two apart (1.8 m) are too far.
    assert_eq!((full.manifolds().len(), full.manifolds().missing()), (7, 0));
    assert!(full.manifolds().is_complete());
    assert_eq!(
        (small.manifolds().len(), small.manifolds().missing()),
        (3, 4)
    );
    assert!(!small.manifolds().is_complete());
    assert_eq!(
        small.manifolds().as_slice(),
        &full.manifolds().as_slice()[..3]
    );
    // The next computation starts from scratch.
    small.collide();
    assert_eq!(
        (small.manifolds().len(), small.manifolds().missing()),
        (3, 4)
    );
    Ok(())
}

#[test]
fn a_destroyed_body_leaves_no_manifold_after_a_new_computation() -> Result<(), PhysicsError> {
    let mut world = world(Vec2::ZERO, 16)?;
    row_of_circles(&mut world, 3)?;
    world.find_pairs();
    world.collide();
    assert_eq!(world.manifolds().len(), 2);
    let victim = world
        .iter()
        .nth(1)
        .map(|(handle, _)| handle)
        .ok_or(PhysicsError::StaleHandle)?;
    world.destroy_body(victim)?;
    // The pair list still names the destroyed body until the pairs are found again.
    world.collide();
    assert!(
        world.manifolds().is_empty(),
        "{:?}",
        world.manifolds().as_slice()
    );
    world.find_pairs();
    world.collide();
    assert!(world.manifolds().is_empty());
    Ok(())
}

#[test]
fn collision_applies_no_force_and_leaves_the_state_hash_alone() -> Result<(), PhysicsError> {
    let mut world = world(Vec2::ZERO, 16)?;
    ground(&mut world)?;
    world.create_body(&crate_at(0.0, 0.49))?;
    world.find_pairs();
    let before = world.state_hash();
    world.collide();
    world.collide();
    assert_eq!(world.state_hash(), before);
    assert_eq!(world.manifolds().len(), 1);
    Ok(())
}

#[test]
fn bodies_at_huge_coordinates_and_with_huge_shapes_give_finite_manifolds_or_none()
-> Result<(), PhysicsError> {
    let mut world = world(Vec2::ZERO, 16)?;
    let far = 3.0e38;
    for x in [far, far, -far] {
        world.create_body(&crate_at(x, 0.0))?;
    }
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(far, 1.0),
        Shape2::Circle { radius: 1.0e8 },
    ))?;
    world.step(DT)?;
    assert!(!world.manifolds().is_empty());
    for entry in world.manifolds().as_slice() {
        assert!(entry.manifold.normal.x.is_finite() && entry.manifold.normal.y.is_finite());
        for p in entry.manifold.points() {
            assert!(p.point.x.is_finite() && p.point.y.is_finite() && p.separation.is_finite());
            assert!(p.anchor_a.x.is_finite() && p.anchor_b.y.is_finite());
        }
    }
    Ok(())
}

fn busy_scene() -> Result<World, PhysicsError> {
    let mut world = world(STANDARD_GRAVITY, 64)?;
    ground(&mut world)?;
    for i in 0..10_u8 {
        let x = f32::from(i);
        let mut desc = crate_at(0.4 * x - 2.0, 0.6 + 0.8 * x);
        desc.rotation = Rot2::from_cos_sin(10.0, x - 5.0).unwrap_or(Rot2::IDENTITY);
        desc.angular_velocity = 0.5 - 0.1 * x;
        if i % 3 == 0 {
            desc.shape = Shape2::Circle { radius: 0.4 };
        }
        world.create_body(&desc)?;
    }
    Ok(world)
}

#[test]
fn two_runs_of_a_busy_scene_make_the_same_manifolds_at_every_step() -> Result<(), PhysicsError> {
    let (mut first, mut second) = (busy_scene()?, busy_scene()?);
    let mut with_contacts = 0;
    for step in 0..200 {
        first.step(DT)?;
        second.step(DT)?;
        assert_eq!(
            first.manifolds().hash(),
            second.manifolds().hash(),
            "step {step}"
        );
        assert_eq!(
            first.manifolds().as_slice(),
            second.manifolds().as_slice(),
            "step {step}"
        );
        with_contacts += u32::from(!first.manifolds().is_empty());
    }
    assert!(with_contacts > 20, "{with_contacts}");
    Ok(())
}

#[test]
fn manifolds_follow_the_pair_order_and_every_one_has_a_unit_normal_and_valid_points()
-> Result<(), PhysicsError> {
    let mut world = busy_scene()?;
    for step in 0..200 {
        world.step(DT)?;
        let mut pairs = world.pairs().as_slice().iter();
        for entry in world.manifolds().as_slice() {
            // Each manifold belongs to a pair of the list, and the list is walked in order.
            assert!(
                pairs.any(|pair| (pair.first, pair.second) == (entry.first, entry.second)),
                "step {step}: manifold without its pair, or out of order"
            );
            let n = entry.manifold.normal;
            assert!(
                (n.x * n.x + n.y * n.y - 1.0).abs() < 1.0e-5,
                "step {step}: {n:?}"
            );
            let points = entry.manifold.points();
            assert!(!points.is_empty() && points.len() <= 2, "step {step}");
            assert!(
                points
                    .iter()
                    .all(|p| p.separation <= DEFAULT_SPECULATIVE_MARGIN),
                "step {step}: {points:?}"
            );
        }
    }
    Ok(())
}
