//! A game event as data, and the queue it waits in until the frame plays it.

use crate::error::SyncError;
use audio::bus::BusId;
use audio::engine::SoundHandle;
use ecs::Entity;

/// One sound to play: what, how loud, how high, on which bus, and where it comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundCue {
    /// The registered sound.
    pub sound: SoundHandle,
    /// The entity the sound comes from; its world position pans the sound. `None` plays it in
    /// the centre.
    pub source: Option<Entity>,
    /// Linear gain, finite and not negative.
    pub gain: f32,
    /// Playback speed ratio, see `audio::mixer::PlayParams`.
    pub pitch: f32,
    /// The bus to play into.
    pub bus: BusId,
}

impl SoundCue {
    /// A cue of `sound` with gain one, pitch one, on the master bus, without a source.
    #[must_use]
    pub fn new(sound: SoundHandle) -> Self {
        Self {
            sound,
            source: None,
            gain: 1.0,
            pitch: 1.0,
            bus: BusId::MASTER,
        }
    }

    /// The same cue coming from `source`.
    #[must_use]
    pub fn at(self, source: Entity) -> Self {
        Self {
            source: Some(source),
            ..self
        }
    }
}

/// The cues waiting for the next frame, in the order they were pushed. Fixed capacity, allocated
/// once in [`CueQueue::new`].
pub struct CueQueue {
    cues: Vec<SoundCue>,
    capacity: usize,
}

impl CueQueue {
    /// A queue for up to `capacity` cues.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            cues: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Adds a cue after the ones already waiting.
    ///
    /// # Errors
    /// [`SyncError::QueueFull`] when the queue is full; nothing changes.
    pub fn push(&mut self, cue: SoundCue) -> Result<(), SyncError> {
        if self.cues.len() >= self.capacity {
            return Err(SyncError::QueueFull);
        }
        self.cues.push(cue);
        Ok(())
    }

    /// How many cues wait.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cues.len()
    }

    /// True when no cue waits.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }

    /// How many cues the queue holds at most.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn drain(&mut self) -> std::vec::Drain<'_, SoundCue> {
        self.cues.drain(..)
    }
}
