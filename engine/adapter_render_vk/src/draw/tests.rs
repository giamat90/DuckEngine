//! The shader, pipeline and descriptor objects against the fake driver.

use super::*;
use crate::fake::draw::DrawCommand;
use crate::fake::{self, FakeGpu, Script};
use crate::vk_present::CommandBufferHandle;

fn draw() -> (crate::VulkanContext, Draw) {
    let context = fake::started(Script::with_gpus(vec![FakeGpu::good("Fake GPU", 2)]));
    match Draw::new(context.core()) {
        Ok(draw) => (context, draw),
        Err(error) => panic!("expected the draw objects, got: {error}"),
    }
}

const CMD: CommandBufferHandle = CommandBufferHandle(core::ptr::null_mut());
const MODULE_WORDS: [u32; 5] = [0x0723_0203, 0, 0, 0, 0];

fn spec(module: Handle, entry: &CStr) -> PipelineSpec<'_> {
    PipelineSpec {
        vertex: Stage { module, entry },
        fragment: Stage { module, entry },
        stride: 32,
        attributes: &[
            Attribute {
                location: 0,
                format: vd::FORMAT_R32G32_SFLOAT,
                offset: 0,
            },
            Attribute {
                location: 1,
                format: vd::FORMAT_R32G32_SFLOAT,
                offset: 8,
            },
        ],
        blend: false,
        depth_test: false,
        depth_write: false,
        color_format: crate::vk_present::FORMAT_B8G8R8A8_UNORM,
        depth_format: vd::FORMAT_UNDEFINED,
    }
}

fn module_of(draw: &mut Draw) -> Handle {
    match draw.create_shader(&MODULE_WORDS) {
        Ok(module) => module,
        Err(error) => panic!("expected a module, got: {error}"),
    }
}

#[test]
fn the_device_has_two_set_layouts_and_one_pipeline_layout() {
    let (_context, draw) = draw();
    let layouts = fake::with(|s| (s.draw.set_layouts.len(), s.draw.pipeline_layouts.len()));
    assert_eq!(layouts, (2, 1));
    assert_ne!(draw.layout, 0);
}

