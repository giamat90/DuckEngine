//! Tests of the fallback on targets without register access.

use super::*;

#[test]
fn every_call_reports_unsupported_as_a_value() {
    assert_eq!(Err(FpEnvError::Unsupported), current());
    assert_eq!(Err(FpEnvError::Unsupported), assert_deterministic());
    assert_eq!(Err(FpEnvError::Unsupported), set_deterministic());
    assert_eq!(Err(FpEnvError::Unsupported), FpEnv::DETERMINISTIC.apply());
    assert!(FpGuard::deterministic().is_err());
}
