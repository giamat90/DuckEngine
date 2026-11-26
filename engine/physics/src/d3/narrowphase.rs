//! The 3D narrowphase: which routine handles which pair of shapes.
//!
//! | first / second | sphere, capsule | box |
//! |---|---|---|
//! | sphere, capsule | closest points of two segments, widened by the radii | the segment against the box, result flipped |
//! | box | the segment against the box | separating axes, reference face, clipping ([`super::box_box`]) |
//!
//! Every pair with a convex hull in it goes to [`super::face_clip`] (GJK and EPA for the normal, then the
//! support faces clipped, up to four points; card 8-18).
//!
//! A sphere is a capsule whose segment has length zero, so one routine serves both. Only `+ - * /`
//! and `sqrt` are used, in a fixed order; no `sin`, `cos` or `atan2`. Any input that is not valid
//! (a shape out of range, a value that is not finite, a negative margin) and any result that is not
//! finite gives `None`, never a manifold with a NaN in it.

use super::box_box;
use super::face_clip;
use super::gjk::{Convex, Core};
use super::manifold::{ContactPoint3, Manifold3};
use super::{D3, Shape3};
use crate::core::{Placed, Quantity};
use base::math::{Quat, Vec3};

/// The normal used when two spheres have their centres in the same place: the x axis.
const COINCIDENT_NORMAL: Vec3 = Vec3::X;
/// A squared length below which a segment is a point and a vector has no direction.
const MIN_LENGTH_SQUARED: f32 = 1.0e-12;
/// Two segments are parallel when `sin^2` of their angle is at most this.
const PARALLEL_SINE_SQUARED: f32 = 1.0e-6;
/// Two capsules lying alongside each other (card 8-89): `sin^2` of the angle between their axes is
/// at most this (about 18 degrees), the range in which the end of their overlap is a second contact
/// and not an arbitrary point of a crossing. It was first chosen on the mixed pile of card 8-21 alone
/// (seed 12: with no bound 9 of its 90 bodies are asleep at step 1200, with 0.1 all are), but card
/// 8-90 found that this is one lucky pile and not an effect of the bound: over ten seeds of that pile
/// the number of piles all asleep at step 1200 is 2, 3, 2, 4, 5 and 4 of 10 for no far end, 0.0025,
/// 0.1, 0.6, 0.97 and no bound, which is noise at that sample size (the pile is chaotic: with 0.1,
/// seeds 5 and 7 are asleep by step 2400, seed 1 by 4000, seeds 2 and 3 not by 4000). No effect of the bound is detectable at
/// this sample size; it is kept as geometry, not as tuning.
const NEAR_PARALLEL_SINE_SQUARED: f32 = 0.1;
/// The shortest overlap of two parallel segments that gets two contact points.
const MIN_OVERLAP: f32 = 1.0e-4;
/// The least a capsule end may be from the nearest point and still get a contact, whatever the
/// margin (it was the whole rule before card 8-21): with a margin of zero or a millimetre the two ends
/// of a capsule lying flat would otherwise be told apart by rounding.
const MIN_END_BAND: f32 = 5.0e-3;
/// Steps of the search for the point of a segment closest to a box.
const SEARCH_STEPS: u32 = 40;

/// An end of a capsule at most this much, in metres, farther from a box than the nearest point of
/// its axis is as near as that point: the distance is flat along the segment there, the nearest
/// point is no better place for a contact than the end, and it moves with the last digits of the
/// search (card 9-9).
const NEAREST_REDUNDANT: f32 = 1.0e-3;

/// A segment and a radius: a sphere (both ends the same point) or a capsule.
#[derive(Clone, Copy)]
struct Round {
    start: Vec3,
    end: Vec3,
    radius: f32,
}

impl Round {
    fn of(placed: &Placed<D3>) -> Option<Self> {
        match placed.shape {
            Shape3::Sphere { radius } => Some(Self {
                start: placed.position,
                end: placed.position,
                radius,
            }),
            Shape3::Capsule {
                half_height,
                radius,
            } => {
                let half_axis = placed.rotation.rotate(Vec3::Y) * half_height;
                Some(Self {
                    start: placed.position - half_axis,
                    end: placed.position + half_axis,
                    radius,
                })
            }
            _ => None,
        }
    }

