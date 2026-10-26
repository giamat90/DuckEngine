//! Dimension-specific vector types: [`Vec2`], [`Vec3`], [`Vec4`].
//!
//! Each is plain `repr(C)` data of [`Scalar`] components in field order, so `x` comes first in
//! memory. The alignment is that of one scalar, not of a SIMD register: a SIMD path converts
//! with `to_array` and `from_array`.
//!
//! The order of every floating-point operation is fixed and there is no fused multiply-add, so
//! the results are the same on every run and a SIMD path can reproduce them bit for bit:
//! `dot` adds the products left to right, `lerp` is `a * (1 - t) + b * t`, `length` is the
//! correctly rounded square root of the squared length. `min` and `max` pick `a < b ? a : b`
//! and `a > b ? a : b`, so on a tie or a NaN they return the second operand (the same rule as
//! the x86 instructions and as [`crate::simd`]). Simulation code must not produce NaN.

use super::Scalar;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// The operations every dimension shares. `$n` is the component count, the identifiers are the
/// fields in order.
macro_rules! vector_common {
    ($name:ident, $n:literal, $first:ident $(, $rest:ident)*) => {
        impl $name {
            /// Every component `0.0`.
            pub const ZERO: Self = Self::splat(0.0);
            /// Every component `1.0`.
            pub const ONE: Self = Self::splat(1.0);

            /// All components set to `value`.
            #[inline]
            #[must_use]
            pub const fn splat(value: Scalar) -> Self {
                Self { $first: value $(, $rest: value)* }
            }

            /// The components in field order.
            #[inline]
            #[must_use]
            pub const fn from_array(components: [Scalar; $n]) -> Self {
                let [$first $(, $rest)*] = components;
                Self { $first $(, $rest)* }
            }

            /// The components in field order.
            #[inline]
            #[must_use]
            pub const fn to_array(self) -> [Scalar; $n] {
                [self.$first $(, self.$rest)*]
            }

            #[inline]
            fn map(self, f: impl Fn(Scalar) -> Scalar) -> Self {
                Self { $first: f(self.$first) $(, $rest: f(self.$rest))* }
            }

            #[inline]
            fn zip(self, other: Self, f: impl Fn(Scalar, Scalar) -> Scalar) -> Self {
                Self { $first: f(self.$first, other.$first) $(, $rest: f(self.$rest, other.$rest))* }
            }

            /// Sum of the component products, added left to right.
            #[inline]
            #[must_use]
            pub fn dot(self, other: Self) -> Scalar {
                self.$first * other.$first $(+ self.$rest * other.$rest)*
            }

            /// `self.dot(self)`.
            #[inline]
            #[must_use]
            pub fn length_squared(self) -> Scalar {
                self.dot(self)
            }

            /// Euclidean length: the correctly rounded square root of the squared length.
            #[inline]
            #[must_use]
            pub fn length(self) -> Scalar {
                self.length_squared().sqrt()
            }

            /// Length of `self - other`.
            #[inline]
            #[must_use]
            pub fn distance(self, other: Self) -> Scalar {
                (self - other).length()
            }

            /// `self` divided by its length, or `None` when that length is zero, NaN or
            /// infinite. The length comes from the squared length, so the useful range is
            /// components of about `1e-19` to `1e19` in magnitude: a larger vector overflows
            /// the square and a much smaller one underflows it to zero (both give `None`),
            /// and in between the square is subnormal and the result is less accurate.
            #[inline]
            #[must_use]
            pub fn normalized(self) -> Option<Self> {
                let length = self.length();
                if length.is_finite() && length > 0.0 {
                    Some(self / length)
                } else {
                    None
                }
            }

            /// Per component `a < b ? a : b`: returns `other`'s component on a tie or a NaN.
            #[inline]
            #[must_use]
            pub fn min(self, other: Self) -> Self {
                self.zip(other, |a, b| if a < b { a } else { b })
            }

            /// Per component `a > b ? a : b`: returns `other`'s component on a tie or a NaN.
            #[inline]
            #[must_use]
            pub fn max(self, other: Self) -> Self {
                self.zip(other, |a, b| if a > b { a } else { b })
            }

            /// Clears the sign bit of every component.
            #[inline]
            #[must_use]
            pub fn abs(self) -> Self {
                self.map(Scalar::abs)
            }

            /// `self * (1 - t) + other * t`. For finite operands it gives `self` at `t = 0` and
            /// `other` at `t = 1` bit for bit, except that a zero may come out as `+0.0`. A
            /// `t` outside `0..=1` extrapolates.
            #[inline]
            #[must_use]
            pub fn lerp(self, other: Self, t: Scalar) -> Self {
                self * (1.0 - t) + other * t
            }
        }

        impl Default for $name {
            #[inline]
            fn default() -> Self {
                Self::ZERO
            }
        }

        impl Add for $name {
            type Output = Self;
            #[inline]
            fn add(self, rhs: Self) -> Self {
                self.zip(rhs, |a, b| a + b)
            }
        }

        impl Sub for $name {
            type Output = Self;
            #[inline]
            fn sub(self, rhs: Self) -> Self {
                self.zip(rhs, |a, b| a - b)
            }
        }

        impl Neg for $name {
            type Output = Self;
            #[inline]
            fn neg(self) -> Self {
                self.map(|a| -a)
            }
        }

        impl Mul<Scalar> for $name {
            type Output = Self;
            #[inline]
            fn mul(self, rhs: Scalar) -> Self {
                self.map(|a| a * rhs)
            }
        }

        impl Mul<$name> for Scalar {
            type Output = $name;
            #[inline]
            fn mul(self, rhs: $name) -> $name {
                rhs * self
            }
        }

        impl Div<Scalar> for $name {
            type Output = Self;
            #[inline]
            fn div(self, rhs: Scalar) -> Self {
                self.map(|a| a / rhs)
            }
        }

        impl AddAssign for $name {
            #[inline]
            fn add_assign(&mut self, rhs: Self) {
                *self = *self + rhs;
            }
        }

        impl SubAssign for $name {
            #[inline]
            fn sub_assign(&mut self, rhs: Self) {
                *self = *self - rhs;
            }
        }

        impl MulAssign<Scalar> for $name {
            #[inline]
            fn mul_assign(&mut self, rhs: Scalar) {
                *self = *self * rhs;
            }
        }

        impl DivAssign<Scalar> for $name {
            #[inline]
            fn div_assign(&mut self, rhs: Scalar) {
                *self = *self / rhs;
            }
        }
    };
}

