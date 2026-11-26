//! Card 8-58: the speed edge on what cards 8-51 and 8-52 left untried: hits between different
//! forms, a hull of 32 corners and an irregular hull, spin about x and y, friction at a hull, taller
//! stacks, and a cell whose edge is not monotone. Measured on 2026-11-21 in 3D (release scan,
//! steps of 50 m/s and 50 rad/s, bodies 0.9 m apart, 30 steps) and pinned at a few points as
//! observations, not promises; every pinned speed keeps 100 m/s or more from the first refused one
//! in the scan. The exact edge of a cell may differ on another machine; a pin that moves is a
//! finding to report, not a bound to loosen. Card 8-94: the prisms were first scanned in a frame
//! that `Hull::new` had chosen by rounding; they are now in the frame of their points (a corner
//! faces the other body) and the pins of the 24- and 32-corner prisms are re-measured there.
//!
//! What the numbers say: hits between different forms are refused from 900 to 4900 m/s (never in
//! some cells), the lowest 900 at ratio 1000; the prisms of 24 and 32 corners are refused head-on
//! with no friction from 1250 and 1600 m/s, the box is not; spin about x or y does not refuse a
//! capsule or a box, but refuses the hull of the 8 corners of a box about y from 750
//! rad/s (a square prism turned 45 degrees is not refused by spin at all); friction moves the edge
//! of a hit off the line by 13 to 25%; stacks of 12 and 16 are never refused.

use base::dmath::sin_cos;
use base::math::Vec3;
use physics::core::PhysicsError;
use physics::d3;

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 30;
const GAP: f32 = 0.9;
const FRICTION: f32 = 0.5;
const OFFSET: f32 = 0.4;
const HEAVY: f32 = 1000.0;
const FRICTIONS: [f32; 3] = [0.0, 0.5, 3.0];
const MAX_PAIRS: u32 = 256;
/// The stack scene: steps run, floor half width, side offset of the thrown body, clamp limits.
const STACK_STEPS: u32 = 240;
const FLOOR_HALF_WIDTH: f32 = 20.0;
const STACK_SIDE: f32 = 0.05;
const CLAMP_LINEAR: f32 = 500.0;
const CLAMP_ANGULAR: f32 = 50.0;

#[derive(Clone, Copy, Debug)]
enum Form {
    Sphere,
    Box,
    Capsule,
    /// A hull of 8 corners (a box shape).
    Hull8,
    /// A square prism turned 45 degrees about y (its corners on the axes), 8 corners.
    Diamond,
    /// A 12-sided prism: 24 corners.
    Prism12,
    /// A 16-sided prism: 32 corners, the most a hull has.
    Prism16,
    /// A hull of 11 points of no symmetry.
    Irregular,
}

const FORMS: [Form; 8] = [
    Form::Sphere,
    Form::Box,
    Form::Capsule,
    Form::Hull8,
    Form::Diamond,
    Form::Prism12,
    Form::Prism16,
    Form::Irregular,
];

const IRREGULAR: [[f32; 3]; 11] = [
    [-0.6, -0.4, -0.3],
    [0.7, -0.5, -0.2],
    [0.5, -0.3, 0.6],
    [-0.5, -0.5, 0.5],
    [-0.3, 0.6, -0.1],
    [0.2, 0.5, 0.3],
    [0.0, 0.8, 0.0],
    [0.6, 0.1, 0.0],
    [-0.7, 0.0, 0.1],
    [0.1, -0.7, 0.1],
    [0.3, 0.0, -0.6],
];

/// The corners of a prism of height one with `sides` sides of circumradius `radius`.
fn prism(sides: u8, radius: f32) -> Vec<Vec3> {
    let mut points = Vec::new();
    for side in 0..sides {
        let (sin, cos) = sin_cos(f32::from(side) * std::f32::consts::TAU / f32::from(sides));
        for y in [-0.5, 0.5] {
            points.push(Vec3::new(radius * cos, y, radius * sin));
        }
    }
    points
}

/// The 8 corners of the unit box.
fn unit_box_corners() -> Vec<Vec3> {
    let mut points = Vec::new();
    for x in [-0.5, 0.5] {
        for y in [-0.5, 0.5] {
            points.extend([-0.5, 0.5].map(|z| Vec3::new(x, y, z)));
        }
    }
    points
}

