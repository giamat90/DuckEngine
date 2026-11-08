//! Buses: a fixed tree of stereo accumulators under the master bus.
//!
//! Every voice plays into one bus. Each bus has a gain and a mute, both reached by a linear ramp
//! of [`crate::mixer::RAMP_FRAMES`] frames, and adds itself into its parent; the master bus is the
//! root and its sum is what the mixer writes out. The tree is fixed when the mixer is built
//! (card 5-6): a bus may only have a parent that comes before it in the table, so one pass from
//! the last bus to the first mixes every child before its parent.

use crate::voice::Ramp;

/// The most buses a mixer can have, the master included.
pub const MAX_BUSES: u8 = 16;

/// Frames mixed per internal chunk; the scratch buffer of each bus holds one chunk.
pub(crate) const CHUNK_FRAMES: usize = 256;

/// Samples (two per frame) in one chunk.
pub(crate) const CHUNK_SAMPLES: usize = CHUNK_FRAMES * 2;

/// A bus of a mixer: an index into its bus table. Bus 0 is the master.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusId(u8);

impl BusId {
    /// The root bus; every mixer has it.
    pub const MASTER: Self = Self(0);

    /// The bus at `index` of the table. Whether the table has it is checked where it is used.
    #[must_use]
    pub const fn from_index(index: u8) -> Self {
        Self(index)
    }

    /// The position in the table.
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }
}

pub(crate) struct Bus {
    parent: usize,
    gain: f32,
    muted: bool,
    ramp: Ramp,
    scratch: Vec<f32>,
}

impl Bus {
    /// A bus under `parent` (the master names itself, and its parent is never used); allocates
    /// its chunk buffer.
    pub(crate) fn new(parent: usize) -> Self {
        Self {
            parent,
            gain: 1.0,
            muted: false,
            ramp: Ramp::at(1.0),
            scratch: vec![0.0; CHUNK_SAMPLES],
        }
    }

    pub(crate) fn parent(&self) -> usize {
        self.parent
    }

    pub(crate) fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
        self.retarget();
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.retarget();
    }

    fn retarget(&mut self) {
        self.ramp.set(if self.muted { 0.0 } else { self.gain });
    }

    /// Silences the first `len` samples of the chunk buffer.
    pub(crate) fn clear(&mut self, len: usize) {
        if let Some(part) = self.scratch.get_mut(..len) {
            part.fill(0.0);
        }
    }

    /// The first `len` samples of the chunk buffer, where voices and children add themselves.
    pub(crate) fn chunk(&mut self, len: usize) -> Option<&mut [f32]> {
        self.scratch.get_mut(..len)
    }

    /// Adds the first samples of this bus, scaled by its gain ramp, to `target`.
    pub(crate) fn add_into(&mut self, target: &mut [f32]) {
        self.pass(target, |out, value| *out += value);
    }

    /// Writes this bus, scaled by its gain ramp, over `out` (the master's last step).
    pub(crate) fn write_to(&mut self, out: &mut [f32]) {
        self.pass(out, |out, value| *out = value);
    }

    /// Steps the gain ramp once per frame and hands each scaled sample to `combine`.
    fn pass(&mut self, target: &mut [f32], combine: impl Fn(&mut f32, f32)) {
        let frames = self.scratch.chunks_exact(2).zip(target.chunks_exact_mut(2));
        for (frame, into) in frames {
            combine_frame(frame, into, self.ramp.tick(), &combine);
        }
    }
}

fn combine_frame(frame: &[f32], into: &mut [f32], gain: f32, combine: &impl Fn(&mut f32, f32)) {
    if let ([left, right], [into_left, into_right]) = (frame, into) {
        combine(into_left, left * gain);
        combine(into_right, right * gain);
    }
}