    fn direction(&self) -> Vec3 {
        self.end - self.start
    }

    fn at(&self, t: f32) -> Vec3 {
        self.start + self.direction() * t
    }

    fn centre(&self) -> Vec3 {
        (self.start + self.end) * 0.5
    }

    fn is_point(&self) -> bool {
        self.direction().length_squared() <= MIN_LENGTH_SQUARED
    }
}

/// True when the shape's sizes are valid and the placement is finite.
fn is_valid(placed: &Placed<D3>) -> bool {
    super::shape::is_valid(&placed.shape)
        && placed.position.is_finite()
        && Quantity::is_finite(&placed.rotation)
}

fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// The parameters of the points of `p` and `q` closest to each other when one of them is a point.
fn closest_with_a_point(p: &Round, q: &Round, r: Vec3) -> (f32, f32) {
    if p.is_point() && q.is_point() {
        return (0.0, 0.0);
    }
    if p.is_point() {
        let d2 = q.direction();
        return (0.0, clamp01(d2.dot(r) / d2.length_squared()));
    }
    let d1 = p.direction();
    (clamp01(-d1.dot(r) / d1.length_squared()), 0.0)
}

/// The parameters of the closest points `first.at(.0)` and `second.at(.1)` of two segments.
fn closest_parameters(first: &Round, second: &Round) -> (f32, f32) {
    let offset = first.start - second.start;
    if first.is_point() || second.is_point() {
        return closest_with_a_point(first, second, offset);
    }
    let (dir1, dir2) = (first.direction(), second.direction());
    let (len1, len2, cross_dot) = (dir1.length_squared(), dir2.length_squared(), dir1.dot(dir2));
    let (on_first, on_second) = (dir1.dot(offset), dir2.dot(offset));
    let denominator = len1 * len2 - cross_dot * cross_dot;
    let along_first = if denominator > PARALLEL_SINE_SQUARED * len1 * len2 {
        clamp01((cross_dot * on_second - on_first * len2) / denominator)
    } else {
        0.0
    };
    let along_second = (cross_dot * along_first + on_second) / len2;
    if along_second < 0.0 {
        (clamp01(-on_first / len1), 0.0)
    } else if along_second > 1.0 {
        (clamp01((cross_dot - on_first) / len1), 1.0)
    } else {
        (along_first, along_second)
    }
}

/// A unit vector perpendicular to `direction` (which is not zero).
fn perpendicular(direction: Vec3) -> Option<Vec3> {
    let a = direction.abs();
    let axis = if a.x <= a.y && a.x <= a.z {
        Vec3::X
    } else if a.y <= a.z {
        Vec3::Y
    } else {
        Vec3::Z
    };
    direction.cross(axis).normalized()
}

/// The normal from `p` to `q` for the closest points `from` and `to`: their direction, or when they
/// coincide the direction across both segments, or any direction across one, or a fixed one.
fn pair_normal(p: &Round, q: &Round, from: Vec3, to: Vec3) -> Vec3 {
    let gap = to - from;
    if gap.length_squared() > MIN_LENGTH_SQUARED {
        return gap.normalized().unwrap_or(COINCIDENT_NORMAL);
    }
    let across = p.direction().cross(q.direction()).normalized();
    let alone = [p, q]
        .iter()
        .find(|s| !s.is_point())
        .and_then(|s| perpendicular(s.direction()));
    across.or(alone).unwrap_or(COINCIDENT_NORMAL)
}

/// The contact of the surfaces whose axis points are `from` (first shape) and `to` (second).
fn contact(from: Vec3, to: Vec3, normal: Vec3, radii: (f32, f32)) -> ContactPoint3 {
    let on_first = from + normal * radii.0;
    let on_second = to - normal * radii.1;
    ContactPoint3 {
        point: (on_first + on_second) * 0.5,
        separation: (to - from).dot(normal) - radii.0 - radii.1,
    }
}

