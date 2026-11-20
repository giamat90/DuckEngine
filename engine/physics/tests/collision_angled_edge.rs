//! Card 8-52: the speed edge on what card 8-51 left untried: hits where both bodies move and one
//! of them at an angle, a hull of 24 corners, friction with spin on capsules and hulls, and a stack
//! hit by a box, a capsule or a hull. Measured on 2026-11-20 in 3D (release scan, steps of 50 m/s
//! and 50 rad/s) and pinned at a few points as observations, not promises; every pinned speed keeps
//! a margin of one or more scan steps from the first refused one.
//!
//! What the numbers say: moving both bodies, or tilting one, moves the edge by a factor of about
//! two to three in either direction and the edge stays above 900 m/s per body; a hull of 24 corners
//! is refused like boxes and capsules; friction decides whether a spinning capsule is refused,
//! hulls hardly; and a stack hit by a box or a capsule is never refused, it is only thrown through
//! the floor (there is no continuous collision detection), which the clamp turns into a bounded fall.

use base::dmath;
use base::math::Vec3;
use physics::core::PhysicsError;
use physics::d3;

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 30;
const CLAMP_LINEAR: f32 = 500.0;
const CLAMP_ANGULAR: f32 = 50.0;
/// Corners of the round hull: a 12-sided prism.
const SIDES: u8 = 12;
/// The fastest given speed that is still accepted (the given limit is 5000 m/s).
const TOP_SPEED: f32 = 4900.0;
/// Distance between the two bodies of a hit, and the friction they have unless a test sets it.
const GAP: f32 = 0.9;
const FRICTION: f32 = 0.5;
/// The stack scene: steps run, width of the floor, side offset and height of the thrown body.
const STACK_STEPS: u32 = 240;
const FLOOR_HALF_WIDTH: f32 = 20.0;
const STACK_SIDE: f32 = 0.05;
const STACK_TOP: f32 = 8.95;
const MAX_PAIRS: u32 = 256;

#[derive(Clone, Copy, Debug)]
enum Form {
    Sphere,
    Box,
    Capsule,
    Hull,
    Round,
}

const FORMS: [Form; 5] = [
    Form::Sphere,
    Form::Box,
    Form::Capsule,
    Form::Hull,
    Form::Round,
];

fn shape(form: Form) -> Result<d3::Shape3, PhysicsError> {
    let corners = |ring: Vec<(f32, f32)>| {
        let points: Vec<Vec3> = ring
            .into_iter()
            .flat_map(|(x, z)| [-0.5, 0.5].map(|y| Vec3::new(x, y, z)))
            .collect();
        d3::Hull::new(&points).map(d3::Shape3::Hull)
    };
    Ok(match form {
        Form::Sphere => d3::Shape3::Sphere { radius: 0.5 },
        Form::Box => d3::Shape3::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        },
        Form::Capsule => d3::Shape3::Capsule {
            half_height: 0.5,
            radius: 0.5,
        },
        Form::Hull => corners(vec![(-0.5, -0.5), (-0.5, 0.5), (0.5, -0.5), (0.5, 0.5)])?,
        Form::Round => {
            let ring = (0..SIDES)
                .map(|k| {
                    let angle = f32::from(k) * std::f32::consts::TAU / f32::from(SIDES);
                    let (sin, cos) = dmath::sin_cos(angle);
                    (0.5 * cos, 0.5 * sin)
                })
                .collect();
            corners(ring)?
        }
    })
}

