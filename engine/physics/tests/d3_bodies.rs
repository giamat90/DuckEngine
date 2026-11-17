//! Card 8-3: the `D=3` instance has bodies that fall, spin, and hash like the 2D ones. The shapes
//! here are the mass and bounds half of the instance; the narrowphase is card 8-4.

use base::dmath::{cos, sin};
use base::math::{Quat, Vec3};
use physics::core::{Bounds, Dim, HashBits, PhysicsError, StateHasher};
use physics::d3::{
    Aabb3, BodyDesc, BodyKind, D3, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

const DT: f32 = 0.001;

fn near(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance * b.abs().max(1.0)
}

fn sphere(radius: f32) -> Shape3 {
    Shape3::Sphere { radius }
}

fn cuboid(x: f32, y: f32, z: f32) -> Shape3 {
    Shape3::Box {
        half_extents: Vec3::new(x, y, z),
    }
}

fn capsule(half_height: f32, radius: f32) -> Shape3 {
    Shape3::Capsule {
        half_height,
        radius,
    }
}

fn world(gravity: Vec3) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 8,
        gravity,
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })
}

fn about(axis: Vec3, angle: f32) -> Quat {
    let Some(q) = Quat::from_axis_angle(axis, angle) else {
        panic!("the axis is a unit vector");
    };
    q
}

fn about_y(angle: f32) -> Quat {
    about(Vec3::Y, angle)
}

fn valid(shape: Shape3, density: f32) -> (f32, Vec3) {
    let Some(props) = mass_and_inertia(shape, density) else {
        panic!("{shape:?} at {density} is a valid body");
    };
    props
}

// ---- integration --------------------------------------------------------------------------

#[test]
fn a_body_falls_and_drifts_like_a_particle() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::new(0.0, -10.0, 0.0))?;
    let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec3::new(1.0, 50.0, -2.0), sphere(0.5));
    desc.linear_velocity = Vec3::new(2.0, 0.0, 1.0);
    let handle = w.create_body(&desc)?;
    for _ in 0..1000 {
        w.step(DT)?;
    }
    let body = w.body(handle)?;
    // After n = 1000 semi-implicit steps: v_y = n g dt, y = y0 + g dt^2 n (n + 1) / 2.
    assert!(near(body.linear_velocity().y, -10.0, 1e-4));
    assert!(near(
        body.position().y,
        50.0 - 10.0 * DT * DT * 1000.0 * 1001.0 / 2.0,
        1e-4
    ));
    assert!(near(body.position().x, 3.0, 1e-4));
    assert!(near(body.position().z, -1.0, 1e-4));
    Ok(())
}

#[test]
fn a_spinning_body_turns_by_angular_velocity_times_time() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec3::ZERO, cuboid(0.5, 1.0, 0.25));
    desc.angular_velocity = Vec3::new(0.0, 2.0, 0.0);
    let handle = w.create_body(&desc)?;
    for _ in 0..1000 {
        w.step(DT)?;
    }
    // One second at 2 rad/s about Y: the local X axis ends at (cos 2, 0, -sin 2).
    let x_axis = w.body(handle)?.rotation().rotate(Vec3::X);
    assert!(near(x_axis.x, cos(2.0), 2e-3), "{x_axis:?}");
    assert!(near(x_axis.z, -(sin(2.0)), 2e-3), "{x_axis:?}");
    assert!(x_axis.y.abs() < 1e-3, "{x_axis:?}");
    // A body with no torque keeps its angular velocity.
    assert_eq!(w.body(handle)?.angular_velocity(), Vec3::new(0.0, 2.0, 0.0));
    Ok(())
}

#[test]
fn the_orientation_stays_a_unit_quaternion_while_tumbling() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec3::ZERO, cuboid(1.0, 0.5, 0.25));
    desc.angular_velocity = Vec3::new(3.0, -2.0, 5.0);
    let handle = w.create_body(&desc)?;
    for _ in 0..20_000 {
        w.step(DT)?;
    }
    let q = w.body(handle)?.rotation();
    assert!((q.length() - 1.0).abs() < 1e-5, "length {}", q.length());
    Ok(())
}

