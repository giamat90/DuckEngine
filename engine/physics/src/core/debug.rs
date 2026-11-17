//! What the debug-draw ports of both dimensions share: the roles a drawn line or point can have,
//! and the length of a drawn contact normal. The ports themselves (`d2::IPhysicsDebugDraw`,
//! `d3::IPhysicsDebugDraw`) differ in the vector type they take.

/// What a drawn line or point stands for. The application picks the colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugColour {
    /// A dynamic body that is awake.
    Awake,
    /// A dynamic body that sleeps.
    Asleep,
    /// A static body.
    Static,
    /// A contact point of a manifold.
    ContactPoint,
    /// The normal of a manifold, drawn from each of its points.
    ContactNormal,
}

/// Length in metres of the line drawn for a contact normal.
pub const NORMAL_LENGTH: f32 = 0.25;
