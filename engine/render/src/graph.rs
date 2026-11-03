//! Render graph v0 (design: `wiki/architecture/render-port-v0.md` section 4).
//!
//! A frame is declared as passes that write one colour target and read other resources. Compiling
//! is a pure function of the declaration: it drops passes whose output nothing needs, keeps the
//! rest in declaration order, derives the barriers (state changes between passes) and the lifetime
//! of each transient resource. Recording writes barriers, pass begin/end and the caller's draw
//! commands into a [`CommandStream`]; the graph never talks to a device.
//!
//! Version 0 keeps the declaration order (the order is the author's, validated, not rearranged) and
//! reserves memory aliasing of transients: the lifetimes are computed so a later version can alias.
//! Everything is fixed-size and heap-free, so declaring, compiling and recording allocate nothing.
//! The price is stack size: a compiled graph is about 10 KB and is passed by value.

use std::fmt;

use crate::desc::{PassDesc, RenderTarget, ResourceState, TextureDesc, TextureFormat};
use crate::ids::TextureId;
use crate::stream::CommandStream;

/// Most passes in one graph.
pub const MAX_PASSES: usize = 32;
/// Most resources in one graph.
pub const MAX_RESOURCES: usize = 32;
/// Most resources one pass reads.
pub const MAX_READS: usize = 4;
/// Capacity of the barrier list: each pass changes its reads and its target, each resource ends once.
pub const MAX_BARRIERS: usize = MAX_PASSES * (MAX_READS + 1) + MAX_RESOURCES;

/// A resource of a graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceId(usize);

impl ResourceId {
    /// Position in declaration order.
    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

/// A pass of a graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassId(usize);

impl PassId {
    /// Position in declaration order.
    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

/// What can go wrong declaring or recording a graph. Errors are values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphError {
    /// More than [`MAX_PASSES`] passes.
    TooManyPasses,
    /// More than [`MAX_RESOURCES`] resources.
    TooManyResources,
    /// A pass reads more than [`MAX_READS`] resources.
    TooManyReads,
    /// A resource id does not belong to this graph.
    UnknownResource,
    /// A transient resource is read before any pass writes it.
    ReadBeforeWrite,
    /// A pass reads the resource it writes.
    SameResourceReadAndWritten,
    /// The backbuffer cannot be sampled.
    BackbufferRead,
    /// The target is not a colour texture.
    TargetNotRenderable,
    /// A transient resource used by a kept pass has no texture bound at record time.
    Unbound(ResourceId),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyPasses => write!(f, "too many passes (at most {MAX_PASSES})"),
            Self::TooManyResources => write!(f, "too many resources (at most {MAX_RESOURCES})"),
            Self::TooManyReads => write!(f, "too many reads in one pass (at most {MAX_READS})"),
            Self::UnknownResource => write!(f, "unknown resource"),
            Self::ReadBeforeWrite => write!(f, "transient resource read before it is written"),
            Self::SameResourceReadAndWritten => write!(f, "a pass reads the resource it writes"),
            Self::BackbufferRead => write!(f, "the backbuffer cannot be read"),
            Self::TargetNotRenderable => write!(f, "the target is not a colour texture"),
            Self::Unbound(id) => write!(f, "transient resource {} has no texture bound", id.0),
        }
    }
}

impl std::error::Error for GraphError {}

/// A fixed-capacity list of `Copy` items: no heap, `push` reports a full list.
#[derive(Clone, Copy)]
struct Fixed<T: Copy, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Copy, const N: usize> Fixed<T, N> {
    fn new(fill: T) -> Self {
        Self {
            items: [fill; N],
            len: 0,
        }
    }

    fn push(&mut self, item: T) -> bool {
        match self.items.get_mut(self.len) {
            Some(slot) => {
                *slot = item;
                self.len += 1;
                true
            }
            None => false,
        }
    }

