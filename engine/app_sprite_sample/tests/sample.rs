//! The sample's frame on the Null device: the golden shader asset is accepted, the triangle and the
//! sprites are valid command streams (the Null device checks handles, pipeline, bindings and index
//! ranges), and an asset that does not match the sprite vertex layout is refused. No GPU needed.

use app_sprite_sample::{GOLDEN_SHADER, Sample, SampleError, Scene};
use assets::dshd::{ModuleSpec, Program, Stage, UniformBlock, VertexInput};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::SurfaceId;
use render::null::NullDevice;

const SIZE: (u32, u32) = (640, 360);

fn device_and_surface() -> (NullDevice, SurfaceId) {
    let mut device = NullDevice::new();
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: 1,
            width: SIZE.0,
            height: SIZE.1,
        })
        .unwrap_or_else(|_| unreachable!());
    (device, surface)
}

fn sample() -> (NullDevice, SurfaceId, Sample) {
    let (mut device, surface) = device_and_surface();
    let program = Program::parse(GOLDEN_SHADER).unwrap_or_else(|_| unreachable!());
    let sample = Sample::new(&mut device, &program, surface, SIZE);
    assert_eq!(sample.as_ref().err(), None);
    (device, surface, sample.unwrap_or_else(|_| unreachable!()))
}

fn draws(device: &mut NullDevice, sample: &mut Sample, frame: u32, scene: Scene) -> u32 {
    let stats = sample.draw(device, frame, scene);
    assert_eq!(stats.as_ref().err(), None);
    stats.map_or(0, |s| s.draws)
}

#[test]
fn the_triangle_is_one_draw() {
    let (mut device, _, mut sample) = sample();
    assert_eq!(draws(&mut device, &mut sample, 0, Scene::Triangle), 1);
}

#[test]
fn the_sprites_are_one_draw_per_texture() {
    let (mut device, _, mut sample) = sample();
    // The checker (layer 1) and the three squares on the white texture (layer 2).
    assert_eq!(draws(&mut device, &mut sample, 0, Scene::Sprites), 2);
}

#[test]
fn frames_can_follow_each_other_in_any_order() {
    let (mut device, _, mut sample) = sample();
    for frame in 0..6 {
        let scene = if frame % 3 == 0 {
            Scene::Triangle
        } else {
            Scene::Sprites
        };
        let expected = if scene == Scene::Triangle { 1 } else { 2 };
        assert_eq!(draws(&mut device, &mut sample, frame, scene), expected);
    }
}

#[test]
fn a_resize_keeps_the_sample_drawing_and_zero_is_refused() {
    let (mut device, _, mut sample) = sample();
    assert_eq!(sample.size(), SIZE);
    assert_eq!(sample.resize(&mut device, (800, 600)), Ok(()));
    assert_eq!(sample.size(), (800, 600));
    assert_eq!(draws(&mut device, &mut sample, 1, Scene::Sprites), 2);
    assert_eq!(
        sample.resize(&mut device, (0, 600)),
        Err(SampleError::NoCamera)
    );
    assert_eq!(sample.size(), (800, 600));
}

#[test]
fn a_lost_surface_is_an_error_value() {
    let (mut device, surface, mut sample) = sample();
    device.destroy_surface(surface);
    let result = sample.draw(&mut device, 0, Scene::Triangle);
    assert!(matches!(result, Err(SampleError::Render(_))), "{result:?}");
}

const INPUTS: [VertexInput; 3] = [
    VertexInput {
        location: 0,
        components: 2,
    },
    VertexInput {
        location: 1,
        components: 2,
    },
    VertexInput {
        location: 2,
        components: 4,
    },
];
const CAMERA: UniformBlock = UniformBlock {
    binding: 0,
    size: 64,
};

