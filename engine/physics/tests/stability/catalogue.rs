//! The scene catalogue: the settled scenes of the other modules by name, each as a function of
//! the sleep configuration, for the tests that run every scene (the execution matrix and the
//! sleeping tests).

use super::chains::{chain_scene, loaded_bridge};
use super::mixed::mixed;
use super::scene::{Lie, Scene, W1_HALF, column, pyramid};
use physics::d2::{PhysicsError, SleepConfig};

/// Builds a scene with the given sleep configuration.
pub type Build = fn(SleepConfig) -> Result<Scene, PhysicsError>;

/// A scene and its name.
pub struct Entry {
    pub name: &'static str,
    pub build: Build,
}

/// The height of the column of the catalogue.
const COLUMN_HEIGHT: usize = 10;

fn pyramid_ten(sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    pyramid(10, W1_HALF, sleep)
}

fn column_ten(sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    column(&[1.0; COLUMN_HEIGHT], W1_HALF, sleep)
}

fn heavy_between_light(sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    column(&[1.0, 100.0, 1.0], W1_HALF, sleep)
}

fn whip_twenty(sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    chain_scene(20, Lie::Horizontal, 1.0, sleep)
}

fn hanging_twenty(sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    chain_scene(20, Lie::Hanging, 1.0, sleep)
}

fn bridge_with_load(sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    loaded_bridge(1.0, sleep)
}

fn mixed_whip(sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    mixed(Lie::Horizontal, sleep).map(|mixed| mixed.scene)
}

fn mixed_resting(sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    mixed(Lie::Hanging, sleep).map(|mixed| mixed.scene)
}

/// Every scene of the catalogue.
pub const ALL: [Entry; 8] = [
    Entry {
        name: "pyramid 10 rows",
        build: pyramid_ten,
    },
    Entry {
        name: "column of 10",
        build: column_ten,
    },
    Entry {
        name: "mass ratio 1:100:1",
        build: heavy_between_light,
    },
    Entry {
        name: "whip 20 links",
        build: whip_twenty,
    },
    Entry {
        name: "hanging chain 20 links",
        build: hanging_twenty,
    },
    Entry {
        name: "bridge with a load",
        build: bridge_with_load,
    },
    Entry {
        name: "mixed with a whip",
        build: mixed_whip,
    },
    Entry {
        name: "mixed at rest",
        build: mixed_resting,
    },
];

/// The scenes that come to rest and can fall asleep (indexes into [`ALL`]): the pyramid, the
/// column, the hanging chain and the mixed scene at rest.
pub const RESTING: [usize; 4] = [0, 1, 4, 7];

/// Steps that leave room for each of the [`RESTING`] scenes to fall asleep (they do at about step
/// 36, 40, 78 and 67 with the default sleep configuration).
pub const STEPS_TO_SLEEP: [u32; 4] = [60, 60, 100, 300];
