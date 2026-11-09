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

#[test]
fn the_icount_workload_is_deterministic_and_names_its_size() -> Result<(), Box<dyn Error>> {
    let first = tools_audio_bench::icount_workload(16, 5)?;
    assert_eq!(first, tools_audio_bench::icount_workload(16, 5)?);
    assert!(first.starts_with("voices 16 blocks 5 checksum "), "{first}");
    let digest = first.rsplit(' ').next().unwrap_or_default();
    assert_eq!(digest.len(), 16);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    Ok(())
}

#[test]
fn the_icount_checksum_proves_the_work_it_depends_on_every_block_and_every_voice()
-> Result<(), Box<dyn Error>> {
    let base = tools_audio_bench::icount_workload(16, 5)?;
    assert_ne!(base, tools_audio_bench::icount_workload(16, 6)?);
    assert_ne!(base, tools_audio_bench::icount_workload(17, 5)?);
    Ok(())
}

#[test]
fn the_icount_workload_refuses_an_empty_run() {
    assert!(tools_audio_bench::icount_workload(0, 5).is_err());
    assert!(tools_audio_bench::icount_workload(16, 0).is_err());
}
