//! A frame through the scene update, the extraction and the sprite batcher, in the order of the
//! frame plan, and two runs that agree.

use crate::common::{at, drawable, render_scene, textures, unit};
use ecs::World;
use frame::{FramePlan, Phase};
use render::scene::RenderScene;
use render::sprite::{Batch, SpriteBatcher, SpriteVertex};
use scene::{Transform2D, TransformUpdater};
use sync_render::{SpriteRenderer, extract_sprites};

type Failure = Box<dyn std::error::Error>;

struct Sim {
    world: World,
    updater: TransformUpdater,
    scene: RenderScene,
    batcher: SpriteBatcher,
    staging: Vec<SpriteVertex>,
    batches: Vec<Batch>,
}

impl Sim {
    fn run_phase(&mut self, phase: Phase) -> Result<(), Failure> {
        match phase {
            Phase::SceneUpdate => {
                self.updater.update(&mut self.world)?;
            }
            Phase::SyncRender => {
                extract_sprites(&mut self.world, &mut self.scene)?;
            }
            Phase::Render => {
                let batches = self.batcher.prepare_scene(&self.scene, &mut self.staging)?;
                self.batches = batches.to_vec();
            }
            _ => {}
        }
        Ok(())
    }
}

fn run() -> Result<(Vec<Batch>, Vec<SpriteVertex>), Failure> {
    let tex = textures(2);
    let mut sim = Sim {
        world: World::new(),
        updater: TransformUpdater::new(),
        scene: render_scene(16),
        batcher: SpriteBatcher::new(16)?,
        staging: vec![SpriteVertex::default(); 64],
        batches: Vec::new(),
    };
    for (n, layer) in [(0_u8, 1_i16), (1, 0), (2, 1), (3, 0)] {
        let renderer = SpriteRenderer {
            layer,
            ..unit(tex[usize::from(n % 2)])
        };
        let local = Transform2D {
            rotation: f32::from(n) * 0.1,
            ..at(f32::from(n), 0.0)
        };
        drawable(&mut sim.world, local, renderer)?;
    }
    for phase in FramePlan::new(0) {
        sim.run_phase(phase)?;
    }
    let staged = sim.staging[..16].to_vec();
    Ok((sim.batches, staged))
}

#[test]
fn the_phases_of_a_frame_turn_entities_into_batches_in_layer_order() -> Result<(), Failure> {
    let (batches, staged) = run()?;
    // Layer 0 first (entities 1 and 3, textures 1 and 1), then layer 1 (entities 0 and 2,
    // textures 0 and 0): two batches of two sprites.
    assert_eq!(batches.len(), 2);
    assert!(batches.iter().all(|b| b.sprite_count == 2));
    let first = staged[0].position;
    assert!((first[0] - 0.5).abs() < 1.0 && first[0] > 0.0);
    Ok(())
}

#[test]
fn two_runs_stage_the_same_vertices_bit_for_bit() -> Result<(), Failure> {
    let (batches_a, staged_a) = run()?;
    let (batches_b, staged_b) = run()?;
    assert_eq!(batches_a.len(), 2);
    assert_eq!(batches_a, batches_b);
    let bits = |v: &[SpriteVertex]| -> Vec<u32> {
        v.iter()
            .flat_map(|s| [s.position[0].to_bits(), s.position[1].to_bits()])
            .collect()
    };
    assert_eq!(bits(&staged_a), bits(&staged_b));
    Ok(())
}
