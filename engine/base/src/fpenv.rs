//! Floating-point control state: read, set and assert the per-thread control register.
//!
//! Simulation code needs the same arithmetic on every thread and machine: round to nearest even,
//! denormals kept (no flush-to-zero, no denormals-are-zero), no trapping exceptions. A driver or
//! an audio library loaded in the process can change the register ([rules digest section 5]), so
//! each simulation job calls [`assert_deterministic`] (or [`set_deterministic`]) before it
//! starts to compute.
//!
//! The register is **per thread**. Changing it on one thread never changes it on another, but a
//! new thread may start with the state of the thread that created it (Linux does), or with the
//! default state (other systems may); do not assume either way. A worker sets or asserts the state
//! at the start of every job.
//!
//! # Compiler model
//!
//! Rust and LLVM assume the default floating-point environment and cannot see the register: they
//! may constant-fold, hoist or move floating-point operations across the calls of this module.
//! The helper guarantees the **hardware state** only. A simulation job must put the call and its
//! floating-point work on opposite sides of an opaque boundary (for example the job entry is a
//! function that is not inlined into the code that calls the helper); the tests here use
//! `std::hint::black_box` for the same reason. Public functions only move the register to the
//! default environment that the compiler assumes ([`set_deterministic`], [`FpGuard::deterministic`])
//! or read it; no public function can write another state. The guard restores the state it found
//! when it was made: if that was not the default, the code around it was already outside the
//! compiler's model.
//!
//! Targets: `x86_64` reads and writes `MXCSR` (`stmxcsr`, `ldmxcsr`) and `aarch64` reads and writes
//! `FPCR` (`mrs`, `msr`), both through inline assembly. The stable `_mm_getcsr` and `_mm_setcsr`
//! intrinsics are deprecated since Rust 1.75 and the workspace denies warnings, so they are not
//! used. On any other target nothing is read or written and every call reports
//! [`FpEnvError::Unsupported`] as a value. Only the control bits are compared and written; the
//! sticky exception flags and reserved bits are left alone.
//!
//! [rules digest section 5]: https://github.com/GiaMat90/DuckEngine/blob/main/wiki/rules-digest.md
#![cfg_attr(
    any(target_arch = "x86_64", target_arch = "aarch64"),
    expect(
        unsafe_code,
        reason = "the control register is read and written with inline assembly; every block has a SAFETY comment"
    )
)]

use core::marker::PhantomData;

/// Layout of `MXCSR` (Intel SDM vol. 1, section 10.2.3). Shared with the fallback below, which
/// repeats it only so that the common code compiles; there nothing is ever read or written.
#[cfg(not(target_arch = "aarch64"))]
mod mxcsr {
    /// Bits that select behaviour: DAZ (6), the six exception masks (7 to 12), rounding (13, 14)
    /// and FTZ (15). Bits 0 to 5 are sticky flags and are not part of the state.
    pub const CONTROL_MASK: u32 = 0xFFC0;
    /// Round to nearest even, no FTZ, no DAZ, all six exceptions masked (the power-on value).
    pub const DETERMINISTIC: u32 = 0x1F80;
    /// Bits 13 and 14 select the rounding mode.
    pub const ROUNDING_MASK: u32 = 0b11 << 13;
    /// Position of the rounding field.
    pub const ROUNDING_SHIFT: u32 = 13;
    /// Rounding field value: to nearest, ties to even.
    #[cfg(test)]
    pub const NEAREST: u32 = 0b00;
    /// Rounding field value: toward negative infinity.
    pub const DOWN: u32 = 0b01;
    /// Rounding field value: toward positive infinity.
    pub const UP: u32 = 0b10;
    /// Rounding field value: toward zero.
    pub const TOWARD_ZERO: u32 = 0b11;
    /// Bit 15: results that are denormal are replaced by zero.
    pub const FLUSH_TO_ZERO: u32 = 1 << 15;
    /// Bit 6: denormal inputs are read as zero.
    pub const DENORMALS_ARE_ZERO: u32 = 1 << 6;

    /// Whether the DAZ bit is set in `raw`.
    pub const fn denormals_are_zero(raw: u32) -> bool {
        raw & DENORMALS_ARE_ZERO != 0
    }
}

/// `MXCSR` through `stmxcsr` and `ldmxcsr`.
#[cfg(target_arch = "x86_64")]
mod target {
    use core::arch::asm;

    pub use super::mxcsr::*;

    /// Whether this target has register access.
    pub const SUPPORTED: bool = true;

    /// The register.
    pub fn read() -> u32 {
        let mut csr: u32 = 0;
        let out = &raw mut csr;
        // SAFETY: `stmxcsr` stores 4 bytes at the address, which is a live local; SSE is part of
        // the x86_64 baseline; the instruction touches no stack and no flags.
        unsafe {
            asm!("stmxcsr [{}]", in(reg) out, options(nostack, preserves_flags));
        };
        csr
    }

    /// Writes the register.
    pub fn write(value: u32) {
        let input = &raw const value;
        // SAFETY: `ldmxcsr` reads 4 bytes from a live local; the value is the last value read with
        // only control bits replaced, so no reserved bit is set (that would fault).
        unsafe {
            asm!("ldmxcsr [{}]", in(reg) input, options(nostack, preserves_flags));
        };
    }
}