/// Two-component vector.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vec2 {
    /// First component.
    pub x: Scalar,
    /// Second component.
    pub y: Scalar,
}

impl Vec2 {
    /// The unit vector along +X.
    pub const X: Self = Self::new(1.0, 0.0);
    /// The unit vector along +Y.
    pub const Y: Self = Self::new(0.0, 1.0);

    /// From the components in field order.
    #[inline]
    #[must_use]
    pub const fn new(x: Scalar, y: Scalar) -> Self {
        Self { x, y }
    }

    /// Rotated a quarter turn counter-clockwise: `(x, y)` becomes `(-y, x)`.
    #[inline]
    #[must_use]
    pub fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// The scalar cross product `x1 * y2 - y1 * x2`: the signed area of the parallelogram,
    /// positive when `other` is counter-clockwise from `self`.
    #[inline]
    #[must_use]
    pub fn cross(self, other: Self) -> Scalar {
        self.x * other.y - self.y * other.x
    }

    /// This vector with a `z` component.
    #[inline]
    #[must_use]
    pub const fn extend(self, z: Scalar) -> Vec3 {
        Vec3::new(self.x, self.y, z)
    }
}

vector_common!(Vec2, 2, x, y);

/// Three-component vector.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vec3 {
    /// First component.
    pub x: Scalar,
    /// Second component.
    pub y: Scalar,
    /// Third component.
    pub z: Scalar,
}

impl Vec3 {
    /// The unit vector along +X.
    pub const X: Self = Self::new(1.0, 0.0, 0.0);
    /// The unit vector along +Y (up).
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);
    /// The unit vector along +Z.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// From the components in field order.
    #[inline]
    #[must_use]
    pub const fn new(x: Scalar, y: Scalar, z: Scalar) -> Self {
        Self { x, y, z }
    }

    /// Right-handed cross product: `X.cross(Y)` is `Z`. Each component is two products and one
    /// subtraction.
    #[inline]
    #[must_use]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// This vector with a `w` component.
    #[inline]
    #[must_use]
    pub const fn extend(self, w: Scalar) -> Vec4 {
        Vec4::new(self.x, self.y, self.z, w)
    }

    /// This vector without its `z` component.
    #[inline]
    #[must_use]
    pub const fn truncate(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

vector_common!(Vec3, 3, x, y, z);

/// Four-component vector.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vec4 {
    /// First component.
    pub x: Scalar,
    /// Second component.
    pub y: Scalar,
    /// Third component.
    pub z: Scalar,
    /// Fourth component.
    pub w: Scalar,
}

