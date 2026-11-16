//! Impacts become sound cues, and the cues are heard.

use crate::common::{Failure, Recorder, run, sandbox};

#[test]
fn nothing_is_heard_while_the_bodies_fall_in_the_air() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    assert_eq!(run(&mut sandbox, 25, &mut draw)?, 0);
    assert!(sandbox.peak() < f32::EPSILON);
    Ok(())
}

#[test]
fn a_body_that_lands_is_heard() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let played = run(&mut sandbox, 240, &mut draw)?;
    assert!(played > 0, "an impact played a cue");
    Ok(())
}

#[test]
fn the_cue_reaches_the_mix() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let mut loudest = 0.0_f32;
    for _ in 0..240 {
        run(&mut sandbox, 1, &mut draw)?;
        loudest = loudest.max(sandbox.peak());
    }
    assert!(loudest > 0.01, "{loudest}");
    Ok(())
}