/// Layout of `FPCR` (Arm ARM, `AArch64` register description).
#[cfg(target_arch = "aarch64")]
mod target {
    use core::arch::asm;

    /// Bits that select behaviour: AHP (26), DN (25), FZ (24), `RMode` (22, 23), FZ16 (19), the
    /// trap enables (8 to 12, 15), EBF (13) and the optional FIZ (0), AH (1) and NEP (2), which
    /// change denormal and NaN handling. Bits an implementation does not have are written as zero.
    pub const CONTROL_MASK: u32 = 0x07C8_BF07;
    /// Round to nearest even, no flush-to-zero, no default-NaN, no trap enabled (reset value).
    pub const DETERMINISTIC: u32 = 0;
    /// Bits 22 and 23 select the rounding mode.
    pub const ROUNDING_MASK: u32 = 0b11 << 22;
    /// Position of the rounding field.
    pub const ROUNDING_SHIFT: u32 = 22;
    /// Rounding field value: to nearest, ties to even.
    #[cfg(test)]
    pub const NEAREST: u32 = 0b00;
    /// Rounding field value: toward positive infinity.
    pub const UP: u32 = 0b01;
    /// Rounding field value: toward negative infinity.
    pub const DOWN: u32 = 0b10;
    /// Rounding field value: toward zero.
    pub const TOWARD_ZERO: u32 = 0b11;
    /// Bit 24: FZ flushes denormal inputs and results to zero.
    pub const FLUSH_TO_ZERO: u32 = 1 << 24;
    /// `AArch64` has no separate denormals-are-zero bit: FZ covers inputs too.
    #[cfg(test)]
    pub const DENORMALS_ARE_ZERO: u32 = 0;

    /// Always `false`: there is no separate DAZ bit (FZ, FZ16 and FIZ cover inputs).
    pub const fn denormals_are_zero(_raw: u32) -> bool {
        false
    }

    /// Whether this target has register access.
    pub const SUPPORTED: bool = true;

    /// The register (the upper half of `FPCR` is reserved and reads as zero).
    pub fn read() -> u32 {
        let value: u64;
        // SAFETY: `mrs` of FPCR is allowed at EL0, touches no memory, stack or flags.
        unsafe {
            asm!("mrs {}, fpcr", out(reg) value, options(nostack, preserves_flags));
        };
        // The upper half of FPCR is reserved; masking keeps a stray bit from reading as zero state.
        u32::try_from(value & u64::from(u32::MAX)).unwrap_or(u32::MAX)
    }

    /// Writes the register.
    pub fn write(value: u32) {
        let wide = u64::from(value);
        // SAFETY: `msr` to FPCR is allowed at EL0; the value is the last value read with only
        // control bits replaced, so no reserved bit changes.
        unsafe {
            asm!("msr fpcr, {}", in(reg) wide, options(nostack, preserves_flags));
        };
    }
}

/// Any other target: no access, every call reports `Unsupported`.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod target {
    pub use super::mxcsr::*;

    pub const SUPPORTED: bool = false;

    pub fn read() -> u32 {
        0
    }

    pub fn write(_value: u32) {}
}

/// Rounding mode of the floating-point unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// To nearest, ties to even (the IEEE default and the deterministic state).
    Nearest,
    /// Toward negative infinity.
    Down,
    /// Toward positive infinity.
    Up,
    /// Toward zero (truncate).
    TowardZero,
}

impl Rounding {
    #[cfg(test)]
    const fn bits(self) -> u32 {
        match self {
            Self::Nearest => target::NEAREST,
            Self::Down => target::DOWN,
            Self::Up => target::UP,
            Self::TowardZero => target::TOWARD_ZERO,
        }
    }
}

/// The control bits of the floating-point control register of one thread (a plain value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FpEnv {
    raw: u32,
}

impl FpEnv {
    /// The state simulation code requires: round to nearest even, flush-to-zero off,
    /// denormals-are-zero off, no trapping exception.
    pub const DETERMINISTIC: Self = Self {
        raw: target::DETERMINISTIC,
    };

