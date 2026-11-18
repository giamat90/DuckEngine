//! Card 8-6: the 3D contact solver. Bodies in contact push each other apart, rest on each other,
//! rub, bounce and fall asleep. The scenes are small and the expected values come from the
//! physics, not from the code under test.

use base::dmath::{cos, sin};
use base::math::{Quat, Vec3};
use physics::core::PhysicsError;
use physics::d3::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
};

/// The fixed step of every scene: 60 Hz.
const DT: f32 = 1.0 / 60.0;

fn world_with(gravity: Vec3, sleep: SleepConfig) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 64,
        gravity,
        max_pairs: 1024,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 1024,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    })
}

fn world(gravity: Vec3) -> Result<World, PhysicsError> {
    world_with(gravity, SleepConfig::DISABLED)
}

fn cuboid(x: f32, y: f32, z: f32) -> Shape3 {
    Shape3::Box {
        half_extents: Vec3::new(x, y, z),
    }
}

/// A static box whose top face is the plane `y = 0`, 100 m on each side.
fn floor(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        cuboid(50.0, 1.0, 50.0),
    ))
}

fn dynamic(
    world: &mut World,
    position: Vec3,
    shape: &Shape3,
    configure: impl FnOnce(&mut BodyDesc),
) -> Result<BodyHandle, PhysicsError> {
    let mut desc = BodyDesc::new(BodyKind::Dynamic, position, *shape);
    configure(&mut desc);
    world.create_body(&desc)
}

fn run(world: &mut World, steps: u32) -> Result<(), PhysicsError> {
    for _ in 0..steps {
        world.step(DT)?;
    }
    Ok(())
}

fn position(world: &World, handle: BodyHandle) -> Result<Vec3, PhysicsError> {
    Ok(world.body(handle)?.position())
}

fn velocity(world: &World, handle: BodyHandle) -> Result<Vec3, PhysicsError> {
    Ok(world.body(handle)?.linear_velocity())
}

// ---- resting ------------------------------------------------------------------------------

#[test]
fn a_box_dropped_on_the_floor_comes_to_rest_on_it() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let crate_ = dynamic(
        &mut w,
        Vec3::new(0.0, 2.0, 0.0),
        &cuboid(0.5, 0.5, 0.5),
        |_| {},
    )?;
    run(&mut w, 240)?;
    let at = position(&w, crate_)?;
    assert!(
        (at.y - 0.5).abs() < 5.0e-3,
        "a box of half size 0.5 rests with its centre at 0.5, not {}",
        at.y
    );
    assert!(
        at.x.abs() < 1.0e-3 && at.z.abs() < 1.0e-3,
        "it stays above where it fell: {at:?}"
    );
    assert!(velocity(&w, crate_)?.length() < 1.0e-2);
    Ok(())
}

#[test]
fn a_sphere_and_a_capsule_rest_on_the_floor() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let ball = dynamic(
        &mut w,
        Vec3::new(-2.0, 1.5, 0.0),
        &Shape3::Sphere { radius: 0.4 },
        |_| {},
    )?;
    let pill = dynamic(
        &mut w,
        Vec3::new(2.0, 1.5, 0.0),
        &Shape3::Capsule {
            half_height: 0.5,
            radius: 0.25,
        },
        |d| {
            d.rotation = Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2)
                .unwrap_or(Quat::IDENTITY);
        },
    )?;
    run(&mut w, 300)?;
    assert!((position(&w, ball)?.y - 0.4).abs() < 5.0e-3);
    // Lying on its side, a capsule rests at its radius above the floor.
    assert!((position(&w, pill)?.y - 0.25).abs() < 5.0e-3);
    Ok(())
}

// ---- collisions ---------------------------------------------------------------------------

