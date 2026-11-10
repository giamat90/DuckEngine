//! Runs a command stream on the open command buffer of a frame: passes with dynamic rendering,
//! and the layout changes the stream asks for or a pass needs.
//!
//! The stream was checked whole by the bookkeeping device before it gets here, so every handle
//! is live and every rule holds. The layout of every image is tracked here (the backbuffer) and in
//! [`Resources`] (the textures), not read from the stream: a pass moves its targets to the
//! attachment layouts itself, so a stream that skips a barrier still draws, as it does on GL.
//!
//! The state commands (pipeline, viewport, binds) and the draws are in [`commands`]. Descriptor
//! sets are written lazily: a bind only notes what is bound, and the first draw after a bind
//! takes a pair of sets from the frame's pool, writes them and binds them.
#![expect(
    unsafe_code,
    reason = "dynamic rendering is a C interface; every unsafe block has a SAFETY comment"
)]

use super::TextureRec;
use super::convert::render_error;
use crate::draw::{Bindings, Draw};
use crate::frame::{Transition, barrier};
use crate::gear::Gear;
use crate::presenter::Backbuffer;
use crate::resources::{Resources, TextureId as OwnTextureId};
use crate::sync::state_access;
use crate::table::Table;
use crate::upload::EVERYTHING;
use crate::vk_draw::Handle;
use crate::vk_present::{
    ACCESS_NONE, ClearColor, CommandBufferHandle, Extent2D, ImageHandle, ImageViewHandle,
    LAYOUT_COLOR_ATTACHMENT, LAYOUT_PRESENT_SRC, LAYOUT_UNDEFINED, LOAD_OP_CLEAR, Offset2D, Rect2D,
    RenderingAttachmentInfo, RenderingInfo, STAGE_ALL_COMMANDS, STORE_OP_STORE,
    STYPE_RENDERING_ATTACHMENT_INFO, STYPE_RENDERING_INFO, SubresourceRange,
};
use crate::vk_resource::{ASPECT_DEPTH, LAYOUT_DEPTH_ATTACHMENT, LAYOUT_SHADER_READ};
use render::desc::{PassDesc, RenderTarget, ResourceState};
use render::error::RenderError;
use render::ids::TextureId;
use render::stream::{Command, CommandStream};

mod commands;
pub use commands::Objects;

const INVALID: RenderError = RenderError::InvalidDescription;

/// `VK_ATTACHMENT_LOAD_OP_LOAD`.
const LOAD_OP_LOAD: u32 = 0;

const DEPTH_RANGE: SubresourceRange = SubresourceRange {
    aspect_mask: ASPECT_DEPTH,
    ..SubresourceRange::COLOR
};

/// What the commands of a pass need to know about its target.
#[derive(Clone, Copy, Default)]
pub struct PassTarget {
    /// The size, for the viewport.
    pub extent: (u32, u32),
    /// `B8G8R8A8` (else `R8G8B8A8`): picks the pipeline variant.
    pub bgra: bool,
    /// The backbuffer: its viewport is flipped (see [`commands`]).
    pub flip: bool,
}

/// What the frame knows about its backbuffer while commands are recorded.
pub struct FrameState {
    pub back: Backbuffer,
    /// The layout the backbuffer image is in at this point of the recording.
    pub layout: u32,
    /// A rendering scope is open (a pass began and has not ended).
    pub in_pass: bool,
    /// The target of the open pass.
    pub target: PassTarget,
    /// What the stream bound so far, and whether the sets must be made again before a draw.
    pub bindings: Bindings,
    pub dirty: bool,
}

impl FrameState {
    /// A frame whose image was just acquired.
    #[must_use]
    pub fn new(back: Backbuffer) -> Self {
        Self {
            back,
            layout: LAYOUT_UNDEFINED,
            in_pass: false,
            target: PassTarget::default(),
            bindings: Bindings::default(),
            dirty: true,
        }
    }
}

/// Where the layout of an attachment is kept.
#[derive(Clone, Copy)]
enum Slot {
    Back,
    Texture(OwnTextureId),
}

/// An image a pass or a barrier names.
#[derive(Clone, Copy)]
struct Attachment {
    slot: Slot,
    image: ImageHandle,
    view: ImageViewHandle,
    width: u32,
    height: u32,
    bgra: bool,
    range: SubresourceRange,
}

/// One run of a stream on a frame's command buffer.
pub struct Exec<'a> {
    pub gear: &'a Gear,
    pub cmd: CommandBufferHandle,
    pub textures: &'a Table<TextureRec>,
    pub resources: &'a mut Resources,
    pub state: &'a mut FrameState,
    pub draw: &'a Draw,
    /// The descriptor pool of the frame's slot.
    pub pool: Handle,
    pub objects: Objects<'a>,
}

