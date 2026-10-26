//! SIMD abstraction: engine code never touches intrinsics.
//!
//! [`scalar`] is the reference and is always built. A per-ISA backend (today
//! `sse2` on x86-64) must give the same bits as the reference for every
//! operation; the property tests in each backend compare them. [`F32x4`] is the
//! backend of the build target, or the scalar reference where there is none.

pub mod scalar;
#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
pub mod sse2;

#[cfg(not(all(target_arch = "x86_64", target_feature = "sse2")))]
pub use scalar::F32x4;
#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
pub use sse2::F32x4;
