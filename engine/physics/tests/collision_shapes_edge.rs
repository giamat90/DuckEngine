//! Card 8-51: the speed edge on the scenes that cards 8-47 to 8-49 did not try: capsules and hulls,
//! hits off the line of centres, rows of up to eight, the other axes of spin, friction, and the
//! opt-in clamp of card 8-50. Measured on 2026-11-20 in 3D (2D has circles and boxes only, which
//! the earlier files cover) and pinned at a few points as observations, not promises.
//!
//! What the numbers say: a hit off the line of centres gives the light body a spin (impulse times
//! lever arm over its small moment of inertia) that reaches the spin bound long before the speed
//! bound; boxes, capsules and hulls are refused from about a thousand metres per second upward,
//! spheres hardly ever; and a world that clamps steps every scene that is refused by default.
//! Card 8-93: the far-offset hull cell flips with the size of the cube's corners (edge 1050 or
//! 1350), so its pin is asserted over sizes, and no longer says that the hull is refused earlier
//! than the box.

use base::math::Vec3;
use physics::core::PhysicsError;
use physics::d3;

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 30;
/// The limits of the clamping world: the linear one is the largest speed a body keeps.
const CLAMP_LINEAR: f32 = 500.0;
const CLAMP_ANGULAR: f32 = 50.0;
const SHAPES: [Form; 4] = [Form::Sphere, Form::Box, Form::Capsule, Form::Hull];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Form {
    Sphere,
    Box,
    Capsule,
    Hull,
}

fn shape(form: Form) -> Result<d3::Shape3, PhysicsError> {
    shape_scaled(form, 1.0)
}

/// The shape of `form`; the corners of the hull are multiplied by `scale` (card 8-93).
fn shape_scaled(form: Form, scale: f32) -> Result<d3::Shape3, PhysicsError> {
    Ok(match form {
        Form::Sphere => d3::Shape3::Sphere { radius: 0.5 },
        Form::Box => d3::Shape3::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        },
        Form::Capsule => d3::Shape3::Capsule {
            half_height: 0.5,
            radius: 0.5,
        },
        Form::Hull => d3::Shape3::Hull(d3::Hull::new(&cube_corners(scale))?),
    })
}

/// The eight corners of the unit cube, multiplied by `scale`, in the order x, y, z nested (x slowest).
fn cube_corners(scale: f32) -> Vec<Vec3> {
    (0..8_u8)
        .map(|n| {
            let side = |bit: u8| if n & bit == 0 { -0.5 } else { 0.5 };
            Vec3::new(side(4), side(2), side(1)) * scale
        })
        .collect()
}

fn world(max_bodies: u32, clamp: bool) -> Result<d3::World, PhysicsError> {
    let mut world = d3::World::new(d3::WorldConfig {
        max_bodies,
        gravity: Vec3::ZERO,
        max_pairs: 256,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })?;
    if clamp {
        world.set_speed_policy(d3::SpeedPolicy::Clamp {
            linear: CLAMP_LINEAR,
            angular: CLAMP_ANGULAR,
        })?;
    }
    Ok(world)
}

/// A heavy body thrown along x at a light one at rest, the light one `offset` metres off the line
/// of centres; the heavy one spins by `spin`; `clamp` makes the world clamp.
#[derive(Clone, Copy)]
struct Throw {
    form: Form,
    ratio: f32,
    speed: f32,
    offset: f32,
    spin: Vec3,
    clamp: bool,
    friction: f32,
    /// Size of the corners of a hull (1 unless a test asks for a variant; the other forms ignore it).
    scale: f32,
}

impl Throw {
    fn new(form: Form, ratio: f32, speed: f32) -> Self {
        Self {
            form,
            ratio,
            speed,
            offset: 0.0,
            spin: Vec3::ZERO,
            clamp: false,
            friction: 0.5,
            scale: 1.0,
        }
    }

    fn steps(self) -> Result<bool, PhysicsError> {
        let mut world = world(4, self.clamp)?;
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(self.speed, 0.0, 0.0),
            angular_velocity: self.spin,
            density: self.ratio,
            friction: self.friction,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::ZERO,
                shape_scaled(self.form, self.scale)?,
            )
        })?;
        world.create_body(&d3::BodyDesc {
            friction: self.friction,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(0.9, self.offset, 0.0),
                shape_scaled(self.form, self.scale)?,
            )
        })?;
        Ok((0..STEPS).all(|_| world.step(DT).is_ok()))
    }
}

/// A row of `links` bodies of one form, each next `ratio` times lighter and at rest, the first
/// moving along x at `speed`. Whether the row steps 40 times, and the fastest body seen.
fn row(
    form: Form,
    links: u32,
    ratio: f32,
    speed: f32,
    clamp: bool,
) -> Result<(bool, f32), PhysicsError> {
    let mut world = world(links, clamp)?;
    let mut density = (1..links).fold(1.0_f32, |density, _| density * ratio);
    for link in 0..links {
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(if link == 0 { speed } else { 0.0 }, 0.0, 0.0),
            density,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(0.9 * f32::from(u8::try_from(link).unwrap_or(0)), 0.0, 0.0),
                shape(form)?,
            )
        })?;
        density /= ratio;
    }
    let mut peak = 0.0_f32;
    for _ in 0..40 {
        if world.step(DT).is_err() {
            return Ok((false, peak));
        }
        for (_, body) in world.iter() {
            peak = peak.max(body.linear_velocity().length());
        }
    }
    Ok((true, peak))
}

