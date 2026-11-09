//! The game-side handle API and the audio-side processor, joined by two lock-free queues.
//!
//! [`engine`] builds a pair. The [`AudioEngine`] stays on the game thread: it owns the sounds, makes
//! the voice handles and sends commands. The [`AudioProcessor`] is an [`AudioRenderer`] that
//! moves to the device; it owns the [`Mixer`], applies the commands at the start of each block and
//! reports ended voices as events. Design: `audio-port-v0` sections 4 and 15.
//!
//! The processor must be dropped where freeing is allowed (the device port gives it back from
//! `stop` for that reason): dropping it releases the sounds of the voices and commands it still
//! holds.
//!
//! The rule that keeps the audio thread free of allocation: the audio side never holds the last
//! reference to a sound. The game side keeps one until `unregister_sound`, and every voice hands its
//! reference back inside an event, to be dropped on the game thread.

use crate::bus::BusId;
use crate::device::AudioRenderer;
use crate::format::StreamFormat;
use crate::mixer::{
    Mixer, MixerError, PlayParams, VoiceHandle, valid_gain, valid_pan, valid_params, valid_pitch,
};
use crate::sound::Sound;
use base::pool::{Handle, Pool};
use base::spsc::{Consumer, Producer, channel};
use core::fmt;
use std::sync::Arc;

/// Marker for the voice handles the game side makes.
#[derive(Clone, Copy, Debug)]
pub struct Playing;

/// A registered sound.
pub type SoundHandle = Handle<Arc<Sound>>;

/// A voice that was started with [`AudioEngine::play`]. It stays valid until
/// [`AudioEngine::poll_finished`] reports the end of the voice.
pub type VoiceId = Handle<Playing>;

/// Sizes fixed when the engine is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    /// Voices that can be started and not yet reported finished.
    pub max_voices: u32,
    /// Sounds that can be registered at once.
    pub max_sounds: u32,
    /// Commands that can wait for the audio thread; rounded up to a power of two.
    pub command_capacity: u32,
}

/// Why an engine call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// A size in the configuration is 0 or too large.
    InvalidConfig,
    /// The stream format is not one the mixer plays (v0: stereo).
    UnsupportedFormat,
    /// The sound has no frames or more than two channels.
    UnsupportedSound,
    /// Every place in the sound table is taken.
    SoundsFull,
    /// Every voice is playing or waiting for its end to be read.
    VoicesExhausted,
    /// The handle is not (or no longer) valid.
    StaleHandle,
    /// A gain, pan or pitch is out of range or not finite.
    InvalidParameter,
    /// The command queue is full; nothing was sent and nothing was used up.
    QueueFull,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidConfig => "a size in the engine configuration is not valid",
            Self::UnsupportedFormat => "the stream format is not supported",
            Self::UnsupportedSound => "the sound has no frames or too many channels",
            Self::SoundsFull => "the sound table is full",
            Self::VoicesExhausted => "every voice is in use",
            Self::StaleHandle => "the handle is stale",
            Self::InvalidParameter => "a parameter is out of range",
            Self::QueueFull => "the command queue is full",
        })
    }
}

impl std::error::Error for EngineError {}

/// Game thread to audio thread.
enum Command {
    Play {
        id: VoiceId,
        sound: Arc<Sound>,
        params: PlayParams,
    },
    Stop(VoiceId),
    SetGain(VoiceId, f32),
    SetPan(VoiceId, f32),
    SetPitch(VoiceId, f32),
    SetBusGain(BusId, f32),
    SetBusMuted(BusId, bool),
}

/// Audio thread to game thread.
enum Event {
    /// The voice is over; the sound comes back so that it is dropped on the game thread.
    VoiceEnded { id: VoiceId, sound: Arc<Sound> },
}

/// Builds the game-side engine and the audio-side processor for a stream `format`. All
/// allocation happens here.
///
/// # Errors
/// [`EngineError::InvalidConfig`] for a size of 0 or above the limits,
/// [`EngineError::UnsupportedFormat`] unless the format is stereo.
pub fn engine(
    format: StreamFormat,
    config: EngineConfig,
) -> Result<(AudioEngine, AudioProcessor), EngineError> {
    engine_with_buses(format, config, &[])
}

