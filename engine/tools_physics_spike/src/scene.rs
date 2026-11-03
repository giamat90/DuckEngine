//! The scene and the numbers every variant shares: constants, the box list, the state hash and
//! the [`Sim`] trait the harness drives. Nothing here depends on a dimension.

/// Fixed time step in seconds.
pub const DT: f32 = 1.0 / 60.0;
/// Velocity iterations per step.
pub const ITERATIONS: usize = 10;
/// Gravity along Y in m/s^2.
pub const GRAVITY_Y: f32 = -9.81;
/// Coulomb friction coefficient.
pub const FRICTION: f32 = 0.5;
/// Share of the penetration removed per step through the velocity bias.
pub const BIAS_FACTOR: f32 = 0.2;
/// Penetration tolerated without bias, in metres.
pub const PENETRATION_SLOP: f32 = 0.01;
/// Half extent of a dynamic box, in metres.
pub const BOX_HALF: f32 = 0.5;
/// Half length of the ground box along X.
pub const GROUND_HALF_X: f32 = 50.0;
/// Half extent along Z of every box in the 3D variants (the depth of the extruded scene).
#[cfg(any(feature = "variant-a", feature = "variant-b3"))]
pub const DEPTH_HALF: f32 = 0.5;
/// Horizontal pitch of a pyramid row, a little more than one box so side faces do not touch.
pub const ROW_PITCH: f32 = 1.05;

/// One box of the scene. Rotation is zero at the start.
#[derive(Clone, Copy, Debug)]
pub struct BoxDef {
    /// Centre X.
    pub x: f32,
    /// Centre Y.
    pub y: f32,
    /// Half extent along X.
    pub half_x: f32,
    /// Half extent along Y.
    pub half_y: f32,
    /// False for the ground.
    pub dynamic: bool,
}

/// The scenes the harness knows.
#[derive(Clone, Copy, Debug)]
pub enum SceneKind {
    /// A pyramid of this many rows (`rows * (rows + 1) / 2` boxes).
    Pyramid(u16),
    /// A column of this many boxes.
    Column(u16),
}

fn ground() -> BoxDef {
    BoxDef {
        x: 0.0,
        y: -BOX_HALF,
        half_x: GROUND_HALF_X,
        half_y: BOX_HALF,
        dynamic: false,
    }
}

fn unit_box(x: f32, y: f32) -> BoxDef {
    BoxDef {
        x,
        y,
        half_x: BOX_HALF,
        half_y: BOX_HALF,
        dynamic: true,
    }
}

/// The ground first, then the dynamic boxes row by row from the bottom.
#[must_use]
pub fn build(kind: SceneKind) -> Vec<BoxDef> {
    let mut defs = vec![ground()];
    let rows: Vec<(u16, u16)> = match kind {
        SceneKind::Pyramid(rows) => (0..rows).map(|row| (row, rows - row)).collect(),
        SceneKind::Column(count) => (0..count).map(|row| (row, 1)).collect(),
    };
    for (row, count) in rows {
        let left = -f32::from(count - 1) * 0.5 * ROW_PITCH;
        defs.extend(
            (0..count)
                .map(|i| unit_box(left + f32::from(i) * ROW_PITCH, BOX_HALF + f32::from(row))),
        );
    }
    defs
}

/// Every pair `a < b` of `count` bodies, in a fixed order.
pub fn pairs(count: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..count).flat_map(move |a| ((a + 1)..count).map(move |b| (a, b)))
}

/// FNV-1a over the bit patterns of `f32` values (the hash of the design note, section 7).
pub struct Fnv(u64);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

impl Fnv {
    /// A fresh hash.
    #[must_use]
    pub const fn new() -> Self {
        Self(FNV_OFFSET)
    }

    /// Mixes in the four bytes of `value`.
    pub fn push(&mut self, value: f32) {
        for byte in value.to_bits().to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    /// The hash so far.
    #[must_use]
    pub const fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for Fnv {
    fn default() -> Self {
        Self::new()
    }
}

/// What the harness needs from a variant. Body 0 is the ground.
pub trait Sim {
    /// Builds the world from the scene.
    fn from_scene(defs: &[BoxDef]) -> Self;
    /// Advances one fixed step.
    fn step(&mut self);
    /// Bytes of one stored body (`size_of`).
    fn body_bytes() -> usize;
    /// Bytes of one contact constraint (`size_of`).
    fn contact_bytes() -> usize;
    /// Centre of body `i` in the XY plane.
    fn pos_xy(&self, i: usize) -> [f32; 2];
    /// Linear speed of body `i`.
    fn linear_speed(&self, i: usize) -> f32;
    /// Angular speed of body `i`.
    fn angular_speed(&self, i: usize) -> f32;
    /// Deepest penetration seen by the narrowphase in the last step (zero when none).
    fn max_penetration(&self) -> f32;
    /// Number of contact points solved in the last step.
    fn contact_count(&self) -> usize;
    /// Hash of every body's state.
    fn state_hash(&self) -> u64;
}