/// Whether the hit steps, for each of three offsets.
fn steps_at_every_offset(form: Form, ratio: f32, speed: f32) -> Result<bool, PhysicsError> {
    for offset in [0.2, 0.4, 0.6] {
        let hit = Throw {
            offset,
            ..Throw::new(form, ratio, speed)
        };
        if !hit.steps()? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[test]
fn a_hit_off_the_line_of_centres_is_not_refused_at_900_metres_per_second()
-> Result<(), PhysicsError> {
    for form in SHAPES {
        for ratio in [1.0, 10.0, 1000.0] {
            assert!(
                steps_at_every_offset(form, ratio, 900.0)?,
                "{form:?} ratio {ratio}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_box_hit_far_off_the_line_of_centres_is_refused_at_2500_with_no_spin_at_any_mass_ratio()
-> Result<(), PhysicsError> {
    for ratio in [1.0, 10.0, 1000.0] {
        let hit = Throw {
            offset: 0.6,
            ..Throw::new(Form::Box, ratio, 2500.0)
        };
        assert!(!hit.steps()?, "ratio {ratio}");
    }
    Ok(())
}

#[test]
fn a_sphere_is_not_refused_by_the_same_hits() -> Result<(), PhysicsError> {
    for offset in [0.2, 0.4, 0.6] {
        let hit = Throw {
            offset,
            ..Throw::new(Form::Sphere, 1000.0, 2500.0)
        };
        assert!(hit.steps()?, "offset {offset}");
    }
    Ok(())
}

#[test]
fn at_a_far_offset_and_ratio_1000_the_hull_steps_at_900_and_is_refused_at_1500_over_sizes_to_1e_3()
-> Result<(), PhysicsError> {
    // Edge of the hull in the scan (card 8-93, scales 1 +- 3e-6, 1e-4, 1e-3): 1050 as built and for
    // most variants, 1350 (the box's edge) for others, so a pin at 1200 flips with the size of the
    // corners. The box is at 1350 in the scan, so 1200 keeps its step; 1500 is 150 above the
    // highest edge seen.
    let hit = |form, scale, speed| Throw {
        offset: 0.6,
        scale,
        ..Throw::new(form, 1000.0, speed)
    };
    assert!(
        hit(Form::Box, 1.0, 1200.0).steps()?,
        "the box steps at 1200"
    );
    let scales = (-3..=3_i8)
        .map(|step| 1.0 + f32::from(step) * 1.0e-6)
        .chain([1.0 - 1.0e-4, 1.0 + 1.0e-4, 1.0 - 1.0e-3, 1.0 + 1.0e-3]);
    for scale in scales {
        assert!(
            hit(Form::Hull, scale, 900.0).steps()?,
            "the hull, scale {scale}, steps at 900"
        );
        assert!(
            !hit(Form::Hull, scale, 1500.0).steps()?,
            "the hull, scale {scale}, is refused at 1500"
        );
    }
    Ok(())
}

#[test]
fn only_the_spin_about_the_axis_of_the_hit_makes_a_box_refuse_at_5000() -> Result<(), PhysicsError>
{
    let spin = |axis: Vec3| Throw {
        spin: axis * 1000.0,
        ..Throw::new(Form::Box, 1000.0, 5000.0)
    };
    assert!(spin(Vec3::X).steps()?, "about x");
    assert!(spin(Vec3::Y).steps()?, "about y");
    assert!(!spin(Vec3::Z).steps()?, "about z");
    Ok(())
}

#[test]
fn friction_does_not_move_the_spin_at_which_a_box_is_refused_at_2500() -> Result<(), PhysicsError> {
    for friction in [0.0, 0.5, 1.0, 3.0] {
        let at = |spin: f32| Throw {
            spin: Vec3::new(0.0, 0.0, spin),
            friction,
            ..Throw::new(Form::Box, 1000.0, 2500.0)
        };
        assert!(at(700.0).steps()?, "friction {friction}, spin 700");
        assert!(!at(800.0).steps()?, "friction {friction}, spin 800");
    }
    Ok(())
}

#[test]
fn rows_of_spheres_up_to_eight_do_not_refuse_at_the_largest_given_speed() -> Result<(), PhysicsError>
{
    for links in [5, 6, 8] {
        for ratio in [3.0, 10.0] {
            let (steps, peak) = row(Form::Sphere, links, ratio, d3::MAX_GIVEN_SPEED, false)?;
            assert!(
                steps && peak < d3::MAX_SPEED,
                "{links} links, ratio {ratio}: {peak}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_clamping_world_steps_the_off_centre_throws_that_are_refused_by_default()
-> Result<(), PhysicsError> {
    for (form, speed) in [
        (Form::Box, 2500.0),
        (Form::Capsule, 2500.0),
        (Form::Hull, 2500.0),
    ] {
        let hit = Throw {
            offset: 0.6,
            ..Throw::new(form, 1000.0, speed)
        };
        assert!(!hit.steps()?, "{form:?} is refused by default");
        let clamped = Throw { clamp: true, ..hit };
        assert!(clamped.steps()?, "{form:?} steps under the clamp");
    }
    Ok(())
}

/// Rows that are refused by default (the observed cases; not monotone in speed or length).
const REFUSED_ROWS: [(Form, u32, f32, f32); 4] = [
    (Form::Box, 6, 10.0, 2500.0),
    (Form::Box, 5, 10.0, 5000.0),
    // Ratio 3 was refused here until card 8-89 (a capsule keeps its far end beside another one
    // that lies a little off parallel, so the row tumbles differently); ratio 10 is refused before
    // and after.
    (Form::Capsule, 5, 10.0, 2500.0),
    (Form::Hull, 5, 10.0, 1250.0),
];

#[test]
fn some_rows_of_boxes_capsules_and_hulls_are_refused_by_default() -> Result<(), PhysicsError> {
    for (form, links, ratio, speed) in REFUSED_ROWS {
        let (steps, _) = row(form, links, ratio, speed, false)?;
        assert!(
            !steps,
            "{form:?} {links} links, ratio {ratio}, speed {speed}"
        );
    }
    Ok(())
}

#[test]
fn a_clamping_world_steps_every_row_that_is_refused_by_default() -> Result<(), PhysicsError> {
    for (form, links, ratio, speed) in REFUSED_ROWS {
        let (steps, peak) = row(form, links, ratio, speed, true)?;
        assert!(
            steps,
            "{form:?} {links} links, ratio {ratio}, speed {speed}"
        );
        assert!(peak < CLAMP_LINEAR + 0.1, "{peak}");
    }
    Ok(())
}

/// Where a body ended up: x, y, z.
type Place = (f32, f32, f32);

/// A stack of eight boxes on a floor, a ball of density five put on top (overlapping a little, so
/// the contact exists in the first step) moving down at `speed`. The state of the bodies after
/// 240 steps, and how many body-steps the clamp changed.
fn stack(speed: f32, clamp: bool) -> Result<(Vec<Place>, usize), PhysicsError> {
    let mut world = d3::World::new(d3::WorldConfig {
        max_bodies: 16,
        gravity: d3::STANDARD_GRAVITY,
        max_pairs: 256,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })?;
    if clamp {
        world.set_speed_policy(d3::SpeedPolicy::Clamp {
            linear: CLAMP_LINEAR,
            angular: CLAMP_ANGULAR,
        })?;
    }
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(20.0, 1.0, 20.0),
        },
    ))?;
    for level in 0..8_u8 {
        world.create_body(&d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            Vec3::new(0.0, 0.5 + f32::from(level), 0.0),
            shape(Form::Box)?,
        ))?;
    }
    world.create_body(&d3::BodyDesc {
        linear_velocity: Vec3::new(0.0, -speed, 0.0),
        density: 5.0,
        ..d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            Vec3::new(0.0, 8.45, 0.0),
            shape(Form::Sphere)?,
        )
    })?;
    let mut clamped = 0;
    for _ in 0..240 {
        world.step(DT)?;
        clamped += world.clamped_bodies();
    }
    let state = world
        .iter()
        .map(|(_, body)| {
            let position = body.position();
            (position.x, position.y, position.z)
        })
        .collect();
    Ok((state, clamped))
}

#[test]
fn a_clamp_that_does_not_fire_leaves_a_stack_hit_by_a_slow_ball_exactly_as_it_is()
-> Result<(), PhysicsError> {
    let (refusing, _) = stack(10.0, false)?;
    let (clamping, clamped) = stack(10.0, true)?;
    assert_eq!(clamped, 0);
    assert_eq!(refusing.len(), clamping.len());
    for (a, b) in refusing.iter().zip(&clamping) {
        assert_eq!(
            (a.0.to_bits(), a.1.to_bits(), a.2.to_bits()),
            (b.0.to_bits(), b.1.to_bits(), b.2.to_bits())
        );
    }
    Ok(())
}

#[test]
fn a_stack_hit_at_any_speed_stays_finite_under_the_clamp_and_the_clamp_fires_at_5000()
-> Result<(), PhysicsError> {
    for speed in [400.0, 1000.0, 5000.0] {
        let (state, _) = stack(speed, true)?;
        assert!(
            state
                .iter()
                .all(|&(x, y, z)| x.is_finite() && y.is_finite() && z.is_finite()),
            "speed {speed}"
        );
    }
    let (_, clamped) = stack(5000.0, true)?;
    assert!(clamped > 0);
    Ok(())
}
