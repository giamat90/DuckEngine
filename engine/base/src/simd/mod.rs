//! SIMD abstraction: engine code never touches intrinsics.
//!
//! [`scalar`] is the reference and is always built. A per-ISA backend (`sse2` on x86-64, `neon` on
//! `AArch64`) must give the same bits as the reference for every operation; the property tests in
//! each backend compare them. [`F32x4`] is the backend of the build target, or the scalar
//! reference where there is none.

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
pub mod neon;
pub mod scalar;
#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
pub mod sse2;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
pub use neon::F32x4;
#[cfg(not(any(
    all(target_arch = "x86_64", target_feature = "sse2"),
    all(target_arch = "aarch64", target_feature = "neon")
)))]
pub use scalar::F32x4;
#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
pub use sse2::F32x4;
