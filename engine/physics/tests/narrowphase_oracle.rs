//! The 2D narrowphase against a brute-force oracle: an independent reference written here in `f64`
//! (exhaustive separating axes over every candidate axis of a box pair, the signed distance
//! function of a box for the circle cases, the closed form for two circles), compared with the
//! production `collide` on seeded random scenes, plus the properties a solver relies on: symmetry,
//! invariance under an exact translation and a quarter turn, determinism with a pinned hash.
//!
//! Scenes are made of integers over powers of two and rotations from integer `(cos, sin)` pairs
//! (normalised with one square root, which is exact IEEE), so no platform function enters and the
//! pinned hash is the same on every platform.

use base::math::Vec2;
use base::rng::Pcg32;
use physics::core::{Dim, HashBits, StateHasher};
use physics::d2::{ContactPoint2, D2, Manifold2, Placed2, Rot2, Shape2};

/// Allowed difference between an `f32` result and the `f64` oracle (scenes are a few metres wide).
const TOL: f64 = 2.0e-4;
/// Two candidate answers closer than this are a tie, and the tie-break is not compared.
const TIE: f64 = 1.0e-3;
/// A box deeper into another than this may leave a contact point outside the thin box.
const SHALLOW: f64 = -0.2;
/// The bias of the production code toward the first shape's face (`REFERENCE_FACE_BIAS`).
const REFERENCE_BIAS: f64 = 0.0005;
const MARGINS: [f32; 3] = [0.0, 0.02, 0.1];

type P = [f64; 2];

fn int(rng: &mut Pcg32, n: u16) -> i16 {
    i16::try_from(rng.below(u32::from(n)).unwrap_or(0)).unwrap_or(0)
}

fn rotation(rng: &mut Pcg32) -> Rot2 {
    let (a, b) = (int(rng, 201) - 100, int(rng, 201) - 100);
    Rot2::from_cos_sin(f32::from(a), f32::from(b)).unwrap_or(Rot2::IDENTITY)
}

fn size(rng: &mut Pcg32) -> f32 {
    f32::from(64 + int(rng, 192)) / 256.0
}

fn placed(rng: &mut Pcg32, circle: bool) -> Placed2 {
    let shape = if circle {
        Shape2::Circle { radius: size(rng) }
    } else {
        Shape2::Box {
            half_extents: Vec2::new(size(rng), size(rng)),
        }
    };
    let position = Vec2::new(
        f32::from(int(rng, 1024) - 512) / 256.0,
        f32::from(int(rng, 1024) - 512) / 256.0,
    );
    Placed2::new(shape, position, rotation(rng))
}

fn collide(a: &Placed2, b: &Placed2, margin: f32) -> Option<Manifold2> {
    D2::collide(a, b, margin)
}

// ---- the oracle ---------------------------------------------------------------------------------

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn neg(a: P) -> P {
    [-a[0], -a[1]]
}

fn norm(a: P) -> f64 {
    dot(a, a).sqrt()
}

fn wide(v: Vec2) -> P {
    [f64::from(v.x), f64::from(v.y)]
}

/// A shape in `f64`: centre, orientation, and either a radius or half extents.
#[derive(Clone, Copy)]
struct Wide {
    centre: P,
    cos: f64,
    sin: f64,
    circle: Option<f64>,
    half: P,
}

fn widen(p: &Placed2) -> Wide {
    let (cos, sin) = p.rotation.direction();
    let (circle, half) = match p.shape {
        Shape2::Circle { radius } => (Some(f64::from(radius)), [0.0, 0.0]),
        Shape2::Box { half_extents } => (None, wide(half_extents)),
        _ => (None, [0.0, 0.0]),
    };
    Wide {
        centre: wide(p.position),
        cos: f64::from(cos),
        sin: f64::from(sin),
        circle,
        half,
    }
}

impl Wide {
    fn axes(&self) -> [P; 2] {
        [[self.cos, self.sin], [-self.sin, self.cos]]
    }

