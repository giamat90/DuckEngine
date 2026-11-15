//! The parent-child hierarchy as components: each child has a [`Parent`], each parent a
//! [`FirstChild`], and each child but the last a [`NextSibling`].
//!
//! The link components have no public constructor and are neither `Clone` nor `Copy`, so a link
//! only comes from [`set_parent`], which refuses a cycle. As long as nobody removes a link by
//! hand, the links form a forest of ordered trees. Two things break that, and the walks are
//! written to survive both:
//!
//! - A node despawned with `World::despawn` (use [`despawn_subtree`]) leaves its links dangling.
//!   A dangling link reads as absent: a dead parent makes a root; a dead first child or next
//!   sibling ends the list there, so the siblings after it are no longer listed by their parent
//!   and the update cannot reach them (it counts them as unreached).
//! - A link removed with `World::remove` or `Commands::remove` can let a later `set_parent` close
//!   a loop. Every walk is bounded by the number of entities in the world and reports
//!   [`SceneError::BrokenHierarchy`] instead of looping (`children` just ends).
//!
//! Appending a child and taking one out walk the list of its siblings, so both are linear in
//! their number.

use crate::error::SceneError;
use crate::transform::Transform2D;
use ecs::{Component, EcsError, Entity, World};

/// The parent of an entity.
#[derive(Debug, PartialEq, Eq)]
pub struct Parent(Entity);

/// The first child of an entity.
#[derive(Debug, PartialEq, Eq)]
pub struct FirstChild(Entity);

/// The next child of the same parent.
#[derive(Debug, PartialEq, Eq)]
pub struct NextSibling(Entity);

/// A link component: it names one entity.
pub(crate) trait Link: Component {
    fn to(entity: Entity) -> Self;
    fn target(&self) -> Entity;
}

macro_rules! link {
    ($name:ident) => {
        impl Link for $name {
            fn to(entity: Entity) -> Self {
                Self(entity)
            }
            fn target(&self) -> Entity {
                self.0
            }
        }
    };
}

link!(Parent);
link!(FirstChild);
link!(NextSibling);

/// The live entity that link `T` of `entity` names.
pub(crate) fn link_of<T: Link>(world: &World, entity: Entity) -> Option<Entity> {
    world
        .get::<T>(entity)
        .map(Link::target)
        .filter(|target| world.is_alive(*target))
}

/// Points link `T` of `entity` at `target`, or removes it for `None`.
fn set_link<T: Link>(
    world: &mut World,
    entity: Entity,
    target: Option<Entity>,
) -> Result<(), SceneError> {
    match target {
        Some(target) => world.insert(entity, T::to(target))?,
        None if world.has::<T>(entity) => world.remove::<T>(entity)?,
        None => {}
    }
    Ok(())
}

/// The children of an entity, first to last.
#[derive(Clone)]
pub struct Children<'w> {
    world: &'w World,
    next: Option<Entity>,
    remaining: usize,
}

impl Iterator for Children<'_> {
    type Item = Entity;

    fn next(&mut self) -> Option<Entity> {
        let current = self.next?;
        self.remaining = self.remaining.checked_sub(1)?;
        self.next = link_of::<NextSibling>(self.world, current);
        Some(current)
    }
}

/// The parent of `entity`, if it has a live one.
#[must_use]
pub fn parent_of(world: &World, entity: Entity) -> Option<Entity> {
    link_of::<Parent>(world, entity)
}

/// The children of `entity` in order (none for a leaf or a stale entity).
#[must_use]
pub fn children(world: &World, entity: Entity) -> Children<'_> {
    Children {
        world,
        next: link_of::<FirstChild>(world, entity),
        remaining: world.len(),
    }
}

