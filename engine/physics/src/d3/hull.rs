//! A convex hull: the corners of the smallest convex solid around a set of points, stored in the
//! frame of the solid's centre of mass and principal axes.
//!
//! That frame is what keeps the inertia a plain vector of three moments (card 8-3): the points given
//! are moved to the centre of mass and turned to the axes the inertia tensor is diagonal in, and the
//! hull remembers how, so a caller can put the body where the points were
//! (`input = origin + orientation * vertex`).
//!
//! The hull is found by brute force, which is fine for at most [`MAX_INPUT_POINTS`] points and runs
//! once, when the shape is made: every plane through three points that has all the points on one
//! side is a plane of a face; a point on three or more of those planes is a corner. Only `+ - * /`
//! and `sqrt` are used, in a fixed order.

use crate::core::{HashBits, PhysicsError, Quantity, StateHasher};
use base::math::{Quat, Vec3};

/// The most corners a hull may have. A hull is stored inline in [`Shape3`](super::Shape3), which
/// must be `Copy`, so the limit is also a size: about 400 bytes.
pub const MAX_HULL_VERTICES: usize = 32;
/// The most points that may be given; the others are inside, on a face or on an edge.
const MAX_INPUT_POINTS: usize = 64;
/// Planes of faces: a convex solid with 32 corners has at most 60 faces.
const MAX_PLANES: usize = 2 * MAX_HULL_VERTICES;
/// How far from a plane, as a fraction of the size of the point set, a point still lies on it.
const PLANE_TOLERANCE: f32 = 1.0e-5;
/// The smallest area, as a fraction of the size squared, of three points that give a plane.
const MIN_AREA: f32 = 1.0e-6;
/// Sweeps of the Jacobi method; three or four are enough, the rest is margin.
const JACOBI_SWEEPS: usize = 12;
/// How far the faces' area vectors may be from cancelling, as a share of the total area. A larger
/// sum means two faces overlap or a face is missing, and the volume would be wrong by about as
/// much: the points are then refused instead.
const CLOSED_SURFACE: f32 = 1.0e-3;
/// An off-diagonal term below this (relative to the matrix) is zero.
const JACOBI_EPSILON: f32 = 1.0e-12;

type Matrix = [[f32; 3]; 3];

/// A convex hull in its own principal frame. Made by [`Hull::new`] only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hull {
    count: usize,
    vertices: [Vec3; MAX_HULL_VERTICES],
    origin: Vec3,
    orientation: Quat,
    volume: f32,
    moments: Vec3,
}

impl Hull {
    /// The hull of `points`. The points that are not corners of it are dropped.
    ///
    /// # Errors
    ///
    /// [`PhysicsError::NotFinite`] when a coordinate is NaN or infinite; [`PhysicsError::InvalidShape`]
    /// when there are too many points, when they do not enclose a volume (fewer than four, or all
    /// in one plane, line or place), or when the hull has more than [`MAX_HULL_VERTICES`] corners.
    pub fn new(points: &[Vec3]) -> Result<Self, PhysicsError> {
        if points.iter().any(|p| !p.is_finite()) {
            return Err(PhysicsError::NotFinite);
        }
        if points.len() < 4 || points.len() > MAX_INPUT_POINTS {
            return Err(PhysicsError::InvalidShape);
        }
        // Work about the centre of the points' box: the plane tests subtract nearly equal numbers,
        // so their error grows with the distance from the origin and not with the size of the hull.
        let anchor = box_centre(points);
        let mut shifted = [Vec3::ZERO; MAX_INPUT_POINTS];
        for (slot, point) in shifted.iter_mut().zip(points) {
            *slot = *point - anchor;
        }
        let solid = Solid::of(&shifted[..points.len()])?;
        solid.into_hull(anchor)
    }

    /// The corners, in the principal frame, the centre of mass at the origin.
    #[must_use]
    pub fn vertices(&self) -> &[Vec3] {
        &self.vertices[..self.count]
    }

