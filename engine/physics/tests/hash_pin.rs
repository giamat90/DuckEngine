//! The state hash does not cover the broadphase (card 4-11): adding the tree and the pair list to
//! the world must not change what a scene hashes to. The expected values were taken from the world
//! of card 4-10, before any broadphase code existed. The scene creates no rotation from an angle
//! (that would call the platform's sine), so only `+ - * /` and `sqrt` run and the values are the
//! same on every IEEE-754 platform.

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, PhysicsError,
    STANDARD_GRAVITY, Shape2, SolverConfig, World, WorldConfig,
};

const DT: f32 = 1.0 / 60.0;
/// (steps taken, hash after them), measured on the world of card 4-10 with the feature off and on.
/// The scene is a floor and eight bodies dropped on it; the first contact exists after step 45 and
/// the solver (card 4-13) acts from step 46. The values up to step 45 are those of card 4-10 and
/// have not changed: a world in which nothing touches hashes as it did before the solver existed.
const CHECKPOINTS: [(usize, u64); 5] = [
    (1, 0xbde3_cf42_a69c_74bc),
    (10, 0xee03_e516_681a_7925),
    (30, 0x2c0c_d35b_7d5b_114b),
    (44, 0x94c6_0e92_aa72_d6a6),
    (45, 0x6b6e_708e_d2be_a497),
];

/// Hashes after the solver has acted on contacts, measured on Windows and Linux with the feature off
/// and on. Card 4-13 measured `0x7c73_1aea_99b9_1327` at step 100 and `0x90fc_c47e_74e3_9c44` at
/// step 300; card 4-14 re-pinned both on purpose: the constraints are now solved colour by colour
/// instead of in manifold-list order (a different order of the same impulses gives different
/// bits), and at step 300 seven of the eight bodies sleep, which the hash covers (the step 100 value
/// is the same with sleeping on and off: nothing sleeps by then).
const CONTACT_CHECKPOINTS: [(usize, u64); 2] =
    [(100, 0x168b_f777_d8f3_925d), (300, 0x0257_2063_077a_b0ce)];

/// The same scene with sleeping off, at the same steps: only the solve order differs from card 4-13
/// here. At step 300 nothing sleeps (the default scene has seven sleepers by then).
const NO_SLEEP_CHECKPOINTS: [(usize, u64); 2] =
    [(100, 0x168b_f777_d8f3_925d), (300, 0x603b_2ff6_09c9_afd3)];

fn scene() -> Result<World, PhysicsError> {
    scene_with(physics::d2::SleepConfig::DEFAULT)
}

fn scene_with(sleep: physics::d2::SleepConfig) -> Result<World, PhysicsError> {
    let mut world = World::new(WorldConfig {
        max_bodies: 16,
        gravity: STANDARD_GRAVITY,
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    })?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(20.0, 1.0),
        },
    ))?;
    for i in 0..8_u8 {
        let x = f32::from(i);
        let shape = if i % 2 == 0 {
            Shape2::Circle {
                radius: 0.25 + 0.05 * x,
            }
        } else {
            Shape2::Box {
                half_extents: Vec2::new(0.3, 0.2 + 0.02 * x),
            }
        };
        let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec2::new(0.5 * x, 3.0 + x), shape);
        desc.angular_velocity = 0.7 - 0.1 * x;
        desc.linear_velocity = Vec2::new(0.5, 0.0);
        desc.linear_damping = 0.01 * x;
        desc.density = 1.0 + x;
        world.create_body(&desc)?;
    }
    Ok(world)
}

#[test]
fn the_state_hash_of_a_reference_scene_is_the_one_of_card_4_10() -> Result<(), PhysicsError> {
    let mut world = scene()?;
    let mut taken = 0;
    for (steps, expected) in CHECKPOINTS {
        while taken < steps {
            world.step(DT)?;
            taken += 1;
        }
        assert_eq!(world.state_hash(), expected, "after {steps} steps");
    }
    Ok(())
}

/// (steps taken, hash of the manifold buffer after them, manifolds then), measured with card 4-13
/// on Windows and Linux, feature off and on. (Card 4-12 measured the same scene before there was a
/// solver, when the bodies fell through the floor; those values are gone with that behaviour.)
/// Card 4-14 re-pinned step 75 (`0x1001_c68d_b201_a333` before) because the solve order changed;
/// steps 50 and 60 are unchanged.
const MANIFOLD_CHECKPOINTS: [(usize, u64, usize); 3] = [
    (50, 0xa117_d38f_f832_e967, 1),
    (60, 0x9992_82bf_0e2b_51f8, 4),
    (75, 0x10cc_e5c8_1c4f_6d97, 7),
];

#[test]
fn the_state_hash_after_contacts_is_pinned() -> Result<(), PhysicsError> {
    let mut world = scene()?;
    let mut taken = 0;
    for (steps, expected) in CONTACT_CHECKPOINTS {
        while taken < steps {
            world.step(DT)?;
            taken += 1;
        }
        assert_eq!(world.state_hash(), expected, "after {steps} steps");
    }
    Ok(())
}

#[test]
fn the_manifold_buffer_of_a_reference_scene_hashes_to_the_pinned_values() -> Result<(), PhysicsError>
{
    let mut world = scene()?;
    let mut taken = 0;
    for (steps, expected, count) in MANIFOLD_CHECKPOINTS {
        while taken < steps {
            world.step(DT)?;
            taken += 1;
        }
        assert_eq!(
            (world.manifolds().hash(), world.manifolds().len()),
            (expected, count),
            "after {steps} steps"
        );
        assert!(world.manifolds().is_complete());
    }
    Ok(())
}

#[test]
fn the_state_hash_with_sleeping_off_is_pinned() -> Result<(), PhysicsError> {
    let mut world = scene_with(physics::d2::SleepConfig::DISABLED)?;
    let mut taken = 0;
    for (steps, expected) in NO_SLEEP_CHECKPOINTS {
        while taken < steps {
            world.step(DT)?;
            taken += 1;
        }
        assert_eq!(world.state_hash(), expected, "after {steps} steps");
        assert_eq!(world.sleeping_count(), 0);
    }
    Ok(())
}

#[test]
fn the_default_scene_has_seven_sleepers_at_step_300() -> Result<(), PhysicsError> {
    let mut world = scene()?;
    for _ in 0..300 {
        world.step(DT)?;
    }
    assert_eq!(world.sleeping_count(), 7);
    Ok(())
}
