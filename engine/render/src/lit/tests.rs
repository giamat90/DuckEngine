use super::*;
use crate::desc::{PassDesc, RenderTarget};
use crate::dump::dump;
use crate::null::MAX_BINDING_SLOTS;
use base::math::{Mat3, Mat4, Vec3};
use base::pool::Handle;

const GOLDEN: &str = include_str!("../../golden/lit_scene.txt");

fn h<T>(index: u32, generation: u32) -> Handle<T> {
    Handle::from_raw(index, generation)
}

fn float(bytes: &[u8], at: usize) -> f32 {
    let word = bytes
        .get(at..at + 4)
        .and_then(|s| <[u8; 4]>::try_from(s).ok());
    f32::from_le_bytes(word.unwrap_or_default())
}

fn floats<const N: usize>(bytes: &[u8], at: usize) -> [f32; N] {
    std::array::from_fn(|i| float(bytes, at + 4 * i))
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-6
}

fn near_all(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(*x, *y))
}

fn bindings(pipeline: u32, texture: u32) -> LitBindings {
    LitBindings {
        pipeline: h(pipeline, 0),
        vertices: h(10, 0),
        indices: h(11, 0),
        object: h(12, 0),
        light: h(13, 0),
        texture: h(texture, 0),
        sampler: h(15, 0),
    }
}

fn paint() -> Material {
    Material {
        colour: [0.25, 0.5, 0.75, 1.0],
    }
}

fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(0.0, -2.0, 0.0),
        colour: [1.0, 0.5, 0.25],
        ambient: [0.1, 0.2, 0.3],
    }
}

#[test]
fn the_two_blocks_have_the_sizes_the_shader_reads() {
    assert_eq!(OBJECT_BLOCK_BYTES, 192);
    assert_eq!(LIGHT_BLOCK_BYTES, 48);
}

#[test]
fn the_slots_are_distinct_and_inside_the_ports_range() {
    let slots = [SLOT_OBJECT, SLOT_LIGHT];
    for (i, a) in slots.iter().enumerate() {
        assert!(*a < MAX_BINDING_SLOTS);
        assert!(slots[i + 1..].iter().all(|b| a != b));
    }
}

#[test]
fn the_object_block_holds_mvp_model_normal_matrix_and_colour_in_std140_order() {
    let model = Mat4::from_rotation_y(0.5);
    let mvp = Mat4::from_rotation_x(0.25);
    let bytes = object_block(&mvp, &model, &paint()).unwrap_or([0; OBJECT_BLOCK_BYTES]);
    assert!(near_all(&floats::<16>(&bytes, 0), &mvp.to_cols_array()));
    assert!(near_all(&floats::<16>(&bytes, 64), &model.to_cols_array()));
    let rotation = Mat3::from_rotation_y(0.5);
    for (column, axis) in rotation.cols.iter().enumerate() {
        let at = 128 + 16 * column;
        assert!(near_all(
            &floats::<3>(&bytes, at),
            &[axis.x, axis.y, axis.z]
        ));
        assert!(
            near(float(&bytes, at + 12), 0.0),
            "std140 pads a mat3 column to four floats"
        );
    }
}

#[test]
fn the_normal_matrix_undoes_a_non_uniform_scale() {
    let model = Mat4::from_cols(
        base::math::Vec4::new(2.0, 0.0, 0.0, 0.0),
        base::math::Vec4::Y,
        base::math::Vec4::Z,
        base::math::Vec4::W,
    );
    let normal = normal_matrix(&model).unwrap_or(Mat3::IDENTITY);
    assert!(near_all(
        &[normal.cols[0].x, normal.cols[1].y, normal.cols[2].z],
        &[0.5, 1.0, 1.0]
    ));
}

#[test]
fn a_model_that_cannot_be_inverted_gives_no_object_block() {
    let flat = Mat4::from_cols(
        base::math::Vec4::X,
        base::math::Vec4::new(0.0, 0.0, 0.0, 0.0),
        base::math::Vec4::Z,
        base::math::Vec4::W,
    );
    assert!(normal_matrix(&flat).is_none());
    assert!(object_block(&Mat4::IDENTITY, &flat, &paint()).is_none());
    let mut broken = Mat4::IDENTITY;
    broken.cols[3].x = f32::NAN;
    assert!(object_block(&Mat4::IDENTITY, &broken, &paint()).is_none());
    let dark = Material {
        colour: [f32::NAN, 0.0, 0.0, 1.0],
    };
    assert!(object_block(&Mat4::IDENTITY, &Mat4::IDENTITY, &dark).is_none());
}

