//! Box against box: the separating axis test over the fifteen candidate axes (three face normals of
//! each box and the nine cross products of their edges), then a contact from the axis it finds.
//!
//! A face axis gives a face contact: the face of one box (the reference) and the most opposite face
//! of the other (the incident), clipped to the sides of the reference face, up to four points. An
//! edge axis gives one point where the two edges are nearest. The first box's face is preferred to
//! the second's, and a face to an edge, unless the other axis is better by [`AXIS_BIAS`], so that
//! the choice does not flicker from step to step.

use super::clip::{BOX_CAPACITY, Candidates as AnyCandidates, Polygon as AnyPolygon};
use super::manifold::{ContactPoint3, MAX_MANIFOLD_POINTS, Manifold3};
use super::narrowphase::{BoxFrame, sign};
use base::math::Vec3;

/// How much better an axis must be to replace the preferred one.
const AXIS_BIAS: f32 = 5.0e-4;
/// A cross product of two edge directions shorter than this (squared) is no axis: they are parallel.
const MIN_EDGE_AXIS_SQUARED: f32 = 1.0e-6;

type Polygon = AnyPolygon<BOX_CAPACITY>;
type Candidates = AnyCandidates<BOX_CAPACITY>;

/// A box as the test sees it: centre, unit axes in the world, half sizes along them.
struct Obb {
    centre: Vec3,
    axes: [Vec3; 3],
    half: [f32; 3],
}

impl Obb {
    fn new(frame: &BoxFrame) -> Self {
        let q = frame.rotation;
        Self {
            centre: frame.centre,
            axes: [q.rotate(Vec3::X), q.rotate(Vec3::Y), q.rotate(Vec3::Z)],
            half: frame.half.to_array(),
        }
    }

    /// Half the width of the box along the unit vector `direction`.
    fn radius(&self, direction: Vec3) -> f32 {
        (0..3)
            .map(|k| self.half[k] * self.axes[k].dot(direction).abs())
            .sum()
    }
}

#[derive(Clone, Copy)]
enum Feature {
    /// A face normal of the first box.
    FirstFace(usize),
    /// A face normal of the second box.
    SecondFace(usize),
    /// The cross product of an edge of the first box and one of the second.
    Edges(usize, usize),
}

/// A candidate axis: its unit direction (from the first box to the second), the gap along it
/// (negative while the boxes overlap along it) and what it comes from.
#[derive(Clone, Copy)]
struct Axis {
    normal: Vec3,
    separation: f32,
    feature: Feature,
}

fn axis_along(a: &Obb, b: &Obb, direction: Vec3, feature: Feature) -> Axis {
    let along = (b.centre - a.centre).dot(direction);
    Axis {
        normal: direction * sign(along),
        separation: along.abs() - a.radius(direction) - b.radius(direction),
        feature,
    }
}

/// `candidate` when it is better than `best` by more than the bias.
fn prefer(best: Axis, candidate: Axis) -> Axis {
    if candidate.separation > best.separation + AXIS_BIAS {
        candidate
    } else {
        best
    }
}

fn best_face(a: &Obb, b: &Obb) -> Axis {
    let mut best = axis_along(a, b, a.axes[0], Feature::FirstFace(0));
    for k in 1..3 {
        best = prefer(best, axis_along(a, b, a.axes[k], Feature::FirstFace(k)));
    }
    for k in 0..3 {
        best = prefer(best, axis_along(a, b, b.axes[k], Feature::SecondFace(k)));
    }
    best
}

/// The axis with the largest gap, or nothing when it is beyond `margin` (the boxes are apart).
fn best_axis(a: &Obb, b: &Obb, margin: f32) -> Option<Axis> {
    let mut best = best_face(a, b);
    for pair in 0..9 {
        if let Some(edge_axis) = edge_axis(a, b, pair / 3, pair % 3) {
            best = prefer(best, edge_axis);
        }
    }
    (best.separation <= margin).then_some(best)
}

/// The axis across edge `i` of the first box and edge `j` of the second; none when they are parallel.
fn edge_axis(a: &Obb, b: &Obb, i: usize, j: usize) -> Option<Axis> {
    let cross = a.axes[i].cross(b.axes[j]);
    if cross.length_squared() < MIN_EDGE_AXIS_SQUARED {
        return None;
    }
    let direction = cross.normalized()?;
    Some(axis_along(a, b, direction, Feature::Edges(i, j)))
}

/// The face of `incident` that is the most opposite to the direction `n`.
fn incident_face(incident: &Obb, n: Vec3) -> Polygon {
    let mut j = 0;
    for k in 1..3 {
        if incident.axes[k].dot(n).abs() > incident.axes[j].dot(n).abs() {
            j = k;
        }
    }
    let facing = -sign(incident.axes[j].dot(n));
    let centre = incident.centre + incident.axes[j] * (facing * incident.half[j]);
    let (u, v) = ((j + 1) % 3, (j + 2) % 3);
    let (eu, ev) = (
        incident.axes[u] * incident.half[u],
        incident.axes[v] * incident.half[v],
    );
    let mut face = Polygon::empty();
    for corner in [
        centre + eu + ev,
        centre - eu + ev,
        centre - eu - ev,
        centre + eu - ev,
    ] {
        face.push(corner);
    }
    face
}

