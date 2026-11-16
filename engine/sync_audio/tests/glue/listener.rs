//! The pan of a sound by where it is heard from.

use sync_audio::Listener;

fn at(x: f32, half_width: f32) -> Listener {
    Listener { x, half_width }
}

#[test]
fn the_pan_runs_from_hard_left_to_hard_right_over_the_half_width() {
    let listener = at(0.0, 10.0);
    assert!((listener.pan(0.0)).abs() < f32::EPSILON);
    assert!((listener.pan(5.0) - 0.5).abs() < 1e-6);
    assert!((listener.pan(-5.0) + 0.5).abs() < 1e-6);
    assert!((listener.pan(10.0) - 1.0).abs() < 1e-6);
    assert!((listener.pan(-10.0) + 1.0).abs() < 1e-6);
}

#[test]
fn beyond_the_half_width_the_pan_stays_at_the_edge() {
    let listener = at(0.0, 10.0);
    assert!((listener.pan(1000.0) - 1.0).abs() < f32::EPSILON);
    assert!((listener.pan(-1000.0) + 1.0).abs() < f32::EPSILON);
}

#[test]
fn the_pan_is_relative_to_the_listener() {
    let listener = at(100.0, 10.0);
    assert!((listener.pan(100.0)).abs() < f32::EPSILON);
    assert!((listener.pan(105.0) - 0.5).abs() < 1e-6);
    assert!((listener.pan(95.0) + 0.5).abs() < 1e-6);
}

#[test]
fn a_number_that_cannot_be_used_gives_the_centre() {
    assert!(at(0.0, 10.0).pan(f32::NAN).abs() < f32::EPSILON);
    assert!(at(0.0, 10.0).pan(f32::INFINITY).abs() < f32::EPSILON);
    assert!(at(f32::NAN, 10.0).pan(3.0).abs() < f32::EPSILON);
    for half_width in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(at(0.0, half_width).pan(3.0).abs() < f32::EPSILON);
    }
}