/// A vertex-and-fragment asset with the given reflection and placeholder code.
fn asset(inputs: &[VertexInput], uniform: Option<UniformBlock>, texture: Option<u8>) -> Vec<u8> {
    let modules = [
        ModuleSpec {
            stage: Stage::Vertex,
            uniform,
            texture_binding: None,
            inputs,
            code: "void main() {}",
        },
        ModuleSpec {
            stage: Stage::Fragment,
            uniform: None,
            texture_binding: texture,
            inputs: &[],
            code: "void main() {}",
        },
    ];
    let mut bytes = vec![0u8; assets::dshd::encoded_len(&modules).unwrap_or(0)];
    assert!(assets::dshd::encode_into(&mut bytes, &modules).is_ok());
    bytes
}

fn refusal(bytes: &[u8]) -> Option<SampleError> {
    let (mut device, surface) = device_and_surface();
    let program = Program::parse(bytes).unwrap_or_else(|_| unreachable!());
    Sample::new(&mut device, &program, surface, SIZE).err()
}

#[test]
fn a_matching_hand_made_asset_is_accepted() {
    assert_eq!(refusal(&asset(&INPUTS, Some(CAMERA), Some(0))), None);
}

#[test]
fn an_asset_with_another_vertex_layout_is_refused() {
    let two = refusal(&asset(&INPUTS[..2], Some(CAMERA), Some(0)));
    assert!(matches!(two, Some(SampleError::Asset(_))), "{two:?}");
    let mut wide = INPUTS;
    wide[2].components = 3;
    let three = refusal(&asset(&wide, Some(CAMERA), Some(0)));
    assert!(matches!(three, Some(SampleError::Asset(_))), "{three:?}");
    let mut moved = INPUTS;
    moved[2].location = 5;
    let five = refusal(&asset(&moved, Some(CAMERA), Some(0)));
    assert!(matches!(five, Some(SampleError::Asset(_))), "{five:?}");
    let mut narrow = INPUTS;
    narrow[0].components = 3;
    let position = refusal(&asset(&narrow, Some(CAMERA), Some(0)));
    assert!(
        matches!(position, Some(SampleError::Asset(_))),
        "{position:?}"
    );
}

#[test]
fn an_asset_with_the_wrong_camera_block_is_refused() {
    let none = refusal(&asset(&INPUTS, None, Some(0)));
    assert!(matches!(none, Some(SampleError::Asset(_))), "{none:?}");
    let moved = UniformBlock {
        binding: 1,
        size: 64,
    };
    let other = refusal(&asset(&INPUTS, Some(moved), Some(0)));
    assert!(matches!(other, Some(SampleError::Asset(_))), "{other:?}");
    let small = UniformBlock {
        binding: 0,
        size: 16,
    };
    let short = refusal(&asset(&INPUTS, Some(small), Some(0)));
    assert!(matches!(short, Some(SampleError::Asset(_))), "{short:?}");
}

#[test]
fn an_asset_with_the_wrong_texture_unit_is_refused() {
    let none = refusal(&asset(&INPUTS, Some(CAMERA), None));
    assert!(matches!(none, Some(SampleError::Asset(_))), "{none:?}");
    let unit = refusal(&asset(&INPUTS, Some(CAMERA), Some(1)));
    assert!(matches!(unit, Some(SampleError::Asset(_))), "{unit:?}");
}

#[test]
fn an_asset_with_one_stage_is_refused() {
    let module = [ModuleSpec {
        stage: Stage::Vertex,
        uniform: Some(CAMERA),
        texture_binding: None,
        inputs: &INPUTS,
        code: "void main() {}",
    }];
    let mut bytes = vec![0u8; assets::dshd::encoded_len(&module).unwrap_or(0)];
    assert!(assets::dshd::encode_into(&mut bytes, &module).is_ok());
    let result = refusal(&bytes);
    assert!(matches!(result, Some(SampleError::Asset(_))), "{result:?}");
}

#[test]
fn the_errors_name_their_cause() {
    assert!(
        SampleError::Render(RenderError::SurfaceLost)
            .to_string()
            .contains("surface lost")
    );
    assert!(SampleError::Asset("x").to_string().contains('x'));
    assert!(SampleError::NoCamera.to_string().contains("camera"));
}