impl Exec<'_> {
    /// Runs every command of `stream` in order, stopping at the first that fails. A rendering
    /// scope left open by a failure is closed, so the command buffer can still be ended.
    ///
    /// # Errors
    /// `InvalidDescription` for an undecodable command, `InvalidHandle` for a missing record, and
    /// the port error of a failed driver call.
    pub fn run(&mut self, stream: &CommandStream) -> Result<(), RenderError> {
        let result = stream
            .commands()
            .try_for_each(|command| self.execute(command.map_err(|_| INVALID)?));
        if result.is_err() {
            self.close_pass();
        }
        result
    }

    fn execute(&mut self, command: Command) -> Result<(), RenderError> {
        match command {
            Command::BeginPass(pass) => self.begin_pass(&pass),
            Command::EndPass => {
                self.close_pass();
                Ok(())
            }
            Command::Barrier { target, to, .. } => self.barrier(target, to),
            other => self.state_or_draw(other),
        }
    }

    fn close_pass(&mut self) {
        if self.state.in_pass {
            // SAFETY: the buffer is recording and a rendering scope is open.
            unsafe { (self.gear.fns.cmd_end_rendering)(self.cmd) };
            self.state.in_pass = false;
        }
    }

    fn attachment_of(&self, target: RenderTarget) -> Result<Attachment, RenderError> {
        match target {
            RenderTarget::Backbuffer => Ok(Attachment {
                slot: Slot::Back,
                image: self.state.back.image,
                view: self.state.back.view,
                width: self.state.back.extent.width,
                height: self.state.back.extent.height,
                bgra: true,
                range: SubresourceRange::COLOR,
            }),
            RenderTarget::Texture(id) => self.texture_attachment(id),
        }
    }

    fn texture_attachment(&self, id: TextureId) -> Result<Attachment, RenderError> {
        let record = self.textures.get(id).ok_or(RenderError::InvalidHandle)?;
        let handles = self
            .resources
            .texture_handles(record.id)
            .map_err(|error| render_error(&error))?;
        Ok(Attachment {
            slot: Slot::Texture(record.id),
            image: handles.image,
            view: handles.view,
            width: record.width,
            height: record.height,
            bgra: record.bgra,
            range: if record.depth {
                DEPTH_RANGE
            } else {
                SubresourceRange::COLOR
            },
        })
    }

    fn layout_of(&self, attachment: &Attachment) -> Result<u32, RenderError> {
        match attachment.slot {
            Slot::Back => Ok(self.state.layout),
            Slot::Texture(id) => self
                .resources
                .texture_handles(id)
                .map(|handles| handles.layout)
                .map_err(|error| render_error(&error)),
        }
    }

    fn set_layout(&mut self, attachment: &Attachment, layout: u32) -> Result<(), RenderError> {
        match attachment.slot {
            Slot::Back => {
                self.state.layout = layout;
                Ok(())
            }
            Slot::Texture(id) => self
                .resources
                .note_layout(id, layout)
                .map_err(|error| render_error(&error)),
        }
    }

    /// Moves the image to the layout of `to`, from whatever layout it is in. The barrier is
    /// recorded even when the layout stays, because two passes that write the same attachment
    /// need the order. The source side waits for everything before it: the layouts do not say
    /// which stage touched the image last, and a precise source is an optimisation for later.
    fn move_to(&mut self, attachment: &Attachment, to: ResourceState) -> Result<(), RenderError> {
        let want = state_access(to);
        if want.layout == LAYOUT_UNDEFINED {
            // Nothing to move to: the contents are given up, the layout stays as it is.
            return Ok(());
        }
        if want.layout == LAYOUT_PRESENT_SRC && !matches!(attachment.slot, Slot::Back) {
            return Err(INVALID);
        }
        // Neither the swapchain images nor the depth images are created for sampling.
        if want.layout == LAYOUT_SHADER_READ
            && (matches!(attachment.slot, Slot::Back)
                || attachment.range.aspect_mask == ASPECT_DEPTH)
        {
            return Err(RenderError::Unsupported);
        }
        let old = self.layout_of(attachment)?;
        let src = if old == LAYOUT_UNDEFINED {
            (STAGE_ALL_COMMANDS, ACCESS_NONE)
        } else {
            EVERYTHING
        };
        barrier(
            self.gear,
            self.cmd,
            attachment.image,
            &Transition {
                range: attachment.range,
                old_layout: old,
                new_layout: want.layout,
                src,
                dst: (want.stage, want.access),
            },
        );
        self.set_layout(attachment, want.layout)
    }

    fn barrier(&mut self, target: RenderTarget, to: ResourceState) -> Result<(), RenderError> {
        let attachment = self.attachment_of(target)?;
        self.move_to(&attachment, to)
    }

    fn begin_pass(&mut self, pass: &PassDesc) -> Result<(), RenderError> {
        let color = self.attachment_of(pass.target)?;
        if color.range.aspect_mask == ASPECT_DEPTH {
            // The bookkeeping device lets a depth texture be a target when nothing is cleared.
            return Err(RenderError::Unsupported);
        }
        self.move_to(&color, ResourceState::ColorTarget)?;
        let depth = match pass.depth {
            Some(id) => Some(self.depth_for(&color, id)?),
            None => None,
        };
        let rendering = Rendering {
            view: color.view,
            extent: Extent2D {
                width: color.width,
                height: color.height,
            },
            clear: pass.clear_color,
            depth: depth.map(|depth| (depth.view, pass.clear_depth)),
        };
        begin_rendering(self.gear, self.cmd, &rendering);
        self.state.in_pass = true;
        self.state.target = PassTarget {
            extent: (color.width, color.height),
            bgra: color.bgra,
            flip: matches!(color.slot, Slot::Back),
        };
        self.start_pass_state();
        Ok(())
    }

    /// The depth attachment of a pass: it must cover the colour attachment.
    fn depth_for(&mut self, color: &Attachment, id: TextureId) -> Result<Attachment, RenderError> {
        let depth = self.texture_attachment(id)?;
        if depth.width < color.width || depth.height < color.height {
            return Err(INVALID);
        }
        self.move_to(&depth, ResourceState::DepthTarget)?;
        Ok(depth)
    }
}