/// Like [`engine`], with buses under the master: bus `i + 1` has the parent `buses[i]`, which
/// must be the master or a bus that comes before it (see [`Mixer::with_buses`]).
///
/// # Errors
/// The errors of [`engine`], and [`EngineError::InvalidConfig`] for a bad bus table.
pub fn engine_with_buses(
    format: StreamFormat,
    config: EngineConfig,
    buses: &[BusId],
) -> Result<(AudioEngine, AudioProcessor), EngineError> {
    if config.max_sounds == 0 || config.command_capacity == 0 {
        return Err(EngineError::InvalidConfig);
    }
    let mixer =
        Mixer::with_buses(format, config.max_voices, buses).map_err(|error| match error {
            MixerError::UnsupportedFormat => EngineError::UnsupportedFormat,
            _ => EngineError::InvalidConfig,
        })?;
    let (command_tx, command_rx) =
        channel(config.command_capacity).ok_or(EngineError::InvalidConfig)?;
    let (event_tx, event_rx) = channel(config.max_voices).ok_or(EngineError::InvalidConfig)?;
    let voices = usize::try_from(config.max_voices).map_err(|_| EngineError::InvalidConfig)?;
    let command_limit = command_tx.capacity();
    let engine = AudioEngine {
        bus_count: mixer.bus_count(),
        sounds: Pool::with_capacity(config.max_sounds),
        voices: Pool::with_capacity(config.max_voices),
        commands: command_tx,
        events: event_rx,
    };
    let processor = AudioProcessor {
        mixer,
        commands: command_rx,
        events: event_tx,
        command_limit,
        by_id: vec![None; voices],
        by_slot: vec![None; voices],
    };
    Ok((engine, processor))
}

/// The game-thread side; see the module documentation.
pub struct AudioEngine {
    bus_count: usize,
    sounds: Pool<Arc<Sound>>,
    voices: Pool<Playing>,
    commands: Producer<Command>,
    events: Consumer<Event>,
}

impl AudioEngine {
    /// Takes a sound into the table.
    ///
    /// # Errors
    /// `UnsupportedSound` for a sound the mixer cannot play, `SoundsFull`.
    pub fn register_sound(&mut self, sound: Sound) -> Result<SoundHandle, EngineError> {
        if sound.frame_count() == 0 || sound.channels() > 2 {
            return Err(EngineError::UnsupportedSound);
        }
        self.sounds
            .insert(Arc::new(sound))
            .map_err(|_| EngineError::SoundsFull)
    }

    /// Removes a sound from the table. Voices that play it go on; the last reference is dropped
    /// here on the game thread, when the last of them has been reported by `poll_finished`.
    ///
    /// # Errors
    /// `StaleHandle`.
    pub fn unregister_sound(&mut self, handle: SoundHandle) -> Result<(), EngineError> {
        self.sounds
            .remove(handle)
            .map(drop)
            .ok_or(EngineError::StaleHandle)
    }

    /// Starts a voice and returns its handle at once.
    ///
    /// # Errors
    /// `StaleHandle` for an unknown sound, `InvalidParameter`, `VoicesExhausted`, `QueueFull`.
    /// An error uses up nothing.
    pub fn play(&mut self, sound: SoundHandle, params: PlayParams) -> Result<VoiceId, EngineError> {
        let sound = self
            .sounds
            .get(sound)
            .map(Arc::clone)
            .ok_or(EngineError::StaleHandle)?;
        if !valid_params(&params) || !self.has_bus(params.bus) {
            return Err(EngineError::InvalidParameter);
        }
        if self.commands.is_full() {
            return Err(EngineError::QueueFull);
        }
        let id = self
            .voices
            .insert(Playing)
            .map_err(|_| EngineError::VoicesExhausted)?;
        if self
            .commands
            .push(Command::Play { id, sound, params })
            .is_err()
        {
            self.voices.remove(id);
            return Err(EngineError::QueueFull);
        }
        Ok(id)
    }

    /// Fades the voice out and ends it.
    ///
    /// # Errors
    /// `StaleHandle`, `QueueFull`.
    pub fn stop(&mut self, voice: VoiceId) -> Result<(), EngineError> {
        self.send(voice, Command::Stop(voice))
    }

    /// Ramps the voice gain.
    ///
    /// # Errors
    /// `StaleHandle`, `InvalidParameter`, `QueueFull`.
    pub fn set_gain(&mut self, voice: VoiceId, gain: f32) -> Result<(), EngineError> {
        if !valid_gain(gain) {
            return Err(EngineError::InvalidParameter);
        }
        self.send(voice, Command::SetGain(voice, gain))
    }