    /// Where the centre of mass was in the frame the points were given in.
    #[must_use]
    pub fn origin(&self) -> Vec3 {
        self.origin
    }

    /// The turn from the principal frame to the frame the points were given in.
    #[must_use]
    pub fn orientation(&self) -> Quat {
        self.orientation
    }

    /// Calls `visit` with the two ends of each edge of the hull (in the principal frame), once each.
    /// Works the faces out again, so it is for drawing, not for every step.
    pub(super) fn for_each_edge(&self, mut visit: impl FnMut(Vec3, Vec3)) {
        let Ok(solid) = Solid::of(self.vertices()) else {
            return;
        };
        for plane in &solid.planes[..solid.plane_count] {
            let mut polygon = [Vec3::ZERO; MAX_HULL_VERTICES];
            let len = solid.face_polygon(plane, &mut polygon);
            report_edges(&polygon[..len], &mut visit);
        }
    }

    /// Volume, and the three principal moments of inertia at density one.
    pub(super) fn volume_and_moments(&self) -> (f32, Vec3) {
        (self.volume, self.moments)
    }

    /// Whether this hull can be a body's shape: it is made valid, but a shape is `Copy` data and the
    /// check costs nothing.
    pub(super) fn is_valid(&self) -> bool {
        let positive = |x: f32| x.is_finite() && x > 0.0;
        (4..=MAX_HULL_VERTICES).contains(&self.count)
            && self.vertices().iter().all(Quantity::is_finite)
            && positive(self.volume)
            && positive(self.moments.x)
            && positive(self.moments.y)
            && positive(self.moments.z)
    }
}

/// The corners decide the hull: origin, orientation, volume and moments are worked out from them,
/// so hashing the corners is enough to tell two hulls apart.
impl HashBits for Hull {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_u8(u8::try_from(self.count).unwrap_or(u8::MAX));
        for vertex in self.vertices() {
            vertex.write_bits(hasher);
        }
    }
}

/// A face plane: the unit normal pointing out of the solid and the distance along it.
#[derive(Clone, Copy)]
struct Plane {
    normal: Vec3,
    offset: f32,
}

impl Plane {
    fn gap(&self, point: Vec3) -> f32 {
        self.normal.dot(point) - self.offset
    }
}

/// The corners of the solid and its face planes, still in the frame the points were given in.
struct Solid {
    corners: [Vec3; MAX_HULL_VERTICES],
    count: usize,
    planes: [Plane; MAX_PLANES],
    plane_count: usize,
    tolerance: f32,
}

/// Whether the points are not all on one line (within `reach`).
fn spans_a_plane(points: &[Vec3], reach: f32) -> bool {
    let Some(first) = points.first().copied() else {
        return false;
    };
    let far = points.iter().copied().max_by(|a, b| {
        (*a - first)
            .length_squared()
            .total_cmp(&(*b - first).length_squared())
    });
    let Some(along) = far.and_then(|f| (f - first).normalized()) else {
        return false;
    };
    points.iter().any(|p| {
        let offset = *p - first;
        (offset - along * offset.dot(along)).length() > reach
    })
}

/// The centre of the smallest axis-aligned box that holds `points`.
fn box_centre(points: &[Vec3]) -> Vec3 {
    let (low, high) = points.iter().fold(
        (
            Vec3::new(f32::MAX, f32::MAX, f32::MAX),
            Vec3::new(f32::MIN, f32::MIN, f32::MIN),
        ),
        |(low, high), p| {
            (
                Vec3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z)),
                Vec3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z)),
            )
        },
    );
    (low + high) * 0.5
}

fn size_of(points: &[Vec3]) -> f32 {
    let first = points[0];
    points
        .iter()
        .map(|p| {
            let d = *p - first;
            d.x.abs().max(d.y.abs()).max(d.z.abs())
        })
        .fold(0.0, f32::max)
}

