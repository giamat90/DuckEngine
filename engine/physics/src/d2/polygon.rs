//! Convex polygons for the narrowphase: separating-axis search, reference and incident faces,
//! clipping and the circle against a polygon. Every routine works in a frame whose origin is the
//! centre of the first body and whose axes are the world axes, so only the distance between the
//! two bodies enters the arithmetic and a translation of both bodies changes no result.
//!
//! The polygons are counter-clockwise with outward unit normals; edge `i` runs from vertex `i` to
//! vertex `i + 1` (wrapping). Only boxes are built today (the shape set of the crate); the code
//! is written for up to [`MAX_VERTICES`] vertices so a polygon shape needs no new geometry.
//!
//! Tie-breaks, all of them "the lowest index wins" (a strict comparison in a loop that runs in
//! index order): the face of maximum separation, the incident face of minimum alignment.

use super::Rot2;
use super::manifold::FeatureId;
use base::math::Vec2;

/// The most vertices a polygon of the narrowphase has.
pub(super) const MAX_VERTICES: usize = 8;

/// Feature kinds, the first field of a [`FeatureId`]. A contact of the reference face of the
/// second shape adds [`FLIPPED_KIND`], so the same two faces in the other roles never share an id.
const KIND_CIRCLE_FACE: u8 = 1;
const KIND_CIRCLE_VERTEX: u8 = 2;
const KIND_INCIDENT_VERTEX: u8 = 3;
const KIND_CLIPPED: u8 = 4;
const FLIPPED_KIND: u8 = 8;

/// The second polygon becomes the reference polygon only when its best face is better than the
/// first polygon's by more than this (metres). The bias keeps the choice from flickering between
/// two almost equal faces from step to step, which would change the feature ids; it is the value
/// `Box2D` uses (0.1 of its linear slop of 5 mm).
pub(super) const REFERENCE_FACE_BIAS: f32 = 0.0005;

/// A squared length at or below this is treated as zero: a direction cannot be taken from it
/// (the smallest normal `f32`, so its square root still has full precision).
pub(super) const MIN_LENGTH_SQUARED: f32 = f32::MIN_POSITIVE;

/// True when `value` is beyond `limit` or is not a number: the pair is not in contact.
pub(super) fn separated(value: f32, limit: f32) -> bool {
    value.is_nan() || value > limit
}

/// One contact point in the working frame.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LocalPoint {
    pub(super) position: Vec2,
    pub(super) separation: f32,
    pub(super) id: FeatureId,
}

/// A contact in the working frame: the normal from the first polygon to the second and the points.
#[derive(Clone, Copy, Debug)]
pub(super) struct Local {
    pub(super) normal: Vec2,
    pub(super) points: [LocalPoint; 2],
    pub(super) count: usize,
}

impl Local {
    pub(super) fn new(normal: Vec2, points: &[LocalPoint]) -> Self {
        Self::from_points(normal, points.iter().copied())
    }

    /// A contact from the first two of `points`.
    fn from_points(normal: Vec2, points: impl Iterator<Item = LocalPoint>) -> Self {
        let mut stored = [LocalPoint::default(); 2];
        let mut count = 0;
        for (slot, point) in stored.iter_mut().zip(points) {
            *slot = point;
            count += 1;
        }
        Self {
            normal,
            points: stored,
            count,
        }
    }

    pub(super) fn points(&self) -> &[LocalPoint] {
        &self.points[..self.count]
    }
}

/// A convex polygon in the working frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Poly {
    count: usize,
    vertices: [Vec2; MAX_VERTICES],
    normals: [Vec2; MAX_VERTICES],
}

impl Poly {
    /// The box of half extents `half`, turned by `rot` about its centre, whose centre is at
    /// `origin`.
    pub(super) fn from_box(half: Vec2, rot: Rot2, origin: Vec2) -> Self {
        let (cos, sin) = rot.direction();
        let turn = |x: f32, y: f32| Vec2::new(cos * x - sin * y, sin * x + cos * y);
        let mut poly = Self {
            count: 4,
            vertices: [Vec2::ZERO; MAX_VERTICES],
            normals: [Vec2::ZERO; MAX_VERTICES],
        };
        let corners = [(-half.x, -half.y), (half.x, -half.y), (half.x, half.y)];
        let last = (-half.x, half.y);
        let directions = [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)];
        for (i, (x, y)) in corners.into_iter().chain([last]).enumerate() {
            poly.vertices[i] = origin + turn(x, y);
        }
        for (i, (x, y)) in directions.into_iter().enumerate() {
            poly.normals[i] = turn(x, y);
        }
        poly
    }

    /// A polygon from counter-clockwise `vertices` (test helper: the unit normals are computed).
    #[cfg(test)]
    pub(super) fn from_vertices(vertices: &[Vec2]) -> Self {
        let mut poly = Self {
            count: vertices.len(),
            vertices: [Vec2::ZERO; MAX_VERTICES],
            normals: [Vec2::ZERO; MAX_VERTICES],
        };
        for (i, vertex) in vertices.iter().enumerate() {
            let next = vertices[(i + 1) % vertices.len()];
            let edge = next - *vertex;
            let length = edge.length();
            poly.vertices[i] = *vertex;
            poly.normals[i] = Vec2::new(edge.y / length, -edge.x / length);
        }
        poly
    }

    fn vertex(&self, index: usize) -> Vec2 {
        self.vertices[index % self.count]
    }

    fn vertex_points(&self) -> &[Vec2] {
        &self.vertices[..self.count]
    }

    fn face_normals(&self) -> &[Vec2] {
        &self.normals[..self.count]
    }
}

