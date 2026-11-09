//! What the device callback shares with the game thread: the renderer slot and the counters.
//!
//! The callback runs on a thread miniaudio owns. It never allocates and never blocks: the slot is
//! behind a mutex that is only taken with `try_lock` there, and the game thread takes it only while
//! the device is stopped (`ma_device_stop` returns after the last callback), so `try_lock` does not
//! fail in practice; if it ever did, the callback writes silence.

use audio::device::AudioRenderer;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The numbers `IAudioDevice::status` reports; they live as long as the device, across `open`s.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    callbacks: AtomicU64,
    frames: AtomicU64,
    misses: AtomicU64,
}

impl Counters {
    /// Times the renderer was called.
    pub(crate) fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Acquire)
    }

    /// Frames the renderer was asked for, in total.
    pub(crate) fn frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }

    /// Callbacks that found the renderer slot taken and wrote silence (the device's underruns).
    pub(crate) fn misses(&self) -> u64 {
        self.misses.load(Ordering::Acquire)
    }

    fn count(&self, frames: u32) {
        self.callbacks.fetch_add(1, Ordering::AcqRel);
        self.frames.fetch_add(u64::from(frames), Ordering::AcqRel);
    }
}

/// One open stream as the callback sees it.
pub(crate) struct Shared {
    slot: Mutex<Option<Box<dyn AudioRenderer>>>,
    channels: usize,
    max_block: u32,
    counters: Arc<Counters>,
}

impl Shared {
    /// A stream of `channels` channels whose renderer is asked for `max_block` frames at most per
    /// call. Both are raised to one, so the cutting below never divides by zero.
    pub(crate) fn new(channels: usize, max_block: u32, counters: Arc<Counters>) -> Self {
        Self {
            slot: Mutex::new(None),
            channels: channels.max(1),
            max_block: max_block.max(1),
            counters,
        }
    }

    /// The channels of one frame.
    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    /// Hands the renderer to the callback. Only while the device is stopped.
    pub(crate) fn put(&self, renderer: Box<dyn AudioRenderer>) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(renderer);
        }
    }

    /// Takes the renderer back. Only after the device has stopped.
    pub(crate) fn take(&self) -> Option<Box<dyn AudioRenderer>> {
        self.slot.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Runs `during` with the slot taken, as a game thread would hold it.
    #[cfg(test)]
    pub(crate) fn with_slot_taken(&self, during: impl FnOnce()) {
        let guard = self.slot.lock();
        during();
        drop(guard);
    }

    /// The device callback: fills `out` (`frames` frames of `channels` samples) from the renderer,
    /// in calls of at most `max_block` frames; silence when there is no renderer or `out` is not
    /// exactly `frames * channels` long.
    pub(crate) fn render(&self, out: &mut [f32], frames: u32) {
        let wanted = frames as usize * self.channels;
        let Ok(mut slot) = self.slot.try_lock() else {
            self.counters.misses.fetch_add(1, Ordering::AcqRel);
            out.fill(0.0);
            return;
        };
        match slot.as_mut() {
            Some(renderer) if out.len() == wanted => self.render_blocks(renderer.as_mut(), out),
            _ => out.fill(0.0),
        }
    }

    fn render_blocks(&self, renderer: &mut dyn AudioRenderer, out: &mut [f32]) {
        let block_samples = self.max_block as usize * self.channels;
        for chunk in out.chunks_mut(block_samples) {
            let frames = u32::try_from(chunk.len() / self.channels).unwrap_or(u32::MAX);
            renderer.render(chunk, frames);
            self.counters.count(frames);
        }
    }
}