impl Solid {
    fn of(points: &[Vec3]) -> Result<Self, PhysicsError> {
        let size = size_of(points);
        if size <= 0.0 {
            return Err(PhysicsError::InvalidShape);
        }
        let mut solid = Self {
            corners: [Vec3::ZERO; MAX_HULL_VERTICES],
            count: 0,
            planes: [Plane {
                normal: Vec3::X,
                offset: 0.0,
            }; MAX_PLANES],
            plane_count: 0,
            tolerance: PLANE_TOLERANCE * size,
        };
        solid.find_planes(points, size)?;
        solid.find_corners(points)?;
        Ok(solid)
    }

    /// The plane through the three points, with the normal pointing out of the solid when all the
    /// points are on one side of it. `Err` when all the points are on it (the solid is flat).
    fn face_through(
        &self,
        points: &[Vec3],
        triple: (usize, usize, usize),
        size: f32,
    ) -> Result<Option<Plane>, PhysicsError> {
        let (i, j, k) = triple;
        let cross = (points[j] - points[i]).cross(points[k] - points[i]);
        let Some(normal) = cross
            .normalized()
            .filter(|_| cross.length() > MIN_AREA * size * size)
        else {
            return Ok(None);
        };
        let offset = normal.dot(points[i]);
        let (mut lowest, mut highest) = (0.0_f32, 0.0_f32);
        for p in points {
            let gap = normal.dot(*p) - offset;
            lowest = lowest.min(gap);
            highest = highest.max(gap);
        }
        let tolerance = self.tolerance;
        if highest <= tolerance && lowest >= -tolerance {
            return Err(PhysicsError::InvalidShape);
        }
        Ok(if highest <= tolerance {
            Some(Plane { normal, offset })
        } else if lowest >= -tolerance {
            Some(Plane {
                normal: -normal,
                offset: -offset,
            })
        } else {
            None
        })
    }

    /// The points within reach of both planes, in `out`; how many.
    fn shared_points(
        &self,
        planes: (&Plane, &Plane),
        points: &[Vec3],
        out: &mut [Vec3; MAX_INPUT_POINTS],
    ) -> usize {
        let reach = 2.0 * self.tolerance;
        let mut count = 0;
        for point in points
            .iter()
            .filter(|p| planes.0.gap(**p).abs() <= reach && planes.1.gap(**p).abs() <= reach)
        {
            out[count] = *point;
            count += 1;
        }
        count
    }

    /// Two planes are the same face when they face the same way and share points that are not all
    /// on one line: the faces of a solid meet along an edge, a line, and never share more. (Comparing
    /// angles would merge the neighbouring faces of a thin plate; comparing the points on each plane
    /// would keep two planes that overlap on part of a face and count that part twice.)
    fn same_face(&self, planes: (&Plane, &Plane), points: &[Vec3]) -> bool {
        if planes.0.normal.dot(planes.1.normal) <= 0.0 {
            return false;
        }
        let mut shared = [Vec3::ZERO; MAX_INPUT_POINTS];
        let count = self.shared_points(planes, points, &mut shared);
        count >= 3 && spans_a_plane(&shared[..count], 2.0 * self.tolerance)
    }

    /// How many of `points` are within reach of `plane`.
    fn members(&self, plane: &Plane, points: &[Vec3]) -> usize {
        let reach = 2.0 * self.tolerance;
        points
            .iter()
            .filter(|p| plane.gap(**p).abs() <= reach)
            .count()
    }

    /// Puts `plane` in place of the known plane `index` when more points lie on it.
    fn keep_the_fuller(&mut self, index: usize, plane: Plane, points: &[Vec3]) {
        if self.members(&plane, points) > self.members(&self.planes[index], points) {
            self.planes[index] = plane;
        }
    }