#[test]
fn everything_made_is_freed_when_the_draw_objects_are_dropped() {
    let (context, mut draw) = draw();
    let module = module_of(&mut draw);
    assert!(draw.create_pool().is_ok());
    assert!(draw.create_pipeline(&spec(module, c"main")).is_ok());
    drop(draw);
    drop(context);
    let left: Vec<_> = fake::live_objects().into_iter().map(|o| o.0).collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_failed_layout_leaves_nothing_behind() {
    let context = fake::started(Script::with_gpus(vec![FakeGpu::good("Fake GPU", 2)]));
    fake::with(|s| s.fail_call = Some("vkCreatePipelineLayout"));
    assert!(Draw::new(context.core()).is_err());
    drop(context);
    let left: Vec<_> = fake::live_objects().into_iter().map(|o| o.0).collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_shader_module_gets_the_words_it_was_given() {
    let (_context, mut draw) = draw();
    let words = [0x0723_0203, 1, 2, 3, 4, 5];
    let Ok(module) = draw.create_shader(&words) else {
        panic!("a module");
    };
    let kept = fake::with(|s| s.draw.modules.get(&module).cloned());
    assert_eq!(kept.as_deref(), Some(&words[..]));
}

#[test]
fn a_destroyed_object_is_destroyed_once_and_a_stranger_is_ignored() {
    let (_context, mut draw) = draw();
    let module = module_of(&mut draw);
    draw.destroy_shader(module);
    draw.destroy_shader(module);
    draw.destroy_shader(12345);
    assert_eq!(fake::with(|s| s.draw.modules.len()), 0);
}

#[test]
fn a_pipeline_carries_the_vertex_layout_the_formats_and_the_blend_and_depth_state() {
    let (_context, mut draw) = draw();
    let module = module_of(&mut draw);
    let mut wanted = spec(module, c"main");
    wanted.blend = true;
    wanted.depth_test = true;
    wanted.depth_write = true;
    wanted.depth_format = crate::vk_resource::FORMAT_D32_SFLOAT;
    let Ok(pipeline) = draw.create_pipeline(&wanted) else {
        panic!("a pipeline");
    };
    let Some(record) = fake::with(|s| s.draw.pipelines.get(&pipeline).cloned()) else {
        panic!("the fake kept no record");
    };
    assert_eq!(record.stride, 32);
    assert_eq!(
        record.attributes,
        [
            (0, vd::FORMAT_R32G32_SFLOAT, 0),
            (1, vd::FORMAT_R32G32_SFLOAT, 8)
        ]
    );
    assert_eq!(
        record.color_format,
        crate::vk_present::FORMAT_B8G8R8A8_UNORM
    );
    assert_eq!(record.depth_format, crate::vk_resource::FORMAT_D32_SFLOAT);
    assert!(record.blend && record.depth_test && record.depth_write);
    assert_eq!(record.entries, ["main", "main"]);
    assert_eq!(
        record.stages.iter().map(|s| s.0).collect::<Vec<_>>(),
        [vd::SHADER_STAGE_VERTEX, vd::SHADER_STAGE_FRAGMENT]
    );
}

#[test]
fn a_refused_pipeline_is_an_error_value_and_is_not_listed() {
    let (_context, mut draw) = draw();
    let module = module_of(&mut draw);
    fake::with(|s| s.fail_call = Some("vkCreateGraphicsPipelines"));
    assert!(draw.create_pipeline(&spec(module, c"main")).is_err());
    assert!(draw.pipelines.is_empty());
}

#[test]
fn bindings_write_the_bound_slots_and_bind_two_sets() {
    let (_context, mut draw) = draw();
    let Ok(pool) = draw.create_pool() else {
        panic!("a pool");
    };
    let mut bindings = Bindings::default();
    bindings.uniforms[0] = Some(0xB0);
    bindings.textures[3] = Some((0x71, 0x5A));
    assert!(draw.bind_descriptors(CMD, pool, &bindings).is_ok());
    let (writes, commands) = fake::with(|s| (s.draw.writes.clone(), s.draw.commands.clone()));
    assert_eq!(writes.len(), 2);
    assert_eq!(
        (
            writes[0].binding,
            writes[0].descriptor_type,
            writes[0].object
        ),
        (0, vd::DESCRIPTOR_UNIFORM_BUFFER, 0xB0)
    );
    assert_eq!(
        (
            writes[1].binding,
            writes[1].object,
            writes[1].sampler,
            writes[1].layout
        ),
        (3, 0x71, 0x5A, LAYOUT_SHADER_READ)
    );
    let Some(DrawCommand::BindSets { first, sets }) = commands.last().copied() else {
        panic!("sets were bound");
    };
    assert_eq!(first, 0);
    assert_eq!(writes[0].set, sets[0]);
    assert_eq!(writes[1].set, sets[1]);
}

#[test]
fn a_pool_that_is_full_reports_an_error_and_a_reset_makes_room() {
    let (_context, mut draw) = draw();
    let Ok(pool) = draw.create_pool() else {
        panic!("a pool");
    };
    let bindings = Bindings::default();
    for _ in 0..POOL_SETS / 2 {
        assert!(draw.bind_descriptors(CMD, pool, &bindings).is_ok());
    }
    assert!(draw.bind_descriptors(CMD, pool, &bindings).is_err());
    assert_eq!(draw.reset_pool(pool), Ok(()));
    assert!(draw.bind_descriptors(CMD, pool, &bindings).is_ok());
}

#[test]
fn the_state_and_draw_commands_reach_the_driver_as_given() {
    let (_context, mut draw) = draw();
    let module = module_of(&mut draw);
    let Ok(pipeline) = draw.create_pipeline(&spec(module, c"main")) else {
        panic!("a pipeline");
    };
    let viewport = Viewport {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: -4.0,
        min_depth: 0.0,
        max_depth: 1.0,
    };
    let scissor = Rect2D::default();
    draw.bind_pipeline(CMD, pipeline);
    draw.set_viewport(CMD, &viewport, &scissor);
    draw.bind_vertex(CMD, 0xAA, 16);
    draw.bind_index(CMD, 0xBB, 8, true);
    draw.bind_index(CMD, 0xBB, 0, false);
    draw.draw(CMD, 3, 1);
    draw.draw_indexed(CMD, 6, 2);
    let commands = fake::with(|s| s.draw.commands.clone());
    assert_eq!(
        commands,
        [
            DrawCommand::BindPipeline(pipeline),
            DrawCommand::Viewport(viewport),
            DrawCommand::Scissor(scissor),
            DrawCommand::BindVertex {
                buffer: 0xAA,
                offset: 16
            },
            DrawCommand::BindIndex {
                buffer: 0xBB,
                offset: 8,
                index_type: vd::INDEX_UINT32
            },
            DrawCommand::BindIndex {
                buffer: 0xBB,
                offset: 0,
                index_type: vd::INDEX_UINT16
            },
            DrawCommand::Draw {
                vertices: 3,
                instances: 1
            },
            DrawCommand::DrawIndexed {
                indices: 6,
                instances: 2
            },
        ]
    );
}
