//! The world against a plain model: a long random history of spawns, despawns, inserts and removes
//! must leave every entity with exactly the components the model says, and every despawned handle
//! stale, whatever rows `swap_remove` shuffled.

use ecs::{EcsError, Entity, World};

#[derive(Debug, Clone, Copy, PartialEq)]
struct A(i64);
#[derive(Debug, Clone, Copy, PartialEq)]
struct B(i64);
#[derive(Debug, Clone, Copy, PartialEq)]
struct C(i64);

#[derive(Default, Clone, Copy, PartialEq, Debug)]
struct Row {
    a: Option<i64>,
    b: Option<i64>,
    c: Option<i64>,
}

/// A small deterministic generator (xorshift64*), so a failing seed reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap_or(1)).unwrap_or(0)
    }
}

struct Pair {
    world: World,
    model: Vec<(Entity, Row)>,
    dead: Vec<Entity>,
}

fn key(entity: Entity) -> (u32, u32) {
    (entity.index(), entity.generation())
}

impl Pair {
    fn live(&self, pick: usize) -> Option<Entity> {
        self.model.get(pick).map(|(entity, _)| *entity)
    }

    fn step(&mut self, rng: &mut Rng, serial: i64) {
        match rng.below(8) {
            0 | 1 => self.spawn(),
            2 => self.despawn(rng),
            3 | 4 => self.insert(rng, serial),
            _ => self.remove(rng),
        }
    }

    fn spawn(&mut self) {
        let entity = self.world.spawn().unwrap_or_else(|e| panic!("spawn: {e}"));
        self.model.push((entity, Row::default()));
    }

    fn despawn(&mut self, rng: &mut Rng) {
        let Some(entity) = self.live(rng.below(self.model.len().max(1))) else {
            return;
        };
        assert_eq!(self.world.despawn(entity), Ok(()));
        self.model.retain(|(live, _)| *live != entity);
        self.dead.push(entity);
    }

    fn insert(&mut self, rng: &mut Rng, serial: i64) {
        let Some(entity) = self.live(rng.below(self.model.len().max(1))) else {
            return;
        };
        let Some((_, row)) = self.model.iter_mut().find(|(live, _)| *live == entity) else {
            return;
        };
        let which = rng.below(3);
        let done = match which {
            0 => self.world.insert(entity, A(serial)),
            1 => self.world.insert(entity, B(serial)),
            _ => self.world.insert(entity, C(serial)),
        };
        assert_eq!(done, Ok(()));
        match which {
            0 => row.a = Some(serial),
            1 => row.b = Some(serial),
            _ => row.c = Some(serial),
        }
    }

    fn remove(&mut self, rng: &mut Rng) {
        let Some(entity) = self.live(rng.below(self.model.len().max(1))) else {
            return;
        };
        let Some((_, row)) = self.model.iter_mut().find(|(live, _)| *live == entity) else {
            return;
        };
        let which = rng.below(3);
        let (had, done) = match which {
            0 => (row.a.take().is_some(), self.world.remove::<A>(entity)),
            1 => (row.b.take().is_some(), self.world.remove::<B>(entity)),
            _ => (row.c.take().is_some(), self.world.remove::<C>(entity)),
        };
        let expected = if had {
            Ok(())
        } else {
            Err(EcsError::MissingComponent)
        };
        assert_eq!(done, expected);
    }

    fn check(&self) {
        assert_eq!(self.world.len(), self.model.len());
        for (entity, row) in &self.model {
            assert!(self.world.is_alive(*entity));
            assert_eq!(
                self.world.get::<A>(*entity).map(|v| v.0),
                row.a,
                "{:?}",
                key(*entity)
            );
            assert_eq!(
                self.world.get::<B>(*entity).map(|v| v.0),
                row.b,
                "{:?}",
                key(*entity)
            );
            assert_eq!(
                self.world.get::<C>(*entity).map(|v| v.0),
                row.c,
                "{:?}",
                key(*entity)
            );
        }
        for entity in &self.dead {
            assert!(!self.world.is_alive(*entity));
            assert!(self.world.get::<A>(*entity).is_none());
        }
        let mut stored: Vec<Entity> = self.world.entities().collect();
        stored.sort_by_key(|e| key(*e));
        let mut expected_sorted: Vec<Entity> = self.model.iter().map(|(e, _)| *e).collect();
        expected_sorted.sort_by_key(|e| key(*e));
        assert_eq!(stored, expected_sorted);
    }
}

fn run(seed: u64, steps: i64) {
    let mut rng = Rng(seed);
    let mut pair = Pair {
        world: World::new(),
        model: Vec::new(),
        dead: Vec::new(),
    };
    for serial in 0..steps {
        pair.step(&mut rng, serial);
        pair.check();
    }
}

#[test]
fn random_histories_match_the_model() {
    for seed in [1_u64, 2, 3, 0xDEAD_BEEF, 0x1234_5678_9ABC_DEF1] {
        run(seed, 600);
    }
}