#[test]
fn two_equal_spheres_in_an_elastic_head_on_hit_swap_their_velocities() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let elastic = |d: &mut BodyDesc| d.restitution = 1.0;
    let left = dynamic(
        &mut w,
        Vec3::new(-2.0, 0.0, 0.0),
        &Shape3::Sphere { radius: 0.5 },
        |d| {
            elastic(d);
            d.linear_velocity = Vec3::new(5.0, 0.0, 0.0);
        },
    )?;
    let right = dynamic(
        &mut w,
        Vec3::new(2.0, 0.0, 0.0),
        &Shape3::Sphere { radius: 0.5 },
        elastic,
    )?;
    run(&mut w, 90)?;
    let (a, b) = (velocity(&w, left)?, velocity(&w, right)?);
    assert!(a.length() < 0.05, "the first one stops, it moves at {a:?}");
    assert!(
        (b.x - 5.0).abs() < 0.05,
        "the second one leaves at 5 m/s, not {b:?}"
    );
    Ok(())
}

#[test]
fn an_inelastic_hit_leaves_no_approach_speed_and_keeps_the_momentum() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let left = dynamic(
        &mut w,
        Vec3::new(-2.0, 0.0, 0.0),
        &Shape3::Sphere { radius: 0.5 },
        |d| {
            d.restitution = 0.0;
            d.linear_velocity = Vec3::new(4.0, 0.0, 0.0);
        },
    )?;
    let right = dynamic(
        &mut w,
        Vec3::new(2.0, 0.0, 0.0),
        &Shape3::Sphere { radius: 0.5 },
        |d| {
            d.restitution = 0.0;
        },
    )?;
    run(&mut w, 120)?;
    let (a, b) = (velocity(&w, left)?, velocity(&w, right)?);
    assert!((a.x - b.x).abs() < 0.05, "they move together: {a:?} {b:?}");
    assert!(
        ((a.x + b.x) * 0.5 - 2.0).abs() < 0.05,
        "half the speed, the momentum is kept"
    );
    Ok(())
}

#[test]
fn a_dropped_ball_bounces_back_up_to_its_restitution() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let ball = dynamic(
        &mut w,
        Vec3::new(0.0, 5.0, 0.0),
        &Shape3::Sphere { radius: 0.5 },
        |d| {
            d.restitution = 0.5;
        },
    )?;
    let mut fastest_up = 0.0_f32;
    for _ in 0..180 {
        w.step(DT)?;
        fastest_up = fastest_up.max(velocity(&w, ball)?.y);
    }
    // It lands at sqrt(2 g h) = about 9 m/s and leaves at half that.
    let landing = (2.0 * 9.806_65_f32 * 4.5).sqrt();
    assert!(
        (fastest_up - 0.5 * landing).abs() < 0.5,
        "leaves at {fastest_up}, wanted about {}",
        0.5 * landing
    );
    Ok(())
}

// ---- friction -----------------------------------------------------------------------------

/// A box on the floor under gravity tilted by `angle` radians: the same as a box on a slope.
fn box_on_a_slope(angle: f32, friction: f32, seconds: u32) -> Result<f32, PhysicsError> {
    let g = 9.806_65_f32;
    let mut w = world(Vec3::new(g * sin(angle), -g * cos(angle), 0.0))?;
    w.create_body(&{
        let mut desc = BodyDesc::new(
            BodyKind::Static,
            Vec3::new(0.0, -1.0, 0.0),
            cuboid(50.0, 1.0, 50.0),
        );
        desc.friction = friction;
        desc
    })?;
    let crate_ = dynamic(
        &mut w,
        Vec3::new(0.0, 0.5, 0.0),
        &cuboid(0.5, 0.5, 0.5),
        |d| {
            d.friction = friction;
        },
    )?;
    run(&mut w, 60 * seconds)?;
    Ok(position(&w, crate_)?.x)
}

#[test]
fn friction_holds_a_box_on_a_slope_below_the_friction_angle() -> Result<(), PhysicsError> {
    // tan 20 degrees is 0.36, under the coefficient 0.5.
    let slid = box_on_a_slope(20.0_f32.to_radians(), 0.5, 3)?;
    assert!(slid.abs() < 0.01, "the box slid {slid} m");
    Ok(())
}