fn hull(points: &[Vec3]) -> Result<d3::Shape3, PhysicsError> {
    d3::Hull::new(points).map(d3::Shape3::Hull)
}

fn shape(form: Form) -> Result<d3::Shape3, PhysicsError> {
    match form {
        Form::Sphere => Ok(d3::Shape3::Sphere { radius: 0.5 }),
        Form::Box => Ok(d3::Shape3::Box {
            half_extents: Vec3::splat(0.5),
        }),
        Form::Capsule => Ok(d3::Shape3::Capsule {
            half_height: 0.5,
            radius: 0.5,
        }),
        Form::Hull8 => hull(&unit_box_corners()),
        Form::Diamond => hull(&prism(4, 0.7)),
        Form::Prism12 => hull(&prism(12, 0.5)),
        Form::Prism16 => hull(&prism(16, 0.5)),
        Form::Irregular => {
            let points: Vec<Vec3> = IRREGULAR
                .iter()
                .map(|p| Vec3::new(p[0], p[1], p[2]))
                .collect();
            hull(&points)
        }
    }
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

/// A body of `thrown` form and density `ratio` thrown at `speed` along x at a body of `target`
/// form 0.9 m away and `offset` metres off the line. `spin` is the angular velocity of the thrown
/// body.
#[derive(Clone, Copy)]
struct Hit {
    thrown: Form,
    target: Form,
    ratio: f32,
    speed: f32,
    offset: f32,
    spin: Vec3,
    friction: f32,
}

impl Hit {
    fn new(thrown: Form, target: Form, ratio: f32, speed: f32) -> Self {
        Self {
            thrown,
            target,
            ratio,
            speed,
            offset: 0.0,
            spin: Vec3::ZERO,
            friction: FRICTION,
        }
    }

    /// Whether the world steps 30 times without a refusal.
    fn steps(self) -> Result<bool, PhysicsError> {
        let mut world = world(4, Vec3::ZERO, false)?;
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(self.speed, 0.0, 0.0),
            angular_velocity: self.spin,
            density: self.ratio,
            friction: self.friction,
            restitution: 1.0,
            ..d3::BodyDesc::new(d3::BodyKind::Dynamic, Vec3::ZERO, shape(self.thrown)?)
        })?;
        world.create_body(&d3::BodyDesc {
            friction: self.friction,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(GAP, self.offset, 0.0),
                shape(self.target)?,
            )
        })?;
        Ok((0..STEPS).all(|_| world.step(DT).is_ok()))
    }
}

#[test]
fn every_pair_of_forms_steps_at_700_metres_per_second_head_on_and_off_the_line()
-> Result<(), PhysicsError> {
    // The four cells: ratio 1 or 1000, on the line or 0.4 off.
    let cells = [(1.0, 0.0), (1.0, OFFSET), (HEAVY, 0.0), (HEAVY, OFFSET)];
    let pairs = FORMS.into_iter().flat_map(|a| FORMS.map(|b| (a, b)));
    for ((thrown, target), (ratio, offset)) in pairs.flat_map(|p| cells.map(|c| (p, c))) {
        let hit = Hit {
            offset,
            ..Hit::new(thrown, target, ratio, 700.0)
        };
        assert!(
            hit.steps()?,
            "{thrown:?} at {target:?}, ratio {ratio}, {offset} off"
        );
    }
    Ok(())
}

#[test]
fn the_lowest_edges_between_forms_are_about_900_at_ratio_1000_and_refuse_at_1100_to_1400()
-> Result<(), PhysicsError> {
    // First refused speed in the scan: a capsule at a hull of 32 corners 900, an irregular hull at
    // a turned square prism 1150 (900 before card 8-94, in the frame that rounding had chosen), a
    // sphere at a hull of 24 corners 0.4 off 950, an irregular hull at itself 1250.
    let refused = |thrown, target, offset, speed| -> Result<bool, PhysicsError> {
        let hit = Hit {
            offset,
            ..Hit::new(thrown, target, HEAVY, speed)
        };
        Ok(!hit.steps()?)
    };
    assert!(refused(Form::Capsule, Form::Prism16, 0.0, 1100.0)?);
    assert!(refused(Form::Irregular, Form::Diamond, 0.0, 1300.0)?);
    assert!(refused(Form::Sphere, Form::Prism12, OFFSET, 1100.0)?);
    assert!(refused(Form::Irregular, Form::Irregular, 0.0, 1400.0)?);
    Ok(())
}