    /// Keeps `plane` unless it is a face already known; of two planes for one face, the one with
    /// more points on it is kept.
    fn add_plane(&mut self, plane: Plane, points: &[Vec3]) -> Result<(), PhysicsError> {
        let same =
            (0..self.plane_count).find(|i| self.same_face((&self.planes[*i], &plane), points));
        if let Some(index) = same {
            self.keep_the_fuller(index, plane, points);
            return Ok(());
        }
        if self.plane_count == MAX_PLANES {
            return Err(PhysicsError::InvalidShape);
        }
        self.planes[self.plane_count] = plane;
        self.plane_count += 1;
        Ok(())
    }

    /// Adds the plane through the three points when it is a face plane of the solid.
    fn consider_triple(
        &mut self,
        points: &[Vec3],
        triple: (usize, usize, usize),
        size: f32,
    ) -> Result<(), PhysicsError> {
        match self.face_through(points, triple, size)? {
            Some(plane) => self.add_plane(plane, points),
            None => Ok(()),
        }
    }

    fn find_planes(&mut self, points: &[Vec3], size: f32) -> Result<(), PhysicsError> {
        for triple in triples(points.len()) {
            self.consider_triple(points, triple, size)?;
        }
        if self.plane_count < 4 {
            return Err(PhysicsError::InvalidShape);
        }
        Ok(())
    }

    /// Whether `point` is on three or more face planes (a corner) and is not one already found.
    fn is_new_corner(&self, point: Vec3) -> bool {
        let reach = 2.0 * self.tolerance;
        let on = self.planes[..self.plane_count]
            .iter()
            .filter(|plane| plane.gap(point).abs() <= reach)
            .count();
        let seen = self.corners[..self.count]
            .iter()
            .any(|c| (*c - point).length() <= reach);
        on >= 3 && !seen
    }

    fn add_corner(&mut self, point: Vec3) -> Result<(), PhysicsError> {
        if self.count == MAX_HULL_VERTICES {
            return Err(PhysicsError::InvalidShape);
        }
        self.corners[self.count] = point;
        self.count += 1;
        Ok(())
    }

    fn consider_corner(&mut self, point: Vec3) -> Result<(), PhysicsError> {
        if self.is_new_corner(point) {
            self.add_corner(point)
        } else {
            Ok(())
        }
    }

    /// A point on three or more face planes is a corner of the solid; one on two is on an edge and
    /// one on one is inside a face.
    fn find_corners(&mut self, points: &[Vec3]) -> Result<(), PhysicsError> {
        for point in points {
            self.consider_corner(*point)?;
        }
        if self.count < 4 {
            return Err(PhysicsError::InvalidShape);
        }
        Ok(())
    }

    fn mean(&self) -> Vec3 {
        let sum = self.corners[..self.count]
            .iter()
            .fold(Vec3::ZERO, |total, c| total + *c);
        sum * (1.0 / small_count(self.count))
    }

    /// The corners of the face on `plane`, in order round it as seen from outside.
    fn face_polygon(&self, plane: &Plane, out: &mut [Vec3; MAX_HULL_VERTICES]) -> usize {
        let reach = 2.0 * self.tolerance;
        let on_plane = self.corners[..self.count]
            .iter()
            .filter(|corner| plane.gap(**corner).abs() <= reach);
        let mut len = 0;
        for corner in on_plane {
            out[len] = *corner;
            len += 1;
        }
        let total = out[..len].iter().fold(Vec3::ZERO, |sum, c| sum + *c);
        let centre = total * (1.0 / small_count(len.max(1)));
        let across = perpendicular(plane.normal);
        sort_round(
            &mut out[..len],
            centre,
            (across, plane.normal.cross(across)),
        );
        len
    }

    /// Adds the tetrahedra from `reference` to the triangles of the face on `plane`.
    fn add_face(
        &self,
        plane: &Plane,
        reference: Vec3,
        sums: &mut Sums,
    ) -> Result<(), PhysicsError> {
        let mut polygon = [Vec3::ZERO; MAX_HULL_VERTICES];
        let len = self.face_polygon(plane, &mut polygon);
        if len < 3 {
            return Err(PhysicsError::InvalidShape);
        }
        for pair in polygon[1..len].windows(2) {
            sums.add_area([polygon[0], pair[0], pair[1]]);
            sums.add_tetrahedron([
                polygon[0] - reference,
                pair[0] - reference,
                pair[1] - reference,
            ]);
        }
        Ok(())
    }