fn world(max_bodies: u32, gravity: Vec3, clamp: bool) -> Result<d3::World, PhysicsError> {
    let mut world = d3::World::new(d3::WorldConfig {
        max_bodies,
        gravity,
        max_pairs: MAX_PAIRS,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: MAX_PAIRS,
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

/// A body of density `ratio` thrown at `speed` at `angle` degrees from the x axis in the xy plane,
/// at a body 0.9 m away and `offset` metres off the line; the target moves at `speed` against it
/// when `both` is set. Spin about z and the friction of both bodies are as given.
#[derive(Clone, Copy)]
struct Hit {
    form: Form,
    ratio: f32,
    speed: f32,
    angle: f32,
    offset: f32,
    both: bool,
    spin: f32,
    friction: f32,
}

impl Hit {
    fn new(form: Form, ratio: f32, speed: f32) -> Self {
        Self {
            form,
            ratio,
            speed,
            angle: 0.0,
            offset: 0.0,
            both: false,
            spin: 0.0,
            friction: FRICTION,
        }
    }

    /// Whether the world steps 30 times without a refusal.
    fn steps(self) -> Result<bool, PhysicsError> {
        let mut world = world(4, Vec3::ZERO, false)?;
        let (sin, cos) = dmath::sin_cos(self.angle.to_radians());
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(self.speed * cos, self.speed * sin, 0.0),
            angular_velocity: Vec3::new(0.0, 0.0, self.spin),
            density: self.ratio,
            friction: self.friction,
            restitution: 1.0,
            ..d3::BodyDesc::new(d3::BodyKind::Dynamic, Vec3::ZERO, shape(self.form)?)
        })?;
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(if self.both { -self.speed } else { 0.0 }, 0.0, 0.0),
            friction: self.friction,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(GAP, self.offset, 0.0),
                shape(self.form)?,
            )
        })?;
        Ok((0..STEPS).all(|_| world.step(DT).is_ok()))
    }
}

/// Whether a pair of `form` moving against each other steps at every angle and offset tried.
fn steps_at_every_angle_and_offset(
    form: Form,
    ratio: f32,
    speed: f32,
) -> Result<bool, PhysicsError> {
    let mut all = true;
    for angle in [0.0, 30.0, 60.0] {
        for offset in [0.0, 0.4] {
            let hit = Hit {
                angle,
                offset,
                both: true,
                ..Hit::new(form, ratio, speed)
            };
            all &= hit.steps()?;
        }
    }
    Ok(all)
}

#[test]
fn two_bodies_moving_at_an_angle_are_not_refused_at_800_metres_per_second_each()
-> Result<(), PhysicsError> {
    for form in FORMS {
        for ratio in [1.0, 1000.0] {
            assert!(
                steps_at_every_angle_and_offset(form, ratio, 800.0)?,
                "{form:?} ratio {ratio}"
            );
        }
    }
    Ok(())
}

#[test]
fn two_equal_spheres_meeting_at_an_angle_with_no_offset_are_never_refused_but_boxes_are_at_2500()
-> Result<(), PhysicsError> {
    for angle in [0.0, 30.0] {
        let sphere = Hit {
            angle,
            both: true,
            ..Hit::new(Form::Sphere, 1.0, TOP_SPEED)
        };
        assert!(sphere.steps()?, "sphere, angle {angle}");
    }
    let crate_ = Hit {
        angle: 30.0,
        offset: 0.4,
        both: true,
        ..Hit::new(Form::Box, 1.0, 2500.0)
    };
    assert!(!crate_.steps()?, "box 0.4 off, 30 degrees");
    Ok(())
}

#[test]
fn tilting_the_thrown_box_by_60_degrees_moves_the_edge_up_from_about_3100_to_about_4250()
-> Result<(), PhysicsError> {
    let hit = |angle| Hit {
        angle,
        both: true,
        ..Hit::new(Form::Box, 1000.0, 3500.0)
    };
    assert!(!hit(0.0).steps()?, "straight, 3500");
    assert!(hit(60.0).steps()?, "tilted, 3500");
    Ok(())
}

#[test]
fn a_hull_of_24_corners_is_refused_off_the_line_like_the_box_and_not_head_on_at_ratio_1000()
-> Result<(), PhysicsError> {
    let far = Hit {
        offset: 0.6,
        ..Hit::new(Form::Round, 1000.0, 2500.0)
    };
    assert!(!far.steps()?, "0.6 off, 2500");
    let near = Hit {
        offset: 0.2,
        ..Hit::new(Form::Round, 1000.0, TOP_SPEED)
    };
    assert!(near.steps()?, "0.2 off, 4900");
    Ok(())
}

#[test]
fn a_clamping_world_steps_the_hull_of_24_corners_that_is_refused_by_default()
-> Result<(), PhysicsError> {
    let mut world = world(4, Vec3::ZERO, true)?;
    for (x, vx) in [(0.0, 2500.0), (0.9, 0.0)] {
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(vx, 0.0, 0.0),
            density: if vx > 0.0 { 1000.0 } else { 1.0 },
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(x, if vx > 0.0 { 0.0 } else { 0.6 }, 0.0),
                shape(Form::Round)?,
            )
        })?;
    }
    assert!((0..STEPS).all(|_| world.step(DT).is_ok()));
    Ok(())
}

