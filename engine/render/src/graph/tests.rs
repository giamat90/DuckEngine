use super::*;
use crate::desc::SurfaceDesc;
use crate::desc::{TextureDesc, TextureFormat};
use crate::device::IRenderDevice;
use crate::null::NullDevice;
use crate::stream::Command;

const CLEAR: [f32; 4] = [0.1, 0.2, 0.3, 1.0];

fn rgba(size: u32) -> TextureDesc {
    TextureDesc {
        width: size,
        height: size,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }
}

fn real_texture(device: &mut NullDevice) -> TextureId {
    device
        .create_texture(rgba(4))
        .unwrap_or_else(|_| unreachable!())
}

/// Passes and barriers as plain tuples, for readable comparisons.
fn barriers(compiled: &CompiledGraph) -> Vec<(usize, usize, ResourceState, ResourceState)> {
    compiled
        .barriers()
        .iter()
        .map(|b| (b.before, b.resource.index(), b.from, b.to))
        .collect()
}

fn order(compiled: &CompiledGraph) -> Vec<usize> {
    compiled.order().iter().map(|p| p.id.index()).collect()
}

fn decode(stream: &CommandStream) -> Vec<Command> {
    stream
        .commands()
        .collect::<Result<_, _>>()
        .unwrap_or_default()
}

/// `unwrap` stand-in for tests: fails with the error shown.
trait Must<T> {
    fn must(self) -> T;
}

impl<T, E: std::fmt::Debug> Must<T> for Result<T, E> {
    fn must(self) -> T {
        assert!(self.is_ok(), "expected Ok, got {:?}", self.as_ref().err());
        self.unwrap_or_else(|_| unreachable!())
    }
}

use ResourceState::{ColorTarget, Present, ShaderRead, Undefined};

#[test]
fn an_empty_graph_compiles_to_nothing() {
    let compiled = RenderGraph::new().compile();
    assert!(compiled.order().is_empty());
    assert!(compiled.barriers().is_empty());
    assert_eq!(compiled.culled(), 0);
    let mut stream = CommandStream::new();
    assert_eq!(compiled.record(&[], &mut stream, |_, _| {}), Ok(()));
    assert!(stream.is_empty());
}

#[test]
fn one_pass_to_the_backbuffer_gets_a_barrier_in_and_a_present_barrier_out() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let pass = graph.add_pass("main", back, Some(CLEAR), &[]).must();
    let compiled = graph.compile();
    assert_eq!(order(&compiled), vec![pass.index()]);
    assert_eq!(
        barriers(&compiled),
        vec![(0, 0, Undefined, ColorTarget), (1, 0, ColorTarget, Present)]
    );
}

#[test]
fn recording_emits_barriers_pass_begin_the_body_and_pass_end_in_order() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let pass = graph.add_pass("main", back, Some(CLEAR), &[]).must();
    let compiled = graph.compile();
    let mut stream = CommandStream::new();
    let mut seen = Vec::new();
    let result = compiled.record(&[], &mut stream, |id, s| {
        seen.push(id);
        s.draw(3, 1);
    });
    assert_eq!(result, Ok(()));
    assert_eq!(seen, vec![pass]);
    let pass_desc = PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some(CLEAR),
        clear_depth: None,
    };
    assert_eq!(
        decode(&stream),
        vec![
            Command::Barrier {
                target: RenderTarget::Backbuffer,
                from: Undefined,
                to: ColorTarget
            },
            Command::BeginPass(pass_desc),
            Command::Draw {
                vertices: 3,
                instances: 1
            },
            Command::EndPass,
            Command::Barrier {
                target: RenderTarget::Backbuffer,
                from: ColorTarget,
                to: Present
            },
        ]
    );
}

