//! Layer 2 glue crate (design: `wiki/architecture/ecs-and-sandbox-v0.md` section 4). It is the
//! only code that knows the ECS and the audio engine together. A game event is a [`SoundCue`] in
//! a [`CueQueue`]; once a frame [`sync_audio`] turns the cues into `play` calls, pans each by
//! where its source entity is relative to the [`Listener`], and frees the voices that ended.
//! The audio crate never sees the ECS. The call reads the world transform of the source, so run
//! the scene update first, as the frame plan does; a source not updated yet is heard in the
//! centre. Call it every frame: it is what frees the voice slots of the sounds that ended, and
//! the voice of a cue is not kept, so a cue cannot be stopped or changed afterwards. No `unsafe`
//! code.

pub mod cue;
pub mod error;
pub mod listener;
pub mod sync;

pub use cue::{CueQueue, SoundCue};
pub use error::SyncError;
pub use listener::Listener;
pub use sync::{SyncReport, sync_audio};