/// The contact of two axis points that are not the closest pair (card 8-89), with the true distance
/// of the two surfaces as its separation. Along the normal of the closest pair every point of one
/// skew line is as far from the other as the closest pair is, so the separation along it would
/// be the same everywhere; the distance between the points is what grows away from the closest pair.
fn tube_contact(from: Vec3, to: Vec3, normal: Vec3, radii: (f32, f32)) -> ContactPoint3 {
    ContactPoint3 {
        separation: (to - from).length() - radii.0 - radii.1,
        ..contact(from, to, normal, radii)
    }
}

/// The result with the points that are within `margin`, or nothing when none is.
fn within_margin(normal: Vec3, points: &[ContactPoint3], margin: f32) -> Option<Manifold3> {
    let mut kept = [ContactPoint3::default(); 3];
    let mut count = 0;
    for point in points.iter().filter(|p| p.separation <= margin) {
        kept[count] = *point;
        count += 1;
    }
    (count > 0).then(|| Manifold3::new(normal, &kept[..count]))
}

/// The closest points of two parallel segments at both ends of their overlap, when they overlap.
fn parallel_pairs(first: &Round, second: &Round) -> Option<[(Vec3, Vec3); 2]> {
    let (dir1, dir2) = (first.direction(), second.direction());
    let (len1, len2) = (dir1.length_squared(), dir2.length_squared());
    let sine_squared = dir1.cross(dir2).length_squared();
    if first.is_point() || second.is_point() || sine_squared > PARALLEL_SINE_SQUARED * len1 * len2 {
        return None;
    }
    overlap_pairs(first, second)
}

/// The closest points of two segments, neither a point, at both ends of their overlap along `first`.
fn overlap_pairs(first: &Round, second: &Round) -> Option<[(Vec3, Vec3); 2]> {
    let (dir1, dir2) = (first.direction(), second.direction());
    let len2 = dir2.length_squared();
    let length = dir1.length_squared().sqrt();
    let unit = dir1 / length;
    let starts = [second.start, second.end].map(|end| (end - first.start).dot(unit));
    let (low, high) = (
        starts[0].min(starts[1]).max(0.0),
        starts[0].max(starts[1]).min(length),
    );
    if high - low < MIN_OVERLAP {
        return None;
    }
    let pair = |at: f32| {
        let on_first = first.start + unit * at;
        let param = clamp01((on_first - second.start).dot(dir2) / len2);
        (on_first, second.at(param))
    };
    Some([pair(low), pair(high)])
}

/// Two spheres or capsules.
fn round_round(first: &Round, second: &Round, margin: f32) -> Option<Manifold3> {
    let radii = (first.radius, second.radius);
    if let Some(pairs) = parallel_pairs(first, second) {
        let nearer = if (pairs[0].1 - pairs[0].0).length_squared()
            <= (pairs[1].1 - pairs[1].0).length_squared()
        {
            pairs[0]
        } else {
            pairs[1]
        };
        let normal = pair_normal(first, second, nearer.0, nearer.1);
        let points = pairs.map(|(from, to)| contact(from, to, normal, radii));
        return within_margin(normal, &points, margin);
    }
    let (s, t) = closest_parameters(first, second);
    let (from, to) = (first.at(s), second.at(t));
    let normal = pair_normal(first, second, from, to);
    let near = contact(from, to, normal, radii);
    let band = margin.max(MIN_END_BAND);
    match far_end(first, second, (from + to) * 0.5).map(|(a, b)| tube_contact(a, b, normal, radii))
    {
        Some(far) if far.separation <= near.separation + band => {
            within_margin(normal, &[near, far], margin)
        }
        _ => within_margin(normal, &[near], margin),
    }
}

/// The unit direction halfway between two segments' directions, the second turned round when it
/// points against the first, with the sign that makes its largest component positive: the same for
/// either order of the segments, also when the two point against each other.
fn bisector(first: &Round, second: &Round) -> Option<Vec3> {
    let (one, two) = (
        first.direction().normalized()?,
        second.direction().normalized()?,
    );
    let two = if one.dot(two) < 0.0 { -two } else { two };
    let axis = (one + two).normalized()?;
    let largest = axis.abs();
    let biggest = largest.x.max(largest.y).max(largest.z);
    let along_largest = if largest.x >= biggest {
        axis.x
    } else if largest.y >= biggest {
        axis.y
    } else {
        axis.z
    };
    Some(if along_largest < 0.0 { -axis } else { axis })
}