#[test]
fn a_sphere_head_on_at_ratio_1_is_never_refused_by_a_sphere_a_box_or_a_capsule()
-> Result<(), PhysicsError> {
    for target in [Form::Sphere, Form::Box, Form::Capsule] {
        let hit = Hit::new(Form::Sphere, target, 1.0, 4900.0);
        assert!(hit.steps()?, "at {target:?}");
    }
    let capsules = Hit::new(Form::Capsule, Form::Capsule, HEAVY, 4500.0);
    assert!(capsules.steps()?, "two capsules at ratio 1000, 4500");
    Ok(())
}

/// Whether a head-on hit at 2500 m/s with ratio 1000 and the given spin is refused.
fn spinning_is_refused(form: Form, spin: Vec3, friction: f32) -> Result<bool, PhysicsError> {
    let hit = Hit {
        spin,
        friction,
        ..Hit::new(form, form, HEAVY, 2500.0)
    };
    Ok(!hit.steps()?)
}

#[test]
fn spin_of_1000_about_x_or_y_does_not_refuse_a_capsule_or_a_box() -> Result<(), PhysicsError> {
    // The hull of 24 corners was in this list; with a corner facing the other body (card 8-94) the
    // hit at 2500 m/s alone refuses it with no friction (from 1250), so spin says nothing there.
    let spins = [Vec3::X * 1000.0, Vec3::Y * 1000.0];
    for form in [Form::Capsule, Form::Box] {
        for (friction, spin) in FRICTIONS.into_iter().flat_map(|f| spins.map(|s| (f, s))) {
            assert!(
                !spinning_is_refused(form, spin, friction)?,
                "{form:?}, {spin:?}, friction {friction}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_hull_of_eight_corners_is_refused_by_spin_about_y_from_about_750_and_never_about_x()
-> Result<(), PhysicsError> {
    for friction in FRICTIONS {
        let about = |axis: Vec3, turn: f32| spinning_is_refused(Form::Hull8, axis * turn, friction);
        assert!(!about(Vec3::Y, 600.0)?, "y 600, friction {friction}");
        assert!(about(Vec3::Y, 900.0)?, "y 900, friction {friction}");
        assert!(!about(Vec3::X, 1000.0)?, "x 1000, friction {friction}");
    }
    Ok(())
}

#[test]
fn the_prisms_of_24_and_32_corners_are_refused_head_on_at_2500_with_no_friction_and_not_with_friction_half()
-> Result<(), PhysicsError> {
    // Edge in the scan, ratio 1000, a corner facing the other body (card 8-94): with friction 0
    // 1250 (24 corners) and 1600 (32 corners), with friction 0.5 2650 and 3400; the box is not
    // refused head-on at any friction. Before card 8-94 the prism of 24 corners was not refused
    // and that of 32 corners was refused from 2050: the frame `Hull::new` chose (reviews/8-67.md).
    for form in [Form::Prism12, Form::Prism16] {
        assert!(spinning_is_refused(form, Vec3::ZERO, 0.0)?, "{form:?}");
        assert!(
            !spinning_is_refused(form, Vec3::ZERO, FRICTION)?,
            "{form:?}, friction {FRICTION}"
        );
    }
    assert!(!spinning_is_refused(Form::Box, Vec3::ZERO, 0.0)?);
    Ok(())
}

#[test]
fn friction_moves_the_edge_of_a_box_hit_off_the_line_from_about_1850_to_about_2450()
-> Result<(), PhysicsError> {
    let off_the_line = |friction| Hit {
        offset: OFFSET,
        friction,
        ..Hit::new(Form::Box, Form::Box, 1.0, 2150.0)
    };
    assert!(!off_the_line(0.0).steps()?, "friction 0 at 2150");
    assert!(off_the_line(FRICTION).steps()?, "friction 0.5 at 2150");
    Ok(())
}

/// A capsule of density 1000 thrown at 30 degrees at a capsule that moves against it, 0.4 off the
/// line (both at `speed`).
fn capsules_at_an_angle(speed: f32) -> Result<bool, PhysicsError> {
    let mut world = world(4, Vec3::ZERO, false)?;
    let (sin, cos) = sin_cos(30.0_f32.to_radians());
    world.create_body(&d3::BodyDesc {
        linear_velocity: Vec3::new(speed * cos, speed * sin, 0.0),
        density: HEAVY,
        friction: FRICTION,
        restitution: 1.0,
        ..d3::BodyDesc::new(d3::BodyKind::Dynamic, Vec3::ZERO, shape(Form::Capsule)?)
    })?;
    world.create_body(&d3::BodyDesc {
        linear_velocity: Vec3::new(-speed, 0.0, 0.0),
        friction: FRICTION,
        restitution: 1.0,
        ..d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            Vec3::new(GAP, OFFSET, 0.0),
            shape(Form::Capsule)?,
        )
    })?;
    Ok((0..STEPS).all(|_| world.step(DT).is_ok()))
}

#[test]
fn two_capsules_at_30_degrees_step_at_1000_and_are_refused_at_2000_with_a_window_of_steps_near_1400()
-> Result<(), PhysicsError> {
    // In the scan this cell is refused at 1350, steps from 1375 to 1425 and is refused again from
    // 1450: not monotone. Only the two points away from that are pinned.
    assert!(capsules_at_an_angle(1000.0)?);
    assert!(!capsules_at_an_angle(2000.0)?);
    Ok(())
}

/// `boxes` boxes on a floor, a body of density five of `form` on top moving down at `speed`. The
/// lowest height of any dynamic body after 240 steps, or `None` when a step was refused or a body
/// is not finite (`f32::min` would skip a NaN).
fn stack_hit(boxes: u8, form: Form, speed: f32, clamp: bool) -> Result<Option<f32>, PhysicsError> {
    let mut world = world(32, d3::STANDARD_GRAVITY, clamp)?;
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(FLOOR_HALF_WIDTH, 1.0, FLOOR_HALF_WIDTH),
        },
    ))?;
    for level in 0..boxes {
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
            Vec3::new(STACK_SIDE, f32::from(boxes) + 0.95, 0.0),
            shape(form)?,
        )
    })?;
    for _ in 0..STACK_STEPS {
        if world.step(DT).is_err() {
            return Ok(None);
        }
    }
    let heights: Vec<f32> = world
        .iter()
        .filter(|(_, body)| body.kind() == d3::BodyKind::Dynamic)
        .map(|(_, body)| body.position().y)
        .collect();
    let lowest = heights.iter().copied().fold(f32::MAX, f32::min);
    Ok(heights.iter().all(|y| y.is_finite()).then_some(lowest))
}