#[test]
fn a_box_slides_down_a_steep_slope_at_the_acceleration_friction_leaves() -> Result<(), PhysicsError>
{
    let angle = 35.0_f32.to_radians();
    let slid = box_on_a_slope(angle, 0.5, 2)?;
    // a = g (sin - mu cos), x = a t^2 / 2.
    let wanted = 0.5 * 9.806_65 * (sin(angle) - 0.5 * cos(angle)) * 4.0;
    assert!(
        (slid - wanted).abs() < 0.15 * wanted,
        "slid {slid} m, wanted about {wanted} m"
    );
    Ok(())
}

#[test]
fn without_friction_a_box_slides_at_gravity_along_the_slope() -> Result<(), PhysicsError> {
    let angle = 10.0_f32.to_radians();
    let slid = box_on_a_slope(angle, 0.0, 2)?;
    let wanted = 0.5 * 9.806_65 * sin(angle) * 4.0;
    assert!(
        (slid - wanted).abs() < 0.05 * wanted,
        "slid {slid} m, wanted about {wanted} m"
    );
    Ok(())
}

#[test]
fn friction_works_across_the_other_horizontal_axis_too() -> Result<(), PhysicsError> {
    // Gravity tilted along z instead of x: the tangent frame must not favour an axis.
    let g = 9.806_65_f32;
    let angle = 20.0_f32.to_radians();
    let mut w = world(Vec3::new(0.0, -g * cos(angle), g * sin(angle)))?;
    floor(&mut w)?;
    let crate_ = dynamic(
        &mut w,
        Vec3::new(0.0, 0.5, 0.0),
        &cuboid(0.5, 0.5, 0.5),
        |_| {},
    )?;
    run(&mut w, 180)?;
    assert!(position(&w, crate_)?.z.abs() < 0.01);
    Ok(())
}

// ---- stacks and orientation ---------------------------------------------------------------

fn stack(count: u8, seconds: u32) -> Result<(World, Vec<BodyHandle>), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let mut boxes = Vec::new();
    for level in 0..count {
        let height = 0.5 + f32::from(level) + 0.001;
        boxes.push(dynamic(
            &mut w,
            Vec3::new(0.0, height, 0.0),
            &cuboid(0.5, 0.5, 0.5),
            |_| {},
        )?);
    }
    run(&mut w, 60 * seconds)?;
    Ok((w, boxes))
}

/// `sink` and `sideways` are how far a box may be from its ideal place, up or down and across.
fn stays_in_a_column(
    w: &World,
    boxes: &[BodyHandle],
    (sink, sideways): (f32, f32),
) -> Result<(), PhysicsError> {
    for (handle, level) in boxes.iter().zip(0_u8..) {
        let at = position(w, *handle)?;
        let ideal = 0.5 + f32::from(level);
        assert!(
            (at.y - ideal).abs() < sink && at.x.abs() < sideways && at.z.abs() < sideways,
            "box {level} is at {at:?}, wanted about (0, {ideal}, 0)"
        );
        assert!(
            velocity(w, *handle)?.length() < 0.05,
            "box {level} still moves"
        );
    }
    Ok(())
}

#[test]
fn a_stack_of_five_boxes_stands() -> Result<(), PhysicsError> {
    let (w, boxes) = stack(5, 4)?;
    stays_in_a_column(&w, &boxes, (0.02, 0.02))
}

#[test]
fn a_stack_of_ten_boxes_stands() -> Result<(), PhysicsError> {
    let (w, boxes) = stack(10, 5)?;
    // Measured: the top box is 3.6 cm low and 9 mm off the axis (a fixed offset from the order the
    // four points of a face are solved in); the limits leave a margin of about a third.
    stays_in_a_column(&w, &boxes, (0.045, 0.015))
}