    fn as_slice(&self) -> &[T] {
        self.items.get(..self.len).unwrap_or(&[])
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Backbuffer,
    Texture(TextureId),
    Transient(TextureDesc),
}

#[derive(Clone, Copy)]
struct Resource {
    kind: Kind,
    initial: ResourceState,
    last: ResourceState,
}

impl Resource {
    fn is_transient(&self) -> bool {
        matches!(self.kind, Kind::Transient(_))
    }
}

/// One declared pass.
#[derive(Clone, Copy)]
pub struct Pass {
    /// Position in declaration order.
    pub id: PassId,
    /// Name, for dumps and tools.
    pub label: &'static str,
    /// The colour target.
    pub target: ResourceId,
    /// Clear colour; `None` keeps the contents.
    pub clear_color: Option<[f32; 4]>,
    reads: Fixed<ResourceId, MAX_READS>,
}

impl Pass {
    /// Resources the pass samples.
    #[must_use]
    pub fn reads(&self) -> &[ResourceId] {
        self.reads.as_slice()
    }
}

/// A state change of a resource, to happen before the pass at position `before` (the position
/// after the last pass is the end of the frame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Barrier {
    /// Position in the compiled order.
    pub before: usize,
    /// The resource.
    pub resource: ResourceId,
    /// State before.
    pub from: ResourceState,
    /// State after.
    pub to: ResourceState,
}

/// The declaration of a frame.
#[derive(Clone, Copy)]
pub struct RenderGraph {
    resources: Fixed<Resource, MAX_RESOURCES>,
    passes: Fixed<Pass, MAX_PASSES>,
    /// Bit per resource: some earlier pass writes it.
    written: u32,
}

impl Default for RenderGraph {
    fn default() -> Self {
        Self::new()
    }
}

const FILL_RESOURCE: Resource = Resource {
    kind: Kind::Backbuffer,
    initial: ResourceState::Undefined,
    last: ResourceState::Undefined,
};

/// Placeholder that fills unused slots of the fixed arrays.
const FILL_PASS: Pass = Pass {
    id: PassId(0),
    label: "",
    target: ResourceId(0),
    clear_color: None,
    reads: Fixed {
        items: [ResourceId(0); MAX_READS],
        len: 0,
    },
};

fn bit(resource: ResourceId) -> u32 {
    1u32 << resource.0
}

impl RenderGraph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            resources: Fixed::new(FILL_RESOURCE),
            passes: Fixed::new(FILL_PASS),
            written: 0,
        }
    }

    fn add_resource(&mut self, resource: Resource) -> Result<ResourceId, GraphError> {
        let id = ResourceId(self.resources.len);
        if self.resources.push(resource) {
            Ok(id)
        } else {
            Err(GraphError::TooManyResources)
        }
    }

    /// Declares the window's backbuffer: undefined at the start of the frame, presented at its end.
    ///
    /// # Errors
    /// [`GraphError::TooManyResources`] when the graph is full.
    pub fn import_backbuffer(&mut self) -> Result<ResourceId, GraphError> {
        self.add_resource(Resource {
            kind: Kind::Backbuffer,
            initial: ResourceState::Undefined,
            last: ResourceState::Present,
        })
    }

    /// Declares a texture the caller owns, in `initial` state at the start of the frame and in
    /// `last` state at its end. A pass writing it is never culled.
    ///
    /// # Errors
    /// [`GraphError::TooManyResources`] when the graph is full.
    pub fn import_texture(
        &mut self,
        texture: TextureId,
        initial: ResourceState,
        last: ResourceState,
    ) -> Result<ResourceId, GraphError> {
        self.add_resource(Resource {
            kind: Kind::Texture(texture),
            initial,
            last,
        })
    }

    /// Declares a texture that lives only inside the frame; the caller binds a real texture to it
    /// when recording.
    ///
    /// # Errors
    /// [`GraphError::TooManyResources`] when the graph is full.
    pub fn transient(&mut self, desc: TextureDesc) -> Result<ResourceId, GraphError> {
        self.add_resource(Resource {
            kind: Kind::Transient(desc),
            initial: ResourceState::Undefined,
            last: ResourceState::Undefined,
        })
    }

    fn resource(&self, id: ResourceId) -> Result<&Resource, GraphError> {
        self.resources
            .as_slice()
            .get(id.0)
            .ok_or(GraphError::UnknownResource)
    }

    fn check_target(&self, target: ResourceId) -> Result<(), GraphError> {
        match self.resource(target)?.kind {
            Kind::Transient(desc) if desc.format == TextureFormat::Depth32F => {
                Err(GraphError::TargetNotRenderable)
            }
            _ => Ok(()),
        }
    }

    fn check_read(&self, target: ResourceId, read: ResourceId) -> Result<(), GraphError> {
        let resource = self.resource(read)?;
        if matches!(resource.kind, Kind::Backbuffer) {
            return Err(GraphError::BackbufferRead);
        }
        if read == target {
            return Err(GraphError::SameResourceReadAndWritten);
        }
        if resource.is_transient() && self.written & bit(read) == 0 {
            return Err(GraphError::ReadBeforeWrite);
        }
        Ok(())
    }

    /// Declares a pass writing `target` and sampling `reads`. A refused declaration changes nothing.
    ///
    /// # Errors
    /// Any [`GraphError`] about the declaration; see the variants.
    pub fn add_pass(
        &mut self,
        label: &'static str,
        target: ResourceId,
        clear_color: Option<[f32; 4]>,
        reads: &[ResourceId],
    ) -> Result<PassId, GraphError> {
        if reads.len() > MAX_READS {
            return Err(GraphError::TooManyReads);
        }
        self.check_target(target)?;
        let mut list = Fixed::new(target);
        for &read in reads {
            self.check_read(target, read)?;
            list.push(read);
        }
        let id = PassId(self.passes.len);
        let pass = Pass {
            id,
            label,
            target,
            clear_color,
            reads: list,
        };
        if !self.passes.push(pass) {
            return Err(GraphError::TooManyPasses);
        }
        self.written |= bit(target);
        Ok(id)
    }

    /// Bit per pass that some kept consumer needs: a pass is kept when it writes an imported
    /// resource or a resource a later kept pass reads.
    fn live_passes(&self) -> u32 {
        let mut needed = 0u32;
        let mut live = 0u32;
        for (index, pass) in self.passes.as_slice().iter().enumerate().rev() {
            live |= self.keep(index, pass, &mut needed);
        }
        live
    }

    /// The pass's bit when it is kept; a kept pass makes what it reads needed.
    fn keep(&self, index: usize, pass: &Pass, needed: &mut u32) -> u32 {
        if !self.is_needed(pass.target, *needed) {
            return 0;
        }
        *needed = pass.reads().iter().fold(*needed, |mask, &r| mask | bit(r));
        1u32 << index
    }

    fn is_needed(&self, target: ResourceId, needed: u32) -> bool {
        let transient = self.resource(target).is_ok_and(Resource::is_transient);
        !transient || needed & bit(target) != 0
    }

    /// Drops dead passes, derives barriers and lifetimes. Pure: the same graph compiles to the
    /// same result.
    #[must_use]
    pub fn compile(&self) -> CompiledGraph {
        let live = self.live_passes();
        let mut builder = Builder::new(self);
        for (index, pass) in self.passes.as_slice().iter().enumerate() {
            builder.visit(live & (1u32 << index) != 0, *pass);
        }
        builder.finish();
        builder.out
    }
}

