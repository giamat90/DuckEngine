//! The state hash does not cover the broadphase (card 4-11): adding the tree and the pair list to
//! the world must not change what a scene hashes to. The expected values were taken from the world
//! of card 4-10, before any broadphase code existed. The scene creates no rotation from an angle
//! (that would call the platform's sine), so only `+ - * /` and `sqrt` run and the values are the
//! same on every IEEE-754 platform.

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, PhysicsError, STANDARD_GRAVITY, Shape2, World,
    WorldConfig,
};

const DT: f32 = 1.0 / 60.0;
/// (steps taken, hash after them), measured on the world of card 4-10 with the feature off and on.
const CHECKPOINTS: [(usize, u64); 3] = [
    (1, 0xbde3_cf42_a69c_74bc),
    (100, 0x92b2_0491_4261_38ac),
    (300, 0x1e0f_4626_1102_af55),
];

fn scene() -> Result<World, PhysicsError> {
    let mut world = World::new(WorldConfig {
        max_bodies: 16,
        gravity: STANDARD_GRAVITY,
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
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
