//! Handles, capacity, destruction and the contact flag of joints (card 4-16).

use super::common::{
    ball, config, ground, hinge, hinge_between, pin, plank, rod, step_n, world, world_with_gravity,
};
use base::math::Vec2;
use physics::d2::{
    BodyHandle, Joint2, JointDesc, MAX_JOINTS, PhysicsError, RevoluteJoint2, STANDARD_GRAVITY,
    SleepConfig, World,
};

fn two_balls(world: &mut World) -> Result<(BodyHandle, BodyHandle), PhysicsError> {
    let a = ball(world, 0.5, Vec2::new(0.0, 5.0))?;
    let b = ball(world, 0.5, Vec2::new(2.0, 5.0))?;
    Ok((a, b))
}

#[test]
fn a_joint_is_created_found_counted_and_destroyed() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let (a, b) = two_balls(&mut world)?;
    assert_eq!((world.joint_count(), world.joint_capacity()), (0, 64));
    let joint = hinge(&mut world, a, b, Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0))?;
    assert_eq!((joint.index(), joint.generation()), (0, 0));
    assert!(world.contains_joint(joint));
    assert_eq!(world.joint_count(), 1);
    let entry = world.joint(joint)?;
    assert_eq!((entry.first, entry.second, entry.handle), (a, b, joint));
    assert!(!entry.collide_connected);
    assert_eq!(world.joints().len(), 1);
    world.destroy_joint(joint)?;
    assert!(!world.contains_joint(joint));
    assert_eq!((world.joint_count(), world.joints().len()), (0, 0));
    assert_eq!(world.joint(joint).err(), Some(PhysicsError::StaleJoint));
    assert_eq!(world.destroy_joint(joint), Err(PhysicsError::StaleJoint));
    Ok(())
}

#[test]
fn a_destroyed_joint_slot_is_reused_with_a_higher_generation() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let (a, b) = two_balls(&mut world)?;
    let first = hinge_between(&mut world, a, b)?;
    world.destroy_joint(first)?;
    let second = hinge_between(&mut world, a, b)?;
    assert_eq!(
        (second.index(), second.generation()),
        (first.index(), first.generation() + 1)
    );
    assert!(!world.contains_joint(first));
    assert!(world.contains_joint(second));
    Ok(())
}

#[test]
fn joints_are_listed_in_slot_order() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let (a, b) = two_balls(&mut world)?;
    let c = ball(&mut world, 0.5, Vec2::new(4.0, 5.0))?;
    let ab = hinge_between(&mut world, a, b)?;
    let bc = hinge_between(&mut world, b, c)?;
    let ac = hinge_between(&mut world, a, c)?;
    world.destroy_joint(bc)?;
    let again = hinge_between(&mut world, c, b)?;
    assert_eq!(again.index(), bc.index());
    let order: Vec<_> = world.joints().iter().map(|entry| entry.handle).collect();
    assert_eq!(order, vec![ab, again, ac]);
    Ok(())
}

#[test]
fn creating_past_the_joint_capacity_is_an_error_value_and_changes_nothing()
-> Result<(), PhysicsError> {
    let mut world = World::new(config(8, 2, STANDARD_GRAVITY, SleepConfig::DISABLED))?;
    let (a, b) = two_balls(&mut world)?;
    let c = ball(&mut world, 0.5, Vec2::new(4.0, 5.0))?;
    hinge_between(&mut world, a, b)?;
    hinge_between(&mut world, b, c)?;
    let before = (world.state_hash(), world.joint_count());
    assert_eq!(
        hinge_between(&mut world, a, c).err(),
        Some(PhysicsError::JointCapacityExhausted)
    );
    assert_eq!((world.state_hash(), world.joint_count()), before);
    Ok(())
}

#[test]
fn a_world_without_joint_capacity_refuses_every_joint() -> Result<(), PhysicsError> {
    let mut world = World::new(config(8, 0, STANDARD_GRAVITY, SleepConfig::DISABLED))?;
    let (a, b) = two_balls(&mut world)?;
    assert_eq!(
        hinge_between(&mut world, a, b).err(),
        Some(PhysicsError::JointCapacityExhausted)
    );
    Ok(())
}

#[test]
fn a_joint_capacity_above_the_maximum_is_refused() {
    let built = World::new(config(
        8,
        MAX_JOINTS + 1,
        STANDARD_GRAVITY,
        SleepConfig::DISABLED,
    ));
    assert!(matches!(
        built.err(),
        Some(PhysicsError::JointCapacityTooLarge { max }) if max == MAX_JOINTS
    ));
}

