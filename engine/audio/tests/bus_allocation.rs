//! Buses make no allocator call after the mixer is built (counting allocator, per-thread
//! counters): rendering through a bus tree and changing bus gain and mute allocate and free
//! nothing.

use audio::bus::BusId;
use audio::device::AudioRenderer;
use audio::format::{ChannelLayout, StreamFormat};
use audio::mixer::{Mixer, PlayParams};
use audio::sound::Sound;
use base::alloc::{CountingAllocator, count_allocations};
use std::error::Error;
use std::sync::Arc;

#[global_allocator]
static ALLOCATOR: CountingAllocator<std::alloc::System> =
    CountingAllocator::new(std::alloc::System);

const BLOCKS: u32 = 3000;

fn format() -> StreamFormat {
    StreamFormat {
        sample_rate: 48_000,
        layout: ChannelLayout::Stereo,
        max_block_frames: 1024,
    }
}

#[test]
fn rendering_through_buses_and_changing_them_allocates_nothing() -> Result<(), Box<dyn Error>> {
    let parents = [BusId::MASTER, BusId::from_index(1), BusId::MASTER];
    let mut mixer = Mixer::with_buses(format(), 8, &parents)?;
    let sound = Arc::new(Sound::new(48_000, 1, vec![0.25; 700])?);
    let mut out = vec![0.0_f32; 1024 * 2];
    let mut calls = 0;
    let mut ended = 0;
    for block in 0..BLOCKS {
        let bus = BusId::from_index(u8::try_from(block % 4)?);
        let step = f32::from(u8::try_from(block % 10)?) * 0.1;
        let frames = 100 + (block % 700);
        let ((), counts) = count_allocations(|| {
            let params = PlayParams {
                bus,
                ..PlayParams::default()
            };
            let _ = mixer.play(Arc::clone(&sound), params);
            let _ = mixer.set_bus_gain(bus, step);
            let _ = mixer.set_bus_muted(bus, block % 7 == 0);
            mixer.render(&mut out, frames);
        });
        calls += counts.calls();
        // The sounds taken here are dropped outside the counted part, like the engine does.
        while mixer.pop_finished().is_some() {
            ended += 1;
        }
    }
    assert_eq!(calls, 0, "allocator calls");
    assert!(ended > 100, "voices ended: {ended}");
    Ok(())
}