/// The point of `round` at `height` along `axis` (clamped to the segment, which is not a point and not
/// across `axis`).
fn at_height(round: &Round, axis: Vec3, height: f32) -> Vec3 {
    let (start, along) = (round.start.dot(axis), round.direction().dot(axis));
    round.at(clamp01((height - start) / along))
}

/// The end of the overlap of two segments that is further from `middle`, the centre of their closest
/// pair (card 8-89): a capsule that lies a little off parallel beside another has its second contact
/// there, as one on a box has at its far end (card 8-21), or it would rock on the one point. The
/// overlap is measured along the bisector of the two axes, so that it is the same whichever capsule is
/// given first (card 8-90).
fn far_end(first: &Round, second: &Round, middle: Vec3) -> Option<(Vec3, Vec3)> {
    if first.is_point() || second.is_point() {
        return None;
    }
    let (dir1, dir2) = (first.direction(), second.direction());
    if dir1.cross(dir2).length_squared()
        > NEAR_PARALLEL_SINE_SQUARED * dir1.length_squared() * dir2.length_squared()
    {
        return None;
    }
    let axis = bisector(first, second)?;
    let span = |round: &Round| {
        let (a, b) = (round.start.dot(axis), round.end.dot(axis));
        (a.min(b), a.max(b))
    };
    let (shared, other) = (span(first), span(second));
    let (low, high) = (shared.0.max(other.0), shared.1.min(other.1));
    if high - low < MIN_OVERLAP {
        return None;
    }
    let centre = middle.dot(axis);
    let height = if centre - low >= high - centre {
        low
    } else {
        high
    };
    Some((
        at_height(first, axis, height),
        at_height(second, axis, height),
    ))
}

/// A box in the world: where it is, how it is turned, how big.
#[derive(Clone, Copy)]
pub(super) struct BoxFrame {
    pub(super) centre: Vec3,
    pub(super) rotation: Quat,
    pub(super) half: Vec3,
}

/// How a point stands to a box: the signed distance (negative inside), the nearest point of the
/// surface, and the unit direction from the surface to the point (the outward face normal inside).
struct Probe {
    distance: f32,
    closest: Vec3,
    normal: Vec3,
}

pub(super) fn sign(value: f32) -> f32 {
    if value >= 0.0 { 1.0 } else { -1.0 }
}

impl BoxFrame {
    fn local_point(&self, point: Vec3) -> Vec3 {
        self.rotation.conjugate().rotate(point - self.centre)
    }

    fn world_point(&self, local: Vec3) -> Vec3 {
        self.centre + self.rotation.rotate(local)
    }

    /// The probe of a point inside the box (`local` is in the box's frame): the nearest face, with
    /// a negative distance.
    fn inside(&self, local: Vec3) -> Probe {
        let l = local.to_array();
        let h = self.half.to_array();
        let depth = [l[0].abs() - h[0], l[1].abs() - h[1], l[2].abs() - h[2]];
        let axis = if depth[0] >= depth[1] && depth[0] >= depth[2] {
            0
        } else if depth[1] >= depth[2] {
            1
        } else {
            2
        };
        let mut on_face = l;
        on_face[axis] = sign(l[axis]) * h[axis];
        let mut out = [0.0; 3];
        out[axis] = sign(l[axis]);
        Probe {
            distance: depth[axis],
            closest: self.world_point(Vec3::from_array(on_face)),
            normal: self.rotation.rotate(Vec3::from_array(out)),
        }
    }

    fn probe(&self, point: Vec3) -> Probe {
        let local = self.local_point(point);
        let nearest = local.max(-self.half).min(self.half);
        let gap = local - nearest;
        let distance = gap.length();
        match gap.normalized() {
            Some(direction) if distance > 0.0 => Probe {
                distance,
                closest: self.world_point(nearest),
                normal: self.rotation.rotate(direction),
            },
            _ => self.inside(local),
        }
    }

    fn distance(&self, point: Vec3) -> f32 {
        self.probe(point).distance
    }
}