    /// Ramps the voice pan.
    ///
    /// # Errors
    /// `StaleHandle`, `InvalidParameter`, `QueueFull`.
    pub fn set_pan(&mut self, voice: VoiceId, pan: f32) -> Result<(), EngineError> {
        if !valid_pan(pan) {
            return Err(EngineError::InvalidParameter);
        }
        self.send(voice, Command::SetPan(voice, pan))
    }

    /// Changes the playback speed.
    ///
    /// # Errors
    /// `StaleHandle`, `InvalidParameter`, `QueueFull`.
    pub fn set_pitch(&mut self, voice: VoiceId, pitch: f32) -> Result<(), EngineError> {
        if !valid_pitch(pitch) {
            return Err(EngineError::InvalidParameter);
        }
        self.send(voice, Command::SetPitch(voice, pitch))
    }

    /// Ramps the gain of a bus.
    ///
    /// # Errors
    /// `InvalidParameter` for a bus the engine does not have or a gain that is negative or not
    /// finite, `QueueFull`.
    pub fn set_bus_gain(&mut self, bus: BusId, gain: f32) -> Result<(), EngineError> {
        if !self.has_bus(bus) || !valid_gain(gain) {
            return Err(EngineError::InvalidParameter);
        }
        self.push(Command::SetBusGain(bus, gain))
    }

    /// Fades a bus out, or back in to its own gain.
    ///
    /// # Errors
    /// `InvalidParameter` for a bus the engine does not have, `QueueFull`.
    pub fn set_bus_muted(&mut self, bus: BusId, muted: bool) -> Result<(), EngineError> {
        if !self.has_bus(bus) {
            return Err(EngineError::InvalidParameter);
        }
        self.push(Command::SetBusMuted(bus, muted))
    }

    fn has_bus(&self, bus: BusId) -> bool {
        usize::from(bus.index()) < self.bus_count
    }

    fn push(&mut self, command: Command) -> Result<(), EngineError> {
        self.commands
            .push(command)
            .map_err(|_| EngineError::QueueFull)
    }

    /// Takes the next voice that has ended, frees its handle (which is stale from now on) and
    /// drops the sound reference that came back with it. Call it until it returns `None`; a
    /// voice counts against `max_voices` until it has been reported here.
    pub fn poll_finished(&mut self) -> Option<VoiceId> {
        let Event::VoiceEnded { id, sound } = self.events.pop()?;
        drop(sound);
        self.voices.remove(id);
        Some(id)
    }

    fn send(&mut self, voice: VoiceId, command: Command) -> Result<(), EngineError> {
        if !self.voices.contains(voice) {
            return Err(EngineError::StaleHandle);
        }
        self.push(command)
    }
}

/// The audio-thread side; see the module documentation.
pub struct AudioProcessor {
    mixer: Mixer,
    commands: Consumer<Command>,
    events: Producer<Event>,
    /// At most this many commands are applied per block, so the loop is bounded.
    command_limit: usize,
    /// The mixer voice of each game-side handle, by handle index.
    by_id: Vec<Option<VoiceHandle>>,
    /// The game-side handle of each mixer voice, by mixer index.
    by_slot: Vec<Option<VoiceId>>,
}

impl AudioProcessor {
    fn mixer_voice(&self, id: VoiceId) -> Option<VoiceHandle> {
        // No generation check on the game-side handle: a command for a voice is sent only while
        // its handle is valid, and the handle is freed (and reused) only after the voice's end
        // was reported, so a command for an old voice always comes before the next voice's
        // `Play` in the queue and finds its entry empty. The mixer's own handles are checked.
        let index = usize::try_from(id.index()).ok()?;
        self.by_id.get(index).copied().flatten()
    }

    fn start(&mut self, id: VoiceId, sound: Arc<Sound>, params: PlayParams) {
        // The clone is released again below, or travels back in the event; because the game side
        // holds a reference too (or the event returns this one), no drop here frees memory.
        let spare = Arc::clone(&sound);
        match self.mixer.play(sound, params) {
            Ok(voice) => {
                self.register(id, voice);
                drop(spare);
            }
            Err(_) => self.report(id, spare),
        }
    }