/// Makes `child` the last child of `parent`, taking it from its old parent (also when that is
/// `parent`, which moves it to the end).
///
/// # Errors
/// Refuses a despawned entity, an entity without a [`Transform2D`], and a parent that is the
/// child or one of its descendants; the world is unchanged then. After those checks only a
/// storage limit of the world can fail the call.
pub fn set_parent(world: &mut World, child: Entity, parent: Entity) -> Result<(), SceneError> {
    check_member(world, child)?;
    check_member(world, parent)?;
    if is_self_or_ancestor(world, child, parent) {
        return Err(SceneError::Cycle);
    }
    unlink(world, child)?;
    match children(world, parent).last() {
        Some(last) => set_link::<NextSibling>(world, last, Some(child))?,
        None => set_link::<FirstChild>(world, parent, Some(child))?,
    }
    set_link::<Parent>(world, child, Some(parent))
}

/// Takes `child` from its parent; it becomes a root. A root stays as it is.
///
/// # Errors
/// Refuses a despawned entity.
pub fn detach(world: &mut World, child: Entity) -> Result<(), SceneError> {
    check_alive(world, child)?;
    unlink(world, child)
}

/// Despawns `root` and all its descendants, children before parents, and takes `root` from its
/// parent. Returns how many entities were despawned.
///
/// # Errors
/// Refuses a despawned entity, and a hierarchy with a loop ([`SceneError::BrokenHierarchy`] when
/// the loop is on the first-child path, before anything is despawned; a loop found later can stop
/// the call after some descendants are already despawned, with that error or a stale entity).
pub fn despawn_subtree(world: &mut World, root: Entity) -> Result<usize, SceneError> {
    check_alive(world, root)?;
    unlink(world, root)?;
    let mut node = deepest_first_child(world, root)?;
    let mut removed = 0;
    loop {
        let next = next_after(world, node, root)?;
        world.despawn(node)?;
        removed += 1;
        match next {
            Some(following) => node = following,
            None => return Ok(removed),
        }
    }
}

fn check_alive(world: &World, entity: Entity) -> Result<(), SceneError> {
    if world.is_alive(entity) {
        Ok(())
    } else {
        Err(SceneError::Ecs(EcsError::StaleEntity))
    }
}

fn check_member(world: &World, entity: Entity) -> Result<(), SceneError> {
    check_alive(world, entity)?;
    if world.has::<Transform2D>(entity) {
        Ok(())
    } else {
        Err(SceneError::MissingTransform)
    }
}

/// Whether `ancestor` is `node` or one of the entities above it.
fn is_self_or_ancestor(world: &World, ancestor: Entity, node: Entity) -> bool {
    let mut current = Some(node);
    while let Some(entity) = current {
        if entity == ancestor {
            return true;
        }
        current = parent_of(world, entity);
    }
    false
}

/// Takes `child` from its parent's list and clears its own links.
fn unlink(world: &mut World, child: Entity) -> Result<(), SceneError> {
    if let Some(parent) = parent_of(world, child) {
        let next = link_of::<NextSibling>(world, child);
        if link_of::<FirstChild>(world, parent) == Some(child) {
            set_link::<FirstChild>(world, parent, next)?;
        } else if let Some(before) =
            children(world, parent).find(|s| link_of::<NextSibling>(world, *s) == Some(child))
        {
            set_link::<NextSibling>(world, before, next)?;
        }
    }
    set_link::<Parent>(world, child, None)?;
    set_link::<NextSibling>(world, child, None)
}

/// The first leaf below `entity` on the first-child path (the entity itself for a leaf).
fn deepest_first_child(world: &World, entity: Entity) -> Result<Entity, SceneError> {
    let mut current = entity;
    for _ in 0..=world.len() {
        match link_of::<FirstChild>(world, current) {
            Some(child) => current = child,
            None => return Ok(current),
        }
    }
    Err(SceneError::BrokenHierarchy)
}

/// The node after `node` when the subtree of `root` is walked children first.
fn next_after(world: &World, node: Entity, root: Entity) -> Result<Option<Entity>, SceneError> {
    if node == root {
        return Ok(None);
    }
    match link_of::<NextSibling>(world, node) {
        Some(sibling) => deepest_first_child(world, sibling).map(Some),
        None => Ok(parent_of(world, node)),
    }
}
