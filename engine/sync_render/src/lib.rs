//! Layer 2 glue crate (design: `wiki/architecture/ecs-and-sandbox-v0.md` section 4). It is the
//! only code that knows the ECS and the render crate together: an entity with a
//! [`SpriteRenderer`] and a world transform becomes one sprite of the [`render::scene::RenderScene`],
//! the plain snapshot the renderer consumes. The renderer never sees the ECS. No `unsafe` code.

pub mod error;
pub mod extract;
pub mod sprite;

pub use error::SyncError;
pub use extract::{ExtractReport, extract_sprites};
pub use sprite::SpriteRenderer;