#[test]
fn a_tilted_box_dropped_on_the_floor_ends_up_lying_flat() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let tilt = Quat::from_axis_angle(Vec3::new(1.0, 0.0, 1.0), 0.5).unwrap_or(Quat::IDENTITY);
    let crate_ = dynamic(
        &mut w,
        Vec3::new(0.0, 1.5, 0.0),
        &cuboid(0.5, 0.5, 0.5),
        |d| {
            d.rotation = tilt;
        },
    )?;
    run(&mut w, 360)?;
    let up = w.body(crate_)?.rotation().rotate(Vec3::Y);
    assert!(up.dot(Vec3::Y) > 0.999, "its up axis is {up:?}");
    assert!((position(&w, crate_)?.y - 0.5).abs() < 0.01);
    Ok(())
}

// ---- conservation -------------------------------------------------------------------------

fn momenta(w: &World, handles: &[BodyHandle]) -> Result<(Vec3, Vec3), PhysicsError> {
    let (mut linear, mut angular) = (Vec3::ZERO, Vec3::ZERO);
    for handle in handles {
        let body = w.body(*handle)?;
        let mass = body.mass();
        let moment = 0.4 * mass * 0.25; // a sphere of radius 0.5
        linear += body.linear_velocity() * mass;
        angular +=
            body.position().cross(body.linear_velocity() * mass) + body.angular_velocity() * moment;
    }
    Ok((linear, angular))
}

#[test]
fn a_glancing_hit_with_friction_keeps_linear_and_angular_momentum() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let first = dynamic(
        &mut w,
        Vec3::new(-2.0, 0.0, 0.3),
        &Shape3::Sphere { radius: 0.5 },
        |d| {
            d.linear_velocity = Vec3::new(4.0, 0.0, 0.0);
            d.friction = 0.6;
        },
    )?;
    let second = dynamic(
        &mut w,
        Vec3::new(0.0, 0.0, -0.3),
        &Shape3::Sphere { radius: 0.5 },
        |d| {
            d.friction = 0.6;
        },
    )?;
    let handles = [first, second];
    let (linear, angular) = momenta(&w, &handles)?;
    run(&mut w, 120)?;
    let (linear_after, angular_after) = momenta(&w, &handles)?;
    assert!(
        (linear_after - linear).length() < 1.0e-3 * linear.length().max(1.0),
        "{linear:?} -> {linear_after:?}"
    );
    assert!(
        (angular_after - angular).length() < 2.0e-2 * angular.length().max(1.0),
        "{angular:?} -> {angular_after:?}"
    );
    // And they did spin: friction acted across the normal.
    assert!(w.body(second)?.angular_velocity().length() > 0.1);
    Ok(())
}

// ---- sleeping -----------------------------------------------------------------------------

#[test]
fn a_resting_box_falls_asleep_and_its_contact_is_not_solved_again() -> Result<(), PhysicsError> {
    let mut w = world_with(STANDARD_GRAVITY, SleepConfig::DEFAULT)?;
    floor(&mut w)?;
    let crate_ = dynamic(
        &mut w,
        Vec3::new(0.0, 0.52, 0.0),
        &cuboid(0.5, 0.5, 0.5),
        |_| {},
    )?;
    run(&mut w, 240)?;
    assert!(w.is_asleep(crate_)?);
    let stats = w.solver_stats();
    assert_eq!((stats.solved, stats.frozen), (0, 1), "{stats:?}");
    Ok(())
}

#[test]
fn a_sleeping_box_does_not_change_a_single_bit_while_it_sleeps() -> Result<(), PhysicsError> {
    let mut w = world_with(STANDARD_GRAVITY, SleepConfig::DEFAULT)?;
    floor(&mut w)?;
    let crate_ = dynamic(
        &mut w,
        Vec3::new(0.0, 0.52, 0.0),
        &cuboid(0.5, 0.5, 0.5),
        |_| {},
    )?;
    run(&mut w, 240)?;
    assert!(w.is_asleep(crate_)?);
    let hash = w.state_hash();
    run(&mut w, 30)?;
    assert!(w.is_asleep(crate_)?);
    assert_eq!(w.state_hash(), hash);
    Ok(())
}