#[test]
fn an_offscreen_pass_feeds_a_composite_pass() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let scene = graph.transient(rgba(4)).must();
    let first = graph.add_pass("scene", scene, Some(CLEAR), &[]).must();
    let second = graph.add_pass("composite", back, None, &[scene]).must();
    let compiled = graph.compile();
    assert_eq!(order(&compiled), vec![first.index(), second.index()]);
    assert_eq!(
        barriers(&compiled),
        vec![
            (0, 1, Undefined, ColorTarget),
            (1, 1, ColorTarget, ShaderRead),
            (1, 0, Undefined, ColorTarget),
            (2, 0, ColorTarget, Present),
        ]
    );
    assert_eq!(compiled.lifetime(scene), Some((0, 1)));
    assert_eq!(compiled.lifetime(back), Some((1, 1)));
}

#[test]
fn a_pass_whose_output_nothing_reads_is_culled() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let unused = graph.transient(rgba(4)).must();
    graph.add_pass("dead", unused, Some(CLEAR), &[]).must();
    let live = graph.add_pass("main", back, None, &[]).must();
    let compiled = graph.compile();
    assert_eq!(order(&compiled), vec![live.index()]);
    assert_eq!(compiled.culled(), 1);
    assert_eq!(compiled.lifetime(unused), None);
}

#[test]
fn a_chain_of_passes_feeding_nothing_is_culled_whole() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let a = graph.transient(rgba(4)).must();
    let b = graph.transient(rgba(4)).must();
    graph.add_pass("a", a, None, &[]).must();
    graph.add_pass("b", b, None, &[a]).must();
    graph.add_pass("main", back, None, &[]).must();
    let compiled = graph.compile();
    assert_eq!(compiled.order().len(), 1);
    assert_eq!(compiled.culled(), 2);
}

#[test]
fn a_chain_feeding_the_backbuffer_is_kept_whole() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let a = graph.transient(rgba(4)).must();
    let b = graph.transient(rgba(4)).must();
    graph.add_pass("a", a, None, &[]).must();
    graph.add_pass("b", b, None, &[a]).must();
    graph.add_pass("main", back, None, &[b]).must();
    let compiled = graph.compile();
    assert_eq!(compiled.order().len(), 3);
    assert_eq!(compiled.culled(), 0);
}

#[test]
fn an_imported_texture_target_is_never_culled_and_ends_in_its_final_state() {
    let mut device = NullDevice::new();
    let texture = real_texture(&mut device);
    let mut graph = RenderGraph::new();
    let target = graph.import_texture(texture, Undefined, ShaderRead).must();
    graph.add_pass("bake", target, Some(CLEAR), &[]).must();
    let compiled = graph.compile();
    assert_eq!(compiled.culled(), 0);
    assert_eq!(
        barriers(&compiled),
        vec![
            (0, 0, Undefined, ColorTarget),
            (1, 0, ColorTarget, ShaderRead)
        ]
    );
}

#[test]
fn an_imported_texture_already_readable_needs_no_barrier_to_be_read() {
    let mut device = NullDevice::new();
    let atlas = real_texture(&mut device);
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let atlas = graph.import_texture(atlas, ShaderRead, ShaderRead).must();
    graph.add_pass("main", back, None, &[atlas]).must();
    let compiled = graph.compile();
    assert_eq!(
        barriers(&compiled),
        vec![(0, 0, Undefined, ColorTarget), (1, 0, ColorTarget, Present)]
    );
}

#[test]
fn consecutive_writes_and_repeated_reads_need_one_barrier_each() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let scene = graph.transient(rgba(4)).must();
    graph.add_pass("scene", scene, None, &[]).must();
    graph.add_pass("a", back, None, &[scene]).must();
    graph.add_pass("b", back, None, &[scene]).must();
    let compiled = graph.compile();
    let scene_barriers = barriers(&compiled)
        .into_iter()
        .filter(|b| b.1 == scene.index())
        .count();
    assert_eq!(scene_barriers, 2);
    let back_barriers = barriers(&compiled)
        .into_iter()
        .filter(|b| b.1 == back.index())
        .count();
    assert_eq!(back_barriers, 2);
}

