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

type Run = (Vec<Batch>, Vec<SpriteVertex>, Vec<render::ids::TextureId>);

fn run() -> Result<Run, Failure> {
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
    Ok((sim.batches, staged, tex))
}

/// The x of the centre of the sprite staged in slot `slot` (four vertices each).
fn centre_x(staged: &[SpriteVertex], slot: usize) -> f32 {
    staged[slot * 4..slot * 4 + 4]
        .iter()
        .map(|v| v.position[0])
        .sum::<f32>()
        / 4.0
}

#[test]
fn the_phases_of_a_frame_turn_entities_into_batches_in_layer_order() -> Result<(), Failure> {
    let (batches, staged, tex) = run()?;
    // Layer 0 first (entities 1 and 3, texture 1), then layer 1 (entities 0 and 2, texture 0).
    assert_eq!(batches.len(), 2);
    assert_eq!(
        (
            batches[0].texture,
            batches[0].first_sprite,
            batches[0].sprite_count
        ),
        (tex[1], 0, 2)
    );
    assert_eq!(
        (
            batches[1].texture,
            batches[1].first_sprite,
            batches[1].sprite_count
        ),
        (tex[0], 2, 2)
    );
    for (slot, x) in [1.0_f32, 3.0, 0.0, 2.0].into_iter().enumerate() {
        assert!((centre_x(&staged, slot) - x).abs() < 1e-4);
    }
    Ok(())
}

#[test]
fn two_runs_stage_the_same_vertices_bit_for_bit() -> Result<(), Failure> {
    let (batches_a, staged_a, _) = run()?;
    let (batches_b, staged_b, _) = run()?;
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