#[test]
fn a_ball_dropped_on_a_sleeping_box_wakes_it() -> Result<(), PhysicsError> {
    let mut w = world_with(STANDARD_GRAVITY, SleepConfig::DEFAULT)?;
    floor(&mut w)?;
    let crate_ = dynamic(
        &mut w,
        Vec3::new(0.0, 0.52, 0.0),
        &cuboid(0.5, 0.5, 0.5),
        |_| {},
    )?;
    run(&mut w, 240)?;
    assert!(w.is_asleep(crate_)?);
    dynamic(
        &mut w,
        Vec3::new(0.0, 2.0, 0.0),
        &Shape3::Sphere { radius: 0.2 },
        |_| {},
    )?;
    // The ball lands within half a second; box and ball may sleep again a while after.
    let mut woke = false;
    for _ in 0..60 {
        w.step(DT)?;
        woke |= !w.is_asleep(crate_)?;
    }
    assert!(woke, "the sleeping box never woke");
    Ok(())
}

// ---- shapes, determinism, robustness ------------------------------------------------------

/// Plus or minus a half, by whether `bit` of `corner` is set.
fn half(corner: u32, bit: u32) -> f32 {
    if corner >> bit & 1 == 1 { 0.5 } else { -0.5 }
}

fn hull_cube() -> Result<Shape3, PhysicsError> {
    let corners: Vec<Vec3> = (0..8)
        .map(|n| Vec3::new(half(n, 0), half(n, 1), half(n, 2)))
        .collect();
    Ok(Shape3::Hull(physics::d3::Hull::new(&corners)?))
}

#[test]
fn a_hull_dropped_on_the_floor_does_not_fall_through_it() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let piece = dynamic(&mut w, Vec3::new(0.0, 2.0, 0.0), &hull_cube()?, |_| {})?;
    run(&mut w, 240)?;
    let at = position(&w, piece)?;
    assert!(at.is_finite_vec() && at.y > 0.45, "the hull is at {at:?}");
    Ok(())
}

trait FiniteVec {
    fn is_finite_vec(&self) -> bool;
}

impl FiniteVec for Vec3 {
    fn is_finite_vec(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

fn pile(steps: u32) -> Result<u64, PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let shapes = [
        cuboid(0.5, 0.3, 0.4),
        Shape3::Sphere { radius: 0.4 },
        Shape3::Capsule {
            half_height: 0.4,
            radius: 0.2,
        },
        hull_cube()?,
    ];
    for (shape, n) in shapes.iter().cycle().take(12).zip(0_u8..) {
        let (x, layer) = (f32::from(n % 3) * 0.9 - 0.9, f32::from(n / 3));
        dynamic(
            &mut w,
            Vec3::new(x, 1.0 + layer * 1.2, 0.1 * f32::from(n)),
            shape,
            |d| {
                d.angular_velocity = Vec3::new(0.3, 0.2, 0.1);
            },
        )?;
    }
    run(&mut w, steps)?;
    Ok(w.state_hash())
}

#[test]
fn the_same_pile_run_twice_has_the_same_state_hash() -> Result<(), PhysicsError> {
    assert_eq!(pile(240)?, pile(240)?);
    assert_ne!(pile(240)?, pile(120)?);
    Ok(())
}

#[test]
fn a_very_fast_ball_does_not_tunnel_or_make_a_number_that_is_not_finite() -> Result<(), PhysicsError>
{
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let ball = dynamic(
        &mut w,
        Vec3::new(0.0, 3.0, 0.0),
        &Shape3::Sphere { radius: 0.5 },
        |d| {
            d.linear_velocity = Vec3::new(0.0, -200.0, 0.0);
        },
    )?;
    run(&mut w, 120)?;
    let at = position(&w, ball)?;
    assert!(at.is_finite_vec() && at.y > 0.0, "the ball is at {at:?}");
    Ok(())
}