/// The parameter of the point of the segment nearest the box (the distance is convex along it).
fn nearest_parameter(frame: &BoxFrame, round: &Round) -> f32 {
    if round.is_point() {
        return 0.0;
    }
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..SEARCH_STEPS {
        let third = (high - low) / 3.0;
        let (m1, m2) = (low + third, high - third);
        if frame.distance(round.at(m1)) <= frame.distance(round.at(m2)) {
            high = m2;
        } else {
            low = m1;
        }
    }
    (low + high) * 0.5
}

/// The parameters that get a contact: the ends within the band of the nearest distance
/// (`distance_at` being the distance of a point of the segment to the box), and the nearest point
/// itself (`nearest`) unless one of those ends is as near as it (within [`NEAREST_REDUNDANT`]), in
/// which case the nearest point is no more than a place along a flat stretch of the segment. The
/// band is the contact margin, at least [`MIN_END_BAND`]: an end within the margin of the nearest
/// point is a contact worth keeping, and [`within_margin`] still drops any point that is beyond
/// the margin from its plane (card 8-21: a fixed 5 mm dropped the upper end of a capsule leaning
/// by 8 mm, and the capsule rocked on the other end).
///
/// The nearest point is kept next to an end that is farther from the box than it is (card 9-9):
/// when it was dropped, the contact jumped along the capsule at the band, the warm start found no
/// point where the contact was and the capsule was kicked.
fn contact_parameters(
    round: &Round,
    (nearest, band): (f32, f32),
    distance_at: impl Fn(f32) -> f32,
) -> ([f32; 3], usize) {
    let mut chosen = [nearest; 3];
    if round.is_point() {
        return (chosen, 1);
    }
    let best = distance_at(nearest);
    let mut count = 0;
    let mut as_near = false;
    for end in [0.0, 1.0] {
        let gap = distance_at(end);
        if gap <= best + band.max(MIN_END_BAND) {
            chosen[count] = end;
            count += 1;
            as_near |= gap <= best + NEAREST_REDUNDANT;
        }
    }
    if count == 0 || !as_near {
        chosen[count] = nearest;
        count += 1;
    }
    (chosen, count)
}

/// The separating axis of a segment that goes through the box (so it is no use to look for its
/// nearest point): the box swept along the segment has the faces of the box and the faces across
/// the segment and each box edge, and the axis with the largest gap is the way out that is the
/// shortest. Returns the unit axis from the box to the round shape and the gap along it.
fn deep_axis(frame: &BoxFrame, round: &Round) -> (Vec3, f32) {
    let axes = [
        frame.rotation.rotate(Vec3::X),
        frame.rotation.rotate(Vec3::Y),
        frame.rotation.rotate(Vec3::Z),
    ];
    let (offset, direction) = (round.centre() - frame.centre, round.direction());
    let half = frame.half.to_array();
    let mut best = (COINCIDENT_NORMAL, f32::MIN);
    for k in 0..6 {
        let candidate = if k < 3 {
            Some(axes[k])
        } else {
            direction.cross(axes[k - 3]).normalized()
        };
        let Some(unit) = candidate.filter(|u| u.length_squared() > MIN_LENGTH_SQUARED) else {
            continue;
        };
        let box_reach: f32 = (0..3).map(|i| half[i] * axes[i].dot(unit).abs()).sum();
        let along = offset.dot(unit);
        let gap = along.abs() - box_reach - 0.5 * direction.dot(unit).abs() - round.radius;
        if gap > best.1 {
            best = (unit * sign(along), gap);
        }
    }
    best
}

/// A segment that goes through the box: the contacts are the ends of the segment that are deepest
/// along the axis of [`deep_axis`].
fn deep_contact(round: &Round, frame: &BoxFrame, margin: f32) -> Option<Manifold3> {
    let (normal, _) = deep_axis(frame, round);
    let reach: f32 = {
        let half = frame.half.to_array();
        (0..3)
            .map(|i| half[i] * frame.rotation.rotate(basis(i)).dot(normal).abs())
            .sum()
    };
    let gap_of = |p: Vec3| (p - frame.centre).dot(normal) - reach - round.radius;
    let ends = [round.start, round.end];
    let deepest = gap_of(ends[0]).min(gap_of(ends[1]));
    let mut points = [ContactPoint3::default(); 2];
    let mut count = 0;
    for end in ends {
        let separation = gap_of(end);
        if separation <= deepest + margin.max(MIN_END_BAND) && (count == 0 || !round.is_point()) {
            points[count] = ContactPoint3 {
                point: end - normal * (round.radius + separation * 0.5),
                separation,
            };
            count += 1;
        }
    }
    within_margin(normal, &points[..count], margin)
}