    fn corners(&self) -> [P; 4] {
        let [u, v] = self.axes();
        let at = |x: f64, y: f64| {
            [
                self.centre[0] + u[0] * x + v[0] * y,
                self.centre[1] + u[1] * x + v[1] * y,
            ]
        };
        [
            at(-self.half[0], -self.half[1]),
            at(self.half[0], -self.half[1]),
            at(self.half[0], self.half[1]),
            at(-self.half[0], self.half[1]),
        ]
    }

    /// The signed distance of `p` to the shape and the unit direction in which it grows.
    fn sdf(&self, at: P) -> (f64, P) {
        let offset = sub(at, self.centre);
        if let Some(radius) = self.circle {
            let length = norm(offset);
            return (length - radius, unit_or_x(offset, length));
        }
        let [along_u, along_v] = self.axes();
        let local = [dot(offset, along_u), dot(offset, along_v)];
        let excess = [local[0].abs() - self.half[0], local[1].abs() - self.half[1]];
        let sign = [local[0].signum(), local[1].signum()];
        let (distance, local_dir) = if excess[0] > 0.0 || excess[1] > 0.0 {
            let out = [excess[0].max(0.0), excess[1].max(0.0)];
            let distance = norm(out);
            (
                distance,
                [sign[0] * out[0] / distance, sign[1] * out[1] / distance],
            )
        } else if excess[0] >= excess[1] {
            (excess[0], [sign[0], 0.0])
        } else {
            (excess[1], [0.0, sign[1]])
        };
        (
            distance,
            [
                along_u[0] * local_dir[0] + along_v[0] * local_dir[1],
                along_u[1] * local_dir[0] + along_v[1] * local_dir[1],
            ],
        )
    }
}

/// The unit vector of `offset`, or the x axis when it has no length.
fn unit_or_x(offset: P, length: f64) -> P {
    if length > 0.0 {
        [offset[0] / length, offset[1] / length]
    } else {
        [1.0, 0.0]
    }
}

/// Every candidate axis of a box pair: the signed gap along it (positive apart, negative deep) and
/// the unit direction from the first box to the second.
fn box_axes(a: &Wide, b: &Wide) -> Vec<(f64, P)> {
    let mut found = Vec::new();
    for axis in a.axes().into_iter().chain(b.axes()) {
        let project = |w: &Wide| {
            let values = w.corners().map(|c| dot(c, axis));
            (
                values.iter().copied().fold(f64::INFINITY, f64::min),
                values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            )
        };
        let ((a_min, a_max), (b_min, b_max)) = (project(a), project(b));
        let (plus, minus) = (b_min - a_max, a_min - b_max);
        if plus >= minus {
            found.push((plus, axis));
        } else {
            found.push((minus, neg(axis)));
        }
    }
    found
}

/// The oracle's best axis and whether it is a tie with another one or with its own opposite.
fn best_axis(axes: &[(f64, P)]) -> ((f64, P), bool) {
    let mut sorted = axes.to_vec();
    sorted.sort_by(|x, y| y.0.total_cmp(&x.0));
    let tie = sorted[0].0 - sorted[1].0 < TIE;
    (sorted[0], tie)
}

/// The distance and direction (from `a` to `b`) the oracle expects of any pair, and whether the
/// answer is a tie that production may break differently.
fn expected(a: &Placed2, b: &Placed2) -> (f64, P, bool) {
    let (wa, wb) = (widen(a), widen(b));
    match (wa.circle, wb.circle) {
        (None, None) => {
            let ((gap, dir), tie) = best_axis(&box_axes(&wa, &wb));
            (gap, dir, tie)
        }
        (Some(ra), Some(rb)) => {
            let offset = sub(wb.centre, wa.centre);
            let length = norm(offset);
            (
                length - ra - rb,
                [offset[0] / length, offset[1] / length],
                false,
            )
        }
        (None, Some(r)) => {
            let (distance, dir) = wa.sdf(wb.centre);
            (distance - r, dir, inside_tie(&wa, wb.centre))
        }
        (Some(r), None) => {
            let (distance, dir) = wb.sdf(wa.centre);
            (distance - r, neg(dir), inside_tie(&wb, wa.centre))
        }
    }
}

