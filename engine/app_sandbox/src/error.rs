//! Errors of the sandbox: values, never panics.

use core::fmt;

/// Why a frame or a start of the sandbox failed.
#[derive(Debug)]
pub enum Error {
    /// The ECS refused.
    Ecs(ecs::EcsError),
    /// The scene update refused.
    Scene(scene::SceneError),
    /// The physics glue refused.
    Body(sync_physics::SyncError),
    /// The physics world refused.
    Physics(physics::d2::PhysicsError),
    /// The render extraction refused.
    Extract(sync_render::SyncError),
    /// The audio engine refused.
    Audio(audio::engine::EngineError),
    /// The sound of an impact could not be made.
    Sound(audio::sound::SoundError),
    /// The arena could not be built.
    Arena(app_physics_sample::Error),
    /// The render side or the device failed, as text.
    Render(String),
    /// The view has a zero side.
    NoView,
}

macro_rules! from_error {
    ($($source:ty => $variant:ident),* $(,)?) => {$(
        impl From<$source> for Error {
            fn from(error: $source) -> Self {
                Self::$variant(error)
            }
        }
    )*};
}

from_error! {
    ecs::EcsError => Ecs,
    scene::SceneError => Scene,
    sync_physics::SyncError => Body,
    physics::d2::PhysicsError => Physics,
    sync_render::SyncError => Extract,
    audio::engine::EngineError => Audio,
    audio::sound::SoundError => Sound,
    app_physics_sample::Error => Arena,
}

impl From<app_sprite_sample::SampleError> for Error {
    fn from(error: app_sprite_sample::SampleError) -> Self {
        Self::Render(error.to_string())
    }
}

impl From<render::error::RenderError> for Error {
    fn from(error: render::error::RenderError) -> Self {
        Self::Render(error.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ecs(error) => write!(f, "ecs: {error}"),
            Self::Scene(error) => write!(f, "scene: {error}"),
            Self::Body(error) => write!(f, "body: {error}"),
            Self::Physics(error) => write!(f, "physics: {error:?}"),
            Self::Extract(error) => write!(f, "render extraction: {error}"),
            Self::Audio(error) => write!(f, "audio: {error:?}"),
            Self::Sound(error) => write!(f, "sound: {error:?}"),
            Self::Arena(error) => write!(f, "arena: {error}"),
            Self::Render(text) => write!(f, "render: {text}"),
            Self::NoView => f.write_str("the view has a zero side"),
        }
    }
}

impl std::error::Error for Error {}
