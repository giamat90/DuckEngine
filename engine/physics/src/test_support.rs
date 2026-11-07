//! Helpers for the unit tests: the lints deny `unwrap` and `expect`, so a test states its
//! expectation with `must` and fails with the message.

pub(crate) trait Must<T> {
    fn must(self, message: &str) -> T;
}

impl<T> Must<T> for Option<T> {
    fn must(self, message: &str) -> T {
        match self {
            Some(value) => value,
            None => panic!("expected a value: {message}"),
        }
    }
}

impl<T, E: std::fmt::Debug> Must<T> for Result<T, E> {
    fn must(self, message: &str) -> T {
        match self {
            Ok(value) => value,
            Err(error) => panic!("{message}: {error:?}"),
        }
    }
}