#[test]
fn a_joint_needs_two_different_live_bodies_and_not_two_static_ones() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let (a, b) = two_balls(&mut world)?;
    let pin_a = pin(&mut world, 0.0, 0.0)?;
    let pin_b = pin(&mut world, 3.0, 0.0)?;
    assert_eq!(
        hinge_between(&mut world, a, a).err(),
        Some(PhysicsError::InvalidJoint)
    );
    assert_eq!(
        hinge_between(&mut world, pin_a, pin_b).err(),
        Some(PhysicsError::InvalidJoint)
    );
    assert!(hinge_between(&mut world, pin_a, a).is_ok());
    assert!(hinge_between(&mut world, b, pin_b).is_ok());
    world.destroy_body(b)?;
    assert_eq!(
        hinge_between(&mut world, a, b).err(),
        Some(PhysicsError::StaleHandle)
    );
    assert_eq!(
        hinge_between(&mut world, b, a).err(),
        Some(PhysicsError::StaleHandle)
    );
    Ok(())
}

#[test]
fn a_refused_description_changes_nothing_and_uses_no_slot() -> Result<(), PhysicsError> {
    let mut world = World::new(config(8, 1, STANDARD_GRAVITY, SleepConfig::DISABLED))?;
    let (a, b) = two_balls(&mut world)?;
    let before = world.state_hash();
    let nan = hinge(&mut world, a, b, Vec2::new(f32::NAN, 0.0), Vec2::ZERO);
    assert_eq!(nan.err(), Some(PhysicsError::NotFinite));
    let bad_rod = rod(&mut world, a, b, (Vec2::ZERO, Vec2::ZERO), 0.0);
    assert_eq!(bad_rod.err(), Some(PhysicsError::InvalidJoint));
    assert_eq!((world.joint_count(), world.state_hash()), (0, before));
    // The one slot is still free.
    let joint = hinge_between(&mut world, a, b)?;
    assert_eq!((joint.index(), joint.generation()), (0, 0));
    Ok(())
}

#[test]
fn destroying_a_body_destroys_its_joints_and_wakes_the_partner() -> Result<(), PhysicsError> {
    let mut world = World::new(config(8, 8, Vec2::ZERO, SleepConfig::DEFAULT))?;
    let (a, b) = two_balls(&mut world)?;
    let c = ball(&mut world, 0.5, Vec2::new(4.0, 5.0))?;
    let ab = hinge_between(&mut world, a, b)?;
    let bc = hinge_between(&mut world, b, c)?;
    let ca = hinge_between(&mut world, c, a)?;
    step_n(&mut world, 60)?;
    assert_eq!(world.sleeping_count(), 3, "at rest, the island sleeps");
    world.destroy_body(b)?;
    assert!(!world.contains_joint(ab) && !world.contains_joint(bc));
    assert!(world.contains_joint(ca));
    assert_eq!(world.joint_count(), 1);
    assert_eq!(world.joint(ab).err(), Some(PhysicsError::StaleJoint));
    assert_eq!(world.sleeping_count(), 0, "the partners were woken");
    // The freed joint slots are reusable.
    let again = hinge_between(&mut world, a, c)?;
    assert!(world.contains_joint(again));
    step_n(&mut world, 1)?;
    Ok(())
}

#[test]
fn a_joint_to_a_static_body_dies_with_the_dynamic_one_and_with_the_static_one()
-> Result<(), PhysicsError> {
    let mut world = world()?;
    let (a, _) = two_balls(&mut world)?;
    let pin_a = pin(&mut world, 0.0, 6.0)?;
    let joint = hinge(&mut world, pin_a, a, Vec2::ZERO, Vec2::new(0.0, 1.0))?;
    world.destroy_body(pin_a)?;
    assert!(!world.contains_joint(joint));
    assert_eq!(world.joint_count(), 0);
    step_n(&mut world, 5)?;
    Ok(())
}

#[test]
fn bodies_of_a_joint_do_not_collide_unless_asked() -> Result<(), PhysicsError> {
    for collide_connected in [false, true] {
        let mut world = world_with_gravity(Vec2::ZERO)?;
        let a = ball(&mut world, 1.0, Vec2::new(0.0, 0.0))?;
        let b = ball(&mut world, 1.0, Vec2::new(1.5, 0.0))?;
        world.create_joint(&JointDesc {
            collide_connected,
            ..JointDesc::new(
                a,
                b,
                Joint2::Revolute(RevoluteJoint2::new(
                    Vec2::new(0.75, 0.0),
                    Vec2::new(-0.75, 0.0),
                )),
            )
        })?;
        step_n(&mut world, 3)?;
        assert_eq!(
            world.manifolds().len(),
            usize::from(collide_connected),
            "collide_connected {collide_connected}"
        );
    }
    Ok(())
}

#[test]
fn only_the_joined_pair_is_exempt_from_contact() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let a = ball(&mut world, 1.0, Vec2::new(0.0, 0.0))?;
    let b = ball(&mut world, 1.0, Vec2::new(1.5, 0.0))?;
    let c = ball(&mut world, 1.0, Vec2::new(0.75, 1.5))?;
    hinge(
        &mut world,
        a,
        b,
        Vec2::new(0.75, 0.0),
        Vec2::new(-0.75, 0.0),
    )?;
    step_n(&mut world, 3)?;
    assert_eq!(
        world.manifolds().len(),
        2,
        "a-c and b-c touch, a-b is joined"
    );
    let _ = c;
    Ok(())
}

