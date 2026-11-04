//! After `World::new`, nothing the world does allocates: steps, body creation and removal within
//! the capacity, lookups, setters, iteration and the state hash make no allocator call on the
//! calling thread (counting allocator with per-thread counters, as the jobs and render tests do).

use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, PhysicsError, Rot2, STANDARD_GRAVITY, Shape2, World,
    WorldConfig,
};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const CAPACITY: u32 = 64;
const SLOTS: usize = 64;
const DT: f32 = 1.0 / 60.0;

fn build_world() -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: CAPACITY,
        gravity: STANDARD_GRAVITY,
    })
}

fn box_desc(x: f32, y: f32) -> BodyDesc {
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(x, y),
        Shape2::Box {
            half_extents: Vec2::new(0.5, 0.25),
        },
    );
    desc.rotation = Rot2::from_angle(0.3);
    desc.angular_velocity = 1.0;
    desc.linear_damping = 0.1;
    desc
}

fn fill(world: &mut World) -> Result<Vec<BodyHandle>, PhysicsError> {
    (0..CAPACITY)
        .map(|i| world.create_body(&box_desc(f32::from(u8::try_from(i).unwrap_or(0)), 10.0)))
        .collect()
}

#[test]
fn the_counter_sees_an_allocation() {
    let (world, counts) = count_allocations(build_world);
    assert!(world.is_ok());
    assert!(
        counts.calls() >= 1,
        "world creation allocates; if the counter reads zero it is not installed"
    );
}

#[test]
fn a_step_in_steady_state_makes_no_allocator_call() -> Result<(), PhysicsError> {
    let mut world = build_world()?;
    fill(&mut world)?;
    world.step(DT)?; // warm-up
    let (result, counts) = count_allocations(|| {
        for _ in 0..1000 {
            world.step(DT)?;
        }
        Ok::<(), PhysicsError>(())
    });
    assert_eq!(result, Ok(()));
    assert_eq!(counts.calls(), 0, "{counts:?}");
    Ok(())
}

/// One round of everything the world offers, on a full world: create to the capacity, set, push,
/// read, step, hash, iterate, hit the capacity, destroy all. Handles are kept in an array, so the
/// test itself allocates nothing either.
fn churn(world: &mut World, handles: &mut [Option<BodyHandle>]) -> Result<u64, PhysicsError> {
    let desc = box_desc(0.0, 5.0);
    let mut sum = 0_u64;
    for (i, slot) in handles.iter_mut().enumerate() {
        let handle = world.create_body(&desc)?;
        world.set_linear_velocity(
            handle,
            Vec2::new(1.0, f32::from(u8::try_from(i).unwrap_or(0))),
        )?;
        world.set_angular_velocity(handle, 2.0)?;
        world.apply_force(handle, Vec2::new(0.0, 3.0))?;
        world.apply_torque(handle, 4.0)?;
        sum = sum.wrapping_add(u64::from(world.body(handle)?.position().x.to_bits()));
        *slot = Some(handle);
    }
    world.step(DT)?;
    sum = sum.wrapping_add(world.state_hash());
    sum = sum.wrapping_add(world.iter().count() as u64);
    assert_eq!(
        world.create_body(&desc).err(),
        Some(PhysicsError::CapacityExhausted)
    );
    for slot in handles.iter_mut() {
        if let Some(handle) = slot.take() {
            world.destroy_body(handle)?;
        }
    }
    Ok(sum)
}

#[test]
fn creating_destroying_stepping_hashing_and_querying_make_no_allocator_call()
-> Result<(), PhysicsError> {
    let mut world = build_world()?;
    let mut handles = [None; SLOTS];
    churn(&mut world, &mut handles)?; // warm-up
    let (result, counts) = count_allocations(|| {
        let mut sum = 0_u64;
        for _ in 0..20 {
            sum = sum.wrapping_add(churn(&mut world, &mut handles)?);
        }
        Ok::<u64, PhysicsError>(sum)
    });
    assert!(result.is_ok());
    assert_eq!(counts.calls(), 0, "{counts:?}");
    assert!(world.is_empty());
    Ok(())
}
