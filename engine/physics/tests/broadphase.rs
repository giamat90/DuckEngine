//! The broadphase against a brute-force oracle, through the public 2D API: over random scenes of
//! every size and density, the pair list must equal the pairs found by testing every body against
//! every other (the oracle), as a set and in the canonical order, after creation, after steps with
//! moving bodies, and after destroying and creating bodies.
//!
//! The oracle works on the boxes the world reports (`broadphase_bounds`: the tight box grown by the
//! margin when it was stored), because the stored box depends on the history of the body. It uses its
//! own overlap test, not the one of the code under test. A second check, history independent,
//! says that no pair of bodies whose tight boxes overlap is ever missing.

use base::math::Vec2;
use base::rng::Pcg32;
use physics::d2::{
    Aabb2, BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    PhysicsError, Rot2, Shape2, SolverConfig, World, WorldConfig,
};

const DT: f32 = 1.0 / 30.0;
const TWO_PI: f32 = std::f32::consts::TAU;

type Pairs = Vec<(u32, u32)>;

fn world(max_bodies: u32, max_pairs: u32, margin: f32) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies,
        gravity: Vec2::ZERO,
        max_pairs,
        broadphase_margin: margin,
        max_manifolds: 256,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DEFAULT,
    })
}

/// The rule of the oracle: closed boxes, so touching counts.
fn overlap(a: &Aabb2, b: &Aabb2) -> bool {
    a.min.x <= b.max.x && b.min.x <= a.max.x && a.min.y <= b.max.y && b.min.y <= a.max.y
}

fn is_static(world: &World, handle: BodyHandle) -> Result<bool, PhysicsError> {
    Ok(world.body(handle)?.kind() == BodyKind::Static)
}

/// Every pair of live bodies, lower slot first, that are not both static and whose boxes overlap,
/// found by testing all of them against all of them. `tight` tests the bodies' own boxes instead of
/// the stored ones.
fn oracle(world: &World, tight: bool) -> Result<Pairs, PhysicsError> {
    let mut live = Vec::new();
    for (handle, body) in world.iter() {
        let bounds = if tight {
            body.bounds()
        } else {
            world.broadphase_bounds(handle)?
        };
        live.push((handle.index(), is_static(world, handle)?, bounds));
    }
    let mut pairs = Vec::new();
    for (i, a) in live.iter().enumerate() {
        pairs.extend(partners(a, &live[i + 1..]));
    }
    Ok(pairs)
}

/// The pairs of `a` with the bodies of `later` it touches (not when both are static).
fn partners<'a>(
    a: &'a (u32, bool, Aabb2),
    later: &'a [(u32, bool, Aabb2)],
) -> impl Iterator<Item = (u32, u32)> + 'a {
    later
        .iter()
        .filter(move |b| !(a.1 && b.1) && overlap(&a.2, &b.2))
        .map(move |b| (a.0, b.0))
}

fn listed(world: &World) -> Pairs {
    world
        .pairs()
        .as_slice()
        .iter()
        .map(|pair| (pair.first.index(), pair.second.index()))
        .collect()
}

fn random_desc(rng: &mut Pcg32, spread: f32, static_one_in: u32) -> BodyDesc {
    let kind = if rng.below(static_one_in).unwrap_or(1) == 0 {
        BodyKind::Static
    } else {
        BodyKind::Dynamic
    };
    let size = 0.2 + rng.next_f32();
    let shape = if rng.below(2).unwrap_or(0) == 0 {
        Shape2::Circle { radius: size }
    } else {
        Shape2::Box {
            half_extents: Vec2::new(size, 0.2 + rng.next_f32()),
        }
    };
    let position = Vec2::new(rng.next_f32() * spread, rng.next_f32() * spread);
    let mut desc = BodyDesc::new(kind, position, shape);
    desc.rotation = Rot2::from_angle(rng.next_f32() * TWO_PI);
    desc.linear_velocity = Vec2::new(rng.next_f32() - 0.5, rng.next_f32() - 0.5) * 4.0;
    desc.angular_velocity = rng.next_f32() - 0.5;
    desc
}

fn random_world(
    seed: u64,
    count: u32,
    spread: f32,
    margin: f32,
) -> Result<(World, Vec<BodyHandle>), PhysicsError> {
    let mut rng = Pcg32::new(seed, 7);
    let mut world = world(count.max(1), count * count + 1, margin)?;
    let mut handles = Vec::new();
    for _ in 0..count {
        handles.push(world.create_body(&random_desc(&mut rng, spread, 4))?);
    }
    Ok((world, handles))
}