impl Vec4 {
    /// The unit vector along +X.
    pub const X: Self = Self::new(1.0, 0.0, 0.0, 0.0);
    /// The unit vector along +Y.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0, 0.0);
    /// The unit vector along +Z.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0, 0.0);
    /// The unit vector along +W.
    pub const W: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// From the components in field order.
    #[inline]
    #[must_use]
    pub const fn new(x: Scalar, y: Scalar, z: Scalar, w: Scalar) -> Self {
        Self { x, y, z, w }
    }

    /// This vector without its `w` component.
    #[inline]
    #[must_use]
    pub const fn truncate(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

vector_common!(Vec4, 4, x, y, z, w);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;

    /// Random cases per property test; Miri is slow, so it gets fewer.
    const CASES: usize = if cfg!(miri) { 40 } else { 2_000 };
    /// 2^24: the first `f32` where adding 1.0 is lost (ties round to even), which makes the
    /// order of a sum visible in the result.
    const TWO_POW_24: Scalar = 16_777_216.0;
    /// Random components are drawn from `[-RANGE, RANGE)`.
    const RANGE: Scalar = 8.0;
    /// Slack for "unit length" after a normalisation (a few ulps of 1.0).
    const UNIT_TOLERANCE: Scalar = 1.0e-6;

    fn bits(values: &[Scalar]) -> Vec<u32> {
        values.iter().map(|value| value.to_bits()).collect()
    }

    fn assert_same(actual: &[Scalar], expected: &[Scalar]) {
        assert_eq!(bits(actual), bits(expected), "{actual:?} != {expected:?}");
    }

    /// `first, first + 1, first + 2, ...`: every component differs, all exactly representable.
    fn counting<const N: usize>(first: Scalar) -> [Scalar; N] {
        let mut next = first;
        core::array::from_fn(|_| {
            let value = next;
            next += 1.0;
            value
        })
    }

    fn random<const N: usize>(rng: &mut Pcg32) -> [Scalar; N] {
        core::array::from_fn(|_| rng.next_f32() * (2.0 * RANGE) - RANGE)
    }

    /// The same checks for every dimension; `$n` is the number of components.
    macro_rules! suite {
        ($module:ident, $ty:ident, $n:literal) => {
            mod $module {
                use super::*;

                const N: usize = $n;

                fn v(components: [Scalar; N]) -> $ty {
                    $ty::from_array(components)
                }

                #[test]
                fn zero_has_all_bits_clear() {
                    assert_same(&$ty::ZERO.to_array(), &[0.0; N]);
                }

                #[test]
                fn one_is_all_ones() {
                    assert_same(&$ty::ONE.to_array(), &[1.0; N]);
                }

                #[test]
                fn splat_fills_every_component() {
                    assert_same(&$ty::splat(2.5).to_array(), &[2.5; N]);
                }

                #[test]
                fn array_conversion_round_trips() {
                    let components = counting::<N>(1.0);
                    assert_same(&v(components).to_array(), &components);
                }

                #[test]
                fn default_is_zero() {
                    assert_same(&$ty::default().to_array(), &[0.0; N]);
                }

                #[test]
                fn add_is_componentwise() {
                    let (a, b) = (counting::<N>(1.0), counting::<N>(10.0));
                    let expected: [Scalar; N] = core::array::from_fn(|i| a[i] + b[i]);
                    assert_same(&(v(a) + v(b)).to_array(), &expected);
                }

                #[test]
                fn sub_is_componentwise() {
                    let (a, b) = (counting::<N>(1.0), counting::<N>(10.0));
                    let expected: [Scalar; N] = core::array::from_fn(|i| a[i] - b[i]);
                    assert_same(&(v(a) - v(b)).to_array(), &expected);
                }

                #[test]
                fn neg_flips_every_sign_including_zero() {
                    let a = counting::<N>(1.0);
                    let expected: [Scalar; N] = core::array::from_fn(|i| -a[i]);
                    assert_same(&(-v(a)).to_array(), &expected);
                    assert_same(&(-$ty::ZERO).to_array(), &[-0.0; N]);
                }

                #[test]
                fn scalar_product_scales_every_component() {
                    let a = counting::<N>(1.0);
                    let expected: [Scalar; N] = core::array::from_fn(|i| a[i] * 3.0);
                    assert_same(&(v(a) * 3.0).to_array(), &expected);
                    assert_same(&(3.0 * v(a)).to_array(), &expected);
                }

                #[test]
                fn scalar_quotient_divides_every_component() {
                    let a = counting::<N>(1.0);
                    let expected: [Scalar; N] = core::array::from_fn(|i| a[i] / 4.0);
                    assert_same(&(v(a) / 4.0).to_array(), &expected);
                }

                #[test]
                fn division_by_zero_follows_ieee() {
                    let quotient = (v(counting::<N>(1.0)) / 0.0).to_array();
                    assert_same(&quotient, &[Scalar::INFINITY; N]);
                }

                #[test]
                fn assign_operators_match_the_binary_ones() {
                    let (a, b) = (counting::<N>(1.0), counting::<N>(10.0));
                    let mut sum = v(a);
                    sum += v(b);
                    assert_same(&sum.to_array(), &(v(a) + v(b)).to_array());
                    let mut difference = v(a);
                    difference -= v(b);
                    assert_same(&difference.to_array(), &(v(a) - v(b)).to_array());
                    let mut scaled = v(a);
                    scaled *= 3.0;
                    assert_same(&scaled.to_array(), &(v(a) * 3.0).to_array());
                    let mut divided = v(a);
                    divided /= 4.0;
                    assert_same(&divided.to_array(), &(v(a) / 4.0).to_array());
                }

                #[test]
                fn dot_of_small_integers_is_exact() {
                    let a = counting::<N>(1.0);
                    let expected: Scalar = a.iter().map(|c| c * c).sum();
                    assert_eq!(v(a).dot(v(a)).to_bits(), expected.to_bits());
                    assert_eq!(v(a).length_squared().to_bits(), expected.to_bits());
                }

                #[test]
                fn length_of_a_3_4_5_triangle_is_exact() {
                    let mut components = [0.0; N];
                    components[0] = 3.0;
                    components[1] = 4.0;
                    assert_eq!(v(components).length().to_bits(), 5.0_f32.to_bits());
                    assert_eq!(
                        v(components).distance($ty::ZERO).to_bits(),
                        5.0_f32.to_bits()
                    );
                }

                #[test]
                fn distance_is_the_length_of_the_difference() {
                    let (a, b) = (counting::<N>(1.0), counting::<N>(4.0));
                    assert_eq!(
                        v(a).distance(v(b)).to_bits(),
                        (v(a) - v(b)).length().to_bits()
                    );
                }

                #[test]
                fn normalized_divides_by_the_length() {
                    let mut components = [0.0; N];
                    components[0] = 3.0;
                    components[1] = 4.0;
                    let unit = v(components).normalized().map(|u| u.to_array());
                    let mut expected = [0.0; N];
                    expected[0] = 3.0 / 5.0;
                    expected[1] = 4.0 / 5.0;
                    assert_eq!(unit.map(|u| bits(&u)), Some(bits(&expected)));
                }

                #[test]
                fn normalized_refuses_zero_nan_and_infinity() {
                    assert_eq!($ty::ZERO.normalized(), None);
                    assert_eq!($ty::splat(Scalar::NAN).normalized(), None);
                    assert_eq!($ty::splat(Scalar::INFINITY).normalized(), None);
                }

                #[test]
                fn normalized_refuses_a_vector_whose_square_underflows() {
                    assert_eq!($ty::splat(1.0e-30).normalized(), None);
                }

                #[test]
                fn min_and_max_pick_per_component_and_return_other_on_ties() {
                    let a = counting::<N>(1.0);
                    let b: [Scalar; N] =
                        core::array::from_fn(|i| if i % 2 == 0 { 0.5 } else { 100.0 });
                    let lower: [Scalar; N] =
                        core::array::from_fn(|i| if a[i] < b[i] { a[i] } else { b[i] });
                    let upper: [Scalar; N] =
                        core::array::from_fn(|i| if a[i] > b[i] { a[i] } else { b[i] });
                    assert_same(&v(a).min(v(b)).to_array(), &lower);
                    assert_same(&v(a).max(v(b)).to_array(), &upper);
                    assert_same(
                        &$ty::splat(0.0).min($ty::splat(-0.0)).to_array(),
                        &[-0.0; N],
                    );
                    assert_same(&$ty::splat(-0.0).max($ty::splat(0.0)).to_array(), &[0.0; N]);
                }

                #[test]
                fn abs_clears_the_sign_of_every_component() {
                    let a = counting::<N>(-2.0);
                    let expected: [Scalar; N] = core::array::from_fn(|i| a[i].abs());
                    assert_same(&v(a).abs().to_array(), &expected);
                    assert_same(&$ty::splat(-0.0).abs().to_array(), &[0.0; N]);
                }

                #[test]
                fn lerp_hits_both_ends_exactly_and_the_middle() {
                    let (a, b) = (counting::<N>(1.0), counting::<N>(10.0));
                    assert_same(&v(a).lerp(v(b), 0.0).to_array(), &a);
                    assert_same(&v(a).lerp(v(b), 1.0).to_array(), &b);
                    let middle: [Scalar; N] = core::array::from_fn(|i| (a[i] + b[i]) / 2.0);
                    assert_same(&v(a).lerp(v(b), 0.5).to_array(), &middle);
                }

                #[test]
                fn lerp_scales_the_start_by_one_minus_t_first() {
                    // 0.75 * 16 + 0.25 * 8 = 14; the two products and the sum are exact.
                    let from = $ty::splat(16.0);
                    let to = $ty::splat(8.0);
                    assert_same(&from.lerp(to, 0.25).to_array(), &[14.0; N]);
                }

                #[test]
                fn operations_are_commutative_or_antisymmetric_where_ieee_says_so() {
                    let mut rng = Pcg32::new(7, N as u64);
                    for _ in 0..CASES {
                        let (a, b) = (v(random::<N>(&mut rng)), v(random::<N>(&mut rng)));
                        assert_same(&(a + b).to_array(), &(b + a).to_array());
                        assert_same(&(a - b).to_array(), &(-(b - a)).to_array());
                        assert_eq!(a.dot(b).to_bits(), b.dot(a).to_bits());
                        assert_same(&a.min(a).to_array(), &a.to_array());
                        assert_same(&a.max(a).to_array(), &a.to_array());
                    }
                }

                #[test]
                fn length_is_the_square_root_of_the_squared_length() {
                    let mut rng = Pcg32::new(11, N as u64);
                    for _ in 0..CASES {
                        let a = v(random::<N>(&mut rng));
                        assert_eq!(a.length().to_bits(), a.length_squared().sqrt().to_bits());
                    }
                }

                #[test]
                fn normalized_vectors_have_unit_length_and_keep_their_direction() {
                    let mut rng = Pcg32::new(13, N as u64);
                    for _ in 0..CASES {
                        let components = random::<N>(&mut rng);
                        let Some(unit) = v(components).normalized() else {
                            continue;
                        };
                        assert!((unit.length() - 1.0).abs() <= UNIT_TOLERANCE, "{unit:?}");
                        let same_direction = unit
                            .to_array()
                            .iter()
                            .zip(components)
                            .all(|(u, c)| u.is_sign_negative() == c.is_sign_negative());
                        assert!(same_direction, "{unit:?} vs {components:?}");
                    }
                }

                #[test]
                fn division_is_a_true_division_not_a_reciprocal_product() {
                    let mut rng = Pcg32::new(19, N as u64);
                    for divisor in [3.0, 5.0, 7.0] {
                        for _ in 0..CASES {
                            let a = random::<N>(&mut rng);
                            let expected: [Scalar; N] = core::array::from_fn(|i| a[i] / divisor);
                            assert_same(&(v(a) / divisor).to_array(), &expected);
                        }
                    }
                }

                #[test]
                fn lerp_is_start_times_one_minus_t_plus_end_times_t() {
                    let mut rng = Pcg32::new(23, N as u64);
                    for _ in 0..CASES {
                        let (a, b) = (random::<N>(&mut rng), random::<N>(&mut rng));
                        let t = rng.next_f32();
                        let expected: [Scalar; N] =
                            core::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t);
                        assert_same(&v(a).lerp(v(b), t).to_array(), &expected);
                    }
                }

                #[test]
                fn normalized_matches_a_division_by_the_sequential_length() {
                    let mut rng = Pcg32::new(29, N as u64);
                    for _ in 0..CASES {
                        let a = random::<N>(&mut rng);
                        let squared = a.iter().skip(1).fold(a[0] * a[0], |sum, c| sum + c * c);
                        let length = squared.sqrt();
                        let expected: [Scalar; N] = core::array::from_fn(|i| a[i] / length);
                        let actual = v(a).normalized().map(|u| bits(&u.to_array()));
                        assert_eq!(actual, Some(bits(&expected)), "{a:?}");
                    }
                }

                #[test]
                fn normalized_refuses_a_vector_whose_square_overflows() {
                    assert_eq!($ty::splat(1.0e20).normalized(), None);
                    assert!($ty::splat(1.0e10).normalized().is_some());
                }

                #[test]
                fn min_and_max_return_the_second_operand_when_a_lane_is_nan() {
                    let nan = $ty::splat(Scalar::NAN);
                    let one = $ty::splat(1.0);
                    assert_same(&nan.min(one).to_array(), &[1.0; N]);
                    assert_same(&nan.max(one).to_array(), &[1.0; N]);
                    assert!(one.min(nan).to_array().iter().all(|c| c.is_nan()));
                    assert!(one.max(nan).to_array().iter().all(|c| c.is_nan()));
                }

                #[test]
                fn size_and_alignment_are_those_of_n_scalars() {
                    assert_eq!(
                        core::mem::size_of::<$ty>(),
                        N * core::mem::size_of::<Scalar>()
                    );
                    assert_eq!(
                        core::mem::align_of::<$ty>(),
                        core::mem::align_of::<Scalar>()
                    );
                }
            }
        };
    }