    /// A state from raw register bits, to build states no public function can make.
    #[cfg(all(test, any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self {
            raw: raw & target::CONTROL_MASK,
        }
    }

    /// The control bits in the layout of the register (`MXCSR` or `FPCR`); sticky flags and
    /// reserved bits are zero.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.raw
    }

    /// The rounding mode.
    #[must_use]
    pub const fn rounding(self) -> Rounding {
        match (self.raw & target::ROUNDING_MASK) >> target::ROUNDING_SHIFT {
            target::DOWN => Rounding::Down,
            target::UP => Rounding::Up,
            target::TOWARD_ZERO => Rounding::TowardZero,
            _ => Rounding::Nearest,
        }
    }

    /// Whether results that are denormal are replaced by zero.
    #[must_use]
    pub const fn flush_to_zero(self) -> bool {
        self.raw & target::FLUSH_TO_ZERO != 0
    }

    /// Whether denormal inputs are read as zero (always `false` on `AArch64`, where
    /// flush-to-zero covers inputs).
    #[must_use]
    pub const fn denormals_are_zero(self) -> bool {
        target::denormals_are_zero(self.raw)
    }

    /// A copy with another rounding mode.
    #[cfg(test)]
    #[must_use]
    pub const fn with_rounding(self, rounding: Rounding) -> Self {
        let cleared = self.raw & !target::ROUNDING_MASK;
        Self {
            raw: cleared | (rounding.bits() << target::ROUNDING_SHIFT),
        }
    }

    /// A copy with flush-to-zero on or off.
    #[cfg(test)]
    #[must_use]
    pub const fn with_flush_to_zero(self, on: bool) -> Self {
        Self {
            raw: set_bit(self.raw, target::FLUSH_TO_ZERO, on),
        }
    }

    /// A copy with denormals-are-zero on or off (no effect on `AArch64`).
    #[cfg(test)]
    #[must_use]
    pub const fn with_denormals_are_zero(self, on: bool) -> Self {
        Self {
            raw: set_bit(self.raw, target::DENORMALS_ARE_ZERO, on),
        }
    }

    /// Writes these control bits into the register of the calling thread; the other bits of the
    /// register (sticky flags, reserved) keep their value.
    ///
    /// Only the crate can write an arbitrary state, because any state other than
    /// [`FpEnv::DETERMINISTIC`] is outside the floating-point model the compiler assumes (see the
    /// module comment) and a state that unmasks exceptions makes later arithmetic trap.
    ///
    /// # Errors
    /// [`FpEnvError::Unsupported`] on a target without register access.
    pub(crate) fn apply(self) -> Result<(), FpEnvError> {
        if !target::SUPPORTED {
            return Err(FpEnvError::Unsupported);
        }
        let merged = (target::read() & !target::CONTROL_MASK) | (self.raw & target::CONTROL_MASK);
        target::write(merged);
        Ok(())
    }
}

#[cfg(test)]
const fn set_bit(raw: u32, bit: u32, on: bool) -> u32 {
    if on { raw | bit } else { raw & !bit }
}

/// Why a call on the floating-point control state failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpEnvError {
    /// The target has no register access (neither `x86_64` nor `aarch64`).
    Unsupported,
    /// The state is not the deterministic one.
    Mismatch {
        /// The state that was read.
        actual: FpEnv,
        /// The control bits that differ from [`FpEnv::DETERMINISTIC`], in the register layout.
        differing_bits: u32,
    },
}

/// The control state of the calling thread.
///
/// # Errors
/// [`FpEnvError::Unsupported`] on a target without register access.
pub fn current() -> Result<FpEnv, FpEnvError> {
    if !target::SUPPORTED {
        return Err(FpEnvError::Unsupported);
    }
    Ok(FpEnv {
        raw: target::read() & target::CONTROL_MASK,
    })
}

/// Checks that the calling thread is in the deterministic state. Call it at the start of every
/// simulation job; it changes nothing.
///
/// # Errors
/// [`FpEnvError::Mismatch`] with the differing bits, or [`FpEnvError::Unsupported`].
pub fn assert_deterministic() -> Result<(), FpEnvError> {
    let actual = current()?;
    let differing_bits = actual.raw ^ FpEnv::DETERMINISTIC.raw;
    if differing_bits == 0 {
        Ok(())
    } else {
        Err(FpEnvError::Mismatch {
            actual,
            differing_bits,
        })
    }
}

/// Puts the calling thread in the deterministic state and returns the state it had.
///
/// # Errors
/// [`FpEnvError::Unsupported`] on a target without register access.
pub fn set_deterministic() -> Result<FpEnv, FpEnvError> {
    let previous = current()?;
    FpEnv::DETERMINISTIC.apply()?;
    Ok(previous)
}

/// Restores the control state the calling thread had when the guard was made. The guard is not
/// `Send`: the register belongs to the thread that made it.
///
/// Guards must be dropped in the reverse order of their creation (a nested guard restores the state
/// found at its own creation), and `let _ = FpGuard::deterministic()` drops it at once.
#[must_use = "the previous state is restored when the guard is dropped"]
#[derive(Debug)]
pub struct FpGuard {
    previous: FpEnv,
    not_send: PhantomData<*const ()>,
}

impl FpGuard {
    /// Sets the deterministic state; dropping the guard restores the state found here.
    ///
    /// # Errors
    /// [`FpEnvError::Unsupported`] on a target without register access.
    pub fn deterministic() -> Result<Self, FpEnvError> {
        let previous = set_deterministic()?;
        Ok(Self {
            previous,
            not_send: PhantomData,
        })
    }

    /// The state the guard will restore.
    #[must_use]
    pub const fn previous(&self) -> FpEnv {
        self.previous
    }
}

impl Drop for FpGuard {
    fn drop(&mut self) {
        // A failed write cannot be reported from a destructor; the guard exists only where the
        // register was readable, so the write succeeds.
        let _restored = self.previous.apply();
    }
}

#[cfg(all(test, any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tests;

#[cfg(all(test, not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
mod unsupported_tests;
