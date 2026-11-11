//! The sample's frame on the Null device: both shader assets (GLSL and SPIR-V) are accepted, a
//! frame is one indexed draw into a pass with a depth texture (the Null device checks handles,
//! pipeline, bindings, index ranges and the depth texture), a resize makes a new depth texture,
//! and an asset that does not match the mesh vertex layout is refused. No GPU needed.

use app_mesh_sample::mesh_sample::model_for;
use app_mesh_sample::{MESH_GLSL, MESH_SPIRV, MeshError, MeshSample};
use assets::dshd::{ModuleSpec, Program, Stage, UniformBlock, VertexInput};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
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

fn sample_from(asset: &[u8]) -> (NullDevice, SurfaceId, MeshSample) {
    let (mut device, surface) = device_and_surface();
    let program = Program::parse(asset).unwrap_or_else(|_| unreachable!());
    let sample = MeshSample::new(&mut device, &program, surface, SIZE);
    assert_eq!(sample.as_ref().err(), None);
    (device, surface, sample.unwrap_or_else(|_| unreachable!()))
}

#[test]
fn a_frame_is_one_indexed_draw_with_either_asset() {
    for asset in [MESH_GLSL, MESH_SPIRV] {
        let (mut device, _, mut sample) = sample_from(asset);
        for frame in 0..4 {
            let stats = sample.draw(&mut device, frame);
            assert_eq!(stats.as_ref().err(), None);
            assert_eq!(stats.map_or(0, |s| s.draws), 1);
        }
    }
}

#[test]
fn a_resize_makes_a_new_depth_texture_and_zero_is_refused() {
    let (mut device, _, mut sample) = sample_from(MESH_GLSL);
    assert_eq!(sample.resize(&mut device, (800, 600)), Ok(()));
    assert_eq!(sample.size(), (800, 600));
    // The Null device refuses a depth texture of another size than the target, so this draw
    // proves the new one is the one in use.
    assert_eq!(sample.draw(&mut device, 1).err(), None);
    assert_eq!(
        sample.resize(&mut device, (0, 600)),
        Err(MeshError::NoCamera)
    );
    assert_eq!(sample.size(), (800, 600));
}

#[test]
fn a_lost_surface_is_an_error_value() {
    let (mut device, surface, mut sample) = sample_from(MESH_GLSL);
    device.destroy_surface(surface);
    let result = sample.draw(&mut device, 0);
    assert!(matches!(result, Err(MeshError::Render(_))), "{result:?}");
}

#[test]
fn the_cube_turns_from_frame_to_frame() {
    assert_ne!(model_for(0), model_for(1));
    assert_eq!(model_for(0), model_for(100_000));
}

const INPUTS: [VertexInput; 3] = [
    VertexInput {
        location: 0,
        components: 3,
    },
    VertexInput {
        location: 1,
        components: 3,
    },
    VertexInput {
        location: 2,
        components: 2,
    },
];
const TRANSFORM: UniformBlock = UniformBlock {
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

fn refusal(bytes: &[u8]) -> Option<MeshError> {
    let (mut device, surface) = device_and_surface();
    let program = Program::parse(bytes).unwrap_or_else(|_| unreachable!());
    MeshSample::new(&mut device, &program, surface, SIZE).err()
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "reads as a predicate on the result of refusal()"
)]
fn is_asset_error(error: Option<MeshError>) -> bool {
    matches!(error, Some(MeshError::Asset(_)))
}

#[test]
fn a_matching_hand_made_asset_is_accepted() {
    assert_eq!(refusal(&asset(&INPUTS, Some(TRANSFORM), Some(0))), None);
}

#[test]
fn an_asset_with_another_vertex_layout_is_refused() {
    let short = asset(&INPUTS[..2], Some(TRANSFORM), Some(0));
    assert!(is_asset_error(refusal(&short)));
    let mut flat = INPUTS;
    flat[0].components = 2;
    assert!(is_asset_error(refusal(&asset(
        &flat,
        Some(TRANSFORM),
        Some(0)
    ))));
    let mut moved = INPUTS;
    moved[2].location = 5;
    assert!(is_asset_error(refusal(&asset(
        &moved,
        Some(TRANSFORM),
        Some(0)
    ))));
}

#[test]
fn an_asset_with_the_wrong_transform_block_or_texture_unit_is_refused() {
    assert!(is_asset_error(refusal(&asset(&INPUTS, None, Some(0)))));
    let small = UniformBlock {
        binding: 0,
        size: 16,
    };
    assert!(is_asset_error(refusal(&asset(
        &INPUTS,
        Some(small),
        Some(0)
    ))));
    assert!(is_asset_error(refusal(&asset(
        &INPUTS,
        Some(TRANSFORM),
        None
    ))));
    assert!(is_asset_error(refusal(&asset(
        &INPUTS,
        Some(TRANSFORM),
        Some(1)
    ))));
}

#[test]
fn the_sprite_asset_is_not_a_mesh_asset() {
    assert!(is_asset_error(refusal(app_sprite_sample::GOLDEN_SHADER)));
}

#[test]
fn the_errors_name_their_cause() {
    assert!(MeshError::NoCamera.to_string().contains("camera"));
    assert!(MeshError::Asset("x").to_string().contains('x'));
    assert!(MeshError::Staging.to_string().contains("vertex"));
}
