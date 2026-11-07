//! The contact solver through the public 2D API (card 4-13): a body comes to rest on the ground,
//! friction and restitution agree with closed forms, a collision conserves momentum, stacks and a
//! pyramid settle, a fast body does not tunnel through a thin wall within the speculative margin, and
//! a step that would overflow changes nothing.

mod basics;
mod collisions;
mod common;
mod robustness;
mod stacking;