#[test]
fn the_contact_exemption_ends_with_the_joint() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let a = ball(&mut world, 1.0, Vec2::new(0.0, 0.0))?;
    let b = ball(&mut world, 1.0, Vec2::new(1.5, 0.0))?;
    let first = hinge(
        &mut world,
        a,
        b,
        Vec2::new(0.75, 0.0),
        Vec2::new(-0.75, 0.0),
    )?;
    let second = hinge(
        &mut world,
        a,
        b,
        Vec2::new(0.75, 0.0),
        Vec2::new(-0.75, 0.0),
    )?;
    step_n(&mut world, 1)?;
    assert_eq!(world.manifolds().len(), 0);
    world.destroy_joint(first)?;
    step_n(&mut world, 1)?;
    assert_eq!(
        world.manifolds().len(),
        0,
        "the second joint still forbids it"
    );
    world.destroy_joint(second)?;
    world.find_pairs();
    world.collide();
    assert_eq!(world.manifolds().len(), 1, "no joint, the contact is back");
    Ok(())
}

#[test]
fn destroying_a_joint_frees_only_its_own_pair_whatever_its_place_in_the_list()
-> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let a = ball(&mut world, 1.0, Vec2::new(0.0, 0.0))?;
    let b = ball(&mut world, 1.0, Vec2::new(1.5, 0.0))?;
    let c = ball(&mut world, 1.0, Vec2::new(0.75, 1.2))?;
    let ab = hinge(
        &mut world,
        a,
        b,
        Vec2::new(0.75, 0.0),
        Vec2::new(-0.75, 0.0),
    )?;
    let bc = hinge(&mut world, b, c, Vec2::ZERO, Vec2::new(0.75, -1.2))?;
    world.find_pairs();
    world.collide();
    assert_eq!(world.manifolds().len(), 1, "only a-c is not joined");
    // The pair of the later joint leaves the list: b-c touch again, a-b stay apart.
    world.destroy_joint(bc)?;
    world.find_pairs();
    world.collide();
    assert_eq!(world.manifolds().len(), 2, "a-c and b-c");
    // And the earlier one: a-b touch again too.
    world.destroy_joint(ab)?;
    world.find_pairs();
    world.collide();
    assert_eq!(world.manifolds().len(), 3);
    Ok(())
}

#[test]
fn joined_dynamic_bodies_share_an_island_and_a_static_end_joins_none() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let (a, b) = two_balls(&mut world)?;
    let far = ball(&mut world, 0.5, Vec2::new(40.0, 5.0))?;
    let anchor = pin(&mut world, 40.0, 8.0)?;
    hinge_between(&mut world, a, b)?;
    hinge(&mut world, anchor, far, Vec2::ZERO, Vec2::new(0.0, 3.0))?;
    step_n(&mut world, 1)?;
    assert_eq!(world.island_of(a)?, world.island_of(b)?);
    assert_ne!(world.island_of(a)?, world.island_of(far)?);
    assert_eq!(world.island_of(anchor)?, None);
    assert_eq!(world.island_count(), 2);
    Ok(())
}

#[test]
fn a_ground_body_does_not_merge_the_islands_of_two_hanging_bodies() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let g = ground(&mut world)?;
    let a = plank(&mut world, Vec2::new(-5.0, 3.0), Vec2::new(0.5, 0.5), 1.0)?;
    let b = plank(&mut world, Vec2::new(5.0, 3.0), Vec2::new(0.5, 0.5), 1.0)?;
    hinge(&mut world, g, a, Vec2::new(-5.0, 1.0), Vec2::new(0.0, -2.0))?;
    hinge(&mut world, g, b, Vec2::new(5.0, 1.0), Vec2::new(0.0, -2.0))?;
    step_n(&mut world, 1)?;
    assert_ne!(world.island_of(a)?, world.island_of(b)?);
    Ok(())
}

#[test]
fn creating_and_destroying_a_joint_wakes_a_sleeping_island() -> Result<(), PhysicsError> {
    let mut world = World::new(config(8, 8, Vec2::ZERO, SleepConfig::DEFAULT))?;
    let (a, b) = two_balls(&mut world)?;
    step_n(&mut world, 60)?;
    assert_eq!(world.sleeping_count(), 2);
    let joint = hinge_between(&mut world, a, b)?;
    assert_eq!(world.sleeping_count(), 0, "a new joint wakes both bodies");
    step_n(&mut world, 60)?;
    assert_eq!(world.sleeping_count(), 2);
    world.destroy_joint(joint)?;
    assert_eq!(
        world.sleeping_count(),
        0,
        "losing a joint wakes both bodies"
    );
    Ok(())
}
