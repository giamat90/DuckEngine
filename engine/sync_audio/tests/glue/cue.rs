//! The cue and the queue.

use crate::common::{Failure, rig, tone};
use audio::bus::BusId;
use ecs::World;
use sync_audio::{CueQueue, SoundCue, SyncError};

#[test]
fn a_new_cue_has_the_defaults_and_no_source() -> Result<(), Failure> {
    let mut rig = rig(2)?;
    let sound = tone(&mut rig.engine, 8, 0.5)?;
    let cue = SoundCue::new(sound);
    assert_eq!(cue.sound, sound);
    assert_eq!(cue.source, None);
    assert!((cue.gain - 1.0).abs() < f32::EPSILON);
    assert!((cue.pitch - 1.0).abs() < f32::EPSILON);
    assert_eq!(cue.bus, BusId::MASTER);
    Ok(())
}

#[test]
fn at_sets_the_source_and_keeps_the_rest() -> Result<(), Failure> {
    let mut rig = rig(2)?;
    let sound = tone(&mut rig.engine, 8, 0.5)?;
    let mut world = World::new();
    let entity = world.spawn()?;
    let cue = SoundCue::new(sound).at(entity);
    assert_eq!(cue.source, Some(entity));
    assert_eq!(cue.sound, sound);
    Ok(())
}

#[test]
fn a_queue_keeps_its_capacity_and_starts_empty() {
    let queue = CueQueue::new(3);
    assert_eq!(queue.capacity(), 3);
    assert!(queue.is_empty());
    assert_eq!(queue.len(), 0);
}

#[test]
fn pushing_counts_and_a_full_queue_refuses_and_stays_as_it_was() -> Result<(), Failure> {
    let mut rig = rig(2)?;
    let sound = tone(&mut rig.engine, 8, 0.5)?;
    let mut queue = CueQueue::new(2);
    assert_eq!(queue.push(SoundCue::new(sound)), Ok(()));
    assert_eq!(queue.push(SoundCue::new(sound)), Ok(()));
    assert_eq!(queue.push(SoundCue::new(sound)), Err(SyncError::QueueFull));
    assert_eq!(queue.len(), 2);
    assert!(!queue.is_empty());
    Ok(())
}

#[test]
fn a_queue_of_capacity_zero_takes_nothing() -> Result<(), Failure> {
    let mut rig = rig(2)?;
    let sound = tone(&mut rig.engine, 8, 0.5)?;
    let mut queue = CueQueue::new(0);
    assert_eq!(queue.push(SoundCue::new(sound)), Err(SyncError::QueueFull));
    Ok(())
}

#[test]
fn the_error_says_what_failed() {
    assert_eq!(SyncError::QueueFull.to_string(), "the cue queue is full");
}
