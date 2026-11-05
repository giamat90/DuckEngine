//! The chain, bridge and ground scenes (card 4-16). The bounds were fixed before the first
//! measurement; the measured values are in reviews/4-16.md.

use super::common::{DT, ball, config, ground, hinge_error, pin, plank};
use super::scenes::{
    Chain, LINK_HALF, bridge, chain, chain_on_ground, energy, hanging_chain, worst_hinge_error,
};
use base::math::Vec2;
use physics::d2::{PhysicsError, STANDARD_GRAVITY, SleepConfig, World};

/// The most any hinge of a long chain may come apart, in metres (0.1 m is a link thickness).
const CHAIN_ERROR: f32 = 0.05;
/// Links in the chain of the card.
const LINKS: u8 = 20;

/// A chain of `LINKS` links released horizontally from a pin 10 m up: a whip.
fn whip(last_density: f32) -> Result<(World, Chain), PhysicsError> {
    let mut world = world_for(LINKS)?;
    let anchor = pin(&mut world, 0.0, 10.0)?;
    let chain = chain(
        &mut world,
        anchor,
        Vec2::new(0.0, 10.0),
        LINKS,
        last_density,
    )?;
    Ok((world, chain))
}

fn world_for(links: u8) -> Result<World, PhysicsError> {
    World::new(config(
        u32::from(links) + 8,
        u32::from(links) + 8,
        STANDARD_GRAVITY,
        SleepConfig::DISABLED,
    ))
}

#[test]
fn a_chain_of_twenty_links_whips_without_coming_apart_or_exploding() -> Result<(), PhysicsError> {
    let (mut world, chain) = whip(1.0)?;
    let start = energy(&world, &chain.links)?;
    let mut worst = 0.0_f32;
    let mut peak_energy = start;
    let mut fastest = 0.0_f32;
    for _ in 0..600 {
        world.step(DT)?;
        worst = worst.max(worst_hinge_error(&world, &chain.joints)?);
        peak_energy = peak_energy.max(energy(&world, &chain.links)?);
        for &link in &chain.links {
            let body = world.body(link)?;
            fastest = fastest.max(body.linear_velocity().length());
            // The chain is 10 m long and hangs from (0, 10): nothing can be farther than that.
            let from_pin = (body.position() - Vec2::new(0.0, 10.0)).length();
            assert!(from_pin < 10.05, "a link is {from_pin} m from the pin");
        }
    }
    assert!(worst < CHAIN_ERROR, "worst anchor separation {worst} m");
    assert!(fastest < 60.0, "fastest link {fastest} m/s");
    assert!(
        peak_energy <= start * 1.01,
        "energy grew from {start} to {peak_energy} J"
    );
    Ok(())
}

#[test]
fn a_chain_that_swings_for_a_minute_loses_energy_and_keeps_its_joints() -> Result<(), PhysicsError>
{
    let (mut world, chain) = whip(1.0)?;
    let start = energy(&world, &chain.links)?;
    let mut worst = 0.0_f32;
    for _ in 0..3600 {
        world.step(DT)?;
        worst = worst.max(worst_hinge_error(&world, &chain.joints)?);
    }
    let end = energy(&world, &chain.links)?;
    assert!(worst < CHAIN_ERROR, "worst anchor separation {worst} m");
    assert!(end < start, "{end} J against {start} J");
    Ok(())
}

#[test]
fn a_weight_ten_times_a_link_at_the_end_of_the_chain_holds() -> Result<(), PhysicsError> {
    // The last link has density 10: 0.5 kg against 0.05 kg links.
    let (mut world, chain) = whip(10.0)?;
    let mut worst = 0.0_f32;
    for _ in 0..600 {
        world.step(DT)?;
        worst = worst.max(worst_hinge_error(&world, &chain.joints)?);
    }
    assert!(worst < CHAIN_ERROR, "worst anchor separation {worst} m");
    Ok(())
}

#[test]
fn a_bridge_between_two_pins_carries_a_dropped_load() -> Result<(), PhysicsError> {
    let mut world = world_for(24)?;
    let planks = bridge(&mut world, 8.0, 5.0, 16, 1.0)?;
    let load = plank(&mut world, Vec2::new(4.0, 6.5), Vec2::new(0.25, 0.25), 1.0)?;
    let mut worst = 0.0_f32;
    for _ in 0..480 {
        world.step(DT)?;
        worst = worst.max(worst_hinge_error(&world, &planks.joints)?);
    }
    assert!(worst < CHAIN_ERROR, "worst anchor separation {worst} m");
    let at = world.body(load)?.position();
    assert!(
        at.y > 2.0 && at.x > 2.0 && at.x < 6.0,
        "the load is at {at:?}"
    );
    Ok(())
}