/// The best separating face of a polygon against another one.
#[derive(Clone, Copy, Debug)]
struct Separation {
    value: f32,
    edge: usize,
}

/// The face of `p` along which `q` is furthest away (or least deep), and that distance: the
/// largest, over the faces of `p`, of the smallest signed distance of a vertex of `q` to the face.
/// Positive when `q` lies entirely outside the face. The first face wins a tie. A NaN in the
/// first face stays and makes the caller report "no contact".
fn max_separation(p: &Poly, q: &Poly) -> Separation {
    let faces = p.face_normals().iter().zip(p.vertex_points());
    best_face(faces.map(|(normal, from)| {
        q.vertex_points()
            .iter()
            .map(|vertex| normal.dot(*vertex - *from))
            .fold(f32::INFINITY, f32::min)
    }))
}

/// The largest of `values` and its index; the first wins a tie. The first value is taken as it
/// is, so a NaN there stays and the caller reports "no contact".
fn best_face(values: impl Iterator<Item = f32>) -> Separation {
    let mut best = Separation {
        value: f32::NAN,
        edge: 0,
    };
    for (edge, value) in values.enumerate() {
        if edge == 0 || value > best.value {
            best = Separation { value, edge };
        }
    }
    best
}

/// The face of `incident` that faces `normal` most directly (its normal is the most opposite one);
/// the first face wins a tie.
fn incident_face(incident: &Poly, normal: Vec2) -> usize {
    let mut best = (0, f32::INFINITY);
    for (index, face) in incident.face_normals().iter().enumerate() {
        let alignment = normal.dot(*face);
        if alignment < best.1 {
            best = (index, alignment);
        }
    }
    best.0
}

/// A point of the incident face on its way through the clipping.
#[derive(Clone, Copy, Debug)]
struct ClipPoint {
    position: Vec2,
    id: FeatureId,
}

/// At most two clip points.
#[derive(Clone, Copy, Debug)]
struct Segment {
    points: [ClipPoint; 2],
    count: usize,
}

impl Segment {
    fn push(&mut self, point: ClipPoint) {
        if let Some(slot) = self.points.get_mut(self.count) {
            *slot = point;
            self.count += 1;
        }
    }

    fn push_all(&mut self, points: impl IntoIterator<Item = ClipPoint>) {
        for point in points {
            self.push(point);
        }
    }

    fn points(&self) -> &[ClipPoint] {
        &self.points[..self.count]
    }

    /// The part of the segment on the side of the plane `dot(axis, p) = offset` that `axis` points
    /// to (points on the plane are kept). A point where the segment crosses the plane gets `id`.
    fn clipped(&self, axis: Vec2, offset: f32, id: FeatureId) -> Self {
        let distance = |point: &ClipPoint| axis.dot(point.position) - offset;
        let mut kept = Self {
            points: self.points,
            count: 0,
        };
        kept.push_all(
            self.points()
                .iter()
                .filter(|point| distance(point) >= 0.0)
                .copied(),
        );
        if let [first, second] = *self.points() {
            let cut = crossing(&first, &second, [distance(&first), distance(&second)], id);
            kept.push_all(cut);
        }
        kept
    }
}

/// The point where the segment from `first` to `second`, whose signed distances to a plane are
/// `distances`, crosses the plane; `None` unless the two lie strictly on opposite sides.
fn crossing(
    first: &ClipPoint,
    second: &ClipPoint,
    distances: [f32; 2],
    id: FeatureId,
) -> Option<ClipPoint> {
    let [d0, d1] = distances;
    // Signs, not the product: two tiny distances of opposite sign would underflow to zero.
    ((d0 < 0.0 && d1 > 0.0) || (d0 > 0.0 && d1 < 0.0)).then(|| ClipPoint {
        position: first.position + (second.position - first.position) * (d0 / (d0 - d1)),
        id,
    })
}