#[test]
fn invalid_declarations_are_refused() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let scene = graph.transient(rgba(4)).must();
    let depth = graph
        .transient(TextureDesc {
            format: TextureFormat::Depth32F,
            ..rgba(4)
        })
        .must();
    let ghost = ResourceId(99);
    assert_eq!(
        graph.add_pass("p", ghost, None, &[]),
        Err(GraphError::UnknownResource)
    );
    assert_eq!(
        graph.add_pass("p", back, None, &[ghost]),
        Err(GraphError::UnknownResource)
    );
    assert_eq!(
        graph.add_pass("p", scene, None, &[scene]),
        Err(GraphError::SameResourceReadAndWritten)
    );
    assert_eq!(
        graph.add_pass("p", scene, None, &[back]),
        Err(GraphError::BackbufferRead)
    );
    assert_eq!(
        graph.add_pass("p", back, None, &[scene]),
        Err(GraphError::ReadBeforeWrite)
    );
    assert_eq!(
        graph.add_pass("p", depth, None, &[]),
        Err(GraphError::TargetNotRenderable)
    );
    let many = [scene; MAX_READS + 1];
    assert_eq!(
        graph.add_pass("p", back, None, &many),
        Err(GraphError::TooManyReads)
    );
}

#[test]
fn a_refused_declaration_changes_nothing() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let scene = graph.transient(rgba(4)).must();
    assert!(graph.add_pass("p", back, None, &[scene]).is_err());
    graph.add_pass("main", back, None, &[]).must();
    assert_eq!(graph.compile().order().len(), 1);
}

#[test]
fn the_fixed_capacities_are_enforced_with_errors() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    for _ in 0..MAX_PASSES {
        assert!(graph.add_pass("p", back, None, &[]).is_ok());
    }
    assert_eq!(
        graph.add_pass("p", back, None, &[]),
        Err(GraphError::TooManyPasses)
    );
    let mut graph = RenderGraph::new();
    for _ in 0..MAX_RESOURCES {
        assert!(graph.transient(rgba(4)).is_ok());
    }
    assert_eq!(graph.transient(rgba(4)), Err(GraphError::TooManyResources));
}

#[test]
fn a_full_graph_still_compiles_within_its_barrier_capacity() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let mut shared = Vec::new();
    for _ in 0..MAX_READS {
        let t = graph.transient(rgba(4)).must();
        graph.add_pass("w", t, None, &[]).must();
        shared.push(t);
    }
    while graph.add_pass("main", back, None, &shared).is_ok() {}
    let compiled = graph.compile();
    assert!(compiled.barriers().len() <= MAX_BARRIERS);
    assert_eq!(compiled.order().len(), MAX_PASSES);
}

#[test]
fn recording_with_an_unbound_transient_fails_and_writes_nothing() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let scene = graph.transient(rgba(4)).must();
    graph.add_pass("scene", scene, None, &[]).must();
    graph.add_pass("main", back, None, &[scene]).must();
    let compiled = graph.compile();
    let mut stream = CommandStream::new();
    let mut calls = 0;
    let result = compiled.record(&[], &mut stream, |_, _| calls += 1);
    assert_eq!(result, Err(GraphError::Unbound(scene)));
    assert_eq!(calls, 0);
    assert!(stream.is_empty());
}

#[test]
fn a_culled_transient_needs_no_binding() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let unused = graph.transient(rgba(4)).must();
    graph.add_pass("dead", unused, None, &[]).must();
    graph.add_pass("main", back, None, &[]).must();
    let mut stream = CommandStream::new();
    assert_eq!(graph.compile().record(&[], &mut stream, |_, _| {}), Ok(()));
}

