use super::*;
use base::math::Quat;
use physics::d3::Shape3;

fn rng() -> Pcg32 {
    Pcg32::new(7, 3)
}

fn within(value: f32, low: f32, high: f32) -> bool {
    (low..=high).contains(&value)
}

#[test]
fn the_scenery_is_a_floor_and_four_walls_around_the_arena() {
    let pieces = scenery();
    assert_eq!(pieces.len(), 5);
    let floor = pieces[0];
    assert!(
        (floor.position.y + floor.half.y).abs() < 1e-6,
        "its top is y = 0"
    );
    for wall in &pieces[1..] {
        assert!(
            (wall.position.y - wall.half.y).abs() < 1e-6,
            "a wall stands on the floor"
        );
        let face = if wall.position.x == 0.0 {
            wall.position.z.abs() - wall.half.z
        } else {
            wall.position.x.abs() - wall.half.x
        };
        assert!(
            (face - ARENA_HALF).abs() < 1e-6,
            "its inner face is the arena edge"
        );
    }
}

#[test]
fn every_scenery_piece_has_a_positive_size() {
    for piece in scenery() {
        assert!(piece.half.x > 0.0 && piece.half.y > 0.0 && piece.half.z > 0.0);
    }
}

#[test]
fn a_sphere_has_a_radius_and_a_mesh_scale_of_that_radius() {
    let made = piece(&mut rng(), Piece::Sphere, Vec3::new(1.0, 5.0, 2.0));
    let Shape3::Sphere { radius } = made.desc.shape else {
        unreachable!("a sphere piece makes a sphere");
    };
    assert!(within(radius, MIN_SIZE, MIN_SIZE + EXTRA_SIZE));
    assert_eq!(made.scale, Vec3::splat(radius));
    assert_eq!(made.desc.position, Vec3::new(1.0, 5.0, 2.0));
}

#[test]
fn a_box_scale_is_its_half_extents() {
    let made = piece(&mut rng(), Piece::Box, Vec3::ZERO);
    let Shape3::Box { half_extents } = made.desc.shape else {
        unreachable!("a box piece makes a box");
    };
    assert_eq!(made.scale, half_extents);
    for side in [half_extents.x, half_extents.y, half_extents.z] {
        assert!(within(side, MIN_SIZE, MIN_SIZE + EXTRA_SIZE));
    }
}

#[test]
fn a_capsule_scale_matches_the_unit_capsule_mesh() {
    let made = piece(&mut rng(), Piece::Capsule, Vec3::ZERO);
    let Shape3::Capsule {
        half_height,
        radius,
    } = made.desc.shape
    else {
        unreachable!("a capsule piece makes a capsule");
    };
    assert!((radius - CAPSULE_RADIUS * made.scale.x).abs() < 1e-6);
    assert!((half_height - CAPSULE_HALF_HEIGHT * made.scale.x).abs() < 1e-6);
    assert_eq!(
        made.scale,
        Vec3::splat(made.scale.x),
        "one scale for all three axes"
    );
}

#[test]
fn a_piece_is_tilted_by_a_unit_rotation() {
    let mut random = rng();
    for _ in 0..20 {
        let made = piece(&mut random, Piece::Box, Vec3::ZERO);
        let q = made.desc.rotation;
        let length = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
        assert!((length - 1.0).abs() < 1e-4);
    }
}

#[test]
fn the_same_seed_makes_the_same_pieces() {
    let (mut a, mut b) = (rng(), rng());
    for kind in [Piece::Sphere, Piece::Box, Piece::Capsule] {
        let (first, second) = (
            piece(&mut a, kind, Vec3::ZERO),
            piece(&mut b, kind, Vec3::ZERO),
        );
        assert_eq!(first.scale, second.scale);
        assert_eq!(first.desc.rotation, second.desc.rotation);
    }
}

#[test]
fn a_drop_point_is_over_the_floor_and_away_from_the_walls() {
    let mut random = rng();
    for _ in 0..200 {
        let at = drop_point(&mut random);
        assert!(at.x.abs() <= ARENA_HALF - MARGIN && at.z.abs() <= ARENA_HALF - MARGIN);
        assert!((at.y - DROP_HEIGHT).abs() < 1e-6);
    }
}

#[test]
fn the_drop_points_spread_over_both_sides() {
    let mut random = rng();
    let points: Vec<Vec3> = (0..200).map(|_| drop_point(&mut random)).collect();
    assert!(points.iter().any(|p| p.x > 1.0) && points.iter().any(|p| p.x < -1.0));
    assert!(points.iter().any(|p| p.z > 1.0) && points.iter().any(|p| p.z < -1.0));
}

#[test]
fn the_stream_cycles_through_the_three_pieces() {
    let mut random = rng();
    let kinds: Vec<Piece> = (0..300).map(|_| stream_piece(&mut random)).collect();
    for wanted in [Piece::Sphere, Piece::Box, Piece::Capsule] {
        assert!(kinds.contains(&wanted), "{wanted:?} never came");
    }
}

fn tilts(count: usize) -> Vec<Quat> {
    let mut random = rng();
    (0..count)
        .map(|_| piece(&mut random, Piece::Box, Vec3::ZERO).desc.rotation)
        .collect()
}

#[test]
fn the_tilts_turn_by_up_to_a_half_turn_about_an_upward_axis() {
    let all = tilts(400);
    assert!(all.iter().all(|q| *q != Quat::IDENTITY));
    assert!(
        all.iter().all(|q| q.y >= 0.0 && q.w >= -1e-6),
        "angle in 0..=pi, axis up"
    );
    let mean = |f: fn(&Quat) -> f32| {
        all.iter().map(f).sum::<f32>() / f32::from(u16::try_from(all.len()).unwrap_or(1))
    };
    assert!(mean(|q| q.y) > 0.15, "the axis leans up");
    assert!(mean(|q| q.w) < 0.85, "not a sliver of a turn");
    assert!(all.iter().any(|q| q.x > 0.1) && all.iter().any(|q| q.x < -0.1));
    assert!(all.iter().any(|q| q.z > 0.1) && all.iter().any(|q| q.z < -0.1));
}

#[test]
fn a_capsule_is_between_the_smallest_and_largest_size() {
    let mut random = rng();
    for _ in 0..50 {
        let made = piece(&mut random, Piece::Capsule, Vec3::ZERO);
        let Shape3::Capsule { radius, .. } = made.desc.shape else {
            unreachable!("a capsule piece makes a capsule");
        };
        assert!(within(
            radius,
            CAPSULE_RADIUS * 2.0 * MIN_SIZE,
            CAPSULE_RADIUS * 2.0 * (MIN_SIZE + EXTRA_SIZE)
        ));
    }
}

#[test]
fn drops_reach_the_edge_of_the_allowed_area_and_no_further() {
    let mut random = rng();
    let reach = ARENA_HALF - MARGIN;
    let points: Vec<Vec3> = (0..400).map(|_| drop_point(&mut random)).collect();
    for axis in [|p: &Vec3| p.x, |p: &Vec3| p.z] {
        let widest = points.iter().map(|p| axis(p).abs()).fold(0.0, f32::max);
        assert!(widest > reach * 0.9 && widest <= reach, "{widest}");
    }
}

#[test]
fn the_floor_lies_below_zero_and_the_walls_either_side() {
    let blocks = scenery();
    assert!(blocks[0].position.y < 0.0);
    assert!(blocks[1].position.x < 0.0 && blocks[2].position.x > 0.0);
    assert!(blocks[3].position.z < 0.0 && blocks[4].position.z > 0.0);
}