    suite!(vec2, Vec2, 2);
    suite!(vec3, Vec3, 3);
    suite!(vec4, Vec4, 4);

    mod vec2_only {
        use super::*;

        #[test]
        fn new_and_axes_place_the_components() {
            assert_same(&Vec2::new(1.0, 2.0).to_array(), &[1.0, 2.0]);
            assert_same(&Vec2::X.to_array(), &[1.0, 0.0]);
            assert_same(&Vec2::Y.to_array(), &[0.0, 1.0]);
        }

        #[test]
        fn fields_are_in_memory_order() {
            assert_eq!(core::mem::offset_of!(Vec2, x), 0);
            assert_eq!(core::mem::offset_of!(Vec2, y), 4);
        }

        #[test]
        fn perp_rotates_a_quarter_turn_counter_clockwise() {
            assert_same(&Vec2::X.perp().to_array(), &[-0.0, 1.0]);
            assert_same(&Vec2::Y.perp().to_array(), &[-1.0, 0.0]);
            assert_same(&Vec2::new(3.0, 4.0).perp().to_array(), &[-4.0, 3.0]);
        }

        #[test]
        fn cross_is_the_signed_area_of_the_parallelogram() {
            assert_eq!(Vec2::X.cross(Vec2::Y).to_bits(), 1.0_f32.to_bits());
            assert_eq!(Vec2::Y.cross(Vec2::X).to_bits(), (-1.0_f32).to_bits());
            assert_eq!(
                Vec2::new(2.0, 3.0).cross(Vec2::new(5.0, 7.0)).to_bits(),
                (-1.0_f32).to_bits()
            );
        }