/// Whether a head-on hit at 2500 m/s with ratio 1000 and the given spin about z is refused.
fn spinning_is_refused(form: Form, spin: f32, friction: f32) -> Result<bool, PhysicsError> {
    let hit = Hit {
        spin,
        friction,
        ..Hit::new(form, 1000.0, 2500.0)
    };
    Ok(!hit.steps()?)
}

#[test]
fn friction_decides_whether_a_capsule_spinning_at_500_is_refused_and_all_refuse_at_1000()
-> Result<(), PhysicsError> {
    assert!(
        spinning_is_refused(Form::Capsule, 500.0, 0.0)?,
        "friction 0"
    );
    for friction in [0.5, 1.0, 3.0] {
        assert!(
            !spinning_is_refused(Form::Capsule, 500.0, friction)?,
            "friction {friction}"
        );
    }
    for friction in [0.0, 0.5, 1.0, 3.0] {
        assert!(
            spinning_is_refused(Form::Capsule, 1000.0, friction)?,
            "friction {friction}"
        );
    }
    Ok(())
}

#[test]
fn a_hull_of_eight_corners_is_never_refused_by_spin_and_one_of_24_is_refused_from_about_600()
-> Result<(), PhysicsError> {
    for friction in [0.0, 0.5, 1.0, 3.0] {
        assert!(
            !spinning_is_refused(Form::Hull, 1000.0, friction)?,
            "eight corners, friction {friction}"
        );
        assert!(
            !spinning_is_refused(Form::Round, 500.0, friction)?,
            "24 corners, spin 500, friction {friction}"
        );
        assert!(
            spinning_is_refused(Form::Round, 700.0, friction)?,
            "24 corners, spin 700, friction {friction}"
        );
    }
    Ok(())
}

/// Eight boxes on a floor, a body of density five on top moving down at `speed` (the hit is in
/// the first steps). The lowest height of any body after 240 steps and whether it stepped.
fn stack_hit(form: Form, speed: f32, clamp: bool) -> Result<(bool, f32), PhysicsError> {
    let mut world = world(16, d3::STANDARD_GRAVITY, clamp)?;
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(FLOOR_HALF_WIDTH, 1.0, FLOOR_HALF_WIDTH),
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
            Vec3::new(STACK_SIDE, STACK_TOP, 0.0),
            shape(form)?,
        )
    })?;
    for _ in 0..STACK_STEPS {
        if world.step(DT).is_err() {
            return Ok((false, 0.0));
        }
    }
    let lowest = world
        .iter()
        .filter(|(_, body)| body.kind() == d3::BodyKind::Dynamic)
        .map(|(_, body)| body.position().y)
        .fold(f32::MAX, f32::min);
    Ok((true, lowest))
}

#[test]
fn a_stack_hit_by_a_box_a_capsule_or_a_hull_is_never_refused_by_default() -> Result<(), PhysicsError>
{
    for form in [Form::Box, Form::Capsule, Form::Round] {
        for speed in [10.0, 400.0, 1000.0, TOP_SPEED] {
            let (steps, _) = stack_hit(form, speed, false)?;
            assert!(steps, "{form:?} at {speed}");
        }
    }
    Ok(())
}

#[test]
fn a_stack_hit_at_3000_ends_thousands_of_metres_below_the_floor_and_the_clamp_limits_the_fall_to_its_speed_times_the_run()
-> Result<(), PhysicsError> {
    for form in [Form::Box, Form::Capsule, Form::Round] {
        let (_, free) = stack_hit(form, 3000.0, false)?;
        let (_, clamped) = stack_hit(form, 3000.0, true)?;
        assert!(free < -5000.0, "{form:?} free fall {free}");
        assert!(clamped > -2500.0, "{form:?} under the clamp {clamped}");
    }
    Ok(())
}

#[test]
fn a_stack_hit_at_10_metres_per_second_leaves_every_body_above_the_floor()
-> Result<(), PhysicsError> {
    for form in [Form::Box, Form::Capsule, Form::Round] {
        let (_, lowest) = stack_hit(form, 10.0, false)?;
        assert!(lowest > 0.0, "{form:?}: {lowest}");
    }
    Ok(())
}
