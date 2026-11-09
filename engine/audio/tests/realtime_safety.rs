//! Real-time safety of the mixer (card 5-3): once the mixer is built, rendering and the commands
//! make no allocator call on the calling thread (counting allocator with per-thread counters), over
//! thousands of blocks that start, change, stop and finish voices.

use audio::device::AudioRenderer;
use audio::format::{ChannelLayout, StreamFormat};
use audio::mixer::{Mixer, MixerError, PlayParams, VoiceHandle};
use audio::sound::Sound;
use base::alloc::{CountingAllocator, count_allocations};
use std::error::Error;
use std::sync::Arc;

#[global_allocator]
static ALLOCATOR: CountingAllocator<std::alloc::System> =
    CountingAllocator::new(std::alloc::System);

const BLOCKS: u32 = 5000;
const SHORT_FRAMES: usize = 300;
const LONG_FRAMES: usize = 4000;
const VOICES: u32 = 4;
const WATCHED: usize = 6;

fn format() -> StreamFormat {
    StreamFormat {
        sample_rate: 48_000,
        layout: ChannelLayout::Stereo,
        max_block_frames: 1024,
    }
}

/// What one run of blocks did.
#[derive(Default)]
struct Tally {
    finished: u32,
    refused: u32,
}

/// The sounds, the handles being watched and the output buffer, all made before counting.
struct Scene {
    short: Arc<Sound>,
    long: Arc<Sound>,
    out: Vec<f32>,
    watched: [Option<VoiceHandle>; WATCHED],
}

impl Scene {
    fn replace_watched(&mut self, mixer: &mut Mixer, block: u32, handle: VoiceHandle) {
        let slot = usize::try_from(block).unwrap_or(0) % WATCHED;
        if let Some(old) = self.watched.get_mut(slot).and_then(|w| w.replace(handle)) {
            let _ = mixer.stop(old);
            let _ = mixer.set_gain(old, 0.1);
        }
    }

    fn start_voice(&mut self, mixer: &mut Mixer, block: u32, tally: &mut Tally) {
        let looping = block.is_multiple_of(5);
        let sound = if looping { &self.long } else { &self.short };
        let params = PlayParams {
            gain: 0.5,
            pan: 0.25,
            pitch: 1.0 + f32::from(u8::try_from(block % 7).unwrap_or(0)) * 0.1,
            looping,
            ..PlayParams::default()
        };
        match mixer.play(Arc::clone(sound), params) {
            Ok(handle) => self.replace_watched(mixer, block, handle),
            Err(MixerError::VoicesExhausted) => tally.refused += 1,
            Err(_) => {}
        }
    }

    fn block(&mut self, mixer: &mut Mixer, block: u32, tally: &mut Tally) {
        self.start_voice(mixer, block, tally);
        mixer.render(&mut self.out, 100 + block % 900);
        while mixer.pop_finished().is_some() {
            tally.finished += 1;
        }
    }
}

#[test]
fn render_and_commands_make_no_allocator_call() -> Result<(), Box<dyn Error>> {
    let mut mixer = Mixer::new(format(), VOICES)?;
    // The scene keeps one reference to each sound, so no drop during the run is the last one.
    let mut scene = Scene {
        short: Arc::new(Sound::new(44_100, 1, vec![0.25; SHORT_FRAMES])?),
        long: Arc::new(Sound::new(48_000, 2, vec![0.1; LONG_FRAMES * 2])?),
        out: vec![0.0; 1024 * 2],
        watched: [None; WATCHED],
    };
    let mut tally = Tally::default();

    let ((), counts) = count_allocations(|| {
        for block in 0..BLOCKS {
            scene.block(&mut mixer, block, &mut tally);
        }
    });

    assert_eq!(counts.allocations, 0, "allocations");
    assert_eq!(counts.reallocations, 0, "reallocations");
    assert_eq!(counts.deallocations, 0, "deallocations");
    assert!(
        tally.finished > 1000,
        "voices did finish: {}",
        tally.finished
    );
    assert!(tally.refused > 0, "the capacity limit was reached");
    Ok(())
}

#[test]
fn the_counting_allocator_sees_a_build_of_the_mixer() {
    let (mixer, counts) = count_allocations(|| Mixer::new(format(), VOICES));
    assert!(mixer.is_ok());
    assert!(counts.allocations > 0, "the harness counts allocations");
}

#[test]
fn render_frees_nothing_when_the_mixer_holds_the_only_reference() -> Result<(), Box<dyn Error>> {
    let mut mixer = Mixer::new(format(), VOICES)?;
    let sound = Arc::new(Sound::new(48_000, 1, vec![0.5; 64])?);
    mixer.play(sound, PlayParams::default())?;
    let mut out = vec![0.0_f32; 256 * 2];

    let ((), during_render) = count_allocations(|| mixer.render(&mut out, 256));
    assert_eq!(during_render.deallocations, 0, "render dropped the sound");
    assert_eq!(mixer.active_voices(), 0, "the voice ended");

    let (taken, _) = count_allocations(|| mixer.pop_finished());
    let ((), on_drop) = count_allocations(|| drop(taken));
    assert_eq!(
        on_drop.deallocations, 2,
        "the sound is freed by the caller: Arc and samples"
    );
    Ok(())
}
