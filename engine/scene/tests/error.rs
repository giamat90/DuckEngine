//! The errors of the scene say what went wrong and convert from the world's errors.

use ecs::EcsError;
use scene::SceneError;

#[test]
fn each_error_has_its_own_message() {
    let messages = [
        SceneError::Ecs(EcsError::StaleEntity).to_string(),
        SceneError::MissingTransform.to_string(),
        SceneError::Cycle.to_string(),
        SceneError::BrokenHierarchy.to_string(),
    ];
    assert!(messages[0].contains("despawned"), "{}", messages[0]);
    assert!(
        messages[1].contains("no local transform"),
        "{}",
        messages[1]
    );
    assert!(messages[2].contains("descendants"), "{}", messages[2]);
    assert!(messages[3].contains("loop"), "{}", messages[3]);
}

#[test]
fn a_world_error_converts_into_a_scene_error() {
    assert_eq!(
        SceneError::from(EcsError::MissingComponent),
        SceneError::Ecs(EcsError::MissingComponent)
    );
}
