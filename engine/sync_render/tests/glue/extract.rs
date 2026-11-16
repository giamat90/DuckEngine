//! What the extraction copies, skips and drops.

use crate::common::{at, camera, drawable, refresh, render_scene, textures, unit};
use base::math::Vec2;
use ecs::World;
use render::sprite::UvRect;
use scene::{Transform2D, WorldTransform2D, set_parent};
use sync_render::{ExtractReport, SpriteRenderer, SyncError, extract_sprites};

const EPSILON: f32 = 1e-4;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

#[test]
fn a_new_renderer_takes_the_defaults() {
    let texture = textures(1)[0];
    let renderer = SpriteRenderer::new(texture, Vec2::new(2.0, 3.0));
    assert_eq!(renderer.texture, texture);
    assert_eq!(renderer.size, Vec2::new(2.0, 3.0));
    assert_eq!(renderer.uv, UvRect::FULL);
    assert_eq!(renderer.tint, [255; 4]);
    assert_eq!(renderer.layer, 0);
}

#[test]
fn an_entity_with_a_transform_and_a_renderer_becomes_one_sprite() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let renderer = SpriteRenderer {
        texture,
        size: Vec2::new(2.0, 4.0),
        uv: UvRect {
            min: Vec2::new(0.25, 0.0),
            max: Vec2::new(0.5, 1.0),
        },
        tint: [10, 20, 30, 40],
        layer: -3,
    };
    drawable(&mut world, at(3.0, -2.0), renderer)?;
    refresh(&mut world);
    let mut scene = render_scene(4);
    let report = extract_sprites(&mut world, &mut scene)?;
    assert_eq!(
        report,
        ExtractReport {
            extracted: 1,
            skipped: 0,
            dropped: 0
        }
    );
    let sprite = scene.sprites()[0];
    assert_eq!(sprite.texture, texture);
    assert_eq!(sprite.position, Vec2::new(3.0, -2.0));
    assert_eq!(sprite.size, Vec2::new(2.0, 4.0));
    assert_eq!(sprite.uv, renderer.uv);
    assert_eq!(sprite.tint, [10, 20, 30, 40]);
    assert_eq!(sprite.layer, -3);
    assert!(close(sprite.angle, 0.0));
    Ok(())
}

#[test]
fn the_angle_comes_from_the_world_matrix() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let local = Transform2D {
        rotation: 0.5,
        ..at(0.0, 0.0)
    };
    drawable(&mut world, local, unit(texture))?;
    refresh(&mut world);
    let mut scene = render_scene(1);
    extract_sprites(&mut world, &mut scene)?;
    assert!(close(scene.sprites()[0].angle, 0.5));
    Ok(())
}

#[test]
fn the_scale_multiplies_the_size_along_each_axis() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let local = Transform2D {
        scale: Vec2::new(2.0, 3.0),
        ..at(0.0, 0.0)
    };
    drawable(&mut world, local, unit(texture))?;
    refresh(&mut world);
    let mut scene = render_scene(1);
    extract_sprites(&mut world, &mut scene)?;
    let size = scene.sprites()[0].size;
    assert!(close(size.x, 2.0) && close(size.y, 3.0));
    Ok(())
}

#[test]
fn a_child_is_drawn_where_its_parent_and_its_own_offset_put_it() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let parent_local = Transform2D {
        rotation: std::f32::consts::FRAC_PI_2,
        scale: Vec2::new(2.0, 2.0),
        ..at(10.0, 0.0)
    };
    let parent = drawable(&mut world, parent_local, unit(texture))?;
    let child = drawable(&mut world, at(1.0, 0.0), unit(texture))?;
    assert!(set_parent(&mut world, child, parent).is_ok());
    refresh(&mut world);
    let mut scene = render_scene(2);
    extract_sprites(&mut world, &mut scene)?;
    let drawn = scene.sprites()[1];
    // One unit along the parent's x axis, which is turned to +y and doubled in length.
    assert!(close(drawn.position.x, 10.0) && close(drawn.position.y, 2.0));
    assert!(close(drawn.angle, std::f32::consts::FRAC_PI_2));
    assert!(close(drawn.size.x, 2.0) && close(drawn.size.y, 2.0));
    Ok(())
}

#[test]
fn an_entity_without_a_renderer_or_a_world_transform_is_not_a_sprite() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let no_renderer = world.spawn()?;
    world.insert(no_renderer, WorldTransform2D::default())?;
    let no_transform = world.spawn()?;
    world.insert(no_transform, unit(texture))?;
    let mut scene = render_scene(4);
    let report = extract_sprites(&mut world, &mut scene)?;
    assert_eq!(report, ExtractReport::default());
    assert!(scene.sprites().is_empty());
    Ok(())
}

#[test]
fn the_scene_holds_the_sprites_in_the_storage_order() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    for x in [5.0, 1.0, 3.0] {
        drawable(&mut world, at(x, 0.0), unit(texture))?;
    }
    refresh(&mut world);
    let mut scene = render_scene(4);
    extract_sprites(&mut world, &mut scene)?;
    let xs: Vec<f32> = scene.sprites().iter().map(|s| s.position.x).collect();
    assert_eq!(xs, vec![5.0, 1.0, 3.0]);
    Ok(())
}

