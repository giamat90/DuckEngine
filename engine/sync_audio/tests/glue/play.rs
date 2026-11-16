//! What a call plays, how it pans, and what it counts.

use crate::common::{Failure, close, emitter, last_frame, listener, rig, tone};
use audio::bus::BusId;
use ecs::World;
use sync_audio::{CueQueue, SoundCue, SyncReport, sync_audio};

const CENTRE: f32 = std::f32::consts::FRAC_1_SQRT_2;

#[test]
fn a_cue_without_a_source_plays_in_the_centre() -> Result<(), Failure> {
    let mut rig = rig(4)?;
    let sound = tone(&mut rig.engine, 4800, 0.5)?;
    let mut queue = CueQueue::new(4);
    queue.push(SoundCue::new(sound))?;
    let report = sync_audio(&World::new(), &mut rig.engine, &mut queue, &listener());
    assert_eq!(
        report,
        SyncReport {
            played: 1,
            failed: 0,
            finished: 0
        }
    );
    let (left, right) = last_frame(&mut rig.processor);
    assert!(close(left, 0.5 * CENTRE) && close(right, 0.5 * CENTRE));
    Ok(())
}

#[test]
fn a_source_to_the_right_pans_right_and_to_the_left_pans_left() -> Result<(), Failure> {
    let mut world = World::new();
    let right = emitter(&mut world, 10.0)?;
    let left = emitter(&mut world, -10.0)?;
    for (source, expected) in [(right, (0.0, 0.5)), (left, (0.5, 0.0))] {
        let mut rig = rig(4)?;
        let sound = tone(&mut rig.engine, 4800, 0.5)?;
        let mut queue = CueQueue::new(4);
        queue.push(SoundCue::new(sound).at(source))?;
        sync_audio(&world, &mut rig.engine, &mut queue, &listener());
        let (l, r) = last_frame(&mut rig.processor);
        assert!(close(l, expected.0) && close(r, expected.1));
    }
    Ok(())
}

#[test]
fn a_source_that_is_gone_or_has_no_world_transform_plays_in_the_centre() -> Result<(), Failure> {
    let mut world = World::new();
    let gone = emitter(&mut world, 10.0)?;
    world.despawn(gone)?;
    let bare = world.spawn()?;
    for source in [gone, bare] {
        let mut rig = rig(4)?;
        let sound = tone(&mut rig.engine, 4800, 0.5)?;
        let mut queue = CueQueue::new(4);
        queue.push(SoundCue::new(sound).at(source))?;
        let report = sync_audio(&world, &mut rig.engine, &mut queue, &listener());
        assert_eq!(report.played, 1);
        let (l, r) = last_frame(&mut rig.processor);
        assert!(close(l, 0.5 * CENTRE) && close(r, 0.5 * CENTRE));
    }
    Ok(())
}

#[test]
fn the_gain_of_the_cue_is_applied() -> Result<(), Failure> {
    let mut rig = rig(4)?;
    let sound = tone(&mut rig.engine, 4800, 0.5)?;
    let mut queue = CueQueue::new(4);
    queue.push(SoundCue {
        gain: 0.5,
        ..SoundCue::new(sound)
    })?;
    sync_audio(&World::new(), &mut rig.engine, &mut queue, &listener());
    let (left, _) = last_frame(&mut rig.processor);
    assert!(close(left, 0.25 * CENTRE));
    Ok(())
}

#[test]
fn the_pitch_of_the_cue_is_applied() -> Result<(), Failure> {
    // A sound of 100 frames: at pitch two it ends inside one block of 64, at pitch one it does not.
    for (pitch, finished) in [(2.0, 1), (1.0, 0)] {
        let mut rig = rig(4)?;
        let sound = tone(&mut rig.engine, 100, 0.5)?;
        let mut queue = CueQueue::new(4);
        queue.push(SoundCue {
            pitch,
            ..SoundCue::new(sound)
        })?;
        sync_audio(&World::new(), &mut rig.engine, &mut queue, &listener());
        last_frame(&mut rig.processor);
        let report = sync_audio(&World::new(), &mut rig.engine, &mut queue, &listener());
        assert_eq!(report.finished, finished);
    }
    Ok(())
}