        #[test]
        fn extend_appends_a_component() {
            assert_same(
                &Vec2::new(1.0, 2.0).extend(3.0).to_array(),
                &[1.0, 2.0, 3.0],
            );
        }
    }

    #[test]
    fn vec3_cross_with_itself_is_zero_and_swapping_negates() {
        let mut rng = Pcg32::new(17, 3);
        for _ in 0..CASES {
            let (a, b) = (
                Vec3::from_array(random(&mut rng)),
                Vec3::from_array(random(&mut rng)),
            );
            assert_same(&a.cross(a).to_array(), &[0.0; 3]);
            assert_same(&a.cross(b).to_array(), &(-b.cross(a)).to_array());
        }
    }

    mod vec3_only {
        use super::*;

        #[test]
        fn new_and_axes_place_the_components() {
            assert_same(&Vec3::new(1.0, 2.0, 3.0).to_array(), &[1.0, 2.0, 3.0]);
            assert_same(&Vec3::X.to_array(), &[1.0, 0.0, 0.0]);
            assert_same(&Vec3::Y.to_array(), &[0.0, 1.0, 0.0]);
            assert_same(&Vec3::Z.to_array(), &[0.0, 0.0, 1.0]);
        }

        #[test]
        fn fields_are_in_memory_order() {
            assert_eq!(core::mem::offset_of!(Vec3, x), 0);
            assert_eq!(core::mem::offset_of!(Vec3, y), 4);
            assert_eq!(core::mem::offset_of!(Vec3, z), 8);
        }

