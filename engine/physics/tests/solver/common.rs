//! Scene-building helpers shared by the solver tests. Everything returns a `Result`, because the
//! lints deny `unwrap` and `expect`.

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    PhysicsError, Rot2, STANDARD_GRAVITY, Shape2, SleepConfig, SolverConfig, World, WorldConfig,
};

/// The fixed step of every solver scene: 60 Hz.
pub const DT: f32 = 1.0 / 60.0;

/// A world for `max_bodies` bodies with room for every pair and manifold the scenes make.
pub fn world(max_bodies: u32, gravity: Vec2) -> Result<World, PhysicsError> {
    world_with(max_bodies, gravity, SolverConfig::DEFAULT)
}

/// As [`world`] with another solver configuration.
pub fn world_with(
    max_bodies: u32,
    gravity: Vec2,
    solver: SolverConfig,
) -> Result<World, PhysicsError> {
    world_of(WorldConfig {
        solver,
        ..default_config(max_bodies, gravity)
    })
}

/// As [`world`] with another sleep configuration.
pub fn world_sleeping(
    max_bodies: u32,
    gravity: Vec2,
    sleep: SleepConfig,
) -> Result<World, PhysicsError> {
    world_of(WorldConfig {
        sleep,
        ..default_config(max_bodies, gravity)
    })
}

/// The configuration [`world`] uses.
pub fn default_config(max_bodies: u32, gravity: Vec2) -> WorldConfig {
    WorldConfig {
        max_bodies,
        gravity,
        max_pairs: 4096,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 4096,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    }
}

/// A world from a full configuration.
pub fn world_of(config: WorldConfig) -> Result<World, PhysicsError> {
    World::new(config)
}

pub fn earth() -> Vec2 {
    STANDARD_GRAVITY
}

/// A static box whose top face is the line `y = 0`, 100 m wide.
pub fn ground(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(50.0, 1.0),
        },
    ))
}

/// A dynamic box of the given half extents at `(x, y)`.
pub fn crate_of(world: &mut World, half: Vec2, x: f32, y: f32) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(x, y),
        Shape2::Box { half_extents: half },
    ))
}

/// A dynamic circle at `(x, y)`.
pub fn ball(world: &mut World, radius: f32, x: f32, y: f32) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(x, y),
        Shape2::Circle { radius },
    ))
}

pub fn step_n(world: &mut World, steps: u32) -> Result<(), PhysicsError> {
    for _ in 0..steps {
        world.step(DT)?;
    }
    Ok(())
}

pub fn position(world: &World, body: BodyHandle) -> Result<Vec2, PhysicsError> {
    Ok(world.body(body)?.position())
}

pub fn velocity(world: &World, body: BodyHandle) -> Result<Vec2, PhysicsError> {
    Ok(world.body(body)?.linear_velocity())
}

pub fn speed(world: &World, body: BodyHandle) -> Result<f32, PhysicsError> {
    Ok(velocity(world, body)?.length())
}

pub fn angle(world: &World, body: BodyHandle) -> Result<f32, PhysicsError> {
    Ok(world.body(body)?.rotation().angle())
}

/// A rotation of `angle` radians made without the platform sine: from an integer-free (cos, sin)
/// pair, which keeps scenes the same on every platform.
pub fn rotation_of(cos: f32, sin: f32) -> Rot2 {
    Rot2::from_cos_sin(cos, sin).unwrap_or(Rot2::IDENTITY)
}

/// Total kinetic plus potential energy of the dynamic bodies, in joules (gravity along -y).
pub fn energy(world: &World, gravity_y: f32) -> f64 {
    world
        .iter()
        .filter(|(_, body)| body.inverse_mass() > 0.0)
        .map(|(_, body)| {
            let mass = f64::from(body.mass());
            let v = body.linear_velocity();
            let w = f64::from(body.angular_velocity());
            let inertia = 1.0 / f64::from(body.inverse_inertia());
            let kinetic = 0.5 * mass * f64::from(v.dot(v)) + 0.5 * inertia * w * w;
            kinetic - mass * f64::from(gravity_y) * f64::from(body.position().y)
        })
        .sum()
}

/// A world whose pair and manifold lists hold only `max_pairs` and `max_manifolds` entries.
pub fn tight_world(max_pairs: u32, max_manifolds: u32) -> Result<World, PhysicsError> {
    let mut config = default_config(16, Vec2::new(0.0, -9.8));
    config.max_pairs = max_pairs;
    config.max_manifolds = max_manifolds;
    world_of(config)
}