/// Moves the backbuffer to the present layout, from wherever the frame left it.
pub fn present_layout(gear: &Gear, cmd: CommandBufferHandle, state: &mut FrameState) {
    let want = state_access(ResourceState::Present);
    let src = if state.layout == LAYOUT_UNDEFINED {
        (STAGE_ALL_COMMANDS, ACCESS_NONE)
    } else {
        EVERYTHING
    };
    barrier(
        gear,
        cmd,
        state.back.image,
        &Transition {
            range: SubresourceRange::COLOR,
            old_layout: state.layout,
            new_layout: want.layout,
            src,
            dst: (want.stage, want.access),
        },
    );
    state.layout = want.layout;
}

/// What a rendering scope draws into.
struct Rendering {
    view: ImageViewHandle,
    extent: Extent2D,
    clear: Option<[f32; 4]>,
    /// The depth view and the value to clear it to, if it is cleared.
    depth: Option<(ImageViewHandle, Option<f32>)>,
}

fn attachment_info(
    view: ImageViewHandle,
    layout: u32,
    clear: Option<[f32; 4]>,
) -> RenderingAttachmentInfo {
    RenderingAttachmentInfo {
        s_type: STYPE_RENDERING_ATTACHMENT_INFO,
        p_next: core::ptr::null(),
        image_view: view,
        image_layout: layout,
        resolve_mode: 0,
        resolve_image_view: 0,
        resolve_image_layout: 0,
        load_op: if clear.is_some() {
            LOAD_OP_CLEAR
        } else {
            LOAD_OP_LOAD
        },
        store_op: STORE_OP_STORE,
        clear_value: ClearColor {
            float32: clear.unwrap_or([0.0; 4]),
        },
    }
}

fn begin_rendering(gear: &Gear, cmd: CommandBufferHandle, rendering: &Rendering) {
    let color = attachment_info(rendering.view, LAYOUT_COLOR_ATTACHMENT, rendering.clear);
    // A depth clear value is the first float of the union (the stencil word after it stays 0).
    let depth = rendering.depth.map(|(view, clear)| {
        attachment_info(
            view,
            LAYOUT_DEPTH_ATTACHMENT,
            clear.map(|value| [value, 0.0, 0.0, 0.0]),
        )
    });
    let info = RenderingInfo {
        s_type: STYPE_RENDERING_INFO,
        p_next: core::ptr::null(),
        flags: 0,
        render_area: Rect2D {
            offset: Offset2D { x: 0, y: 0 },
            extent: rendering.extent,
        },
        layer_count: 1,
        view_mask: 0,
        color_attachment_count: 1,
        color_attachments: &raw const color,
        depth_attachment: depth
            .as_ref()
            .map_or(core::ptr::null(), core::ptr::from_ref),
        stencil_attachment: core::ptr::null(),
    };
    // SAFETY: the buffer is recording; `info` and the attachments it points to live until the
    // call returns; the views belong to live images that were moved to the attachment layouts
    // just before.
    unsafe { (gear.fns.cmd_begin_rendering)(cmd, &raw const info) };
}
