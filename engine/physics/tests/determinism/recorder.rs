//! The replay recorder: runs a scene under one execution configuration and keeps, after every
//! step, the state hash and a dump of the bit pattern of every live body. Two traces are compared
//! step by step; a difference is reported as the first step, the body and the field that differ,
//! so a red run says where the order or the thread count leaked in, not only that it did.

use super::scenes::Scene;
use physics::d2::{PhysicsError, SolverExecution, SolverStats, World};
use std::fmt;

/// The time step of every replay: 60 Hz.
pub const DT: f32 = 1.0 / 60.0;

/// The fields of the dump of one body, in order: slot, generation, position, rotation pair,
/// linear velocity, angular velocity and the sleep flag. Floats are dumped as their bit patterns.
pub const FIELDS: [&str; 10] = [
    "slot",
    "generation",
    "position.x",
    "position.y",
    "rotation.cos",
    "rotation.sin",
    "velocity.x",
    "velocity.y",
    "angular velocity",
    "asleep",
];

/// The dump of one body.
pub type BodyBits = [u32; FIELDS.len()];

/// What is kept of one step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// `World::state_hash` after the step (it also covers the warm start of the solver).
    pub hash: u64,
    /// `World::sleeping_count` after the step.
    pub asleep: usize,
    /// The counters of the solve of the step: counts of work, equal in every configuration.
    pub stats: SolverStats,
    /// The live bodies in slot order.
    pub bodies: Vec<BodyBits>,
}

/// The samples of a run, one per step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    pub samples: Vec<Sample>,
}

/// The first place where two traces differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    pub step: usize,
    /// What differs: a field of a body, or why the dumps cannot be compared.
    pub what: String,
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "first difference at step {}: {}", self.step, self.what)
    }
}

fn dump(world: &World) -> Vec<BodyBits> {
    world
        .iter()
        .map(|(handle, body)| {
            let (cos, sin) = body.rotation().direction();
            let (position, velocity) = (body.position(), body.linear_velocity());
            [
                handle.index(),
                handle.generation(),
                position.x.to_bits(),
                position.y.to_bits(),
                cos.to_bits(),
                sin.to_bits(),
                velocity.x.to_bits(),
                velocity.y.to_bits(),
                body.angular_velocity().to_bits(),
                u32::from(body.is_asleep()),
            ]
        })
        .collect()
}

/// The sample of `world` as it is now.
fn sample(world: &World) -> Sample {
    Sample {
        hash: world.state_hash(),
        asleep: world.sleeping_count(),
        stats: world.solver_stats(),
        bodies: dump(world),
    }
}

/// Runs `scene` for its steps under `execution` and records every step.
///
/// # Errors
/// Any error of the world: a scene of the harness never makes one.
pub fn record(scene: &Scene, execution: &SolverExecution) -> Result<Trace, PhysicsError> {
    let mut world = (scene.build)()?;
    world.set_execution(execution.clone());
    let mut samples = Vec::new();
    for step in 0..scene.steps {
        if let Some(events) = scene.events {
            events(&mut world, step)?;
        }
        world.step(DT)?;
        samples.push(sample(&world));
    }
    Ok(Trace { samples })
}

fn body_difference(left: &[BodyBits], right: &[BodyBits]) -> Option<String> {
    if left.len() != right.len() {
        return Some(format!(
            "{} live bodies against {}",
            left.len(),
            right.len()
        ));
    }
    left.iter()
        .zip(right)
        .enumerate()
        .find(|(_, (a, b))| a != b)
        .map(|(position, (a, b))| {
            let field = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(0);
            format!(
                "body {position} (slot {}), {}: {:#010x} against {:#010x}",
                a[0], FIELDS[field], a[field], b[field]
            )
        })
}

impl Trace {
    /// The first step, body and field where `self` and `other` differ; `None` when equal.
    #[must_use]
    pub fn first_difference(&self, other: &Self) -> Option<Difference> {
        if self.samples.len() != other.samples.len() {
            return Some(Difference {
                step: self.samples.len().min(other.samples.len()),
                what: format!(
                    "{} steps against {}",
                    self.samples.len(),
                    other.samples.len()
                ),
            });
        }
        self.samples
            .iter()
            .zip(&other.samples)
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(step, (a, b))| Difference {
                step,
                what: body_difference(&a.bodies, &b.bodies).unwrap_or_else(|| {
                    format!(
                        "the bodies are equal but the state hash is {:#018x} against {:#018x} \
                         or the sleeping count is {} against {}",
                        a.hash, b.hash, a.asleep, b.asleep
                    )
                }),
            })
    }
}

/// Compares `got` with `reference` and describes the first difference with the names of both
/// runs; `Ok` when they are equal.
///
/// # Errors
/// The message of the first difference.
pub fn same(reference: &Trace, got: &Trace, context: &str) -> Result<(), String> {
    match reference.first_difference(got) {
        None => Ok(()),
        Some(difference) => Err(format!("{context}: {difference}")),
    }
}