#[test]
fn a_static_body_does_not_move() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::new(0.0, -10.0, 0.0))?;
    let mut desc = BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, 5.0, 0.0),
        cuboid(1.0, 1.0, 1.0),
    );
    desc.rotation = about_y(0.3);
    let handle = w.create_body(&desc)?;
    for _ in 0..100 {
        w.step(DT)?;
    }
    let body = w.body(handle)?;
    assert_eq!(body.position(), Vec3::new(0.0, 5.0, 0.0));
    assert_eq!(body.rotation(), about_y(0.3));
    Ok(())
}

// ---- hash ---------------------------------------------------------------------------------

#[test]
fn two_identical_worlds_have_the_same_hash_at_every_step() -> Result<(), PhysicsError> {
    let build = || -> Result<World, PhysicsError> {
        let mut w = world(Vec3::new(0.0, -10.0, 0.0))?;
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(0.0, 3.0, 0.0),
            capsule(0.5, 0.25),
        );
        desc.angular_velocity = Vec3::new(1.0, 2.0, 3.0);
        w.create_body(&desc)?;
        Ok(w)
    };
    let (mut a, mut b) = (build()?, build()?);
    for _ in 0..100 {
        a.step(DT)?;
        b.step(DT)?;
        assert_eq!(a.state_hash(), b.state_hash());
    }
    Ok(())
}

#[test]
fn the_hash_sees_orientation_shape_and_angular_velocity() -> Result<(), PhysicsError> {
    let hash_of = |edit: &dyn Fn(&mut BodyDesc)| -> Result<u64, PhysicsError> {
        let mut w = world(Vec3::ZERO)?;
        let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec3::ZERO, cuboid(1.0, 1.0, 1.0));
        edit(&mut desc);
        w.create_body(&desc)?;
        Ok(w.state_hash())
    };
    let base = hash_of(&|_| {})?;
    assert_eq!(base, hash_of(&|_| {})?);
    assert_ne!(base, hash_of(&|d| d.rotation = about_y(0.1))?);
    assert_ne!(
        base,
        hash_of(&|d| d.angular_velocity = Vec3::new(0.0, 0.0, 1.0))?
    );
    assert_ne!(base, hash_of(&|d| d.shape = cuboid(1.0, 1.0, 2.0))?);
    assert_ne!(base, hash_of(&|d| d.shape = sphere(1.0))?);
    assert_ne!(
        hash_of(&|d| d.shape = sphere(1.0))?,
        hash_of(&|d| d.shape = capsule(1.0, 1.0))?
    );
    Ok(())
}

// ---- mass properties ------------------------------------------------------------------------

fn mass_and_inertia(shape: Shape3, density: f32) -> Option<(f32, Vec3)> {
    D3::mass_properties(&shape, density).ok().map(|p| {
        (
            p.mass,
            Vec3::new(
                1.0 / p.inv_inertia.x,
                1.0 / p.inv_inertia.y,
                1.0 / p.inv_inertia.z,
            ),
        )
    })
}

#[test]
fn a_sphere_has_the_closed_form_mass_and_inertia() {
    let (r, rho) = (0.5_f32, 3.0_f32);
    let mass = rho * 4.0 / 3.0 * PI * r * r * r;
    let inertia = 0.4 * mass * r * r;
    let (m, i) = valid(sphere(r), rho);
    assert!(near(m, mass, 1e-5));
    for component in [i.x, i.y, i.z] {
        assert!(near(component, inertia, 1e-5), "{i:?} vs {inertia}");
    }
}