/// `face` cut to the four side planes of the reference face number `face_axis` of `reference`.
fn clip_to_sides(reference: &Obb, face_axis: usize, face: &Polygon) -> Polygon {
    let mut polygon = *face;
    for k in [(face_axis + 1) % 3, (face_axis + 2) % 3] {
        for direction in [1.0, -1.0] {
            let side = reference.axes[k] * direction;
            let (centre, half) = (reference.centre, reference.half[k]);
            polygon = polygon.clipped(|x| (x - centre).dot(side) - half);
        }
    }
    polygon
}

/// The face contact with face `face` of `reference` as the reference face; `n` points from the
/// reference box to the incident one.
fn face_contact(
    reference: &Obb,
    incident: &Obb,
    face: usize,
    n: Vec3,
    margin: f32,
) -> Option<Manifold3> {
    let plane_point = reference.centre + n * reference.half[face];
    let whole = incident_face(incident, n);
    let mut candidates = Candidates::from_polygon(
        &clip_to_sides(reference, face, &whole),
        plane_point,
        n,
        margin,
    );
    if candidates.len == 0 {
        candidates = Candidates::from_polygon(&whole, plane_point, n, margin);
    }
    if candidates.len > MAX_MANIFOLD_POINTS {
        return Some(Manifold3::new(n, &candidates.reduced(n)));
    }
    (candidates.len > 0).then(|| Manifold3::new(n, candidates.as_slice()))
}

/// The corner of `obb` farthest along `direction`, leaving out the axis `skip` (the edge's own).
fn extreme_point(obb: &Obb, direction: Vec3, skip: usize) -> Vec3 {
    let mut point = obb.centre;
    for k in (0..3).filter(|k| *k != skip) {
        point += obb.axes[k] * (sign(obb.axes[k].dot(direction)) * obb.half[k]);
    }
    point
}

/// The one point where edge `i` of the first box and edge `j` of the second are nearest.
fn edge_contact(first: &Obb, second: &Obb, edges: (usize, usize), axis: &Axis) -> Manifold3 {
    let start_first = extreme_point(first, axis.normal, edges.0);
    let start_second = extreme_point(second, -axis.normal, edges.1);
    let (dir_first, dir_second) = (first.axes[edges.0], second.axes[edges.1]);
    let offset = start_first - start_second;
    let cosine = dir_first.dot(dir_second);
    let (along_first, along_second) = (dir_first.dot(offset), dir_second.dot(offset));
    let second_param = (along_second - cosine * along_first) / (1.0 - cosine * cosine);
    let first_param = second_param * cosine - along_first;
    let nearest_first = start_first + dir_first * first_param;
    let nearest_second = start_second + dir_second * second_param;
    let point = ContactPoint3 {
        point: (nearest_first + nearest_second) * 0.5,
        separation: axis.separation,
    };
    Manifold3::new(axis.normal, &[point])
}

/// The manifold of two boxes, normal from the first to the second.
pub(super) fn collide(first: &BoxFrame, second: &BoxFrame, margin: f32) -> Option<Manifold3> {
    let (a, b) = (Obb::new(first), Obb::new(second));
    let axis = best_axis(&a, &b, margin)?;
    match axis.feature {
        Feature::FirstFace(k) => face_contact(&a, &b, k, axis.normal, margin),
        Feature::SecondFace(k) => {
            face_contact(&b, &a, k, -axis.normal, margin).map(Manifold3::flipped)
        }
        Feature::Edges(i, j) => Some(edge_contact(&a, &b, (i, j), &axis)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3::clip::BOX_CAPACITY as CAPACITY;

    fn candidate(x: f32, z: f32, separation: f32) -> ContactPoint3 {
        ContactPoint3 {
            point: Vec3::new(5.0 + x, 0.0, -3.0 + z),
            separation,
        }
    }

    fn candidates(points: &[ContactPoint3]) -> Candidates {
        let mut list = Candidates {
            items: [ContactPoint3::default(); CAPACITY],
            len: points.len(),
        };
        list.items[..points.len()].copy_from_slice(points);
        list
    }

    #[test]
    fn six_candidates_are_cut_to_the_deepest_the_farthest_and_the_two_widest_on_either_side() {
        let list = candidates(&[
            candidate(2.0, 0.2, -0.1),
            candidate(0.0, 0.0, -0.3),
            candidate(2.0, -0.2, -0.1),
            candidate(4.0, 0.0, -0.1),
            candidate(2.0, 1.0, -0.1),
            candidate(2.0, -1.0, -0.1),
        ]);
        let kept = list.reduced(Vec3::Y);
        let at = |i: usize| (kept[i].point.x - 5.0, kept[i].point.z + 3.0);
        assert_eq!(at(0), (0.0, 0.0), "the deepest first");
        assert_eq!(at(1), (4.0, 0.0), "then the farthest from it");
        assert_eq!(at(2), (2.0, -1.0), "then the widest on one side");
        assert_eq!(at(3), (2.0, 1.0), "then the widest on the other");
    }
}