/// True when `point` is inside the box and nearly as close to a second face as to the nearest one.
fn inside_tie(w: &Wide, point: P) -> bool {
    let [u, v] = w.axes();
    let offset = sub(point, w.centre);
    let d = [
        dot(offset, u).abs() - w.half[0],
        dot(offset, v).abs() - w.half[1],
    ];
    (d[0] <= 0.0 && d[1] <= 0.0 && (d[0] - d[1]).abs() < TIE) || d[0].abs().max(d[1].abs()) < TIE
}

// ---- comparison -----------------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Tally {
    none: u32,
    one: u32,
    two: u32,
    speculative: u32,
}

fn check_pair(a: &Placed2, b: &Placed2, margin: f32, tally: &mut Tally, label: &str) {
    let (gap, dir, tie) = expected(a, b);
    let result = collide(a, b, margin);
    let limit = f64::from(margin);
    if gap > limit + TOL {
        assert!(result.is_none(), "{label}: gap {gap} must give no contact");
        tally.none += 1;
        return;
    }
    if gap > limit - TOL {
        return; // inside the tolerance band: either answer is correct
    }
    let Some(m) = result else {
        panic!("{label}: gap {gap} within margin {margin} must give a contact");
    };
    match m.points().len() {
        1 => tally.one += 1,
        _ => tally.two += 1,
    }
    tally.speculative += u32::from(gap > 0.0);
    assert!(m.normal.x.is_finite(), "{label}: {m:?}");
    let length = norm(wide(m.normal));
    assert!(
        (length - 1.0).abs() < 1e-5,
        "{label}: normal length {length}"
    );
    if !tie {
        let n = wide(m.normal);
        assert!(
            (n[0] - dir[0]).abs() < TOL * 5.0 && (n[1] - dir[1]).abs() < TOL * 5.0,
            "{label}: normal {n:?} expected {dir:?}"
        );
    }
    check_points(a, b, &m, gap, label);
}

fn check_points(a: &Placed2, b: &Placed2, m: &Manifold2, gap: f64, label: &str) {
    let (wa, wb) = (widen(a), widen(b));
    // For overlapping shapes the deepest point sits at the true depth: a manifold that dropped it
    // would be too shallow. (Apart, the axis gap of two boxes can be smaller than the corner distance.)
    let deepest = m
        .points()
        .iter()
        .map(|point| f64::from(point.separation))
        .fold(f64::INFINITY, f64::min);
    assert!(
        gap > 0.0 || deepest <= gap + TOL,
        "{label}: deepest point {deepest} shallower than {gap}"
    );
    for point in m.points() {
        let separation = f64::from(point.separation);
        let p = wide(point.point);
        // No point is deeper than the deepest approach of the two shapes, except by the bias that
        // keeps the first shape's face the reference (it lets a face that is worse by up to that
        // much be the reference).
        assert!(
            separation >= gap - TOL - REFERENCE_BIAS,
            "{label}: point {separation} deeper than {gap}"
        );
        if gap > SHALLOW {
            // The point is half way between a surface point of each shape, so it is within half the
            // separation of both surfaces (a vertex shifted along the reference normal may leave the
            // other shape by up to that much), and never far inside either.
            let allowed = separation.abs() / 2.0 + TOL;
            assert!(
                wa.sdf(p).0.abs() <= allowed,
                "{label}: point outside the first by {}",
                wa.sdf(p).0
            );
            assert!(
                wb.sdf(p).0.abs() <= allowed,
                "{label}: point outside the second by {} (sep {separation}, gap {gap})",
                wb.sdf(p).0
            );
        }
        // The anchors are the point relative to each centre.
        let (aa, ab) = (wide(point.anchor_a), wide(point.anchor_b));
        assert!(
            (p[0] - wa.centre[0] - aa[0]).abs() < 2.0 * TOL,
            "{label}: anchor a"
        );
        assert!(
            (p[1] - wa.centre[1] - aa[1]).abs() < 2.0 * TOL,
            "{label}: anchor a"
        );
        assert!(
            (p[0] - wb.centre[0] - ab[0]).abs() < 2.0 * TOL,
            "{label}: anchor b"
        );
        assert!(
            (p[1] - wb.centre[1] - ab[1]).abs() < 2.0 * TOL,
            "{label}: anchor b"
        );
    }
}

