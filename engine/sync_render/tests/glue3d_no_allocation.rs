//! After warm-up, the mesh extraction makes no allocator call on the calling thread.

use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec3;
use base::pool::{Handle, Pool};
use ecs::World;
use render::camera::PerspectiveCamera;
use render::ids::Texture;
use render::lit::DirectionalLight;
use render::mesh_scene::{MeshRef, MeshScene};
use scene::{Transform3D, TransformUpdater3D};
use std::alloc::System;
use sync_render::SyncError;
use sync_render::d3::{MeshRenderer, extract_meshes};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn a_warm_mesh_extraction_does_not_allocate() -> Result<(), SyncError> {
    let mut pool = Pool::<Texture>::with_capacity(1);
    let texture = pool
        .insert(Texture)
        .unwrap_or_else(|_| Handle::from_raw(0, 0));
    let mut world = World::new();
    for n in 0..200_u8 {
        let e = world.spawn()?;
        world.insert(
            e,
            Transform3D::from_position(Vec3::new(f32::from(n), 0.0, 0.0)),
        )?;
        world.insert(e, MeshRenderer::new(MeshRef(u32::from(n % 4)), texture))?;
    }
    assert!(TransformUpdater3D::new().update(&mut world).is_ok());
    let camera = PerspectiveCamera {
        position: Vec3::new(0.0, 0.0, 9.0),
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: 1.0,
        aspect: 1.0,
        near: 0.1,
        far: 50.0,
    };
    let light = DirectionalLight {
        direction: Vec3::new(0.0, -1.0, 0.0),
        colour: [1.0; 3],
        ambient: [0.1; 3],
    };
    let mut scene = MeshScene::new(256, camera, light).unwrap_or_else(|_| unreachable!());
    extract_meshes(&mut world, &mut scene)?;
    let (result, counts) = count_allocations(|| extract_meshes(&mut world, &mut scene));
    assert_eq!(result?.extracted, 200);
    assert_eq!(counts.calls(), 0);
    Ok(())
}