        #[test]
        fn cross_of_the_axes_is_right_handed() {
            assert_same(&Vec3::X.cross(Vec3::Y).to_array(), &Vec3::Z.to_array());
            assert_same(&Vec3::Y.cross(Vec3::Z).to_array(), &Vec3::X.to_array());
            assert_same(&Vec3::Z.cross(Vec3::X).to_array(), &Vec3::Y.to_array());
        }

        #[test]
        fn cross_of_small_integers_is_exact() {
            let product = Vec3::new(1.0, 2.0, 3.0).cross(Vec3::new(4.0, 5.0, 6.0));
            assert_same(&product.to_array(), &[-3.0, 6.0, -3.0]);
        }

        #[test]
        fn dot_adds_left_to_right() {
            // (2^24 + 1) + 1 stays 2^24; 2^24 + (1 + 1) would be 2^24 + 2.
            let sum = Vec3::new(TWO_POW_24, 1.0, 1.0).dot(Vec3::ONE);
            assert_eq!(sum.to_bits(), TWO_POW_24.to_bits());
        }

        #[test]
        fn normalized_keeps_every_component_in_place() {
            // 4 + 9 + 36 = 49, length 7.
            let unit = Vec3::new(2.0, 3.0, 6.0).normalized();
            let expected = [2.0 / 7.0, 3.0 / 7.0, 6.0 / 7.0];
            assert_eq!(unit.map(|u| bits(&u.to_array())), Some(bits(&expected)));
        }