/// The result of compiling a [`RenderGraph`].
#[derive(Clone, Copy)]
pub struct CompiledGraph {
    resources: Fixed<Resource, MAX_RESOURCES>,
    order: Fixed<Pass, MAX_PASSES>,
    barriers: Fixed<Barrier, MAX_BARRIERS>,
    lifetimes: [Option<(usize, usize)>; MAX_RESOURCES],
    culled: usize,
}

impl CompiledGraph {
    /// The kept passes, in execution order.
    #[must_use]
    pub fn order(&self) -> &[Pass] {
        self.order.as_slice()
    }

    /// All barriers, in execution order.
    #[must_use]
    pub fn barriers(&self) -> &[Barrier] {
        self.barriers.as_slice()
    }

    /// How many passes were dropped.
    #[must_use]
    pub fn culled(&self) -> usize {
        self.culled
    }

    /// First and last position (in [`Self::order`]) that uses the resource; `None` when no kept
    /// pass touches it. A later version may alias transients whose lifetimes do not overlap.
    #[must_use]
    pub fn lifetime(&self, resource: ResourceId) -> Option<(usize, usize)> {
        self.lifetimes.get(resource.0).copied().flatten()
    }

    fn render_target(
        &self,
        bound: &[(ResourceId, TextureId)],
        id: ResourceId,
    ) -> Result<RenderTarget, GraphError> {
        let resource = self
            .resources
            .as_slice()
            .get(id.0)
            .ok_or(GraphError::UnknownResource)?;
        match resource.kind {
            Kind::Backbuffer => Ok(RenderTarget::Backbuffer),
            Kind::Texture(texture) => Ok(RenderTarget::Texture(texture)),
            Kind::Transient(_) => bound
                .iter()
                .find(|(resource, _)| *resource == id)
                .map(|(_, texture)| RenderTarget::Texture(*texture))
                .ok_or(GraphError::Unbound(id)),
        }
    }

    /// Every resource a kept pass touches must resolve to a target.
    fn check_bound(&self, bound: &[(ResourceId, TextureId)]) -> Result<(), GraphError> {
        let used = self.lifetimes.iter().enumerate();
        used.filter(|(_, life)| life.is_some())
            .try_for_each(|(index, _)| self.render_target(bound, ResourceId(index)).map(drop))
    }

