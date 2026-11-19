//! The world transform update: one depth-first pass over the hierarchy, for 2D and for 3D.

use crate::error::SceneError;
use crate::hierarchy::{FirstChild, NextSibling, link_of, parent_of};
use crate::transform::{Transform2D, Transform3D, WorldTransform2D, WorldTransform3D};
use core::ops::Mul;
use ecs::{Component, Entity, World};

/// One dimension of the scene: its local transform, the world transform derived from it, and the
/// other dimension's local transform (a tree of the other dimension is not this update's).
trait Space {
    type Local: Component + Copy + Default;
    type World: Component;
    type Other: Component;
    type Matrix: Copy + Mul<Output = Self::Matrix>;

    fn local_matrix(local: &Self::Local) -> Self::Matrix;
    fn matrix_of(world: &Self::World) -> Self::Matrix;
    fn world_from(matrix: Self::Matrix) -> Self::World;
    fn set_matrix(world: &mut Self::World, matrix: Self::Matrix);
}

struct Flat;
struct Solid;

impl Space for Flat {
    type Local = Transform2D;
    type World = WorldTransform2D;
    type Other = Transform3D;
    type Matrix = base::math::Mat3;

    fn local_matrix(local: &Transform2D) -> Self::Matrix {
        local.to_mat3()
    }
    fn matrix_of(world: &WorldTransform2D) -> Self::Matrix {
        world.matrix
    }
    fn world_from(matrix: Self::Matrix) -> WorldTransform2D {
        WorldTransform2D { matrix }
    }
    fn set_matrix(world: &mut WorldTransform2D, matrix: Self::Matrix) {
        world.matrix = matrix;
    }
}

impl Space for Solid {
    type Local = Transform3D;
    type World = WorldTransform3D;
    type Other = Transform2D;
    type Matrix = base::math::Mat4;

    fn local_matrix(local: &Transform3D) -> Self::Matrix {
        local.to_mat4()
    }
    fn matrix_of(world: &WorldTransform3D) -> Self::Matrix {
        world.matrix
    }
    fn world_from(matrix: Self::Matrix) -> WorldTransform3D {
        WorldTransform3D { matrix }
    }
    fn set_matrix(world: &mut WorldTransform3D, matrix: Self::Matrix) {
        world.matrix = matrix;
    }
}

/// What one update did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UpdateReport {
    /// Entities with a [`Transform2D`] whose world transform was written.
    pub updated: usize,
    /// Entities with a [`Transform2D`] that the walk did not reach, so their world transform is
    /// stale: a sibling cut off by a `World::despawn` of an earlier one, or a node whose link was
    /// removed by hand. Zero for a scene edited only through this crate; in a scene broken by hand
    /// it is a lower bound (an entity reached twice hides a missing one).
    pub unreached: usize,
}

/// Recomputes the [`WorldTransform2D`] of every entity that has a [`Transform2D`], parents before
/// children: a root gets its local matrix, a child its parent's world matrix times its own local
/// one. Entities without a live parent are roots (also one without a [`Transform2D`] that has
/// children, which then counts as the identity). A child without a [`Transform2D`] is the identity
/// too and passes its parent's matrix on; it gets a world transform but is not counted. The walk
/// follows the first-child and next-sibling links, so it needs no stack and no search, and a
/// chain of any depth is safe.
///
/// The updater keeps its list of roots between calls, so a call after the first makes no
/// allocator call while the roots do not outgrow it and every entity already has its world
/// transform (the first call inserts it).
#[derive(Debug, Default)]
pub struct TransformUpdater {
    roots: Vec<Entity>,
}

impl TransformUpdater {
    /// An updater with no scratch yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates every world transform.
    ///
    /// # Errors
    /// [`SceneError::BrokenHierarchy`] when a walk meets a loop (a link was removed by hand), and a
    /// storage limit of the world (only the first update of an entity inserts).
    pub fn update(&mut self, world: &mut World) -> Result<UpdateReport, SceneError> {
        run::<Flat>(world, &mut self.roots)
    }
}

