//! With no sink installed a scope is a no-op: nothing is delivered and nothing fails.
//! This file never installs a sink; the global sink is per process, so it has its own test binary.

use base::profile::{scope, sink_installed};

#[test]
fn a_scope_without_a_sink_does_nothing() {
    assert!(!sink_installed());
    let guard = scope("unset");
    drop(guard);
    base::profile_scope!("unset macro");
    assert!(!sink_installed());
}