#[test]
fn a_chain_sliding_on_the_ground_keeps_its_joints_and_stays_above_it() -> Result<(), PhysicsError> {
    let mut world = world_for(16)?;
    ground(&mut world)?;
    let chain = chain_on_ground(&mut world, -2.0, 10, 0.0)?;
    let first = *chain.links.first().ok_or(PhysicsError::NotFinite)?;
    world.set_linear_velocity(first, Vec2::new(3.0, 0.0))?;
    let mut worst = 0.0_f32;
    let mut lowest = f32::MAX;
    for _ in 0..300 {
        world.step(DT)?;
        worst = worst.max(worst_hinge_error(&world, &chain.joints)?);
        for &link in &chain.links {
            lowest = lowest.min(world.body(link)?.position().y);
        }
    }
    assert!(worst < 0.03, "worst anchor separation {worst} m");
    // A flat link rests with its centre 0.05 m up.
    assert!(lowest > LINK_HALF.y - 0.03, "a link sank to {lowest}");
    Ok(())
}

/// A chain of ten links dropped 0.3 m onto the ground, with sleeping on.
fn sleeping_chain() -> Result<(World, Chain), PhysicsError> {
    let mut world = World::new(config(24, 24, STANDARD_GRAVITY, SleepConfig::DEFAULT))?;
    ground(&mut world)?;
    let chain = chain_on_ground(&mut world, -2.0, 10, 0.3)?;
    Ok((world, chain))
}

/// How many links of `chain` are asleep (the world's count includes any other body).
fn sleeping_links(world: &World, chain: &Chain) -> Result<usize, PhysicsError> {
    let mut count = 0;
    for &link in &chain.links {
        count += usize::from(world.is_asleep(link)?);
    }
    Ok(count)
}

#[test]
fn a_chain_sleeps_and_wakes_as_one_island() -> Result<(), PhysicsError> {
    let (mut world, chain) = sleeping_chain()?;
    let links = chain.links.len();
    let mut slept_at = None;
    for step in 0..900 {
        world.step(DT)?;
        let asleep = world.sleeping_count();
        assert!(
            asleep == 0 || asleep == links,
            "step {step}: {asleep} of {links} asleep"
        );
        if asleep == links && slept_at.is_none() {
            slept_at = Some(step);
        }
    }
    assert!(slept_at.is_some(), "the chain never fell asleep");
    assert_eq!(world.island_count(), 1, "ten joined links are one island");
    // A ball dropped on the middle of the chain wakes all of it at once.
    let middle = world.body(chain.links[5])?.position();
    ball(&mut world, 0.2, middle + Vec2::new(0.0, 1.0))?;
    let mut woke = false;
    for _ in 0..120 {
        world.step(DT)?;
        let asleep = sleeping_links(&world, &chain)?;
        assert!(asleep == 0 || asleep == links);
        woke |= asleep == 0;
    }
    assert!(woke, "the ball did not wake the chain");
    assert!(worst_hinge_error(&world, &chain.joints)? < 0.03);
    Ok(())
}

#[test]
fn a_chain_hanging_from_a_pin_sleeps_with_its_joints_intact() -> Result<(), PhysicsError> {
    let mut world = World::new(config(24, 24, STANDARD_GRAVITY, SleepConfig::DEFAULT))?;
    let anchor = pin(&mut world, 0.0, 6.0)?;
    let chain = hanging_chain(&mut world, anchor, Vec2::new(0.0, 6.0), 6)?;
    let mut worst = 0.0_f32;
    for _ in 0..300 {
        world.step(DT)?;
        worst = worst.max(worst_hinge_error(&world, &chain.joints)?);
    }
    assert!(worst < CHAIN_ERROR, "worst anchor separation {worst} m");
    assert_eq!(world.sleeping_count(), 6, "the hanging chain is asleep");
    for &joint in &chain.joints {
        assert!(hinge_error(&world, joint)? < 0.03);
    }
    Ok(())
}

#[test]
fn waking_one_link_wakes_the_whole_chain_through_the_joints_alone() -> Result<(), PhysicsError> {
    let (mut world, chain) = sleeping_chain()?;
    let links = chain.links.len();
    for _ in 0..900 {
        world.step(DT)?;
    }
    assert_eq!(sleeping_links(&world, &chain)?, links, "the chain sleeps");
    // No contact and no force touches the chain: only the wake call on the end link.
    world.wake(chain.links[0])?;
    world.step(DT)?;
    assert_eq!(sleeping_links(&world, &chain)?, 0, "the island woke as one");
    Ok(())
}
