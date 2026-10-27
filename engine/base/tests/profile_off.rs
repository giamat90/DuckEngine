//! With the `profiling` feature off the macro expands to nothing, so its argument is never evaluated.

#![cfg(not(feature = "profiling"))]

use std::cell::Cell;

#[test]
fn the_macro_does_not_evaluate_its_argument_when_the_feature_is_off() {
    let evaluated = Cell::new(false);
    base::profile_scope!({
        evaluated.set(true);
        "name"
    });
    assert!(!evaluated.get());
}