const STACK_FORMS: [Form; 5] = [
    Form::Box,
    Form::Capsule,
    Form::Prism12,
    Form::Prism16,
    Form::Irregular,
];

#[test]
fn stacks_of_12_and_16_boxes_are_never_refused_when_hit_at_any_speed() -> Result<(), PhysicsError> {
    let cases = [12, 16]
        .into_iter()
        .flat_map(|n| STACK_FORMS.map(|f| (n, f)));
    for (boxes, form) in cases {
        for speed in [10.0, 400.0, 1000.0, 4900.0] {
            assert!(
                stack_hit(boxes, form, speed, false)?.is_some(),
                "{boxes} boxes, {form:?} at {speed}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_stack_of_12_or_16_hit_at_10_metres_per_second_stays_on_the_floor() -> Result<(), PhysicsError>
{
    for boxes in [12, 16] {
        for form in STACK_FORMS {
            let lowest = stack_hit(boxes, form, 10.0, false)?;
            assert!(
                lowest.is_some_and(|y| y > 0.4),
                "{boxes}, {form:?}: {lowest:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_taller_stack_hit_at_4900_ends_far_below_the_floor_and_the_clamp_bounds_the_fall()
-> Result<(), PhysicsError> {
    for form in [Form::Box, Form::Prism16, Form::Irregular] {
        let free = stack_hit(12, form, 4900.0, false)?;
        let clamped = stack_hit(12, form, 4900.0, true)?;
        assert!(free.is_some_and(|y| y < -5000.0), "{form:?} free {free:?}");
        assert!(
            clamped.is_some_and(|y| y > -2500.0),
            "{form:?} clamped {clamped:?}"
        );
    }
    Ok(())
}