#[test]
fn a_box_has_the_closed_form_mass_and_inertia() {
    let (hx, hy, hz, rho) = (0.5_f32, 1.5_f32, 1.25_f32, 2.0_f32);
    let mass = rho * 8.0 * hx * hy * hz;
    let (m, i) = valid(cuboid(hx, hy, hz), rho);
    assert!(near(m, mass, 1e-5));
    // I_x = m (w_y^2 + w_z^2) / 12 with full widths w = 2 h, that is m (h_y^2 + h_z^2) / 3.
    assert!(near(i.x, mass * (hy * hy + hz * hz) / 3.0, 1e-5));
    assert!(near(i.y, mass * (hx * hx + hz * hz) / 3.0, 1e-5));
    assert!(near(i.z, mass * (hx * hx + hy * hy) / 3.0, 1e-5));
}

#[test]
fn a_capsule_is_a_cylinder_plus_a_sphere_about_its_axis() {
    let (h, r, rho) = (0.75_f32, 0.25_f32, 2.0_f32);
    let cylinder = rho * PI * r * r * 2.0 * h;
    let ball = rho * 4.0 / 3.0 * PI * r * r * r;
    let (m, i) = valid(capsule(h, r), rho);
    assert!(near(m, cylinder + ball, 1e-5));
    // About the local Y axis: cylinder m r^2 / 2 plus sphere 2 m r^2 / 5.
    assert!(
        near(i.y, cylinder * r * r / 2.0 + ball * 0.4 * r * r, 1e-4),
        "{i:?}"
    );
    // Across the axis: cylinder m (3 r^2 + (2h)^2) / 12 plus the two hemispheres moved to the ends,
    // sphere m (2 r^2 / 5 + h^2 + 3 h r / 4).
    let across =
        cylinder * (3.0 * r * r + 4.0 * h * h) / 12.0 + ball * (0.4 * r * r + h * h + 0.75 * h * r);
    assert!(near(i.x, across, 1e-4), "{i:?} vs {across}");
    assert!(near(i.z, across, 1e-4), "{i:?} vs {across}");
}

#[test]
fn a_very_short_capsule_is_nearly_a_sphere() {
    let (m, i) = valid(capsule(1.0e-4, 0.5), 1.0);
    let (sm, si) = valid(sphere(0.5), 1.0);
    assert!(near(m, sm, 1e-3));
    assert!(near(i.x, si.x, 1e-3) && near(i.y, si.y, 1e-3) && near(i.z, si.z, 1e-3));
}

#[test]
fn shapes_and_densities_that_are_not_positive_and_finite_are_refused() {
    let bad = [0.0, -1.0, f32::NAN, f32::INFINITY];
    for v in bad {
        assert!(mass_and_inertia(sphere(v), 1.0).is_none(), "radius {v}");
        assert!(
            mass_and_inertia(cuboid(v, 1.0, 1.0), 1.0).is_none(),
            "box x {v}"
        );
        assert!(
            mass_and_inertia(cuboid(1.0, v, 1.0), 1.0).is_none(),
            "box y {v}"
        );
        assert!(
            mass_and_inertia(cuboid(1.0, 1.0, v), 1.0).is_none(),
            "box z {v}"
        );
        assert!(
            mass_and_inertia(capsule(v, 1.0), 1.0).is_none(),
            "half height {v}"
        );
        assert!(
            mass_and_inertia(capsule(1.0, v), 1.0).is_none(),
            "capsule radius {v}"
        );
        assert!(mass_and_inertia(sphere(1.0), v).is_none(), "density {v}");
    }
    // The mass is finite but the moment of inertia overflows.
    assert!(mass_and_inertia(sphere(1.0e10), 1.0).is_none());
    // Two negative sides make a positive volume: the shape must still be refused.
    assert!(mass_and_inertia(cuboid(-1.0, -2.0, 3.0), 1.0).is_none());
    assert!(mass_and_inertia(cuboid(-1.0, 2.0, -3.0), 1.0).is_none());
    assert!(mass_and_inertia(cuboid(1.0, -2.0, -3.0), 1.0).is_none());
    assert!(mass_and_inertia(capsule(-1.5, -2.5), 1.0).is_none());
    assert!(
        mass_and_inertia(sphere(1.0e20), 1.0).is_none(),
        "mass overflows"
    );
    assert!(
        mass_and_inertia(sphere(1.0e-20), 1.0).is_none(),
        "inertia underflows"
    );
}

