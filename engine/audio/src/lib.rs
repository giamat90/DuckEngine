//! The audio ports v0: [`device::IAudioDevice`] with the [`device::AudioRenderer`] callback it
//! drives, [`decoder::IAudioDecoder`] with the [`sound::Sound`] it produces, and the Null and
//! Offline devices.
//!
//! Layer 2: depends on `base` only. No audio API appears here; adapters (layer 3) implement the
//! port. Design: `wiki/architecture/audio-port-v0.md`.

#[cfg(any(test, feature = "contract-tests"))]
pub mod contract;
pub mod decoder;
pub mod device;
pub mod error;
#[cfg(test)]
mod faulty;
pub mod format;
pub mod mixer;
pub mod null;
pub mod offline;
mod session;
pub mod sound;
mod voice;
pub mod wav;
