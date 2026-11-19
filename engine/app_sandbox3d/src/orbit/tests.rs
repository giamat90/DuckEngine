use super::*;

fn start() -> OrbitCamera {
    OrbitCamera::new(Vec3::new(0.0, 1.0, 0.0), 0.0, 0.5, 16.0)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn the_camera_sits_at_its_distance_from_the_target_looking_at_it() {
    let camera = start().camera(1.5);
    let offset = camera.position - camera.target;
    assert!(near(offset.length(), 16.0));
    assert_eq!(camera.target, Vec3::new(0.0, 1.0, 0.0));
    assert!(near(camera.aspect, 1.5));
    assert!(camera.view_projection().is_some());
}

#[test]
fn zero_yaw_looks_along_minus_z_from_the_front() {
    let at = OrbitCamera::new(Vec3::ZERO, 0.0, 0.05, 10.0).position();
    assert!(at.z > 9.9 && near(at.x, 0.0), "{at:?}");
}

#[test]
fn a_drag_to_the_right_turns_the_view_and_a_drag_down_raises_it() {
    let mut camera = start();
    camera.drag(100.0, 0.0);
    assert!(near(camera.yaw(), -100.0 * TURN_PER_PIXEL));
    assert!(near(camera.pitch(), 0.5));
    camera.drag(0.0, 50.0);
    assert!(near(camera.pitch(), 0.5 + 50.0 * TURN_PER_PIXEL));
}

#[test]
fn the_pitch_stays_between_the_horizon_and_the_zenith() {
    let mut camera = start();
    camera.drag(0.0, 1.0e6);
    assert!(near(camera.pitch(), MAX_PITCH));
    camera.drag(0.0, -1.0e6);
    assert!(near(camera.pitch(), MIN_PITCH));
}

#[test]
fn the_wheel_comes_closer_or_goes_back_within_limits() {
    let mut camera = start();
    camera.zoom(1.0);
    assert!(near(camera.distance(), 16.0 * (1.0 - ZOOM_PER_NOTCH)));
    camera.zoom(-1.0);
    assert!(camera.distance() > 14.4);
    camera.zoom(1000.0);
    assert!(near(camera.distance(), MIN_DISTANCE));
    camera.zoom(-1000.0);
    assert!(near(camera.distance(), MAX_DISTANCE));
}

#[test]
fn input_that_is_not_a_number_changes_nothing() {
    let mut camera = start();
    let before = camera;
    camera.drag(f32::NAN, f32::INFINITY);
    camera.zoom(f32::NAN);
    camera.zoom(f32::INFINITY);
    assert_eq!(camera, before);
}

#[test]
fn a_new_camera_is_brought_into_range() {
    let camera = OrbitCamera::new(Vec3::ZERO, f32::NAN, f32::NAN, f32::NAN);
    assert!(near(camera.yaw(), 0.0));
    assert!(near(camera.pitch(), MIN_PITCH));
    assert!(near(camera.distance(), MIN_DISTANCE));
    let far = OrbitCamera::new(Vec3::ZERO, 0.0, 9.0, 900.0);
    assert!(near(far.pitch(), MAX_PITCH));
    assert!(near(far.distance(), MAX_DISTANCE));
}

#[test]
fn the_position_follows_yaw_and_pitch_on_a_sphere_round_the_target() {
    let at = OrbitCamera::new(Vec3::new(1.0, 2.0, 3.0), 0.7, 0.4, 10.0).position();
    let offset = at - Vec3::new(1.0, 2.0, 3.0);
    assert!(near(offset.length(), 10.0));
    assert!(near(offset.x, 10.0 * 0.4_f32.cos() * 0.7_f32.sin()));
    assert!(near(offset.y, 10.0 * 0.4_f32.sin()));
    assert!(near(offset.z, 10.0 * 0.4_f32.cos() * 0.7_f32.cos()));
}
