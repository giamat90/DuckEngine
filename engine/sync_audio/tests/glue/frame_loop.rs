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
            Phase::Gameplay if frame == 1 => {
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
fn a_cue_raised_in_gameplay_is_heard_from_the_next_audio_phase() -> Result<(), Failure> {
    let heard = run()?;
    // Frame 0 is silent, frame 1 has the cue (right of centre), frame 2 still plays.
    assert!(f32::from_bits(heard[0]).abs() < 1e-6 && f32::from_bits(heard[1]).abs() < 1e-6);
    let (left, right) = (f32::from_bits(heard[2]), f32::from_bits(heard[3]));
    assert!(right > left && left > 0.0);
    assert!(f32::from_bits(heard[5]) > 0.0);
    Ok(())
}

#[test]
fn two_runs_give_the_same_samples_bit_for_bit() -> Result<(), Failure> {
    assert_eq!(run()?, run()?);
    Ok(())
}
