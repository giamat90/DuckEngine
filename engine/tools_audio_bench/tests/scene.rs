//! The benchmark scene: a mixer with a fixed number of looping voices that never end, so every
//! block costs the same.

use std::error::Error;
use tools_audio_bench::{BLOCK_FRAMES, SAMPLE_RATE, Scene, block_percent};

#[test]
fn a_scene_keeps_all_its_voices_playing_block_after_block() -> Result<(), Box<dyn Error>> {
    // Long enough to pass the end of the sound several times at the highest pitch (loop point).
    let mut scene = Scene::new(8)?;
    assert_eq!(scene.active_voices(), 8);
    for _ in 0..4000 {
        scene.render_block();
    }
    assert_eq!(scene.active_voices(), 8);
    Ok(())
}

#[test]
fn the_output_is_not_silent_and_is_finite() -> Result<(), Box<dyn Error>> {
    let mut scene = Scene::new(16)?;
    scene.render_block();
    let peak = scene.output().iter().fold(0.0_f32, |p, s| p.max(s.abs()));
    assert!(peak > 0.0 && peak.is_finite());
    assert_eq!(scene.output().len(), BLOCK_FRAMES as usize * 2);
    Ok(())
}

#[test]
fn zero_voices_are_refused() {
    assert!(Scene::new(0).is_err());
}

#[test]
fn a_block_is_a_share_of_the_core_in_proportion_to_its_duration() {
    let block_ns = f64::from(BLOCK_FRAMES) / f64::from(SAMPLE_RATE) * 1e9;
    assert!((block_percent(block_ns, BLOCK_FRAMES, SAMPLE_RATE) - 100.0).abs() < 1e-9);
    assert!((block_percent(block_ns / 4.0, BLOCK_FRAMES, SAMPLE_RATE) - 25.0).abs() < 1e-9);
}