/// The same update for 3D: recomputes the [`WorldTransform3D`] of every entity that has a
/// [`Transform3D`], with the same rules as [`TransformUpdater`] (parents first, a missing local
/// transform counts as the identity, no allocation after a warm-up). A world may hold 2D trees
/// and 3D trees: each updater skips the roots of the other dimension, and a tree is expected to
/// be all one dimension: `set_parent` refuses to join the two, and an entity with both transforms is updated by neither (a parent whose transform was removed by hand and that has children of both kinds is walked by both).
#[derive(Debug, Default)]
pub struct TransformUpdater3D {
    roots: Vec<Entity>,
}

impl TransformUpdater3D {
    /// An updater with no scratch yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates every world transform.
    ///
    /// # Errors
    /// As [`TransformUpdater::update`].
    pub fn update(&mut self, world: &mut World) -> Result<UpdateReport, SceneError> {
        run::<Solid>(world, &mut self.roots)
    }
}

fn run<S: Space>(world: &mut World, roots: &mut Vec<Entity>) -> Result<UpdateReport, SceneError> {
    let members = collect_roots::<S>(world, roots);
    let mut updated = 0;
    for root in roots.iter() {
        updated += update_tree::<S>(world, *root)?;
    }
    Ok(UpdateReport {
        updated,
        unreached: members.saturating_sub(updated),
    })
}

/// Fills `roots` with the roots of the world (reusing its capacity) and returns how many entities
/// have a local transform of this dimension.
fn collect_roots<S: Space>(world: &World, roots: &mut Vec<Entity>) -> usize {
    roots.clear();
    roots.extend(world.entities().filter(|e| is_root::<S>(world, *e)));
    world
        .entities()
        .filter(|e| world.has::<S::Local>(*e))
        .count()
}

/// A root has no live parent and either a transform or children, and no transform of the other
/// dimension.
fn is_root<S: Space>(world: &World, entity: Entity) -> bool {
    parent_of(world, entity).is_none()
        && !world.has::<S::Other>(entity)
        && (world.has::<S::Local>(entity) || link_of::<FirstChild>(world, entity).is_some())
}

/// Writes the world transform of `root` and of everything below it, parents first.
fn update_tree<S: Space>(world: &mut World, root: Entity) -> Result<usize, SceneError> {
    let mut node = root;
    let mut updated = 0;
    for _ in 0..=world.len() {
        write_world::<S>(world, node)?;
        updated += usize::from(world.has::<S::Local>(node));
        match next_in_preorder(world, node, root) {
            Some(next) => node = next,
            None => return Ok(updated),
        }
    }
    Err(SceneError::BrokenHierarchy)
}

/// The node after `node` in the walk of the tree under `root`: its first child, else the next
/// sibling of the nearest ancestor-or-self that has one, never leaving `root`.
fn next_in_preorder(world: &World, node: Entity, root: Entity) -> Option<Entity> {
    if let Some(child) = link_of::<FirstChild>(world, node) {
        return Some(child);
    }
    let mut current = node;
    while current != root {
        if let Some(sibling) = link_of::<NextSibling>(world, current) {
            return Some(sibling);
        }
        current = parent_of(world, current)?;
    }
    None
}

fn write_world<S: Space>(world: &mut World, node: Entity) -> Result<(), SceneError> {
    let local = S::local_matrix(
        &world
            .get::<S::Local>(node)
            .map_or_else(S::Local::default, |t| *t),
    );
    let above = parent_of(world, node)
        .and_then(|parent| world.get::<S::World>(parent))
        .map(|w| S::matrix_of(w));
    let matrix = above.map_or(local, |parent_matrix| parent_matrix * local);
    if let Some(existing) = world.get_mut::<S::World>(node) {
        S::set_matrix(existing, matrix);
        return Ok(());
    }
    world.insert(node, S::world_from(matrix))?;
    Ok(())
}