fn check_against_oracle(world: &World, what: &str) -> Result<(), PhysicsError> {
    assert!(world.pairs().is_complete(), "{what}: list too small");
    let expected = oracle(world, false)?;
    assert_eq!(listed(world), expected, "{what}");
    let listed = listed(world);
    for pair in oracle(world, true)? {
        assert!(
            listed.contains(&pair),
            "{what}: tight pair {pair:?} missing"
        );
    }
    Ok(())
}

#[test]
fn random_scenes_of_every_size_and_density_match_the_oracle_exactly() -> Result<(), PhysicsError> {
    let mut total = 0;
    for count in [0, 1, 2, 3, 5, 12, 40, 90] {
        for (spread, margin) in [
            (3.0, 0.0),
            (12.0, 0.0),
            (60.0, 0.0),
            (12.0, 0.4),
            (60.0, 1.5),
        ] {
            total += check_seeds(count, spread, margin)?;
        }
    }
    assert!(total > 5000, "the scenes must contain pairs: {total}");
    Ok(())
}

/// Checks six random scenes of one size, density and margin; returns the number of pairs found.
fn check_seeds(count: u32, spread: f32, margin: f32) -> Result<usize, PhysicsError> {
    let mut total = 0;
    for seed in 0..6 {
        let (mut world, _) = random_world(seed, count, spread, margin)?;
        world.find_pairs();
        check_against_oracle(&world, &format!("n {count} spread {spread} seed {seed}"))?;
        total += world.pairs().len();
    }
    Ok(total)
}

#[test]
fn nothing_overlapping_gives_an_empty_list_and_everything_overlapping_gives_every_pair()
-> Result<(), PhysicsError> {
    let mut apart = world(20, 400, 0.0)?;
    let mut piled = world(20, 400, 0.0)?;
    for i in 0..20_u8 {
        let unit = Shape2::Circle { radius: 0.5 };
        apart.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::new(f32::from(i) * 3.0, 0.0),
            unit,
        ))?;
        piled.create_body(&BodyDesc::new(BodyKind::Dynamic, Vec2::ZERO, unit))?;
    }
    apart.find_pairs();
    piled.find_pairs();
    assert!(apart.pairs().is_empty());
    assert_eq!(piled.pairs().len(), 20 * 19 / 2);
    assert_eq!(listed(&piled), oracle(&piled, false)?);
    assert_eq!(listed(&piled).first(), Some(&(0, 1)));
    assert_eq!(listed(&piled).last(), Some(&(18, 19)));
    Ok(())
}

#[test]
fn a_grid_of_boxes_that_touch_exactly_pairs_every_neighbour_including_diagonals()
-> Result<(), PhysicsError> {
    let mut grid = world(100, 2000, 0.0)?;
    for y in 0..10_u8 {
        for x in 0..10_u8 {
            grid.create_body(&BodyDesc::new(
                BodyKind::Dynamic,
                Vec2::new(f32::from(x), f32::from(y)),
                Shape2::Box {
                    half_extents: Vec2::new(0.5, 0.5),
                },
            ))?;
        }
    }
    grid.find_pairs();
    // 9 x 10 pairs along each axis and 9 x 9 x 2 diagonal ones (edges and corners touch exactly).
    assert_eq!(grid.pairs().len(), 2 * 9 * 10 + 2 * 9 * 9);
    assert_eq!(listed(&grid), oracle(&grid, false)?);
    Ok(())
}

#[test]
fn bodies_moving_for_many_steps_keep_matching_the_oracle() -> Result<(), PhysicsError> {
    let (mut total, mut changes) = (0, 0);
    for seed in 0..12 {
        let (mut world, _) = random_world(seed, 40, 14.0, DEFAULT_BROADPHASE_MARGIN)?;
        let mut before = listed(&world);
        for step in 0..60 {
            world.step(DT)?;
            check_against_oracle(&world, &format!("seed {seed} step {step}"))?;
            total += world.pairs().len();
            changes += usize::from(listed(&world) != before);
            before = listed(&world);
        }
    }
    assert!(total > 5000, "the scenes must contain pairs: {total}");
    assert!(
        changes > 100,
        "the pairs must change as bodies move: {changes}"
    );
    Ok(())
}

