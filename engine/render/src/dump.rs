//! Text dump of a command stream, one command per line: the form of the checked-in golden files.
//!
//! A golden test builds a scene's stream and compares its dump with a file in `golden/`. Handles
//! print as `#index.generation`; floats print with `{:?}` so `1.0` stays `1.0`. A stream that does
//! not decode ends with an `error:` line, so a broken stream is a visible diff, not a panic.

use crate::desc::{IndexType, PassDesc, Rect, RenderTarget};
use crate::stream::{Command, CommandStream, Commands, STREAM_VERSION};
use base::pool::Handle;
use core::fmt::Write;

/// The dump of a stream.
#[must_use]
pub fn dump(stream: &CommandStream) -> String {
    dump_bytes(stream.as_bytes())
}

/// The dump of a raw buffer (replay files, fuzzed input): never panics.
#[must_use]
pub fn dump_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    let version_ok = bytes
        .first_chunk::<4>()
        .is_some_and(|v| u32::from_le_bytes(*v) == STREAM_VERSION);
    if version_ok {
        let _ = writeln!(out, "render-stream v{STREAM_VERSION}");
    }
    for command in Commands::new(bytes) {
        match command {
            Ok(command) => line(&mut out, &command),
            Err(error) => {
                let _ = writeln!(out, "error: {error}");
            }
        }
    }
    out
}

fn handle<T>(h: Handle<T>) -> String {
    format!("#{}.{}", h.index(), h.generation())
}

fn index_type(t: IndexType) -> &'static str {
    match t {
        IndexType::U16 => "u16",
        IndexType::U32 => "u32",
    }
}

fn option<T: core::fmt::Debug>(value: Option<T>) -> String {
    value.map_or_else(|| "none".to_owned(), |v| format!("{v:?}"))
}

fn pass(p: &PassDesc) -> String {
    let target = match p.target {
        RenderTarget::Backbuffer => "backbuffer".to_owned(),
        RenderTarget::Texture(t) => format!("texture({})", handle(t)),
    };
    format!(
        "target={target} clear_color={} clear_depth={}",
        option(p.clear_color),
        option(p.clear_depth)
    )
}

fn viewport(r: &Rect) -> String {
    format!("x={} y={} w={} h={}", r.x, r.y, r.width, r.height)
}

fn line(out: &mut String, command: &Command) {
    let text = match command {
        Command::BeginPass(p) => format!("begin_pass {}", pass(p)),
        Command::EndPass => "end_pass".to_owned(),
        Command::SetPipeline(p) => format!("set_pipeline {}", handle(*p)),
        Command::SetViewport(r) => format!("set_viewport {}", viewport(r)),
        Command::BindUniforms { slot, buffer } => {
            format!("bind_uniforms slot={slot} buffer={}", handle(*buffer))
        }
        Command::BindTexture {
            slot,
            texture,
            sampler,
        } => {
            format!(
                "bind_texture slot={slot} texture={} sampler={}",
                handle(*texture),
                handle(*sampler)
            )
        }
        Command::BindVertexBuffer {
            slot,
            buffer,
            offset,
        } => {
            format!(
                "bind_vertex_buffer slot={slot} buffer={} offset={offset}",
                handle(*buffer)
            )
        }
        Command::BindIndexBuffer {
            buffer,
            offset,
            index_type: t,
        } => {
            format!(
                "bind_index_buffer buffer={} offset={offset} type={}",
                handle(*buffer),
                index_type(*t)
            )
        }
        Command::Draw {
            vertices,
            instances,
        } => format!("draw vertices={vertices} instances={instances}"),
        Command::DrawIndexed { indices, instances } => {
            format!("draw_indexed indices={indices} instances={instances}")
        }
    };
    out.push_str(&text);
    out.push('\n');
}

#[cfg(test)]
mod tests;