    /// The hull, its origin put back by `anchor` (the shift the points were given in).
    fn into_hull(self, anchor: Vec3) -> Result<Hull, PhysicsError> {
        let reference = self.mean();
        let mut sums = Sums::default();
        for plane in &self.planes[..self.plane_count] {
            self.add_face(plane, reference, &mut sums)?;
        }
        if !(sums.volume.is_finite() && sums.volume > 0.0 && sums.is_closed()) {
            return Err(PhysicsError::InvalidShape);
        }
        let to_centre = sums.first_moment * (1.0 / sums.volume);
        let centre = reference + to_centre;
        let covariance = sums.covariance_about(to_centre);
        let (moments, axes) = principal(inertia_of(covariance));
        let mut hull = Hull {
            count: self.count,
            vertices: [Vec3::ZERO; MAX_HULL_VERTICES],
            origin: centre + anchor,
            orientation: quaternion_of(axes),
            volume: sums.volume,
            moments: Vec3::new(moments[0], moments[1], moments[2]),
        };
        for (slot, corner) in hull.vertices.iter_mut().zip(&self.corners[..self.count]) {
            let d = *corner - centre;
            *slot = Vec3::new(
                column(axes, 0).dot(d),
                column(axes, 1).dot(d),
                column(axes, 2).dot(d),
            );
        }
        if hull.is_valid() {
            Ok(hull)
        } else {
            Err(PhysicsError::InvalidShape)
        }
    }
}

/// The edges of a face given in order round it. A shared edge is on two faces that go round it in
/// opposite directions: only the direction that starts at the smaller point is reported.
fn report_edges(polygon: &[Vec3], visit: &mut impl FnMut(Vec3, Vec3)) {
    for (i, from) in polygon.iter().enumerate() {
        let to = polygon[(i + 1) % polygon.len()];
        if comes_first(*from, to) {
            visit(*from, to);
        }
    }
}

/// Whether the edge `from`..`to` is the one of its two directions that is reported (the other
/// face reports `to`..`from`): the one that starts at the smaller point.
fn comes_first(from: Vec3, to: Vec3) -> bool {
    (from.x, from.y, from.z)
        .partial_cmp(&(to.x, to.y, to.z))
        .is_some_and(std::cmp::Ordering::is_lt)
}

/// Some unit vector across `normal`.
fn perpendicular(normal: Vec3) -> Vec3 {
    let helper = if normal.x.abs() < 0.6 {
        Vec3::X
    } else {
        Vec3::Y
    };
    normal.cross(helper).normalized().unwrap_or(Vec3::Z)
}

/// Whether `a` comes before `b` going counter-clockwise round `centre` in the plane of `frame`,
/// starting from `frame.0`. Uses signs and one cross product, no angles.
fn comes_before(a: Vec3, b: Vec3, frame: (Vec3, Vec3)) -> bool {
    let half = |p: Vec3| {
        let (u, v) = (p.dot(frame.0), p.dot(frame.1));
        usize::from(v < 0.0 || (v == 0.0 && u < 0.0))
    };
    let (ha, hb) = (half(a), half(b));
    if ha != hb {
        return ha < hb;
    }
    let turn = frame.0.cross(frame.1);
    a.cross(b).dot(turn) > 0.0
}

fn sort_round(polygon: &mut [Vec3], centre: Vec3, frame: (Vec3, Vec3)) {
    for p in polygon.iter_mut() {
        *p -= centre;
    }
    // Insertion sort: at most 32 items, and the same order on every machine.
    for i in 1..polygon.len() {
        let mut j = i;
        while j > 0 && comes_before(polygon[j], polygon[j - 1], frame) {
            polygon.swap(j, j - 1);
            j -= 1;
        }
    }
    for p in polygon.iter_mut() {
        *p += centre;
    }
}