fn run_oracle(kind_a: bool, kind_b: bool, stream: u64, scenes: u32) -> Tally {
    let mut rng = Pcg32::new(0x4_12, stream);
    let mut tally = Tally::default();
    for scene in 0..scenes {
        let (a, b) = (placed(&mut rng, kind_a), placed(&mut rng, kind_b));
        for margin in MARGINS {
            let label = format!("stream {stream} scene {scene} margin {margin}");
            check_pair(&a, &b, margin, &mut tally, &label);
        }
    }
    tally
}

#[test]
fn box_against_box_agrees_with_the_exhaustive_separating_axis_oracle() {
    let tally = run_oracle(false, false, 1, 4000);
    assert!(tally.none > 2000, "{tally:?}");
    assert!(tally.two > 800 && tally.one > 50, "{tally:?}");
    assert!(tally.speculative > 20, "{tally:?}");
}

#[test]
fn circle_against_box_agrees_with_the_signed_distance_oracle() {
    let tally = run_oracle(true, false, 2, 4000);
    assert!(tally.none > 2000 && tally.one > 800, "{tally:?}");
    assert_eq!(tally.two, 0);
    assert!(tally.speculative > 20, "{tally:?}");
}

#[test]
fn box_against_circle_agrees_with_the_signed_distance_oracle() {
    let tally = run_oracle(false, true, 3, 4000);
    assert!(tally.none > 2000 && tally.one > 800, "{tally:?}");
    assert_eq!(tally.two, 0);
}

#[test]
fn circle_against_circle_agrees_with_the_closed_form() {
    let tally = run_oracle(true, true, 4, 4000);
    assert!(tally.none > 1000 && tally.one > 400, "{tally:?}");
    assert!(tally.speculative > 20, "{tally:?}");
}

// ---- properties -------------------------------------------------------------------------------------

fn scenes(stream: u64, count: u32) -> Vec<(Placed2, Placed2)> {
    let mut rng = Pcg32::new(0x4_12, stream);
    (0..count)
        .map(|i| {
            let first = placed(&mut rng, i % 4 < 2);
            let second = placed(&mut rng, i % 2 == 1);
            (first, second)
        })
        .collect()
}

