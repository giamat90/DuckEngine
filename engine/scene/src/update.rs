//! The world transform update: one depth-first pass over the hierarchy.

use crate::error::SceneError;
use crate::hierarchy::{FirstChild, NextSibling, link_of, parent_of};
use crate::transform::{Transform2D, WorldTransform2D};
use ecs::{Entity, World};

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
        let members = collect_roots(world, &mut self.roots);
        let mut updated = 0;
        for root in &self.roots {
            updated += update_tree(world, *root)?;
        }
        Ok(UpdateReport {
            updated,
            unreached: members.saturating_sub(updated),
        })
    }
}

/// Fills `roots` with the roots of the world (reusing its capacity) and returns how many entities
/// have a [`Transform2D`].
fn collect_roots(world: &World, roots: &mut Vec<Entity>) -> usize {
    roots.clear();
    roots.extend(world.entities().filter(|e| is_root(world, *e)));
    world
        .entities()
        .filter(|e| world.has::<Transform2D>(*e))
        .count()
}

/// A root has no live parent and either a transform or children.
fn is_root(world: &World, entity: Entity) -> bool {
    parent_of(world, entity).is_none()
        && (world.has::<Transform2D>(entity) || link_of::<FirstChild>(world, entity).is_some())
}

/// Writes the world transform of `root` and of everything below it, parents first.
fn update_tree(world: &mut World, root: Entity) -> Result<usize, SceneError> {
    let mut node = root;
    let mut updated = 0;
    for _ in 0..=world.len() {
        write_world(world, node)?;
        updated += usize::from(world.has::<Transform2D>(node));
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

fn write_world(world: &mut World, node: Entity) -> Result<(), SceneError> {
    let local = world
        .get::<Transform2D>(node)
        .map_or(Transform2D::IDENTITY, |t| *t)
        .to_mat3();
    let above = parent_of(world, node)
        .and_then(|parent| world.get::<WorldTransform2D>(parent))
        .map(|w| w.matrix);
    let matrix = above.map_or(local, |parent_matrix| parent_matrix * local);
    if let Some(existing) = world.get_mut::<WorldTransform2D>(node) {
        existing.matrix = matrix;
        return Ok(());
    }
    world.insert(node, WorldTransform2D { matrix })?;
    Ok(())
}