    fn register(&mut self, id: VoiceId, voice: VoiceHandle) {
        let by_id = usize::try_from(id.index())
            .ok()
            .and_then(|index| self.by_id.get_mut(index));
        if let Some(entry) = by_id {
            *entry = Some(voice);
        }
        let by_slot = usize::try_from(voice.index())
            .ok()
            .and_then(|index| self.by_slot.get_mut(index));
        if let Some(entry) = by_slot {
            *entry = Some(id);
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Play { id, sound, params } => self.start(id, sound, params),
            Command::Stop(id) => self.with_voice(id, Mixer::stop),
            Command::SetGain(id, gain) => {
                self.with_voice(id, |mixer, voice| mixer.set_gain(voice, gain));
            }
            Command::SetPan(id, pan) => {
                self.with_voice(id, |mixer, voice| mixer.set_pan(voice, pan));
            }
            Command::SetPitch(id, pitch) => {
                self.with_voice(id, |mixer, voice| mixer.set_pitch(voice, pitch));
            }
            Command::SetBusGain(bus, gain) => {
                let _ = self.mixer.set_bus_gain(bus, gain);
            }
            Command::SetBusMuted(bus, muted) => {
                let _ = self.mixer.set_bus_muted(bus, muted);
            }
        }
    }

    /// Runs `change` on the mixer voice of `id`; a handle that is unknown (the voice ended) is
    /// ignored, and so is a refusal (the game side already checked the values).
    fn with_voice(
        &mut self,
        id: VoiceId,
        change: impl FnOnce(&mut Mixer, VoiceHandle) -> Result<(), MixerError>,
    ) {
        if let Some(voice) = self.mixer_voice(id) {
            let _ = change(&mut self.mixer, voice);
        }
    }

    /// Applies the waiting commands, at most `command_limit` of them.
    fn drain_commands(&mut self) {
        let _ = (0..self.command_limit)
            .take_while(|_| self.apply_next())
            .count();
    }

    /// Applies one waiting command; false when there was none.
    fn apply_next(&mut self) -> bool {
        let Some(command) = self.commands.pop() else {
            return false;
        };
        self.apply(command);
        true
    }

    fn report(&mut self, id: VoiceId, sound: Arc<Sound>) {
        if let Err(Event::VoiceEnded { sound, .. }) =
            self.events.push(Event::VoiceEnded { id, sound })
        {
            // Not reachable: the queue has room for every voice the game side can have
            // outstanding. If it ever happened, leaking the sound is safer than freeing it here.
            core::mem::forget(sound);
        }
    }

    fn release(&mut self, voice: VoiceHandle, sound: Arc<Sound>) {
        let id = usize::try_from(voice.index())
            .ok()
            .and_then(|index| self.by_slot.get_mut(index))
            .and_then(Option::take);
        let Some(id) = id else {
            // Not reachable: every mixer voice was registered with its game-side handle.
            core::mem::forget(sound);
            return;
        };
        let entry = usize::try_from(id.index())
            .ok()
            .and_then(|index| self.by_id.get_mut(index));
        if let Some(entry) = entry {
            *entry = None;
        }
        self.report(id, sound);
    }
}

impl AudioRenderer for AudioProcessor {
    fn render(&mut self, out: &mut [f32], frames: u32) {
        self.drain_commands();
        self.mixer.render(out, frames);
        while let Some((voice, sound)) = self.mixer.pop_finished() {
            self.release(voice, sound);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::ChannelLayout;

    fn format() -> StreamFormat {
        StreamFormat {
            sample_rate: 48_000,
            layout: ChannelLayout::Stereo,
            max_block_frames: 256,
        }
    }

    /// A refusal by the mixer cannot happen through `AudioEngine` (the pools match), so the
    /// processor is fed directly, with a mixer that is smaller than the game-side pool.
    #[test]
    fn a_play_the_mixer_refuses_returns_the_sound_in_an_event()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut game, mut processor) = engine(
            format(),
            EngineConfig {
                max_voices: 2,
                max_sounds: 1,
                command_capacity: 4,
            },
        )?;
        processor.mixer = Mixer::new(format(), 1)?;
        let sound = game.register_sound(Sound::new(48_000, 1, vec![0.5; 4_000])?)?;
        let first = game.play(sound, PlayParams::default())?;
        let second = game.play(sound, PlayParams::default())?;
        let mut out = vec![0.0; 64];
        processor.render(&mut out, 32);
        // The second voice was refused: its end is reported at once, with its sound.
        assert_eq!(game.poll_finished(), Some(second));
        assert_eq!(game.poll_finished(), None);
        assert!(game.stop(first).is_ok());
        game.unregister_sound(sound)?;
        Ok(())
    }
}