#[test]
fn a_recorded_graph_is_accepted_by_the_null_device() {
    let mut device = NullDevice::new();
    let target = real_texture(&mut device);
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: 1,
            width: 8,
            height: 8,
        })
        .unwrap_or_else(|_| unreachable!());
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let scene = graph.transient(rgba(4)).must();
    graph.add_pass("scene", scene, Some(CLEAR), &[]).must();
    graph.add_pass("main", back, None, &[scene]).must();
    let mut stream = CommandStream::new();
    let recorded = graph
        .compile()
        .record(&[(scene, target)], &mut stream, |_, _| {});
    assert_eq!(recorded, Ok(()));
    let frame = device
        .begin_frame(surface)
        .unwrap_or_else(|_| unreachable!());
    let stats = device.submit(frame, &stream);
    assert_eq!(device.end_frame(frame), Ok(()));
    assert_eq!(
        stats,
        Ok(crate::desc::SubmitStats {
            commands: 8,
            draws: 0
        })
    );
}

#[test]
fn recording_a_second_frame_reuses_the_stream_capacity() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    graph.add_pass("main", back, Some(CLEAR), &[]).must();
    let compiled = graph.compile();
    let mut stream = CommandStream::new();
    assert_eq!(
        compiled.record(&[], &mut stream, |_, s| s.draw(3, 1)),
        Ok(())
    );
    let capacity = stream.capacity();
    let bytes = stream.as_bytes().to_vec();
    stream.clear();
    assert_eq!(
        compiled.record(&[], &mut stream, |_, s| s.draw(3, 1)),
        Ok(())
    );
    assert_eq!(stream.capacity(), capacity);
    assert_eq!(stream.as_bytes(), bytes.as_slice());
}

#[test]
fn compiling_is_pure_and_repeatable() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let scene = graph.transient(rgba(4)).must();
    graph.add_pass("scene", scene, None, &[]).must();
    graph.add_pass("main", back, None, &[scene]).must();
    let first = graph.compile();
    let second = graph.compile();
    assert_eq!(barriers(&first), barriers(&second));
    assert_eq!(order(&first), order(&second));
}

#[test]
fn graph_errors_have_distinct_readable_messages() {
    let all = [
        GraphError::TooManyPasses,
        GraphError::TooManyResources,
        GraphError::TooManyReads,
        GraphError::UnknownResource,
        GraphError::ReadBeforeWrite,
        GraphError::SameResourceReadAndWritten,
        GraphError::BackbufferRead,
        GraphError::TargetNotRenderable,
        GraphError::Unbound(ResourceId(0)),
    ];
    let messages: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (i, a) in messages.iter().enumerate() {
        assert!(!a.is_empty());
        assert!(messages.iter().skip(i + 1).all(|b| a != b));
    }
}

#[test]
fn pass_ids_follow_declaration_order() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let first = graph.add_pass("a", back, None, &[]).must();
    let second = graph.add_pass("b", back, None, &[]).must();
    assert_eq!((first.index(), second.index()), (0, 1));
}

#[test]
fn only_the_resources_a_kept_pass_reads_keep_their_writers() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let dead = graph.transient(rgba(4)).must();
    let live = graph.transient(rgba(4)).must();
    graph.add_pass("dead", dead, None, &[]).must();
    graph.add_pass("live", live, None, &[]).must();
    graph.add_pass("main", back, None, &[live]).must();
    let compiled = graph.compile();
    assert_eq!(order(&compiled), vec![1, 2]);
    assert_eq!(compiled.culled(), 1);
}

#[test]
fn an_unbound_transient_used_late_stops_recording_before_anything_is_written() {
    let mut graph = RenderGraph::new();
    let back = graph.import_backbuffer().must();
    let late = graph.transient(rgba(4)).must();
    graph.add_pass("first", back, None, &[]).must();
    graph.add_pass("late", late, None, &[]).must();
    graph.add_pass("main", back, None, &[late]).must();
    let mut stream = CommandStream::new();
    let result = graph.compile().record(&[], &mut stream, |_, _| {});
    assert_eq!(result, Err(GraphError::Unbound(late)));
    assert!(stream.is_empty());
}

#[test]
fn the_barrier_capacity_covers_every_change_a_full_graph_can_make() {
    assert_eq!(MAX_BARRIERS, 192);
}