fn basis(i: usize) -> Vec3 {
    match i {
        0 => Vec3::X,
        1 => Vec3::Y,
        _ => Vec3::Z,
    }
}

/// A sphere or capsule against a box; the normal points from the box to the round shape.
fn round_box(round: &Round, frame: &BoxFrame, margin: f32) -> Option<Manifold3> {
    let nearest = nearest_parameter(frame, round);
    let probe = frame.probe(round.at(nearest));
    if probe.distance <= 0.0 {
        return deep_contact(round, frame, margin);
    }
    let (normal, plane_point) = (probe.normal, probe.closest);
    let gap_at = |t: f32| (round.at(t) - plane_point).dot(normal) - round.radius;
    let (params, count) =
        contact_parameters(round, (nearest, margin), |t| frame.distance(round.at(t)));
    let mut points = [ContactPoint3::default(); 3];
    for (slot, t) in points.iter_mut().zip(&params[..count]) {
        let separation = gap_at(*t);
        *slot = ContactPoint3 {
            point: round.at(*t) - normal * (round.radius + separation * 0.5),
            separation,
        };
    }
    within_margin(normal, &points[..count], margin)
}

fn frame_of(placed: &Placed<D3>) -> Option<BoxFrame> {
    match placed.shape {
        Shape3::Box { half_extents } => Some(BoxFrame {
            centre: placed.position,
            rotation: placed.rotation,
            half: half_extents,
        }),
        _ => None,
    }
}

/// A shape as GJK sees it: a core and a radius.
fn convex_of(placed: &Placed<D3>) -> Convex<'_> {
    let at = placed.position;
    match placed.shape {
        Shape3::Sphere { radius } => Convex {
            core: Core::Point(at),
            radius,
        },
        Shape3::Capsule {
            half_height,
            radius,
        } => {
            let along = placed.rotation.rotate(Vec3::Y) * half_height;
            Convex {
                core: Core::Segment(at - along, at + along),
                radius,
            }
        }
        Shape3::Box { half_extents } => Convex {
            core: Core::Box(BoxFrame {
                centre: at,
                rotation: placed.rotation,
                half: half_extents,
            }),
            radius: 0.0,
        },
        Shape3::Hull(ref hull) => Convex {
            core: Core::Hull {
                vertices: hull.vertices(),
                centre: at,
                rotation: placed.rotation,
            },
            radius: 0.0,
        },
    }
}

fn has_hull(placed: &Placed<D3>) -> bool {
    matches!(placed.shape, Shape3::Hull(_))
}

fn dispatch(a: &Placed<D3>, b: &Placed<D3>, margin: f32) -> Option<Manifold3> {
    if has_hull(a) || has_hull(b) {
        return face_clip::contact(&convex_of(a), &convex_of(b), margin);
    }
    match (frame_of(a), frame_of(b)) {
        (Some(first), Some(second)) => box_box::collide(&first, &second, margin),
        (Some(frame), None) => round_box(&Round::of(b)?, &frame, margin),
        (None, Some(frame)) => round_box(&Round::of(a)?, &frame, margin).map(Manifold3::flipped),
        (None, None) => round_round(&Round::of(a)?, &Round::of(b)?, margin),
    }
}

/// The manifold of two placed 3D shapes, normal from `a` to `b` (see [`crate::core::Dim::collide`]).
pub(super) fn collide(a: &Placed<D3>, b: &Placed<D3>, margin: f32) -> Option<Manifold3> {
    if !(is_valid(a) && is_valid(b) && margin >= 0.0) {
        return None;
    }
    let manifold = dispatch(a, b, margin)?;
    manifold.is_finite().then_some(manifold)
}