/// The volume, the first moment and the second moment of a solid, summed over tetrahedra that
/// share one corner (the origin of the frame they are given in).
#[derive(Default)]
struct Sums {
    /// The faces' area vectors added up (zero for a closed surface) and their areas added up.
    area_vector: Vec3,
    area: f32,
    volume: f32,
    first_moment: Vec3,
    covariance: Matrix,
}

impl Sums {
    /// Adds the area vector of the triangle `corners` (counter-clockwise seen from outside).
    fn add_area(&mut self, corners: [Vec3; 3]) {
        let vector = (corners[1] - corners[0]).cross(corners[2] - corners[0]) * 0.5;
        self.area_vector += vector;
        self.area += vector.length();
    }

    /// Whether the faces added close up: their area vectors cancel, up to `CLOSED_SURFACE` of the
    /// total area. Faces that overlap, or leave a gap, do not.
    fn is_closed(&self) -> bool {
        self.area_vector.length() <= CLOSED_SURFACE * self.area
    }

    fn add_tetrahedron(&mut self, corners: [Vec3; 3]) {
        let [a, b, c] = corners;
        let det = a.dot(b.cross(c));
        let sum = a + b + c;
        self.volume += det / 6.0;
        self.first_moment += sum * (det / 24.0);
        add_outer_products(&mut self.covariance, det / 120.0, [a, b, c, sum]);
    }