#[test]
fn the_queue_is_empty_after_a_call_also_when_cues_failed() -> Result<(), Failure> {
    let mut rig = rig(4)?;
    let sound = tone(&mut rig.engine, 4800, 0.5)?;
    rig.engine.unregister_sound(sound)?;
    let mut queue = CueQueue::new(4);
    queue.push(SoundCue::new(sound))?;
    queue.push(SoundCue::new(sound))?;
    let report = sync_audio(&World::new(), &mut rig.engine, &mut queue, &listener());
    assert_eq!(
        report,
        SyncReport {
            played: 0,
            failed: 2,
            finished: 0
        }
    );
    assert!(queue.is_empty());
    Ok(())
}

#[test]
fn a_bad_value_or_a_missing_bus_fails_the_cue_and_nothing_else() -> Result<(), Failure> {
    let mut rig = rig(4)?;
    let sound = tone(&mut rig.engine, 4800, 0.5)?;
    let mut queue = CueQueue::new(8);
    queue.push(SoundCue {
        gain: -1.0,
        ..SoundCue::new(sound)
    })?;
    queue.push(SoundCue {
        gain: f32::NAN,
        ..SoundCue::new(sound)
    })?;
    queue.push(SoundCue {
        pitch: 0.0,
        ..SoundCue::new(sound)
    })?;
    queue.push(SoundCue {
        bus: BusId::from_index(5),
        ..SoundCue::new(sound)
    })?;
    queue.push(SoundCue::new(sound))?;
    let report = sync_audio(&World::new(), &mut rig.engine, &mut queue, &listener());
    assert_eq!(
        report,
        SyncReport {
            played: 1,
            failed: 4,
            finished: 0
        }
    );
    Ok(())
}

#[test]
fn cues_play_in_the_order_they_were_pushed_until_the_voices_run_out() -> Result<(), Failure> {
    let mut rig = rig(4)?;
    let mut queue = CueQueue::new(8);
    for value in [0.1, 0.2, 0.3, 0.4, 0.5] {
        let sound = tone(&mut rig.engine, 4800, value)?;
        queue.push(SoundCue::new(sound))?;
    }
    let report = sync_audio(&World::new(), &mut rig.engine, &mut queue, &listener());
    assert_eq!(
        report,
        SyncReport {
            played: 4,
            failed: 1,
            finished: 0
        }
    );
    let (left, _) = last_frame(&mut rig.processor);
    // The first four are mixed (0.1 + 0.2 + 0.3 + 0.4), the fifth is the one that failed.
    assert!(close(left, 1.0 * CENTRE));
    Ok(())
}

#[test]
fn voices_that_ended_are_freed_before_the_cues_play() -> Result<(), Failure> {
    let mut rig = rig(2)?;
    let sound = tone(&mut rig.engine, 32, 0.5)?;
    let mut queue = CueQueue::new(4);
    queue.push(SoundCue::new(sound))?;
    queue.push(SoundCue::new(sound))?;
    let world = World::new();
    assert_eq!(
        sync_audio(&world, &mut rig.engine, &mut queue, &listener()).played,
        2
    );
    last_frame(&mut rig.processor);
    queue.push(SoundCue::new(sound))?;
    queue.push(SoundCue::new(sound))?;
    let report = sync_audio(&world, &mut rig.engine, &mut queue, &listener());
    assert_eq!(
        report,
        SyncReport {
            played: 2,
            failed: 0,
            finished: 2
        }
    );
    Ok(())
}

#[test]
fn an_empty_queue_and_an_idle_engine_do_nothing() -> Result<(), Failure> {
    let mut rig = rig(2)?;
    let mut queue = CueQueue::new(2);
    let report = sync_audio(&World::new(), &mut rig.engine, &mut queue, &listener());
    assert_eq!(report, SyncReport::default());
    Ok(())
}
