//! The phases of one frame and their order.

/// One phase of the frame loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Pump window events and update the input state.
    Input,
    /// Gameplay systems, which read input and write components.
    Gameplay,
    /// Copy forces from the ECS into the physics world (fixed step).
    SyncPhysicsIn,
    /// Step the physics world (fixed step).
    PhysicsStep,
    /// Copy body poses from the physics world into the ECS (fixed step).
    SyncPhysicsOut,
    /// Update the world transforms of the hierarchy.
    SceneUpdate,
    /// Extract the render snapshot.
    SyncRender,
    /// Render.
    Render,
    /// Turn game events into audio calls.
    SyncAudio,
}

impl Phase {
    /// Every phase in the order of a frame, the fixed-step ones once.
    pub const ORDER: [Self; 9] = [
        Self::Input,
        Self::Gameplay,
        Self::SyncPhysicsIn,
        Self::PhysicsStep,
        Self::SyncPhysicsOut,
        Self::SceneUpdate,
        Self::SyncRender,
        Self::Render,
        Self::SyncAudio,
    ];

    /// True for the phases that run once per fixed step, zero or more times a frame.
    #[must_use]
    pub const fn runs_in_fixed_step(self) -> bool {
        matches!(
            self,
            Self::SyncPhysicsIn | Self::PhysicsStep | Self::SyncPhysicsOut
        )
    }

    /// A short name for logs and reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Gameplay => "gameplay",
            Self::SyncPhysicsIn => "sync_physics_in",
            Self::PhysicsStep => "physics_step",
            Self::SyncPhysicsOut => "sync_physics_out",
            Self::SceneUpdate => "scene_update",
            Self::SyncRender => "sync_render",
            Self::Render => "render",
            Self::SyncAudio => "sync_audio",
        }
    }
}

/// How many phases come before the fixed-step block, and after it.
const BEFORE: usize = 2;
const AFTER: usize = 4;
/// How many phases the fixed-step block holds.
const BLOCK: usize = 3;

// Adding or removing a phase must update the three counts above.
const _: () = assert!(Phase::ORDER.len() == BEFORE + BLOCK + AFTER);

/// The phases of one frame, in order, for a given number of fixed steps: the phases before the
/// physics, the physics block once per step, then the phases after it. An iterator with no
/// allocation (not `ExactSizeIterator`: the count of a huge plan does not fit a 32-bit `usize`); the application runs each phase as it comes.
#[derive(Clone, Debug)]
pub struct FramePlan {
    steps: u32,
    next: u64,
}

impl FramePlan {
    /// The plan of a frame that runs `steps` fixed steps.
    #[must_use]
    pub const fn new(steps: u32) -> Self {
        Self { steps, next: 0 }
    }

    const fn total(&self) -> u64 {
        (BEFORE + AFTER) as u64 + BLOCK as u64 * self.steps as u64
    }
}

impl Iterator for FramePlan {
    type Item = Phase;

    fn next(&mut self) -> Option<Phase> {
        if self.next >= self.total() {
            return None;
        }
        let at = self.next;
        self.next += 1;
        let before = BEFORE as u64;
        let block_end = before + BLOCK as u64 * u64::from(self.steps);
        let order = if at < before {
            at
        } else if at < block_end {
            before + (at - before) % BLOCK as u64
        } else {
            before + BLOCK as u64 + (at - block_end)
        };
        Phase::ORDER.get(usize::try_from(order).ok()?).copied()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = usize::try_from(self.total() - self.next.min(self.total())).ok();
        (left.unwrap_or(usize::MAX), left)
    }
}

impl core::iter::FusedIterator for FramePlan {}