#[test]
fn the_light_block_points_at_the_light_with_a_unit_vector() {
    let bytes = sun().block_bytes().unwrap_or([0; LIGHT_BLOCK_BYTES]);
    assert!(near_all(&floats::<4>(&bytes, 0), &[0.0, 1.0, 0.0, 0.0]));
    assert!(near_all(&floats::<4>(&bytes, 16), &[1.0, 0.5, 0.25, 0.0]));
    assert!(near_all(&floats::<4>(&bytes, 32), &[0.1, 0.2, 0.3, 0.0]));
}

#[test]
fn a_light_without_a_direction_is_refused() {
    for direction in [Vec3::new(0.0, 0.0, 0.0), Vec3::new(f32::NAN, 0.0, 1.0)] {
        let light = DirectionalLight { direction, ..sun() };
        assert!(light.block_bytes().is_none());
    }
}

#[test]
fn an_empty_mesh_records_nothing() {
    let mut stream = CommandStream::new();
    record_lit_mesh(&mut stream, &bindings(0, 1), 0);
    assert_eq!(dump(&stream), "render-stream v1\n");
}

fn lit_pass(queue: &mut LitQueue) -> CommandStream {
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some([0.0, 0.0, 0.25, 1.0]),
        clear_depth: Some(1.0),
        depth: Some(h(20, 0)),
    });
    queue.sort();
    queue.record(&mut stream);
    stream.end_pass();
    stream
}

fn scene() -> CommandStream {
    let mut queue = LitQueue::new(4);
    for (pipeline, texture, count) in [(1, 7, 36), (0, 8, 36), (1, 6, 6)] {
        let pushed = queue.push(LitDraw {
            bindings: bindings(pipeline, texture),
            index_count: count,
        });
        assert!(pushed.is_ok());
    }
    lit_pass(&mut queue)
}

#[test]
fn the_lit_scene_matches_the_checked_in_golden_dump() {
    assert_eq!(dump(&scene()), GOLDEN);
}

#[test]
fn draws_are_ordered_by_pipeline_then_texture_then_insertion() {
    let mut queue = LitQueue::new(8);
    for (pipeline, texture) in [(1, 7), (0, 8), (1, 6), (1, 7), (0, 8)] {
        assert!(
            queue
                .push(LitDraw {
                    bindings: bindings(pipeline, texture),
                    index_count: 3
                })
                .is_ok()
        );
    }
    queue.sort();
    let order: Vec<(u32, u32)> = queue
        .draws()
        .iter()
        .map(|d| (d.bindings.pipeline.index(), d.bindings.texture.index()))
        .collect();
    assert_eq!(order, [(0, 8), (0, 8), (1, 6), (1, 7), (1, 7)]);
}

#[test]
fn equal_draws_keep_the_order_they_were_pushed_in() {
    let mut queue = LitQueue::new(4);
    for count in [3, 6, 9] {
        assert!(
            queue
                .push(LitDraw {
                    bindings: bindings(0, 1),
                    index_count: count
                })
                .is_ok()
        );
    }
    queue.sort();
    let counts: Vec<u32> = queue.draws().iter().map(|d| d.index_count).collect();
    assert_eq!(counts, [3, 6, 9]);
}

#[test]
fn a_full_queue_refuses_a_draw_and_is_unchanged() {
    let mut queue = LitQueue::new(1);
    assert!(
        queue
            .push(LitDraw {
                bindings: bindings(0, 1),
                index_count: 3
            })
            .is_ok()
    );
    let refused = queue.push(LitDraw {
        bindings: bindings(0, 1),
        index_count: 6,
    });
    assert_eq!(refused, Err(QueueFull));
    queue.sort();
    assert_eq!(queue.draws().len(), 1);
    queue.clear();
    assert!(queue.draws().is_empty());
    assert_eq!(queue.capacity(), 1);
    assert_eq!(LitQueue::new(5).capacity(), 5);
}

#[test]
fn many_equal_draws_still_keep_their_push_order() {
    let mut queue = LitQueue::new(200);
    for count in 1..=150u32 {
        let texture = if count % 2 == 0 { 1 } else { 2 };
        assert!(
            queue
                .push(LitDraw {
                    bindings: bindings(0, texture),
                    index_count: count
                })
                .is_ok()
        );
    }
    queue.sort();
    let counts: Vec<u32> = queue.draws().iter().map(|d| d.index_count).collect();
    let even = (1..=150u32).filter(|c| c % 2 == 0);
    let odd = (1..=150u32).filter(|c| c % 2 == 1);
    assert_eq!(counts, even.chain(odd).collect::<Vec<_>>());
}
