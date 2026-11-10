use super::*;
use base::math::Vec3;

fn project(camera: &OrthographicCamera, x: f32, y: f32) -> [f32; 2] {
    let m = camera.view_projection().unwrap_or(Mat4::IDENTITY);
    let p = m.transform_point3(Vec3::new(x, y, 0.0));
    [p.x, p.y]
}

fn close(a: [f32; 2], b: [f32; 2]) {
    assert!(
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5,
        "{a:?} != {b:?}"
    );
}

fn camera(x: f32, y: f32, height: f32, aspect: f32) -> OrthographicCamera {
    OrthographicCamera {
        position: Vec2::new(x, y),
        height,
        aspect,
    }
}

#[test]
fn the_camera_position_is_the_centre_of_the_view() {
    close(
        project(&camera(5.0, -2.0, 10.0, 2.0), 5.0, -2.0),
        [0.0, 0.0],
    );
}

#[test]
fn the_view_spans_height_by_height_times_aspect_with_y_up() {
    let c = camera(1.0, 1.0, 10.0, 2.0);
    close(project(&c, 11.0, 6.0), [1.0, 1.0]);
    close(project(&c, -9.0, -4.0), [-1.0, -1.0]);
    close(project(&c, 1.0, 4.0), [0.0, 0.6]);
}

#[test]
fn sprites_at_depth_zero_are_inside_the_clip_cube() {
    let m = camera(0.0, 0.0, 2.0, 1.0)
        .view_projection()
        .unwrap_or(Mat4::IDENTITY);
    let z = m.transform_point3(Vec3::new(0.0, 0.0, 0.0)).z;
    assert!((0.0..=1.0).contains(&z), "z = {z}");
}

#[test]
fn a_camera_with_no_extent_or_a_non_finite_value_has_no_matrix() {
    for c in [
        camera(0.0, 0.0, 0.0, 1.0),
        camera(0.0, 0.0, -1.0, 1.0),
        camera(0.0, 0.0, 1.0, 0.0),
        camera(0.0, 0.0, 1.0, f32::NAN),
        camera(f32::INFINITY, 0.0, 1.0, 1.0),
    ] {
        assert_eq!(c.view_projection(), None);
    }
}

#[test]
fn a_pixel_perfect_camera_shows_whole_pixels_and_snaps_its_position() {
    let c = OrthographicCamera::pixel_perfect(800, 600, Vec2::new(10.13, 5.0), 4.0);
    assert_eq!(
        c,
        Some(OrthographicCamera {
            position: Vec2::new(10.25, 5.0),
            height: 150.0,
            aspect: 800.0 / 600.0,
        })
    );
}

#[test]
fn a_pixel_perfect_camera_needs_a_viewport_and_a_positive_scale() {
    let at = Vec2::new(0.0, 0.0);
    assert_eq!(OrthographicCamera::pixel_perfect(0, 600, at, 4.0), None);
    assert_eq!(OrthographicCamera::pixel_perfect(800, 0, at, 4.0), None);
    assert_eq!(OrthographicCamera::pixel_perfect(800, 600, at, 0.0), None);
    assert_eq!(
        OrthographicCamera::pixel_perfect(800, 600, at, f32::NAN),
        None
    );
}

#[test]
fn matrix_bytes_are_the_column_major_little_endian_floats() {
    let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
    let bytes = matrix_bytes(&m);
    assert_eq!(bytes[0..4], 1.0f32.to_le_bytes());
    assert_eq!(bytes[48..52], 1.0f32.to_le_bytes());
    assert_eq!(bytes[52..56], 2.0f32.to_le_bytes());
    assert_eq!(bytes[56..60], 3.0f32.to_le_bytes());
    assert_eq!(bytes[60..64], 1.0f32.to_le_bytes());
}

#[test]
fn large_viewports_convert_to_floats_correctly() {
    assert_eq!(to_f32(0).to_bits(), 0.0f32.to_bits());
    assert_eq!(to_f32(65_535).to_bits(), 65_535.0f32.to_bits());
    assert_eq!(to_f32(65_536).to_bits(), 65_536.0f32.to_bits());
    assert_eq!(to_f32(70_001).to_bits(), 70_001.0f32.to_bits());
    let c = OrthographicCamera::pixel_perfect(140_000, 70_000, Vec2::new(0.0, 0.0), 1.0);
    let bits = c.map(|c| (c.height.to_bits(), c.aspect.to_bits()));
    assert_eq!(bits, Some((70_000.0f32.to_bits(), 2.0f32.to_bits())));
}

fn perspective(position: Vec3) -> PerspectiveCamera {
    PerspectiveCamera {
        position,
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: core::f32::consts::FRAC_PI_2,
        aspect: 1.0,
        near: 0.5,
        far: 20.0,
    }
}

fn clip(m: &Mat4, p: Vec3) -> [f32; 4] {
    let e = m.to_cols_array();
    [0, 1, 2, 3].map(|r| e[r] * p.x + e[4 + r] * p.y + e[8 + r] * p.z + e[12 + r])
}

#[test]
fn the_point_a_perspective_camera_looks_at_is_in_the_middle_of_the_view() {
    let made = perspective(Vec3::new(0.0, 0.0, 5.0)).view_projection();
    assert!(made.is_some());
    let matrix = made.unwrap_or(Mat4::IDENTITY);
    let centre = clip(&matrix, Vec3::ZERO);
    assert!(
        centre[0].abs() < 1e-5 && centre[1].abs() < 1e-5,
        "{centre:?}"
    );
    let depth = centre[2] / centre[3];
    assert!((0.0..=1.0).contains(&depth), "depth {depth}");
}

#[test]
fn nearer_points_get_a_smaller_depth_than_farther_ones() {
    let m = perspective(Vec3::new(0.0, 0.0, 5.0))
        .view_projection()
        .unwrap_or(Mat4::IDENTITY);
    let depth = |z: f32| {
        let c = clip(&m, Vec3::new(0.0, 0.0, z));
        c[2] / c[3]
    };
    assert!(depth(2.0) < depth(0.0) && depth(0.0) < depth(-5.0));
}

#[test]
fn a_model_matrix_moves_the_object_before_the_camera_sees_it() {
    let camera = perspective(Vec3::new(0.0, 0.0, 5.0));
    let model = Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0));
    let mvp = camera.mvp(&model).unwrap_or(Mat4::IDENTITY);
    let moved = clip(&mvp, Vec3::ZERO);
    let view_projection = camera.view_projection().unwrap_or(Mat4::IDENTITY);
    let plain = clip(&view_projection, Vec3::new(1.0, 0.0, 0.0));
    for (a, b) in moved.iter().zip(plain) {
        assert!((a - b).abs() < 1e-5, "{moved:?} != {plain:?}");
    }
}

#[test]
fn a_perspective_camera_that_cannot_look_has_no_matrix() {
    let from = perspective(Vec3::new(0.0, 0.0, 5.0));
    let cases = [
        perspective(Vec3::ZERO), // at its own target
        PerspectiveCamera { fov_y: 0.0, ..from },
        PerspectiveCamera { near: 30.0, ..from },
        PerspectiveCamera {
            aspect: f32::NAN,
            ..from
        },
    ];
    for c in cases {
        assert_eq!(c.view_projection(), None, "{c:?}");
        assert_eq!(c.mvp(&Mat4::IDENTITY), None);
    }
}