        #[test]
        fn extend_and_truncate_change_the_dimension() {
            assert_same(
                &Vec3::new(1.0, 2.0, 3.0).extend(4.0).to_array(),
                &[1.0, 2.0, 3.0, 4.0],
            );
            assert_same(&Vec3::new(1.0, 2.0, 3.0).truncate().to_array(), &[1.0, 2.0]);
        }
    }

    mod vec4_only {
        use super::*;

        #[test]
        fn new_places_the_components() {
            assert_same(
                &Vec4::new(1.0, 2.0, 3.0, 4.0).to_array(),
                &[1.0, 2.0, 3.0, 4.0],
            );
        }

        #[test]
        fn fields_are_in_memory_order() {
            assert_eq!(core::mem::offset_of!(Vec4, x), 0);
            assert_eq!(core::mem::offset_of!(Vec4, y), 4);
            assert_eq!(core::mem::offset_of!(Vec4, z), 8);
            assert_eq!(core::mem::offset_of!(Vec4, w), 12);
        }

        #[test]
        fn dot_adds_left_to_right() {
            // ((2^24 + 1) + 1) + 1 stays 2^24; a pairwise sum would give 2^24 + 2.
            let sum = Vec4::new(TWO_POW_24, 1.0, 1.0, 1.0).dot(Vec4::ONE);
            assert_eq!(sum.to_bits(), TWO_POW_24.to_bits());
        }

        #[test]
        fn axes_are_the_unit_vectors() {
            assert_same(&Vec4::X.to_array(), &[1.0, 0.0, 0.0, 0.0]);
            assert_same(&Vec4::Y.to_array(), &[0.0, 1.0, 0.0, 0.0]);
            assert_same(&Vec4::Z.to_array(), &[0.0, 0.0, 1.0, 0.0]);
            assert_same(&Vec4::W.to_array(), &[0.0, 0.0, 0.0, 1.0]);
        }

        #[test]
        fn normalized_keeps_every_component_in_place() {
            // 1 + 4 + 4 + 16 = 25, length 5.
            let unit = Vec4::new(1.0, 2.0, 2.0, 4.0).normalized();
            let expected = [1.0 / 5.0, 2.0 / 5.0, 2.0 / 5.0, 4.0 / 5.0];
            assert_eq!(unit.map(|u| bits(&u.to_array())), Some(bits(&expected)));
        }

        #[test]
        fn truncate_drops_the_last_component() {
            assert_same(
                &Vec4::new(1.0, 2.0, 3.0, 4.0).truncate().to_array(),
                &[1.0, 2.0, 3.0],
            );
        }
    }
}