#[test]
fn a_world_refuses_an_invalid_shape() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let desc = BodyDesc::new(BodyKind::Dynamic, Vec3::ZERO, sphere(-1.0));
    assert_eq!(w.create_body(&desc).err(), Some(PhysicsError::InvalidShape));
    Ok(())
}

// ---- bounds ---------------------------------------------------------------------------------

fn bounds_of(shape: Shape3, position: Vec3, rotation: Quat) -> Aabb3 {
    D3::shape_bounds(&shape, position, rotation)
}

fn corners_match(b: Aabb3, min: Vec3, max: Vec3) -> bool {
    let close =
        |a: Vec3, c: Vec3| near(a.x, c.x, 1e-5) && near(a.y, c.y, 1e-5) && near(a.z, c.z, 1e-5);
    close(b.min, min) && close(b.max, max)
}

#[test]
fn a_sphere_is_bounded_by_its_radius_at_any_orientation() {
    for q in [Quat::IDENTITY, about_y(0.7)] {
        let b = bounds_of(sphere(2.0), Vec3::new(1.0, -3.0, 4.0), q);
        assert!(
            corners_match(b, Vec3::new(-1.0, -5.0, 2.0), Vec3::new(3.0, -1.0, 6.0)),
            "{b:?}"
        );
    }
}

#[test]
fn an_unturned_box_is_bounded_by_its_half_extents() {
    let b = bounds_of(
        cuboid(2.0, 1.0, 0.5),
        Vec3::new(10.0, 5.0, 0.0),
        Quat::IDENTITY,
    );
    assert!(
        corners_match(b, Vec3::new(8.0, 4.0, -0.5), Vec3::new(12.0, 6.0, 0.5)),
        "{b:?}"
    );
}

#[test]
fn a_box_turned_a_quarter_turn_about_y_swaps_x_and_z() {
    let b = bounds_of(cuboid(2.0, 1.0, 0.5), Vec3::ZERO, about_y(FRAC_PI_2));
    assert!(
        corners_match(b, Vec3::new(-0.5, -1.0, -2.0), Vec3::new(0.5, 1.0, 2.0)),
        "{b:?}"
    );
}

#[test]
fn a_box_turned_an_eighth_of_a_turn_grows_by_the_sum_of_its_extents() {
    let b = bounds_of(cuboid(2.0, 1.0, 0.5), Vec3::ZERO, about_y(FRAC_PI_4));
    let reach = 2.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        corners_match(
            b,
            Vec3::new(-reach, -1.0, -reach),
            Vec3::new(reach, 1.0, reach)
        ),
        "{b:?}"
    );
}

#[test]
fn a_capsule_is_bounded_along_its_turned_axis_plus_its_radius() {
    // Upright: the axis is Y.
    let b = bounds_of(capsule(1.0, 0.5), Vec3::ZERO, Quat::IDENTITY);
    assert!(
        corners_match(b, Vec3::new(-0.5, -1.5, -0.5), Vec3::new(0.5, 1.5, 0.5)),
        "{b:?}"
    );
    // Turned a quarter turn about Z: the axis is -X, and the extent along X is half_height + radius.
    let turned = about(Vec3::Z, FRAC_PI_2);
    let b = bounds_of(capsule(1.0, 0.5), Vec3::new(0.0, 2.0, 0.0), turned);
    assert!(
        corners_match(b, Vec3::new(-1.5, 1.5, -0.5), Vec3::new(1.5, 2.5, 0.5)),
        "{b:?}"
    );
}

#[test]
fn a_position_that_is_not_finite_or_overflows_gives_invalid_bounds() {
    let s = sphere(1.0);
    assert!(bounds_of(s, Vec3::ZERO, Quat::IDENTITY).is_valid());
    assert!(!bounds_of(s, Vec3::new(f32::NAN, 0.0, 0.0), Quat::IDENTITY).is_valid());
    assert!(!bounds_of(s, Vec3::new(0.0, f32::INFINITY, 0.0), Quat::IDENTITY).is_valid());
    assert!(!bounds_of(s, Vec3::new(0.0, 0.0, f32::NAN), Quat::IDENTITY).is_valid());
    let huge = sphere(3.0e38);
    assert!(!bounds_of(huge, Vec3::new(f32::MAX, 0.0, 0.0), Quat::IDENTITY).is_valid());
    assert!(!bounds_of(huge, Vec3::new(-f32::MAX, 0.0, 0.0), Quat::IDENTITY).is_valid());
}