fn near(a: Vec2, b: Vec2, tol: f32) -> bool {
    (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol
}

/// True when `other` is the same contact as `point`, with the anchors swapped when `mirrored`.
fn same_contact(point: &ContactPoint2, other: &ContactPoint2, mirrored: bool) -> bool {
    let (anchor_a, anchor_b) = if mirrored {
        (other.anchor_b, other.anchor_a)
    } else {
        (other.anchor_a, other.anchor_b)
    };
    near(point.point, other.point, 1e-4)
        && near(point.anchor_a, anchor_a, 1e-4)
        && near(point.anchor_b, anchor_b, 1e-4)
        && (point.separation - other.separation).abs() < 1e-5
}

fn same_point_set(first: &Manifold2, second: &Manifold2, mirrored: bool) -> bool {
    first.points().iter().all(|point| {
        second
            .points()
            .iter()
            .any(|other| same_contact(point, other, mirrored))
    })
}

/// True when the roles of the two boxes can swap: the best faces of the two boxes differ by more
/// than the reference bias, so the same face is the reference either way.
fn reference_is_clear(a: &Placed2, b: &Placed2) -> bool {
    let (wa, wb) = (widen(a), widen(b));
    if wa.circle.is_some() || wb.circle.is_some() {
        return true;
    }
    let from_a = face_best(&wa, &wb);
    let from_b = face_best(&wb, &wa);
    (from_a - from_b).abs() > 0.002 && from_a.max(from_b) < 0.0
}

/// The largest over the four faces of `p` of the smallest distance of a vertex of `q` to the face.
fn face_best(p: &Wide, q: &Wide) -> f64 {
    let [u, v] = p.axes();
    [
        (u, p.half[0]),
        (neg(u), p.half[0]),
        (v, p.half[1]),
        (neg(v), p.half[1]),
    ]
    .iter()
    .map(|&(axis, reach)| {
        let distance = |c: &P| dot(sub(*c, p.centre), axis) - reach;
        q.corners()
            .iter()
            .map(distance)
            .fold(f64::INFINITY, f64::min)
    })
    .fold(f64::NEG_INFINITY, f64::max)
}

/// Compares `collide(a, b)` with `collide(b, a)` for one scene; true when both gave a contact.
fn swapped_result_mirrors(a: &Placed2, b: &Placed2, margin: f32, label: &str) -> bool {
    let (gap, _, _) = expected(a, b);
    if (gap - f64::from(margin)).abs() < 10.0 * TOL || !reference_is_clear(a, b) {
        return false;
    }
    let (forward, backward) = (collide(a, b, margin), collide(b, a, margin));
    let (Some(first), Some(second)) = (forward, backward) else {
        assert_eq!(forward.is_none(), backward.is_none(), "{label}");
        return false;
    };
    let flipped = Vec2::new(-second.normal.x, -second.normal.y);
    assert!(near(first.normal, flipped, 1e-5), "{label}");
    assert_eq!(
        first.points().len(),
        second.points().len(),
        "{label}: {first:?} {second:?}"
    );
    assert!(
        same_point_set(&first, &second, true),
        "{label}: {first:?} vs {second:?}"
    );
    true
}

#[test]
fn swapping_the_two_shapes_mirrors_the_manifold() {
    let mut compared = 0;
    for (i, (a, b)) in scenes(10, 6000).into_iter().enumerate() {
        for margin in MARGINS {
            let label = format!("scene {i} margin {margin}");
            compared += u32::from(swapped_result_mirrors(&a, &b, margin, &label));
        }
    }
    assert!(compared > 2000, "{compared}");
}

#[test]
fn a_box_and_a_circle_swap_roles_bit_for_bit() {
    let mut compared = 0;
    for (a, b) in scenes(11, 4000) {
        let boxed = |p: &Placed2| matches!(p.shape, Shape2::Box { .. });
        if boxed(&a) == boxed(&b) {
            continue;
        }
        for margin in MARGINS {
            let (forward, backward) = (collide(&a, &b, margin), collide(&b, &a, margin));
            assert_eq!(forward.map(Manifold2::flipped), backward);
            compared += u32::from(forward.is_some());
        }
    }
    assert!(compared > 500, "{compared}");
}

fn shift_of(p: &Placed2, shift: Vec2) -> Placed2 {
    Placed2::new(p.shape, p.position + shift, p.rotation)
}

/// Collides a pair at home and moved by `shift`; returns 1 when both gave a contact.
fn shifted_pair_is_identical(a: &Placed2, b: &Placed2, margin: f32, shift: Vec2) -> u32 {
    let home = collide(a, b, margin);
    let away = collide(&shift_of(a, shift), &shift_of(b, shift), margin);
    assert_eq!(home.is_some(), away.is_some());
    let (Some(at_home), Some(moved_away)) = (home, away) else {
        return 0;
    };
    assert_eq!(at_home.normal, moved_away.normal);
    assert_eq!(at_home.points().len(), moved_away.points().len());
    for (p, q) in at_home.points().iter().zip(moved_away.points()) {
        assert_eq!(
            (p.anchor_a, p.anchor_b, p.id),
            (q.anchor_a, q.anchor_b, q.id)
        );
        assert_eq!(p.separation.to_bits(), q.separation.to_bits());
        assert!(near(q.point - shift, p.point, 2.0e-3));
    }
    1
}

#[test]
fn moving_both_bodies_by_an_exact_amount_changes_no_relative_result_bit() {
    let shift = Vec2::new(1024.0, -2048.0);
    let mut compared = 0;
    for (a, b) in scenes(12, 4000) {
        for margin in MARGINS {
            compared += shifted_pair_is_identical(&a, &b, margin, shift);
        }
    }
    assert!(compared > 2000, "{compared}");
}

fn quarter(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

fn turned(p: &Placed2) -> Placed2 {
    let (cos, sin) = p.rotation.direction();
    let rotation = Rot2::from_cos_sin(-sin, cos).unwrap_or(Rot2::IDENTITY);
    Placed2::new(p.shape, quarter(p.position), rotation)
}

/// Collides a pair and the same pair turned a quarter turn; returns 1 when both gave a contact.
fn turned_pair_turns(a: &Placed2, b: &Placed2, margin: f32) -> u32 {
    let (gap, _, tie) = expected(a, b);
    if tie || !reference_is_clear(a, b) || (gap - f64::from(margin)).abs() < 10.0 * TOL {
        return 0;
    }
    let home = collide(a, b, margin);
    let away = collide(&turned(a), &turned(b), margin);
    assert_eq!(home.is_none(), away.is_none());
    let (Some(at_home), Some(turned_away)) = (home, away) else {
        return 0;
    };
    assert!(near(quarter(at_home.normal), turned_away.normal, 1e-5));
    assert_eq!(at_home.points().len(), turned_away.points().len());
    for (p, q) in at_home.points().iter().zip(turned_away.points()) {
        assert!(near(quarter(p.point), q.point, 1e-4));
        assert!(near(quarter(p.anchor_a), q.anchor_a, 1e-4));
        assert!(near(quarter(p.anchor_b), q.anchor_b, 1e-4));
        assert!((p.separation - q.separation).abs() < 1e-5);
        assert_eq!(p.id, q.id);
    }
    1
}

#[test]
fn turning_the_whole_scene_by_a_quarter_turn_turns_the_manifold() {
    let mut compared = 0;
    for (a, b) in scenes(13, 4000) {
        for margin in MARGINS {
            compared += turned_pair_turns(&a, &b, margin);
        }
    }
    assert!(compared > 1500, "{compared}");
}

/// Hashes one result (a flag, then the manifold bits) and returns 1 for a contact.
fn write_result(hasher: &mut StateHasher, result: Option<Manifold2>) -> u32 {
    hasher.write_u8(u8::from(result.is_some()));
    if let Some(manifold) = result {
        manifold.write_bits(hasher);
    }
    u32::from(result.is_some())
}

fn hash_of(stream: u64, count: u32) -> (u64, u32) {
    let mut hasher = StateHasher::new();
    let mut contacts = 0;
    for (a, b) in scenes(stream, count) {
        for margin in MARGINS {
            contacts += write_result(&mut hasher, collide(&a, &b, margin));
        }
    }
    (hasher.finish(), contacts)
}

/// The scenes use no platform function, so the manifolds are bit-identical on every platform (the
/// review file records the Windows and Linux runs, feature off and on).
const PINNED_HASH: u64 = 0x0dac_fba7_9b3b_7b34;
const PINNED_CONTACTS: u32 = 2460;

#[test]
fn the_same_scenes_give_bit_identical_manifolds_every_time_and_the_pinned_hash() {
    let (first, contacts) = hash_of(20, 3000);
    assert_eq!(hash_of(20, 3000), (first, contacts));
    assert_ne!(hash_of(21, 3000).0, first);
    assert_eq!((first, contacts), (PINNED_HASH, PINNED_CONTACTS));
}