/// The reference face and the incident face of a polygon pair, with the role flag of the ids.
struct Faces<'a> {
    reference: &'a Poly,
    incident: &'a Poly,
    edge: usize,
    flipped: bool,
}

impl Faces<'_> {
    fn kind(&self, kind: u8) -> u8 {
        if self.flipped {
            kind + FLIPPED_KIND
        } else {
            kind
        }
    }

    /// The incident face cut to the sides of the reference face.
    fn clipped_incident(&self) -> Segment {
        let normal = self.reference.normals[self.edge];
        let face = incident_face(self.incident, normal);
        let vertex = |i: usize| ClipPoint {
            position: self.incident.vertex(i),
            id: FeatureId::pack(
                self.kind(KIND_INCIDENT_VERTEX),
                self.edge,
                i % self.incident.count,
            ),
        };
        let mut start = Segment {
            points: [vertex(face); 2],
            count: 0,
        };
        start.push(vertex(face));
        start.push(vertex(face + 1));
        let tangent = Vec2::new(-normal.y, normal.x);
        let low = tangent.dot(self.reference.vertex(self.edge));
        let high = tangent.dot(self.reference.vertex(self.edge + 1));
        let id = |side: usize| FeatureId::pack(self.kind(KIND_CLIPPED), self.edge, face * 2 + side);
        start
            .clipped(tangent, low, id(0))
            .clipped(Vec2::new(-tangent.x, -tangent.y), -high, id(1))
    }

    /// The contact point of a point of the clipped incident face: moved half way to the reference
    /// face; `None` when it is further than `margin` from it.
    fn contact_point(&self, point: &ClipPoint, margin: f32) -> Option<LocalPoint> {
        let normal = self.reference.normals[self.edge];
        let separation = normal.dot(point.position - self.reference.vertex(self.edge));
        (!separated(separation, margin)).then(|| LocalPoint {
            position: point.position - normal * (0.5 * separation),
            separation,
            id: point.id,
        })
    }

    /// The contact of the pair seen from the first polygon.
    fn contact(&self, margin: f32) -> Option<Local> {
        let normal = self.reference.normals[self.edge];
        let normal = if self.flipped {
            Vec2::new(-normal.x, -normal.y)
        } else {
            normal
        };
        let clipped = self.clipped_incident();
        let points = clipped
            .points()
            .iter()
            .filter_map(|point| self.contact_point(point, margin));
        let local = Local::from_points(normal, points);
        (local.count > 0).then_some(local)
    }
}

/// The contact of polygon `a` (first) and polygon `b` by the separating axis test over the faces
/// of both: no contact when a face separates them by more than `margin`; otherwise the face of
/// greatest separation (the least deep) is the reference face, the most opposed face of the other
/// polygon is cut to its sides, and the points within `margin` of the reference face are kept.
pub(super) fn collide_polygons(a: &Poly, b: &Poly, margin: f32) -> Option<Local> {
    let from_a = max_separation(a, b);
    if separated(from_a.value, margin) {
        return None;
    }
    let from_b = max_separation(b, a);
    if separated(from_b.value, margin) {
        return None;
    }
    let flipped = from_b.value > from_a.value + REFERENCE_FACE_BIAS;
    let faces = if flipped {
        Faces {
            reference: b,
            incident: a,
            edge: from_b.edge,
            flipped,
        }
    } else {
        Faces {
            reference: a,
            incident: b,
            edge: from_a.edge,
            flipped,
        }
    };
    faces.contact(margin)
}

/// A circle against a polygon.
pub(super) struct CircleQuery<'a> {
    pub(super) polygon: &'a Poly,
    pub(super) radius: f32,
    pub(super) centre: Vec2,
    pub(super) margin: f32,
}

/// Where the circle's centre lies relative to the nearest face.
enum Region {
    Face,
    Vertex(usize),
}