// ---- the box type ---------------------------------------------------------------------------

fn aabb(min: [f32; 3], max: [f32; 3]) -> Aabb3 {
    Aabb3::new(
        Vec3::new(min[0], min[1], min[2]),
        Vec3::new(max[0], max[1], max[2]),
    )
}

#[test]
fn boxes_overlap_when_they_share_volume_or_touch_and_not_when_a_gap_exists_on_any_axis() {
    let a = aabb([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    assert!(a.overlaps(&aabb([0.5, 0.5, 0.5], [2.0, 2.0, 2.0])));
    assert!(
        a.overlaps(&aabb([1.0, 1.0, 1.0], [2.0, 2.0, 2.0])),
        "touching at a corner"
    );
    assert!(
        aabb([-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]).overlaps(&a),
        "contained"
    );
    assert!(!a.overlaps(&aabb([1.5, 0.0, 0.0], [2.0, 1.0, 1.0])));
    assert!(!a.overlaps(&aabb([0.0, 1.5, 0.0], [1.0, 2.0, 1.0])));
    assert!(!a.overlaps(&aabb([0.0, 0.0, 1.5], [1.0, 1.0, 2.0])));
    assert!(
        !aabb([1.5, 0.0, 0.0], [2.0, 1.0, 1.0]).overlaps(&a),
        "the test is symmetric"
    );
    assert!(!aabb([0.0, 1.5, 0.0], [1.0, 2.0, 1.0]).overlaps(&a));
    assert!(!aabb([0.0, 0.0, 1.5], [1.0, 1.0, 2.0]).overlaps(&a));
}

#[test]
fn union_contains_and_fattened_follow_their_definitions() {
    let a = aabb([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    let b = aabb([-1.0, 0.5, 2.0], [0.5, 3.0, 4.0]);
    assert_eq!(a.union(&b), aabb([-1.0, 0.0, 0.0], [1.0, 3.0, 4.0]));
    assert!(a.union(&b).contains(&a) && a.union(&b).contains(&b));
    assert!(!a.contains(&b));
    assert!(a.contains(&a), "a box contains itself");
    assert!(!a.contains(&aabb([0.0, 0.0, 0.0], [1.0, 1.0, 1.5])));
    assert!(!a.contains(&aabb([0.0, 0.0, -0.5], [1.0, 1.0, 1.0])));
    assert!(!a.contains(&aabb([0.0, -0.5, 0.0], [1.0, 1.0, 1.0])));
    assert!(!a.contains(&aabb([-0.5, 0.0, 0.0], [1.0, 1.0, 1.0])));
    assert!(!a.contains(&aabb([0.0, 0.0, 0.0], [1.5, 1.0, 1.0])));
    assert!(!a.contains(&aabb([0.0, 0.0, 0.0], [1.0, 1.5, 1.0])));
    assert_eq!(a.fattened(0.5), aabb([-0.5, -0.5, -0.5], [1.5, 1.5, 1.5]));
}

#[test]
fn the_measure_is_the_surface_area() {
    // 2 (xy + yz + zx) for 2 x 3 x 4 is 52, wherever the box is.
    for (min, max) in [
        ([0.0, 0.0, 0.0], [2.0, 3.0, 4.0]),
        ([5.0, -2.0, 1.0], [7.0, 1.0, 5.0]),
    ] {
        assert!(near(aabb(min, max).measure(), 52.0, 1e-6));
    }
}

#[test]
fn a_box_is_valid_when_finite_and_not_inverted() {
    assert!(aabb([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).is_valid());
    assert!(aabb([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]).is_valid(), "a point");
    assert!(!aabb([1.0, 0.0, 0.0], [0.0, 1.0, 1.0]).is_valid());
    assert!(!aabb([0.0, 1.0, 0.0], [1.0, 0.0, 1.0]).is_valid());
    assert!(!aabb([0.0, 0.0, 1.0], [1.0, 1.0, 0.0]).is_valid());
    for bad in [f32::NAN, f32::INFINITY] {
        assert!(!aabb([bad, 0.0, 0.0], [1.0, 1.0, 1.0]).is_valid());
        assert!(!aabb([0.0, bad, 0.0], [1.0, 1.0, 1.0]).is_valid());
        assert!(!aabb([0.0, 0.0, bad], [1.0, 1.0, 1.0]).is_valid());
        assert!(!aabb([0.0, 0.0, 0.0], [bad, 1.0, 1.0]).is_valid());
        assert!(!aabb([0.0, 0.0, 0.0], [1.0, bad, 1.0]).is_valid());
        assert!(!aabb([0.0, 0.0, 0.0], [1.0, 1.0, bad]).is_valid());
    }
}

// ---- broadphase on the shared tree ------------------------------------------------------------

#[test]
fn the_shared_broadphase_pairs_two_overlapping_3d_bodies_and_not_distant_ones()
-> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let ball =
        |x: f32, y: f32, z: f32| BodyDesc::new(BodyKind::Dynamic, Vec3::new(x, y, z), sphere(1.0));
    let a = w.create_body(&ball(0.0, 0.0, 0.0))?;
    let b = w.create_body(&ball(1.5, 0.0, 1.0))?;
    let far = w.create_body(&ball(0.0, 40.0, 0.0))?;
    w.step(DT)?;
    let pairs = w.pairs();
    let has = |x, y| {
        pairs
            .as_slice()
            .iter()
            .any(|p| (p.first == x && p.second == y) || (p.first == y && p.second == x))
    };
    assert!(has(a, b));
    assert!(!has(a, far) && !has(b, far));
    Ok(())
}

// ---- the two operations of the trait, directly ----------------------------------------------

#[test]
fn inverse_inertia_is_applied_in_the_body_frame_and_returned_in_the_world_frame() {
    let inv = Vec3::new(1.0, 2.0, 4.0);
    let torque = Vec3::new(1.0, 1.0, 1.0);
    assert_eq!(D3::apply_inverse_inertia(Quat::IDENTITY, inv, torque), inv);
    // Turned a quarter turn about Z the body's Y axis lies along world -X: a world torque along X
    // is a body torque along -Y, scaled by inv.y and brought back.
    let turned = about(Vec3::Z, FRAC_PI_2);
    let got = D3::apply_inverse_inertia(turned, inv, Vec3::X);
    assert!(
        near(got.x, 2.0, 1e-5) && got.y.abs() < 1e-5 && got.z.abs() < 1e-5,
        "{got:?}"
    );
}

#[test]
fn a_rotation_that_cannot_be_normalised_is_left_as_it_was() {
    let q = about_y(0.4);
    let kept = D3::advance_rotation(q, Vec3::new(f32::NAN, 0.0, 0.0), 0.01);
    assert_eq!(kept, q);
    let moved = D3::advance_rotation(q, Vec3::new(0.0, 1.0, 0.0), 0.01);
    assert_ne!(moved, q);
    assert!((moved.length() - 1.0).abs() < 1e-6);
}

#[test]
fn a_spin_about_a_skew_axis_turns_every_local_axis_like_the_exact_rotation()
-> Result<(), PhysicsError> {
    let spin = Vec3::new(1.0, 2.0, -2.0); // |spin| = 3 rad/s
    let mut w = world(Vec3::ZERO)?;
    let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec3::ZERO, cuboid(1.0, 1.0, 1.0));
    desc.angular_velocity = spin;
    desc.rotation = about(Vec3::new(0.0, 0.0, 1.0), 0.5);
    let handle = w.create_body(&desc)?;
    for _ in 0..1000 {
        w.step(DT)?;
    }
    // One second at 3 rad/s about the axis `spin / 3`, after the starting turn about Z.
    let exact = about(Vec3::new(1.0 / 3.0, 2.0 / 3.0, -2.0 / 3.0), 3.0) * about(Vec3::Z, 0.5);
    let got = w.body(handle)?.rotation();
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        let (a, b) = (got.rotate(axis), exact.rotate(axis));
        assert!((a - b).length() < 1e-3, "{axis:?}: {a:?} against {b:?}");
    }
    Ok(())
}

#[test]
fn a_body_whose_orientation_is_not_finite_is_refused() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec3::ZERO, sphere(1.0));
    desc.rotation = Quat::new(0.0, f32::NAN, 0.0, 1.0);
    assert_eq!(w.create_body(&desc).err(), Some(PhysicsError::NotFinite));
    Ok(())
}

