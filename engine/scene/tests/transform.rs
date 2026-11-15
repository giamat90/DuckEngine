//! The local transform: scale, then rotation, then translation, as a matrix.

use base::math::Vec2;
use scene::{Transform2D, WorldTransform2D};
use std::f32::consts::FRAC_PI_2;

const EPSILON: f32 = 1e-5;

fn close(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < EPSILON && (a.y - b.y).abs() < EPSILON
}

fn apply(transform: Transform2D, point: Vec2) -> Vec2 {
    WorldTransform2D {
        matrix: transform.to_mat3(),
    }
    .transform_point(point)
}

#[test]
fn the_default_transform_changes_nothing() {
    let point = Vec2::new(3.0, -2.0);
    assert!(close(apply(Transform2D::default(), point), point));
}

#[test]
fn a_position_moves_the_origin() {
    let moved = apply(Transform2D::from_position(Vec2::new(4.0, 5.0)), Vec2::ZERO);
    assert!(close(moved, Vec2::new(4.0, 5.0)));
}

#[test]
fn a_quarter_turn_takes_x_to_y() {
    let turned = Transform2D {
        rotation: FRAC_PI_2,
        ..Transform2D::IDENTITY
    };
    assert!(close(
        apply(turned, Vec2::new(1.0, 0.0)),
        Vec2::new(0.0, 1.0)
    ));
}

#[test]
fn a_scale_stretches_each_axis() {
    let stretched = Transform2D {
        scale: Vec2::new(2.0, 3.0),
        ..Transform2D::IDENTITY
    };
    assert!(close(
        apply(stretched, Vec2::new(1.0, 1.0)),
        Vec2::new(2.0, 3.0)
    ));
}

#[test]
fn scale_comes_first_then_rotation_then_position() {
    let all = Transform2D {
        position: Vec2::new(10.0, 0.0),
        rotation: FRAC_PI_2,
        scale: Vec2::new(2.0, 2.0),
    };
    assert!(close(apply(all, Vec2::new(1.0, 0.0)), Vec2::new(10.0, 2.0)));
}

#[test]
fn the_default_world_transform_is_the_identity() {
    let point = Vec2::new(-1.5, 7.0);
    assert!(close(
        WorldTransform2D::default().transform_point(point),
        point
    ));
}