impl CircleQuery<'_> {
    /// The face with the largest signed distance to the centre (the first wins a tie) and that
    /// distance: positive outside the polygon, zero or negative inside.
    fn nearest_face(&self) -> Separation {
        let faces = self
            .polygon
            .face_normals()
            .iter()
            .zip(self.polygon.vertex_points());
        best_face(faces.map(|(normal, from)| normal.dot(self.centre - *from)))
    }

    /// Whether the centre, outside the polygon, sees the face or one of its two end vertices.
    fn region(&self, edge: usize) -> Region {
        let (start, end) = (self.polygon.vertex(edge), self.polygon.vertex(edge + 1));
        if (self.centre - start).dot(end - start) < 0.0 {
            Region::Vertex(edge)
        } else if (self.centre - end).dot(start - end) < 0.0 {
            Region::Vertex((edge + 1) % self.polygon.count)
        } else {
            Region::Face
        }
    }

    fn face_contact(&self, face: &Separation) -> Local {
        let normal = self.polygon.normals[face.edge];
        let separation = face.value - self.radius;
        let position = self.centre - normal * (0.5 * (face.value + self.radius));
        let id = FeatureId::pack(KIND_CIRCLE_FACE, face.edge, 0);
        Local::new(
            normal,
            &[LocalPoint {
                position,
                separation,
                id,
            }],
        )
    }

    fn vertex_contact(&self, face: &Separation, vertex: usize) -> Option<Local> {
        let corner = self.polygon.vertex(vertex);
        let offset = self.centre - corner;
        let length_squared = offset.length_squared();
        if length_squared <= MIN_LENGTH_SQUARED {
            return Some(self.face_contact(face));
        }
        let length = length_squared.sqrt();
        let separation = length - self.radius;
        if separated(separation, self.margin) {
            return None;
        }
        let normal = Vec2::new(offset.x / length, offset.y / length);
        let on_circle = self.centre - normal * self.radius;
        let id = FeatureId::pack(KIND_CIRCLE_VERTEX, vertex, 0);
        Some(Local::new(
            normal,
            &[LocalPoint {
                position: (corner + on_circle) * 0.5,
                separation,
                id,
            }],
        ))
    }

    /// The contact, or `None` when the circle is further than the margin from the polygon.
    pub(super) fn collide(&self) -> Option<Local> {
        let face = self.nearest_face();
        if separated(face.value - self.radius, self.margin) {
            return None;
        }
        let region = if face.value > 0.0 {
            self.region(face.edge)
        } else {
            Region::Face
        };
        match region {
            Region::Vertex(vertex) => self.vertex_contact(&face, vertex),
            Region::Face => Some(self.face_contact(&face)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Must;

    const HALF: Vec2 = Vec2::new(1.0, 0.5);

    fn unit_box() -> Poly {
        Poly::from_box(Vec2::new(1.0, 1.0), Rot2::IDENTITY, Vec2::ZERO)
    }

    fn box_at(x: f32, y: f32) -> Poly {
        Poly::from_box(Vec2::new(1.0, 1.0), Rot2::IDENTITY, Vec2::new(x, y))
    }

    fn id_of(kind: u8, first: usize, second: usize) -> FeatureId {
        FeatureId::pack(kind, first, second)
    }

    #[test]
    fn a_box_has_counter_clockwise_corners_and_outward_unit_normals() {
        let poly = Poly::from_box(HALF, Rot2::IDENTITY, Vec2::new(10.0, 20.0));
        assert_eq!(poly.count, 4);
        assert_eq!(
            poly.vertex_points(),
            &[
                Vec2::new(9.0, 19.5),
                Vec2::new(11.0, 19.5),
                Vec2::new(11.0, 20.5),
                Vec2::new(9.0, 20.5)
            ]
        );
        let normals = [
            Vec2::new(0.0, -1.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(-1.0, 0.0),
        ];
        assert_eq!(poly.face_normals(), &normals);
    }

    #[test]
    fn a_turned_box_turns_corners_and_normals_alike() {
        let quarter = Rot2::from_cos_sin(0.0, 1.0).unwrap_or(Rot2::IDENTITY);
        let poly = Poly::from_box(HALF, quarter, Vec2::ZERO);
        // The corner (-1, -0.5) turned by 90 degrees is (0.5, -1); the normal (0, -1) is (1, 0).
        assert_eq!(poly.vertices[0], Vec2::new(0.5, -1.0));
        assert_eq!(poly.normals[0], Vec2::new(1.0, 0.0));
    }

    #[test]
    fn the_vertex_index_wraps_at_the_polygon_size() {
        let poly = unit_box();
        assert_eq!(poly.vertex(4), poly.vertex(0));
        assert_eq!(poly.vertex(5), poly.vertex(1));
    }

    #[test]
    fn a_general_polygon_gets_unit_outward_normals() {
        let triangle = Poly::from_vertices(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(0.0, 2.0),
        ]);
        assert_eq!(triangle.count, 3);
        assert_eq!(triangle.normals[0], Vec2::new(0.0, -1.0));
        assert_eq!(triangle.normals[2], Vec2::new(-1.0, 0.0));
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (triangle.normals[1].x - s).abs() < 1e-6 && (triangle.normals[1].y - s).abs() < 1e-6
        );
    }

    #[test]
    fn separated_means_beyond_the_limit_or_not_a_number() {
        assert!(separated(0.1, 0.0));
        assert!(!separated(0.0, 0.0));
        assert!(!separated(-1.0, 0.0));
        assert!(separated(f32::NAN, 0.0));
        assert!(separated(f32::INFINITY, 1e30));
    }

    #[test]
    fn the_best_face_is_the_one_with_the_largest_gap_and_the_first_wins_a_tie() {
        let a = unit_box();
        // Gap 0.5 along +x (face 1).
        let right = max_separation(&a, &box_at(2.5, 0.0));
        assert_eq!((right.edge, right.value), (1, 0.5));
        // Overlap of 0.5 along +x and 1.5 along y: the shallower face still wins.
        let overlap = max_separation(&a, &box_at(1.5, 0.0));
        assert_eq!((overlap.edge, overlap.value), (1, -0.5));
        // Equal depth along x and y: face 1 (the +x face) comes before face 2 (the +y face).
        let tie = max_separation(&a, &box_at(1.5, 1.5));
        assert_eq!((tie.edge, tie.value), (1, -0.5));
        // The same for a gap on the lowest face.
        let below = max_separation(&a, &box_at(0.0, -2.5));
        assert_eq!((below.edge, below.value), (0, 0.5));
    }

    #[test]
    fn a_polygon_that_is_not_a_number_is_refused() {
        let a = unit_box();
        let sick = box_at(f32::NAN, 0.0);
        assert!(separated(max_separation(&a, &sick).value, 0.1));
        assert!(separated(max_separation(&sick, &a).value, 0.1));
        assert!(collide_polygons(&a, &sick, 0.1).is_none());
        assert!(collide_polygons(&sick, &a, 0.1).is_none());
    }

    #[test]
    fn the_incident_face_is_the_most_opposed_one_and_the_first_wins_a_tie() {
        let b = unit_box();
        assert_eq!(incident_face(&b, Vec2::new(0.0, 1.0)), 0);
        assert_eq!(incident_face(&b, Vec2::new(0.0, -1.0)), 2);
        assert_eq!(incident_face(&b, Vec2::new(-1.0, 0.0)), 1);
        assert_eq!(incident_face(&b, Vec2::new(1.0, 0.0)), 3);
        // Exactly between two faces: the lower index.
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq!(incident_face(&b, Vec2::new(-s, s)), 0);
    }

    #[test]
    fn a_segment_is_cut_at_a_plane_and_the_crossing_point_gets_the_given_id() {
        let point = |x: f32, id: u16| ClipPoint {
            position: Vec2::new(x, 0.0),
            id: FeatureId::pack(0, 0, usize::from(id)),
        };
        let mut seg = Segment {
            points: [point(0.0, 0); 2],
            count: 0,
        };
        seg.push(point(-1.0, 1));
        seg.push(point(3.0, 2));
        seg.push(point(9.0, 3)); // a third point is ignored
        assert_eq!(seg.points().len(), 2);
        let cut_id = id_of(0, 0, 9);
        // Keep x >= 1: the second point and the crossing at x = 1 (a quarter of the way).
        let cut = seg.clipped(Vec2::X, 1.0, cut_id);
        let kept: Vec<(f32, FeatureId)> =
            cut.points().iter().map(|p| (p.position.x, p.id)).collect();
        assert_eq!(kept, vec![(3.0, id_of(0, 0, 2)), (1.0, cut_id)]);
        // Keep x <= 1 (axis -x): the first point and the same crossing.
        let other = seg.clipped(Vec2::new(-1.0, 0.0), -1.0, cut_id);
        let kept: Vec<(f32, FeatureId)> = other
            .points()
            .iter()
            .map(|p| (p.position.x, p.id))
            .collect();
        assert_eq!(kept, vec![(-1.0, id_of(0, 0, 1)), (1.0, cut_id)]);
        // Both inside, both outside, and a point exactly on the plane.
        assert_eq!(seg.clipped(Vec2::X, -5.0, cut_id).points().len(), 2);
        assert_eq!(seg.clipped(Vec2::X, 5.0, cut_id).points().len(), 0);
        let touching = seg.clipped(Vec2::X, 3.0, cut_id);
        assert_eq!(touching.points().len(), 1);
        assert_eq!(touching.points()[0].position.x.to_bits(), 3.0_f32.to_bits());
        // A single point is kept or dropped whole.
        let mut single = Segment {
            points: [point(0.0, 0); 2],
            count: 0,
        };
        single.push(point(2.0, 5));
        assert_eq!(single.clipped(Vec2::X, 1.0, cut_id).points().len(), 1);
        assert_eq!(single.clipped(Vec2::X, 3.0, cut_id).points().len(), 0);
    }

    #[test]
    fn two_boxes_resting_overlapped_give_two_points_on_the_reference_face() {
        let a = unit_box();
        let b = box_at(0.0, 1.9);
        let local = collide_polygons(&a, &b, 0.0).must("overlapping boxes touch");
        assert_eq!(local.normal, Vec2::new(0.0, 1.0));
        assert_eq!(local.points().len(), 2);
        for point in local.points() {
            assert!((point.separation + 0.1).abs() < 1e-6, "{point:?}");
            // Half way between the top of A (y = 1) and the bottom of B (y = 0.9).
            assert!((point.position.y - 0.95).abs() < 1e-6, "{point:?}");
        }
        let xs: Vec<f32> = local.points().iter().map(|p| p.position.x).collect();
        assert_eq!(xs, vec![-1.0, 1.0]);
    }

    #[test]
    fn the_second_box_is_the_reference_only_when_clearly_better() {
        let a = unit_box();
        // B is turned by a small angle; A's face is the reference unless B's is better by the bias.
        let b = box_at(0.0, 1.9);
        let local = collide_polygons(&a, &b, 0.0).must("contact");
        assert_eq!(local.points()[0].id, id_of(KIND_INCIDENT_VERTEX, 2, 0));
        // Swapped: the roles of the two boxes are exchanged, and B's reference face is "flipped".
        let swapped = collide_polygons(&b, &a, 0.0).must("contact");
        assert_eq!(swapped.normal, Vec2::new(0.0, -1.0));
        assert_eq!(swapped.points().len(), 2);
        // Move A's face deeper than B's by more than the bias: a box rotated so that B's face wins.
        let turned = Poly::from_box(
            Vec2::new(1.0, 1.0),
            Rot2::from_cos_sin(1.0, 0.02).unwrap_or(Rot2::IDENTITY),
            Vec2::new(0.0, 1.95),
        );
        let local = collide_polygons(&a, &turned, 0.0).must("contact");
        assert!(local.normal.y > 0.99);
    }

    #[test]
    fn a_face_contact_is_cut_to_the_sides_of_the_reference_face() {
        let a = unit_box();
        // B sits half off the right end of A: its lower-left corner is over A, the right one not.
        let b = box_at(1.5, 1.9);
        let local = collide_polygons(&a, &b, 0.0).must("contact");
        assert_eq!(local.normal, Vec2::new(0.0, 1.0));
        assert_eq!(local.points().len(), 2);
        let first = local.points()[0];
        let second = local.points()[1];
        // The surviving corner (0.5, 0.9) and the point where B's bottom edge meets the end of A's face.
        assert_eq!(first.position.x.to_bits(), 0.5_f32.to_bits());
        assert_eq!(first.id, id_of(KIND_INCIDENT_VERTEX, 2, 0));
        assert_eq!(second.position.x.to_bits(), 1.0_f32.to_bits());
        // Side 0 is the start vertex of the reference edge (here the right end of A's top face).
        assert_eq!(second.id, id_of(KIND_CLIPPED, 2, 0));
    }

    #[test]
    fn a_box_over_the_corner_of_another_gives_one_point_when_only_it_is_close() {
        let a = unit_box();
        // A box turned by 45 degrees stands on its corner on top of A.
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let diamond = Poly::from_box(
            Vec2::new(1.0, 1.0),
            Rot2::from_cos_sin(s, s).unwrap_or(Rot2::IDENTITY),
            Vec2::new(0.0, 1.0 + std::f32::consts::SQRT_2 - 0.05),
        );
        let local = collide_polygons(&a, &diamond, 0.0).must("contact");
        assert_eq!(local.points().len(), 1);
        assert!((local.points()[0].separation + 0.05).abs() < 1e-5);
        // With a margin that covers the corners of the neighbouring edge, both are kept.
        let wide = collide_polygons(&a, &diamond, 3.0).must("contact");
        assert_eq!(wide.points().len(), 2);
    }

    #[test]
    fn boxes_further_apart_than_the_margin_have_no_contact_and_within_it_a_speculative_one() {
        let a = unit_box();
        let b = box_at(0.0, 2.05);
        assert!(collide_polygons(&a, &b, 0.0).is_none());
        assert!(collide_polygons(&a, &b, 0.04).is_none());
        let local = collide_polygons(&a, &b, 0.06).must("within the margin");
        assert!((local.points()[0].separation - 0.05).abs() < 1e-6);
        // Separated along the second polygon's face only.
        assert!(collide_polygons(&b, &a, 0.04).is_none());
        // Exactly touching is a contact even with no margin.
        let touching = collide_polygons(&a, &box_at(0.0, 2.0), 0.0).must("touching");
        assert_eq!(touching.points()[0].separation.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn a_hexagon_and_a_box_collide_like_any_convex_polygons() {
        let hexagon: Vec<Vec2> = [
            (1.0, 0.0),
            (0.5, 0.866_025_4),
            (-0.5, 0.866_025_4),
            (-1.0, 0.0),
            (-0.5, -0.866_025_4),
            (0.5, -0.866_025_4),
        ]
        .iter()
        .map(|&(x, y)| Vec2::new(x, y))
        .collect();
        let hex = Poly::from_vertices(&hexagon);
        // A box resting on the flat top of the hexagon, overlapping by 0.1.
        let top = box_at(0.0, 0.866_025_4 + 0.9);
        let local = collide_polygons(&hex, &top, 0.0).must("contact");
        assert!(local.normal.y > 0.999);
        assert_eq!(local.points().len(), 2);
        assert!((local.points()[0].separation + 0.1).abs() < 1e-5);
        // The id carries the hexagon's face index (2, the top edge).
        assert_eq!(local.points()[0].id.raw() / 16 % 16, 1);
    }

    #[test]
    fn two_tiny_distances_on_opposite_sides_still_cross_the_plane() {
        let at = |x: f32| ClipPoint {
            position: Vec2::new(x, 0.0),
            id: FeatureId::default(),
        };
        let id = id_of(KIND_CLIPPED, 1, 0);
        let cut = crossing(&at(-1.0), &at(1.0), [-1.0e-30, 1.0e-30], id).must("crossing");
        assert!(cut.position.x.abs() < 1e-6);
        assert_eq!(cut.id, id);
        assert!(crossing(&at(-1.0), &at(1.0), [1.0e-30, 1.0e-30], id).is_none());
        // A point exactly on the plane is not a crossing: it is kept as it is.
        for distances in [[0.0, 1.0], [1.0, 0.0], [0.0, -1.0], [-1.0, 0.0]] {
            assert!(crossing(&at(-1.0), &at(1.0), distances, id).is_none());
        }
    }

    #[test]
    fn a_centre_exactly_above_an_end_of_a_face_is_still_in_the_face_region() {
        let poly = unit_box();
        for x in [1.0, -1.0] {
            let q = CircleQuery {
                polygon: &poly,
                radius: 0.75,
                centre: Vec2::new(x, 1.5),
                margin: 0.0,
            };
            let local = q.collide().must("overlap");
            assert_eq!(local.normal, Vec2::new(0.0, 1.0));
            assert_eq!(local.points()[0].id, id_of(KIND_CIRCLE_FACE, 2, 0), "x {x}");
        }
    }

    #[test]
    fn a_circle_outside_a_face_touches_it_at_the_midpoint_of_the_gap() {
        let poly = unit_box();
        let q = CircleQuery {
            polygon: &poly,
            radius: 0.5,
            centre: Vec2::new(0.3, 1.45),
            margin: 0.0,
        };
        let local = q.collide().must("overlap");
        assert_eq!(local.normal, Vec2::new(0.0, 1.0));
        let p = local.points()[0];
        assert!((p.separation + 0.05).abs() < 1e-6);
        assert!((p.position.y - 0.975).abs() < 1e-6 && (p.position.x - 0.3).abs() < 1e-6);
        assert_eq!(p.id, id_of(KIND_CIRCLE_FACE, 2, 0));
    }

    #[test]
    fn a_circle_near_a_corner_touches_the_corner_along_the_line_of_centres() {
        let poly = unit_box();
        let q = CircleQuery {
            polygon: &poly,
            radius: 0.5,
            centre: Vec2::new(1.3, 1.4),
            margin: 0.0,
        };
        let local = q.collide().must("overlap");
        let length = (0.3_f32 * 0.3 + 0.4 * 0.4).sqrt();
        assert!((local.normal.x - 0.3 / length).abs() < 1e-6);
        assert!((local.normal.y - 0.4 / length).abs() < 1e-6);
        let p = local.points()[0];
        assert!((p.separation - (length - 0.5)).abs() < 1e-6);
        assert_eq!(p.id, id_of(KIND_CIRCLE_VERTEX, 2, 0));
        // The other end of the same face, and the other faces' corners.
        for (x, y, vertex) in [(-1.3, 1.4, 3), (1.3, -1.4, 1), (-1.3, -1.4, 0)] {
            let q = CircleQuery {
                centre: Vec2::new(x, y),
                ..q
            };
            let local = q.collide().must("overlap");
            assert_eq!(local.points()[0].id, id_of(KIND_CIRCLE_VERTEX, vertex, 0));
        }
    }

    #[test]
    fn a_circle_beyond_the_margin_has_no_contact_face_or_corner() {
        let poly = unit_box();
        let query = |x: f32, y: f32, margin: f32| CircleQuery {
            polygon: &poly,
            radius: 0.5,
            centre: Vec2::new(x, y),
            margin,
        };
        assert!(query(0.0, 1.6, 0.0).collide().is_none());
        assert!(query(0.0, 1.6, 0.09).collide().is_none());
        assert!(query(0.0, 1.6, 0.11).collide().is_some());
        // Near the corner the face distance is within the margin but the corner distance is not.
        assert!(query(1.4, 1.4, 0.0).collide().is_none());
        assert!(query(1.4, 1.4, 0.05).collide().is_none());
        assert!(query(1.4, 1.4, 0.07).collide().is_some());
        assert!(query(f32::NAN, 0.0, 0.0).collide().is_none());
    }

    #[test]
    fn a_circle_with_its_centre_inside_is_pushed_out_through_the_nearest_face() {
        let poly = unit_box();
        let q = CircleQuery {
            polygon: &poly,
            radius: 0.25,
            centre: Vec2::new(0.9, 0.2),
            margin: 0.0,
        };
        let local = q.collide().must("overlap");
        assert_eq!(local.normal, Vec2::new(1.0, 0.0));
        let p = local.points()[0];
        // The centre is 0.1 inside the right face: depth 0.1 + 0.25.
        assert!((p.separation + 0.35).abs() < 1e-6);
        assert_eq!(p.id, id_of(KIND_CIRCLE_FACE, 1, 0));
        // At the very centre of the box all four faces tie: the first face wins.
        let centred = CircleQuery {
            centre: Vec2::ZERO,
            ..q
        };
        assert_eq!(
            centred.collide().must("overlap").normal,
            Vec2::new(0.0, -1.0)
        );
        // A centre exactly on a face is inside (separation zero is not outside).
        let on_face = CircleQuery {
            centre: Vec2::new(0.0, 1.0),
            ..q
        };
        let local = on_face.collide().must("overlap");
        assert_eq!(local.normal, Vec2::new(0.0, 1.0));
        assert!((local.points()[0].separation + 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_circle_on_the_line_of_a_face_beyond_its_end_sees_the_corner() {
        let poly = unit_box();
        // Past the top right corner, level with it within the right face's reach: the corner (1, 1) is the nearest.
        let q = CircleQuery {
            polygon: &poly,
            radius: 0.5,
            centre: Vec2::new(1.25, 1.1),
            margin: 0.0,
        };
        let local = q.collide().must("overlap");
        assert_eq!(local.points()[0].id, id_of(KIND_CIRCLE_VERTEX, 2, 0));
        assert!(local.normal.x > 0.9 && local.normal.y > 0.3);
    }

    #[test]
    fn a_circle_on_a_corner_to_the_last_digit_gets_the_face_normal() {
        let poly = unit_box();
        // The centre is on the corner (1, 1) bit for bit: both faces at the corner have separation
        // zero, the first one wins and the centre counts as inside.
        let q = CircleQuery {
            polygon: &poly,
            radius: 0.5,
            centre: Vec2::new(1.0, 1.0),
            margin: 0.0,
        };
        let local = q.collide().must("overlap");
        assert_eq!(local.normal, Vec2::new(1.0, 0.0));
        // The corner contact itself has no direction when the offset is zero or too small to take
        // a square root from: it falls back to the face contact it was given.
        let face = Separation {
            value: 0.5,
            edge: 2,
        };
        for centre in [Vec2::new(1.0, 1.0), Vec2::new(1.0 + 1.0e-20, 1.0)] {
            let at_corner = CircleQuery { centre, ..q };
            let local = at_corner.vertex_contact(&face, 2).must("contact");
            assert_eq!(local.normal, Vec2::new(0.0, 1.0));
            assert_eq!(local.points()[0].id, id_of(KIND_CIRCLE_FACE, 2, 0));
        }
        // A real offset gives the corner contact.
        let away = CircleQuery {
            centre: Vec2::new(1.0, 1.25),
            ..q
        };
        let local = away.vertex_contact(&face, 2).must("contact");
        assert_eq!(local.points()[0].id, id_of(KIND_CIRCLE_VERTEX, 2, 0));
    }
}
