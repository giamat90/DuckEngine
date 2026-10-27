//! Minimal application crate (layer 4). It is the only place that names the global allocator
//! (ADR-0022): engine crates never do. Build with `--no-default-features` for the default one.

use std::thread;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Name of the allocator this binary was built with. It follows the Cargo feature; it does not probe
/// the running allocator (that would need `unsafe` calls into the C library, which this crate forbids).
/// The tests below therefore prove that both feature configurations build and run, not who serves them.
fn allocator_name() -> &'static str {
    if cfg!(feature = "mimalloc") {
        "mimalloc"
    } else {
        "system"
    }
}

/// `threads` threads each allocate and free `count` boxes; returns the number of boxes handled.
fn churn_threads(threads: usize, count: usize) -> usize {
    let workers: Vec<_> = (0..threads)
        .map(|_| thread::spawn(move || churn(count)))
        .collect();
    workers.into_iter().map(|w| w.join().unwrap_or(0)).sum()
}

fn churn(count: usize) -> usize {
    let boxes: Vec<Box<usize>> = (0..count).map(Box::new).collect();
    boxes.len()
}

fn main() {
    let handled = churn_threads(2, 1000);
    println!(
        "app_runtime: {} allocator, {handled} boxes handled",
        allocator_name()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "mimalloc")]
    #[test]
    fn the_default_build_names_mimalloc() {
        assert_eq!(allocator_name(), "mimalloc");
    }

    #[cfg(not(feature = "mimalloc"))]
    #[test]
    fn the_build_without_the_feature_names_the_system_allocator() {
        assert_eq!(allocator_name(), "system");
    }

    #[test]
    fn threads_allocate_and_free_through_the_global_allocator() {
        assert_eq!(churn_threads(4, 1000), 4000);
        assert_eq!(churn_threads(1, 0), 0);
    }
}