    /// The second moment about the centre of mass, `to_centre` from the origin of the sums.
    fn covariance_about(&self, to_centre: Vec3) -> Matrix {
        let mut about = self.covariance;
        add_outer_products(
            &mut about,
            -self.volume,
            [to_centre, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
        );
        about
    }
}

/// Adds `weight * v v^T` to `matrix` for each of the four vectors.
fn add_outer_products(matrix: &mut Matrix, weight: f32, vectors: [Vec3; 4]) {
    for (row, line) in matrix.iter_mut().enumerate() {
        for (col, cell) in line.iter_mut().enumerate() {
            let sum: f32 = vectors
                .iter()
                .map(|v| component(*v, row) * component(*v, col))
                .sum();
            *cell += weight * sum;
        }
    }
}

/// A small count as a float (at most 255, so exact).
fn small_count(n: usize) -> f32 {
    u8::try_from(n).map_or(f32::from(u8::MAX), f32::from)
}

/// Every `(i, j, k)` with `i < j < k < n`, in order.
fn triples(n: usize) -> impl Iterator<Item = (usize, usize, usize)> {
    (0..n).flat_map(move |i| ((i + 1)..n).flat_map(move |j| ((j + 1)..n).map(move |k| (i, j, k))))
}

fn component(v: Vec3, index: usize) -> f32 {
    v.to_array()[index]
}

/// The inertia tensor of a solid from its second moment: `trace(C) I - C`.
fn inertia_of(covariance: Matrix) -> Matrix {
    let trace = covariance[0][0] + covariance[1][1] + covariance[2][2];
    std::array::from_fn(|row| {
        std::array::from_fn(|col| {
            let diagonal = trace * f32::from(u8::from(row == col));
            diagonal - covariance[row][col]
        })
    })
}

/// Turns the columns `first` and `second` of every row of `matrix` by the angle with the given
/// cosine and sine.
fn turn_columns(matrix: &mut Matrix, pair: (usize, usize), turn: (f32, f32)) {
    let ((first, second), (cosine, sine)) = (pair, turn);
    for row in matrix.iter_mut() {
        let (x, y) = (row[first], row[second]);
        row[first] = cosine * x - sine * y;
        row[second] = sine * x + cosine * y;
    }
}

/// One Jacobi rotation that zeroes `a[first][second]`, applied to the matrix and to the axes.
fn rotate_pair(a: &mut Matrix, axes: &mut Matrix, pair: (usize, usize)) {
    let (first, second) = pair;
    let off = a[first][second];
    if off.abs()
        <= JACOBI_EPSILON * (a[first][first].abs() + a[second][second].abs()).max(f32::MIN_POSITIVE)
    {
        return;
    }
    let theta = (a[second][second] - a[first][first]) / (2.0 * off);
    let sign = if theta < 0.0 { -1.0 } else { 1.0 };
    let tangent = sign / (theta.abs() + (theta * theta + 1.0).sqrt());
    let cosine = 1.0 / (tangent * tangent + 1.0).sqrt();
    let turn = (cosine, tangent * cosine);
    turn_columns(a, pair, turn);
    turn_columns(axes, pair, turn);
    // The same turn from the left: work on the transpose, which is the same matrix.
    *a = transposed(*a);
    turn_columns(a, pair, turn);
    *a = transposed(*a);
}

fn transposed(m: Matrix) -> Matrix {
    std::array::from_fn(|row| std::array::from_fn(|col| m[col][row]))
}

/// The eigenvalues of a symmetric matrix and its eigenvectors as the columns of a rotation.
fn principal(mut tensor: Matrix) -> ([f32; 3], Matrix) {
    let mut axes: Matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..JACOBI_SWEEPS {
        for pair in [(0, 1), (0, 2), (1, 2)] {
            rotate_pair(&mut tensor, &mut axes, pair);
        }
    }
    let third = column(axes, 0).cross(column(axes, 1));
    if third.dot(column(axes, 2)) < 0.0 {
        for row in &mut axes {
            row[2] = -row[2];
        }
    }
    ([tensor[0][0], tensor[1][1], tensor[2][2]], axes)
}

fn column(matrix: Matrix, index: usize) -> Vec3 {
    Vec3::new(matrix[0][index], matrix[1][index], matrix[2][index])
}

/// The quaternion of the rotation whose columns are `m`'s (Shepperd's method: the largest of the
/// four candidates, so the square root is never of a small number).
fn quaternion_of(m: Matrix) -> Quat {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let raw = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        Quat::new(
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            s / 4.0,
        )
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        Quat::new(
            s / 4.0,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[2][1] - m[1][2]) / s,
        )
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        Quat::new(
            (m[0][1] + m[1][0]) / s,
            s / 4.0,
            (m[1][2] + m[2][1]) / s,
            (m[0][2] - m[2][0]) / s,
        )
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        Quat::new(
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            s / 4.0,
            (m[1][0] - m[0][1]) / s,
        )
    };
    raw.normalized().unwrap_or(Quat::IDENTITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_on_one_line_do_not_span_a_plane_wherever_the_line_is() {
        let start = Vec3::new(5.0, -3.0, 2.0);
        let step = Vec3::new(1.0, 2.0, 2.0);
        let on_line: Vec<Vec3> = [0.0, 3.0, -2.0, 1.0]
            .iter()
            .map(|t| start + step * *t)
            .collect();
        assert!(!spans_a_plane(&on_line, 1.0e-3));
        let beside = [on_line.clone(), vec![start + Vec3::new(0.0, 0.01, -0.01)]].concat();
        assert!(spans_a_plane(&beside, 1.0e-3));
        assert!(!spans_a_plane(&beside, 0.1));
        assert!(!spans_a_plane(&[], 1.0e-3));
        assert!(!spans_a_plane(&[start], 1.0e-3));
        assert!(!spans_a_plane(&[start, start + step], 1.0e-3));
    }

    #[test]
    fn a_point_exactly_at_the_reach_from_the_line_is_still_on_it() {
        let line = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(1.0, 0.5, 0.0),
        ];
        assert!(!spans_a_plane(&line, 0.5));
        assert!(spans_a_plane(&line, 0.499));
    }

    /// The matrix whose columns are the turned axes.
    fn matrix_of(turn: Quat) -> Matrix {
        let (x, y, z) = (
            turn.rotate(Vec3::X),
            turn.rotate(Vec3::Y),
            turn.rotate(Vec3::Z),
        );
        [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]]
    }

    fn turn(axis: Vec3, angle: f32) -> Quat {
        Quat::from_axis_angle(axis, angle).unwrap_or(Quat::IDENTITY)
    }

    /// Whether two quaternions turn a few probe vectors to the same place.
    fn turns_alike(first: Quat, second: Quat) -> bool {
        [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(0.3, -0.7, 0.5)]
            .into_iter()
            .all(|probe| (first.rotate(probe) - second.rotate(probe)).length() < 1.0e-4)
    }

    /// The branch of the conversion a matrix takes, numbered as in `quaternion_of`.
    fn branch_of(m: &Matrix) -> u8 {
        let trace = m[0][0] + m[1][1] + m[2][2];
        if trace > 0.0 {
            return 0;
        }
        if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            return 1;
        }
        if m[1][1] > m[2][2] { 2 } else { 3 }
    }

    /// Every branch of the conversion is reached by some rotation (`principal` itself only ends
    /// with a positive trace, so the three other branches are tested here), and each gives back
    /// the rotation it was made from.
    #[test]
    fn a_rotation_matrix_gives_back_its_quaternion_in_every_branch_of_the_conversion() {
        let pi = std::f32::consts::PI;
        let cases = [
            (Quat::IDENTITY, 0),
            (turn(Vec3::new(0.3, 0.5, 0.8), 0.4), 0),
            (turn(Vec3::X, pi), 1),
            (turn(Vec3::new(1.0, 0.1, 0.3), 3.0), 1),
            (turn(Vec3::Y, pi), 2),
            (turn(Vec3::new(0.1, 1.0, 0.2), 3.0), 2),
            (turn(Vec3::Z, pi), 3),
            (turn(Vec3::new(0.2, 0.1, 1.0), 3.0), 3),
        ];
        for (rotation, branch) in cases {
            let m = matrix_of(rotation);
            let branch_taken = branch_of(&m);
            assert_eq!(branch_taken, branch, "{rotation:?}");
            let back = quaternion_of(m);
            assert!(turns_alike(back, rotation), "{rotation:?}");
            assert!((back.length() - 1.0).abs() < 1.0e-4);
        }
    }

    fn a_good_hull() -> Hull {
        let corners = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
        Hull::new(&corners).unwrap_or_else(|_| panic!("a tetrahedron is a hull"))
    }

    /// One field at a time is made bad: each makes the hull invalid, and the good one is valid.
    #[test]
    fn a_hull_is_valid_only_when_every_part_of_it_is() {
        assert!(a_good_hull().is_valid());
        let mut too_few = a_good_hull();
        too_few.count = 3;
        let mut too_many = a_good_hull();
        too_many.count = MAX_HULL_VERTICES + 1;
        let mut not_finite = a_good_hull();
        not_finite.vertices[0] = Vec3::new(f32::NAN, 0.0, 0.0);
        let mut flat = a_good_hull();
        flat.volume = 0.0;
        let mut huge = a_good_hull();
        huge.volume = f32::INFINITY;
        let bad_moment = |x: f32, y: f32, z: f32| {
            let mut hull = a_good_hull();
            hull.moments = Vec3::new(x, y, z);
            hull
        };
        let good = a_good_hull().moments;
        for (name, hull) in [
            ("too few corners", too_few),
            ("too many corners", too_many),
            ("a corner that is not finite", not_finite),
            ("no volume", flat),
            ("infinite volume", huge),
            ("no moment x", bad_moment(0.0, good.y, good.z)),
            ("no moment y", bad_moment(good.x, 0.0, good.z)),
            ("no moment z", bad_moment(good.x, good.y, -1.0)),
            (
                "a moment that is not a number",
                bad_moment(f32::NAN, good.y, good.z),
            ),
        ] {
            assert!(!hull.is_valid(), "{name}");
        }
    }
}
