//! The facade and the own functions never touch the heap.

use base::alloc::{CountingAllocator, count_allocations};
use base::dmath::{self, portable as own};
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn no_function_allocates() {
    let (sum, counts) = count_allocations(|| {
        let mut sum = 0.0_f32;
        for i in 0_u16..500 {
            let x = black_box(f32::from(i) * 0.37 - 90.0);
            let (s, c) = dmath::sin_cos(x);
            sum += s
                + c
                + dmath::tan(x)
                + dmath::atan2(x, 1.5)
                + dmath::asin(x * 0.01)
                + dmath::acos(x * 0.01);
            sum += dmath::exp(x * 0.1)
                + dmath::log(x.abs())
                + dmath::pow(x.abs(), 0.3)
                + dmath::powi(x, 3);
            sum += dmath::madd(x, 2.0, 1.0);
            sum += own::sin(x) + own::cos(x) + own::tan(x) + own::exp(x * 0.1) + own::log(x.abs());
            sum += own::atan2(x, -2.0) + own::asin(x * 0.01) + own::acos(x * 0.01);
            sum += own::pow(x.abs(), 0.3) + own::powi(x, -3);
        }
        sum
    });
    black_box(sum);
    assert_eq!(counts.calls(), 0);
}
