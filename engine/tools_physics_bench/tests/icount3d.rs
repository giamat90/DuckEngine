//! Card 8-29: the deterministic 3D workload of the instruction-count gate. The same pyramid, solver
//! and step count always give the same body count and state hash, so the instruction count of two
//! builds is comparable; a different size or step count gives a different run.

use tools_physics_bench::scene3d::icount_workload3;

#[test]
fn a_pyramid_of_layers_has_the_sum_of_the_squares_in_boxes() {
    let counts: Vec<u16> = [1_u8, 2, 3, 14]
        .into_iter()
        .map(|layers| icount_workload3(layers, 0).map(|(bodies, _)| bodies))
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    assert_eq!(counts, [1, 5, 14, 1015]);
}

#[test]
fn the_same_workload_gives_the_same_hash_twice() {
    let first = icount_workload3(4, 20);
    assert!(first.is_ok());
    assert_eq!(first, icount_workload3(4, 20));
}

#[test]
fn more_steps_or_more_layers_change_the_hash() {
    let base = icount_workload3(4, 20).map(|(_, hash)| hash);
    let later = icount_workload3(4, 21).map(|(_, hash)| hash);
    let taller = icount_workload3(5, 20).map(|(_, hash)| hash);
    assert!(base.is_ok() && later.is_ok() && taller.is_ok());
    assert_ne!(base, later);
    assert_ne!(base, taller);
}

#[test]
fn the_workload_hash_is_the_hash_of_the_serial_scalar_scene() {
    use physics::d3::SolverExecution;
    use tools_physics_bench::scene3d::{Scene3, Scene3World};
    // The pyramid of the catalogue has 6 layers; the workload with 6 layers must be the same world.
    let mut scene = Scene3World::new(Scene3::Pyramid, &SolverExecution::SERIAL, false);
    let expected = scene.as_mut().map(|world| {
        for _ in 0..10 {
            let _ = world.step();
        }
        world.hash()
    });
    assert_eq!(
        icount_workload3(6, 10).map(|(_, hash)| hash).ok(),
        expected.ok()
    );
}