#[test]
fn an_extraction_replaces_the_sprites_of_the_last_one() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let first = drawable(&mut world, at(1.0, 0.0), unit(texture))?;
    drawable(&mut world, at(2.0, 0.0), unit(texture))?;
    refresh(&mut world);
    let mut scene = render_scene(8);
    extract_sprites(&mut world, &mut scene)?;
    extract_sprites(&mut world, &mut scene)?;
    assert_eq!(scene.sprites().len(), 2);
    world.despawn(first)?;
    extract_sprites(&mut world, &mut scene)?;
    assert_eq!(scene.sprites().len(), 1);
    assert_eq!(scene.sprites()[0].position, Vec2::new(2.0, 0.0));
    Ok(())
}

#[test]
fn a_full_scene_keeps_what_fits_and_counts_the_rest() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    for n in 0..5_u8 {
        drawable(&mut world, at(f32::from(n), 0.0), unit(texture))?;
    }
    refresh(&mut world);
    let mut scene = render_scene(3);
    let report = extract_sprites(&mut world, &mut scene)?;
    assert_eq!(
        report,
        ExtractReport {
            extracted: 3,
            skipped: 0,
            dropped: 2
        }
    );
    assert_eq!(scene.sprites().len(), 3);
    Ok(())
}

#[test]
fn values_that_cannot_be_drawn_are_skipped_and_counted() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    drawable(&mut world, at(f32::NAN, 0.0), unit(texture))?;
    drawable(&mut world, at(f32::INFINITY, 0.0), unit(texture))?;
    let flat = Transform2D {
        scale: Vec2::new(0.0, 1.0),
        ..at(0.0, 0.0)
    };
    drawable(&mut world, flat, unit(texture))?;
    let thin = Transform2D {
        scale: Vec2::new(1.0, 0.0),
        ..at(0.0, 0.0)
    };
    drawable(&mut world, thin, unit(texture))?;
    let no_size = SpriteRenderer::new(texture, Vec2::new(1.0, -1.0));
    drawable(&mut world, at(0.0, 0.0), no_size)?;
    let nan_size = SpriteRenderer::new(texture, Vec2::new(f32::NAN, 1.0));
    drawable(&mut world, at(0.0, 0.0), nan_size)?;
    drawable(&mut world, at(7.0, 0.0), unit(texture))?;
    refresh(&mut world);
    let mut scene = render_scene(8);
    let report = extract_sprites(&mut world, &mut scene)?;
    assert_eq!(
        report,
        ExtractReport {
            extracted: 1,
            skipped: 6,
            dropped: 0
        }
    );
    assert_eq!(scene.sprites().len(), 1);
    assert_eq!(scene.sprites()[0].position, Vec2::new(7.0, 0.0));
    Ok(())
}

#[test]
fn the_camera_is_left_as_the_application_set_it() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    drawable(&mut world, at(1.0, 0.0), unit(texture))?;
    refresh(&mut world);
    let mut scene = render_scene(2);
    extract_sprites(&mut world, &mut scene)?;
    assert_eq!(*scene.camera(), camera());
    Ok(())
}

#[test]
fn the_error_says_what_failed() {
    let error = SyncError::Ecs(ecs::EcsError::StaleEntity);
    assert!(error.to_string().starts_with("ecs: "));
}

#[test]
fn a_turn_past_half_a_circle_is_given_as_the_equal_angle_in_range() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let local = Transform2D {
        rotation: 4.0,
        ..at(0.0, 0.0)
    };
    drawable(&mut world, local, unit(texture))?;
    refresh(&mut world);
    let mut scene = render_scene(1);
    extract_sprites(&mut world, &mut scene)?;
    assert!(close(scene.sprites()[0].angle, 4.0 - std::f32::consts::TAU));
    Ok(())
}

#[test]
fn a_mirror_is_drawn_as_a_half_turn_not_mirrored() -> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let local = Transform2D {
        scale: Vec2::new(-1.0, 1.0),
        ..at(0.0, 0.0)
    };
    drawable(&mut world, local, unit(texture))?;
    refresh(&mut world);
    let mut scene = render_scene(1);
    extract_sprites(&mut world, &mut scene)?;
    let drawn = scene.sprites()[0];
    assert!(close(drawn.angle.abs(), std::f32::consts::PI));
    assert!(close(drawn.size.x, 1.0) && close(drawn.size.y, 1.0));
    Ok(())
}

#[test]
fn entities_of_another_archetype_come_after_the_first_one_not_in_spawn_order()
-> Result<(), SyncError> {
    let texture = textures(1)[0];
    let mut world = World::new();
    let plain_first = drawable(&mut world, at(1.0, 0.0), unit(texture))?;
    let extra = drawable(&mut world, at(2.0, 0.0), unit(texture))?;
    world.insert(extra, 7_u32)?;
    drawable(&mut world, at(3.0, 0.0), unit(texture))?;
    refresh(&mut world);
    assert!(world.is_alive(plain_first));
    let mut scene = render_scene(4);
    extract_sprites(&mut world, &mut scene)?;
    let xs: Vec<f32> = scene.sprites().iter().map(|s| s.position.x).collect();
    // The order is by table, then by row: the extra component moved entity 2 to its own table.
    assert_eq!(xs.len(), 3);
    assert!(xs.contains(&2.0) && xs.contains(&1.0) && xs.contains(&3.0));
    assert_eq!(xs.last(), Some(&2.0));
    Ok(())
}