#[test]
fn standard_gravity_points_down_the_y_axis() {
    assert_eq!(STANDARD_GRAVITY, Vec3::new(0.0, -9.806_65, 0.0));
}

fn bits<T: HashBits>(value: &T) -> u64 {
    let mut hasher = StateHasher::new();
    value.write_bits(&mut hasher);
    hasher.finish()
}

#[test]
fn the_hash_of_a_shape_and_a_box_sees_every_field() {
    let shapes = [
        sphere(1.0),
        sphere(2.0),
        cuboid(1.0, 2.0, 3.0),
        cuboid(2.0, 1.0, 3.0),
        cuboid(1.0, 3.0, 2.0),
        cuboid(3.0, 2.0, 1.0),
        capsule(1.0, 2.0),
        capsule(2.0, 1.0),
    ];
    for (i, a) in shapes.iter().enumerate() {
        for b in &shapes[i + 1..] {
            assert_ne!(bits(a), bits(b), "{a:?} and {b:?}");
        }
    }
    let base = aabb([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    for other in [
        aabb([0.5, 0.0, 0.0], [1.0, 1.0, 1.0]),
        aabb([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]),
        aabb([0.0, 0.0, 0.5], [1.0, 1.0, 1.0]),
        aabb([0.0, 0.0, 0.0], [1.5, 1.0, 1.0]),
        aabb([0.0, 0.0, 0.0], [1.0, 1.5, 1.0]),
        aabb([0.0, 0.0, 0.0], [1.0, 1.0, 1.5]),
    ] {
        assert_ne!(bits(&base), bits(&other), "{other:?}");
    }
    for other in [
        Quat::new(0.1, 0.0, 0.0, 1.0),
        Quat::new(0.0, 0.1, 0.0, 1.0),
        Quat::new(0.0, 0.0, 0.1, 1.0),
        Quat::new(0.0, 0.0, 0.0, 0.9),
    ] {
        assert_ne!(bits(&Quat::IDENTITY), bits(&other), "{other:?}");
    }
}

#[test]
fn bounds_of_a_turned_box_and_capsule_match_the_extreme_points() {
    let q = about(Vec3::new(1.0, 2.0, -2.0), 0.9);
    let position = Vec3::new(1.0, -2.0, 3.0);
    // A box: the eight corners.
    let h = Vec3::new(1.5, 2.5, 3.5);
    let mut lo = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut hi = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for corner in 0..8_u32 {
        let sign = |bit: u32| if corner >> bit & 1 == 1 { 1.0 } else { -1.0 };
        let c = position + q.rotate(Vec3::new(sign(0) * h.x, sign(1) * h.y, sign(2) * h.z));
        lo = lo.min(c);
        hi = hi.max(c);
    }
    let b = bounds_of(cuboid(h.x, h.y, h.z), position, q);
    assert!(corners_match(b, lo, hi), "{b:?} against {lo:?} {hi:?}");
    // A capsule: the two cap centres, each pushed out by the radius on every axis.
    let (half_height, radius) = (1.5, 0.75);
    let tip = q.rotate(Vec3::new(0.0, half_height, 0.0));
    let r = Vec3::new(radius, radius, radius);
    let (lo, hi) = (
        (position + tip).min(position - tip) - r,
        (position + tip).max(position - tip) + r,
    );
    let b = bounds_of(capsule(half_height, radius), position, q);
    assert!(corners_match(b, lo, hi), "{b:?} against {lo:?} {hi:?}");
}

#[test]
fn a_sphere_and_a_capsule_with_non_unit_sizes_have_the_closed_form_mass() {
    let (m, i) = valid(sphere(1.5), 2.0);
    let mass = 2.0 * 4.0 / 3.0 * PI * 1.5 * 1.5 * 1.5;
    assert!(near(m, mass, 1e-5) && near(i.x, 0.4 * mass * 2.25, 1e-5));
    let (h, r) = (1.5_f32, 0.75_f32);
    let (m, _) = valid(capsule(h, r), 2.0);
    let expected = 2.0 * (PI * r * r * 2.0 * h + 4.0 / 3.0 * PI * r * r * r);
    assert!(near(m, expected, 1e-5));
}

#[test]
fn an_orientation_must_be_close_to_unit_length() {
    let created = |rotation: Quat| -> Result<(), PhysicsError> {
        let Ok(mut w) = world(Vec3::ZERO) else {
            panic!("a world is built");
        };
        let mut desc = BodyDesc::new(BodyKind::Static, Vec3::ZERO, sphere(1.0));
        desc.rotation = rotation;
        w.create_body(&desc).map(|_| ())
    };
    let scaled = |length_squared: f32| Quat::new(0.0, 0.0, 0.0, length_squared.sqrt());
    assert_eq!(created(Quat::IDENTITY), Ok(()));
    assert_eq!(
        created(scaled(1.0 + 5.0e-5)),
        Ok(()),
        "inside the tolerance"
    );
    assert_eq!(
        created(scaled(1.0 - 5.0e-5)),
        Ok(()),
        "inside the tolerance"
    );
    assert_eq!(created(scaled(1.0 + 3.0e-4)), Err(PhysicsError::NotFinite));
    assert_eq!(created(scaled(1.0 - 3.0e-4)), Err(PhysicsError::NotFinite));
    assert_eq!(
        created(Quat::new(0.0, 0.0, 0.0, 0.0)),
        Err(PhysicsError::NotFinite)
    );
    assert_eq!(
        created(Quat::new(0.0, 0.0, 0.0, 2.0)),
        Err(PhysicsError::NotFinite)
    );
}

/// Moments of a capsule from slices along its axis, a reference that does not use the closed form.
fn sliced_capsule(half_height: f64, radius: f64, density: f64) -> (f64, f64, f64) {
    let slices = 400_000_u32;
    let length = 2.0 * (half_height + radius);
    let dy = length / f64::from(slices);
    let (mut mass, mut along, mut across) = (0.0, 0.0, 0.0);
    for i in 0..slices {
        let y = -(half_height + radius) + (f64::from(i) + 0.5) * dy;
        let beyond = (y.abs() - half_height).max(0.0);
        let rho2 = radius * radius - beyond * beyond;
        let dm = density * std::f64::consts::PI * rho2 * dy;
        mass += dm;
        along += dm * rho2 / 2.0;
        across += dm * (rho2 / 4.0 + y * y);
    }
    (mass, along, across)
}

#[test]
fn a_capsule_matches_a_numerical_integration_of_its_slices() {
    for (h, r) in [(1.5_f32, 0.75_f32), (0.2, 0.5), (3.0, 0.1)] {
        let (m, i) = valid(capsule(h, r), 2.0);
        let (rm, ralong, racross) = sliced_capsule(f64::from(h), f64::from(r), 2.0);
        let close = |a: f32, b: f64| (f64::from(a) - b).abs() <= 1e-4 * b;
        assert!(close(m, rm), "mass {m} against {rm}");
        assert!(close(i.y, ralong), "axis {} against {ralong}", i.y);
        assert!(close(i.x, racross), "across {} against {racross}", i.x);
        assert!(close(i.z, racross), "across {} against {racross}", i.z);
    }
}