#[test]
fn creating_and_destroying_bodies_at_random_keeps_matching_the_oracle() -> Result<(), PhysicsError>
{
    for seed in 0..10 {
        let mut rng = Pcg32::new(seed, 3);
        let mut world = world(30, 900, 0.25)?;
        let mut live: Vec<BodyHandle> = Vec::new();
        for round in 0..120 {
            churn_once(&mut world, &mut live, &mut rng)?;
            world.find_pairs();
            check_against_oracle(&world, &format!("seed {seed} round {round}"))?;
        }
    }
    Ok(())
}

/// One random change of the world: create a body, destroy one, or step.
fn churn_once(
    world: &mut World,
    live: &mut Vec<BodyHandle>,
    rng: &mut Pcg32,
) -> Result<(), PhysicsError> {
    match rng.below(4).unwrap_or(0) {
        0 | 1 if live.len() < 30 => {
            live.push(world.create_body(&random_desc(rng, 10.0, 4))?);
            Ok(())
        }
        2 if !live.is_empty() => {
            let len = u32::try_from(live.len()).unwrap_or(0);
            let at = usize::try_from(rng.below(len).unwrap_or(0)).unwrap_or(0);
            world.destroy_body(live.swap_remove(at))
        }
        _ => world.step(DT),
    }
}

/// The pairs as pairs of positions, which do not depend on the slot a body got: every body of the
/// scene has its own x.
fn by_position(world: &World) -> Result<Vec<(u32, u32)>, PhysicsError> {
    let mut found = Vec::new();
    for pair in world.pairs().as_slice() {
        let a = world.body(pair.first)?.position().x.to_bits();
        let b = world.body(pair.second)?.position().x.to_bits();
        found.push((a.min(b), a.max(b)));
    }
    found.sort_by_key(|&pair| pair);
    Ok(found)
}

#[test]
fn the_set_of_pairs_does_not_depend_on_the_order_the_bodies_were_created_in()
-> Result<(), PhysicsError> {
    let mut rng = Pcg32::new(99, 1);
    let descs: Vec<BodyDesc> = (0..50_u8)
        .map(|i| {
            let mut desc = random_desc(&mut rng, 10.0, 4);
            desc.position.x = f32::from(i) * 0.2;
            desc
        })
        .collect();
    let build = |order: &[usize]| -> Result<Vec<(u32, u32)>, PhysicsError> {
        let mut world = world(50, 2600, 0.3)?;
        for &i in order {
            world.create_body(&descs[i])?;
        }
        world.find_pairs();
        by_position(&world)
    };
    let forward: Vec<usize> = (0..50).collect();
    let backward: Vec<usize> = (0..50).rev().collect();
    let interleaved: Vec<usize> = (0..25).flat_map(|i| [i, 49 - i]).collect();
    let reference = build(&forward)?;
    assert!(!reference.is_empty());
    assert_eq!(build(&backward)?, reference);
    assert_eq!(build(&interleaved)?, reference);
    Ok(())
}

#[test]
fn a_world_replayed_gives_the_same_pair_list_and_tree_at_every_step() -> Result<(), PhysicsError> {
    let run = || -> Result<Vec<(Pairs, u64)>, PhysicsError> {
        let (mut world, _) = random_world(5, 50, 12.0, DEFAULT_BROADPHASE_MARGIN)?;
        let mut trace = Vec::new();
        for _ in 0..40 {
            world.step(DT)?;
            trace.push((listed(&world), world.broadphase_hash()));
        }
        Ok(trace)
    };
    let first = run()?;
    assert_eq!(first, run()?);
    assert!(first.iter().any(|(pairs, _)| !pairs.is_empty()));
    Ok(())
}

#[test]
fn a_pair_list_that_is_too_small_keeps_a_subset_of_the_true_pairs_and_counts_the_rest()
-> Result<(), PhysicsError> {
    let (mut roomy, _) = random_world(3, 40, 6.0, 0.0)?;
    roomy.find_pairs();
    let all = listed(&roomy);
    assert!(all.len() > 10, "the scene must have pairs: {}", all.len());
    for capacity in [0_u32, 1, 5, 10] {
        let mut rng = Pcg32::new(3, 7);
        let mut small = world(40, capacity, 0.0)?;
        for _ in 0..40 {
            small.create_body(&random_desc(&mut rng, 6.0, 4))?;
        }
        small.find_pairs();
        let kept = listed(&small);
        assert_eq!(kept.len(), capacity as usize);
        assert_eq!(small.pairs().missing(), all.len() - kept.len());
        assert!(!small.pairs().is_complete());
        assert!(kept.iter().all(|pair| all.contains(pair)));
        assert!(
            kept.windows(2).all(|two| two[0] < two[1]),
            "sorted, no duplicates"
        );
    }
    Ok(())
}