    fn emit_barriers(
        &self,
        bound: &[(ResourceId, TextureId)],
        position: usize,
        stream: &mut CommandStream,
    ) -> Result<(), GraphError> {
        for barrier in self.barriers().iter().filter(|b| b.before == position) {
            let target = self.render_target(bound, barrier.resource)?;
            stream.barrier(target, barrier.from, barrier.to);
        }
        Ok(())
    }

    /// Writes the frame into `stream`: for each kept pass its barriers, `begin_pass`, whatever
    /// `body` records for it, `end_pass`; then the closing barriers. `bound` gives the texture
    /// behind each transient resource. Nothing is written when a needed transient is unbound.
    ///
    /// # Errors
    /// [`GraphError::Unbound`] naming the first transient without a texture.
    pub fn record(
        &self,
        bound: &[(ResourceId, TextureId)],
        stream: &mut CommandStream,
        mut body: impl FnMut(PassId, &mut CommandStream),
    ) -> Result<(), GraphError> {
        self.check_bound(bound)?;
        for (position, pass) in self.order().iter().enumerate() {
            self.emit_barriers(bound, position, stream)?;
            stream.begin_pass(PassDesc {
                target: self.render_target(bound, pass.target)?,
                clear_color: pass.clear_color,
                clear_depth: None,
            });
            body(pass.id, stream);
            stream.end_pass();
        }
        self.emit_barriers(bound, self.order().len(), stream)
    }
}

/// Walks the kept passes in order, tracking each resource's state to derive barriers.
struct Builder {
    out: CompiledGraph,
    state: [ResourceState; MAX_RESOURCES],
}

impl Builder {
    fn new(graph: &RenderGraph) -> Self {
        let mut state = [ResourceState::Undefined; MAX_RESOURCES];
        for (slot, resource) in state.iter_mut().zip(graph.resources.as_slice()) {
            *slot = resource.initial;
        }
        Self {
            out: CompiledGraph {
                resources: graph.resources,
                order: Fixed::new(FILL_PASS),
                barriers: Fixed::new(Barrier {
                    before: 0,
                    resource: ResourceId(0),
                    from: ResourceState::Undefined,
                    to: ResourceState::Undefined,
                }),
                lifetimes: [None; MAX_RESOURCES],
                culled: 0,
            },
            state,
        }
    }

    fn visit(&mut self, kept: bool, pass: Pass) {
        if kept {
            self.schedule(pass);
        } else {
            self.out.culled += 1;
        }
    }

    fn schedule(&mut self, pass: Pass) {
        let position = self.out.order.len;
        self.out.order.push(pass);
        for &read in pass.reads() {
            self.require(position, read, ResourceState::ShaderRead);
        }
        self.require(position, pass.target, ResourceState::ColorTarget);
    }

    /// Makes `resource` be in state `to` before position `position`, with a barrier when it is not.
    fn require(&mut self, position: usize, resource: ResourceId, to: ResourceState) {
        let Some(current) = self.state.get_mut(resource.0) else {
            return;
        };
        let from = *current;
        *current = to;
        if let Some(life) = self.out.lifetimes.get_mut(resource.0) {
            let first = life.map_or(position, |(first, _)| first);
            *life = Some((first, position));
        }
        if from != to {
            self.out.barriers.push(Barrier {
                before: position,
                resource,
                from,
                to,
            });
        }
    }

    /// Closing barriers: every imported resource a kept pass touched ends in its declared state.
    fn finish(&mut self) {
        let end = self.out.order.len;
        for index in 0..self.out.resources.len {
            self.close_imported(end, ResourceId(index));
        }
    }

    fn close_imported(&mut self, end: usize, resource: ResourceId) {
        let touched = self.out.lifetime(resource).is_some();
        let imported = self.out.resources.as_slice().get(resource.0);
        if let Some(r) = imported.filter(|r| touched && !r.is_transient()) {
            self.close(end, resource, r.last);
        }
    }

    fn close(&mut self, end: usize, resource: ResourceId, last: ResourceState) {
        let Some(current) = self.state.get_mut(resource.0) else {
            return;
        };
        let from = *current;
        *current = last;
        if from != last {
            self.out.barriers.push(Barrier {
                before: end,
                resource,
                from,
                to: last,
            });
        }
    }
}

#[cfg(test)]
mod tests;
