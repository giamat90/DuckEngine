//! A frame through the scene update and the audio phase, in the order of the frame plan, and two
//! runs that agree.

use crate::common::{Failure, Rig, last_frame, listener, rig, tone};
use audio::engine::SoundHandle;
use base::math::Vec2;
use ecs::{Entity, World};
use frame::{FramePlan, Phase};
use scene::{Transform2D, TransformUpdater};
use sync_audio::{CueQueue, SoundCue, sync_audio};

struct Sim {
    rig: Rig,
    world: World,
    updater: TransformUpdater,
    queue: CueQueue,
    sound: SoundHandle,
    source: Entity,
}

impl Sim {
    fn run_phase(&mut self, phase: Phase, frame: u32) -> Result<(), Failure> {
        match phase {
            Phase::Gameplay if frame == 0 => {
                self.queue.push(SoundCue::new(self.sound).at(self.source))?;
            }
            Phase::SceneUpdate => {
                self.updater.update(&mut self.world)?;
            }
            Phase::SyncAudio => {
                sync_audio(
                    &self.world,
                    &mut self.rig.engine,
                    &mut self.queue,
                    &listener(),
                );
            }
            _ => {}
        }
        Ok(())
    }
}

fn sim() -> Result<Sim, Failure> {
    let mut rig = rig(4)?;
    let sound = tone(&mut rig.engine, 4800, 0.5)?;
    let mut world = World::new();
    let source = world.spawn()?;
    world.insert(source, Transform2D::from_position(Vec2::new(4.0, 0.0)))?;
    Ok(Sim {
        rig,
        world,
        updater: TransformUpdater::new(),
        queue: CueQueue::new(4),
        sound,
        source,
    })
}

fn run() -> Result<Vec<u32>, Failure> {
    let mut sim = sim()?;
    let mut heard = Vec::new();
    for frame in 0..3 {
        for phase in FramePlan::new(0) {
            sim.run_phase(phase, frame)?;
        }
        let (left, right) = last_frame(&mut sim.rig.processor);
        heard.push(left.to_bits());
        heard.push(right.to_bits());
    }
    Ok(heard)
}

#[test]
fn a_cue_raised_in_gameplay_is_heard_panned_in_the_same_frame() -> Result<(), Failure> {
    let heard = run()?;
    // The source was never updated before frame 0: the scene update of the frame comes before the
    // audio phase, so the cue of frame 0 is already panned to the right (source at x = 4 of 10).
    for frame in 0..3 {
        let (left, right) = (
            f32::from_bits(heard[frame * 2]),
            f32::from_bits(heard[frame * 2 + 1]),
        );
        assert!(right > left && left > 0.0);
    }
    Ok(())
}

#[test]
fn a_source_whose_world_transform_is_not_updated_yet_is_heard_in_the_centre() -> Result<(), Failure>
{
    let mut sim = sim()?;
    sim.queue.push(SoundCue::new(sim.sound).at(sim.source))?;
    // No scene update: the source has no WorldTransform2D yet.
    sync_audio(&sim.world, &mut sim.rig.engine, &mut sim.queue, &listener());
    let (left, right) = last_frame(&mut sim.rig.processor);
    assert!((left - right).abs() < 1e-6 && left > 0.0);
    Ok(())
}

#[test]
fn two_runs_give_the_same_samples_bit_for_bit() -> Result<(), Failure> {
    assert_eq!(run()?, run()?);
    Ok(())
}
