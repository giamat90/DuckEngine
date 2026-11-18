//! Convex shapes against each other by GJK (the distance between two convex cores) and EPA (how deep
//! they overlap), used for every pair that has a convex hull in it.
//!
//! A shape is a **core** and a radius: a sphere is a point with a radius, a capsule a segment with a
//! radius, a box and a hull their own solid with radius zero. The cores are tested; the radii are
//! added to the gap afterwards, so a sphere against a hull costs one point against the hull.
//!
//! GJK walks a simplex (up to four points) over the difference of the two cores, towards the point of
//! it nearest the origin. When that point is the origin the cores overlap and EPA grows the simplex
//! into a solid that wraps the origin until its nearest face is on the boundary of the difference;
//! that face gives the normal and the depth. The result is one contact point; the up to four points
//! of a face contact are made from it by [`super::face_clip`] (card 8-18).
//!
//! No heap allocation: the simplex and the solid are fixed arrays. Only `+ - * /` and `sqrt` are
//! used, in a fixed order, with ties broken by index, so the result is the same on every run.

use super::manifold::{ContactPoint3, Manifold3};
use super::narrowphase::{BoxFrame, sign};
use base::math::{Quat, Vec3};

/// Rounds of GJK. It needs a handful for shapes of a few dozen corners; this is the guard.
const MAX_GJK_ITERATIONS: usize = 64;
/// Rounds of EPA, the same.
const MAX_EPA_ITERATIONS: usize = 64;
const MAX_POLYTOPE_VERTICES: usize = 64;
const MAX_POLYTOPE_FACES: usize = 128;
/// A squared distance to the origin below which the origin is taken to be in the simplex.
const TOUCHING_SQUARED: f32 = 1.0e-10;
/// GJK stops when the new point brings the simplex no nearer the origin than this share of the gap.
const GJK_PROGRESS: f32 = 1.0e-5;
/// EPA stops when the new point is no farther than this beyond the nearest face.
const EPA_PROGRESS: f32 = 1.0e-5;
/// A face is seen from a point when the point is this far in front of its plane.
const VISIBLE: f32 = 1.0e-6;
/// A length below which a vector has no direction.
const THIN: f32 = 1.0e-5;

/// The solid part of a shape.
#[derive(Clone, Copy)]
pub(super) enum Core<'a> {
    Point(Vec3),
    Segment(Vec3, Vec3),
    Box(BoxFrame),
    Hull {
        vertices: &'a [Vec3],
        centre: Vec3,
        rotation: Quat,
    },
}

/// A shape for GJK: a core, grown by a radius.
pub(super) struct Convex<'a> {
    pub(super) core: Core<'a>,
    pub(super) radius: f32,
}

fn farthest(vertices: &[Vec3], direction: Vec3) -> Vec3 {
    vertices
        .iter()
        .copied()
        .max_by(|x, y| x.dot(direction).total_cmp(&y.dot(direction)))
        .unwrap_or(Vec3::ZERO)
}

fn box_support(frame: &BoxFrame, direction: Vec3) -> Vec3 {
    let local = frame.rotation.conjugate().rotate(direction);
    let corner = Vec3::new(
        sign(local.x) * frame.half.x,
        sign(local.y) * frame.half.y,
        sign(local.z) * frame.half.z,
    );
    frame.centre + frame.rotation.rotate(corner)
}

fn segment_support(start: Vec3, end: Vec3, direction: Vec3) -> Vec3 {
    if direction.dot(end - start) >= 0.0 {
        end
    } else {
        start
    }
}

impl Core<'_> {
    /// The point of the core farthest along `direction`.
    fn support(&self, direction: Vec3) -> Vec3 {
        match *self {
            Self::Point(point) => point,
            Self::Segment(start, end) => segment_support(start, end, direction),
            Self::Box(frame) => box_support(&frame, direction),
            Self::Hull {
                vertices,
                centre,
                rotation,
            } => {
                let local = rotation.conjugate().rotate(direction);
                centre + rotation.rotate(farthest(vertices, local))
            }
        }
    }

    fn centre(&self) -> Vec3 {
        match *self {
            Self::Point(point) => point,
            Self::Segment(start, end) => (start + end) * 0.5,
            Self::Box(frame) => frame.centre,
            Self::Hull { centre, .. } => centre,
        }
    }
}

/// A point of the difference of the two cores, with the points of the cores it came from.
#[derive(Clone, Copy)]
struct Vertex {
    point: Vec3,
    on_first: Vec3,
    on_second: Vec3,
}

fn support_pair(first: &Core, second: &Core, direction: Vec3) -> Vertex {
    let on_first = first.support(direction);
    let on_second = second.support(-direction);
    Vertex {
        point: on_first - on_second,
        on_first,
        on_second,
    }
}

/// The nearest point to the origin of part of the simplex, and the vertices it rests on.
struct Nearest {
    point: Vec3,
    parts: [usize; 3],
    weights: [f32; 3],
    count: usize,
}

fn at_vertex(index: usize, point: Vec3) -> Nearest {
    Nearest {
        point,
        parts: [index, 0, 0],
        weights: [1.0, 0.0, 0.0],
        count: 1,
    }
}

fn nearest_on_segment(points: &[Vec3; 4], ends: (usize, usize)) -> Nearest {
    let (i, j) = ends;
    let along = points[j] - points[i];
    let length = along.length_squared();
    let t = if length > 0.0 {
        -points[i].dot(along) / length
    } else {
        0.0
    };
    if t <= 0.0 {
        return at_vertex(i, points[i]);
    }
    if t >= 1.0 {
        return at_vertex(j, points[j]);
    }
    Nearest {
        point: points[i] + along * t,
        parts: [i, j, 0],
        weights: [1.0 - t, t, 0.0],
        count: 2,
    }
}

/// The region of the triangle the origin is nearest: Ericson, "Real-Time Collision Detection" 5.1.5.
fn nearest_on_triangle(points: &[Vec3; 4], corners: [usize; 3]) -> Nearest {
    let [first, second, third] = corners;
    let (pa, pb, pc) = (points[first], points[second], points[third]);
    let (ab, ac) = (pb - pa, pc - pa);
    let (d1, d2) = (ab.dot(-pa), ac.dot(-pa));
    let (d3, d4) = (ab.dot(-pb), ac.dot(-pb));
    let (d5, d6) = (ab.dot(-pc), ac.dot(-pc));
    let (va, vb, vc) = (d3 * d6 - d5 * d4, d5 * d2 - d1 * d6, d1 * d4 - d3 * d2);
    if d1 <= 0.0 && d2 <= 0.0 {
        return at_vertex(first, pa);
    }
    if d3 >= 0.0 && d4 <= d3 {
        return at_vertex(second, pb);
    }
    if d6 >= 0.0 && d5 <= d6 {
        return at_vertex(third, pc);
    }
    let on_edge = |from: usize, to: usize, fraction: f32, base: Vec3, step: Vec3| Nearest {
        point: base + step * fraction,
        parts: [from, to, 0],
        weights: [1.0 - fraction, fraction, 0.0],
        count: 2,
    };
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return on_edge(first, second, d1 / (d1 - d3), pa, ab);
    }
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return on_edge(first, third, d2 / (d2 - d6), pa, ac);
    }
    let across = (d4 - d3) + (d5 - d6);
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 && across > 0.0 {
        return on_edge(second, third, (d4 - d3) / across, pb, pc - pb);
    }
    let total = va + vb + vc;
    if total.abs() <= f32::MIN_POSITIVE {
        return at_vertex(first, pa);
    }
    let (toward_b, toward_c) = (vb / total, vc / total);
    Nearest {
        point: pa + ab * toward_b + ac * toward_c,
        parts: [first, second, third],
        weights: [1.0 - toward_b - toward_c, toward_b, toward_c],
        count: 3,
    }
}

/// The simplex of GJK: one to four vertices of the difference.
#[derive(Clone, Copy)]
struct Simplex {
    vertices: [Vertex; 4],
    weights: [f32; 4],
    len: usize,
}

impl Simplex {
    fn starting_at(vertex: Vertex) -> Self {
        Self {
            vertices: [vertex; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
            len: 1,
        }
    }

    fn points(&self) -> [Vec3; 4] {
        self.vertices.map(|v| v.point)
    }

    /// The nearest point to the origin of the simplex, or `None` when the origin is inside it.
    fn nearest(&self) -> Option<Nearest> {
        let points = self.points();
        match self.len {
            1 => Some(at_vertex(0, points[0])),
            2 => Some(nearest_on_segment(&points, (0, 1))),
            3 => Some(nearest_on_triangle(&points, [0, 1, 2])),
            _ => nearest_of_tetrahedron(&points),
        }
    }

    /// Keeps only the vertices the nearest point rests on.
    fn keep(&mut self, nearest: &Nearest) {
        let old = self.vertices;
        for slot in 0..nearest.count {
            self.vertices[slot] = old[nearest.parts[slot]];
            self.weights[slot] = nearest.weights[slot];
        }
        self.len = nearest.count;
    }

    fn contains(&self, vertex: &Vertex) -> bool {
        self.vertices[..self.len]
            .iter()
            .any(|v| (v.point - vertex.point).length_squared() <= THIN * THIN)
    }

    fn push(&mut self, vertex: Vertex) {
        self.vertices[self.len] = vertex;
        self.weights[self.len] = 0.0;
        self.len += 1;
    }

    /// The points of the two cores that the nearest point of the difference came from.
    fn witness(&self) -> (Vec3, Vec3) {
        let mut on_first = Vec3::ZERO;
        let mut on_second = Vec3::ZERO;
        for (vertex, weight) in self.vertices[..self.len].iter().zip(self.weights) {
            on_first += vertex.on_first * weight;
            on_second += vertex.on_second * weight;
        }
        (on_first, on_second)
    }
}

/// The nearest point of a tetrahedron to the origin; `None` when the origin is inside it.
fn nearest_of_tetrahedron(points: &[Vec3; 4]) -> Option<Nearest> {
    let mut best: Option<Nearest> = None;
    for (face, opposite) in [
        ([0, 1, 2], 3),
        ([0, 3, 1], 2),
        ([0, 2, 3], 1),
        ([1, 3, 2], 0),
    ] {
        let normal = (points[face[1]] - points[face[0]]).cross(points[face[2]] - points[face[0]]);
        let toward_opposite = normal.dot(points[opposite] - points[face[0]]);
        let toward_origin = normal.dot(-points[face[0]]);
        if toward_opposite != 0.0 && toward_origin * toward_opposite >= 0.0 {
            continue;
        }
        let candidate = nearest_on_triangle(points, face);
        let nearer = best
            .as_ref()
            .is_none_or(|b| candidate.point.length_squared() < b.point.length_squared());
        if nearer {
            best = Some(candidate);
        }
    }
    best
}

/// Where GJK ends: the closest points of two cores that do not overlap, or a simplex that holds
/// (or all but holds) the origin.
enum Gjk {
    Apart { on_first: Vec3, on_second: Vec3 },
    Touching(Simplex),
}

fn first_direction(first: &Core, second: &Core) -> Vec3 {
    let apart = first.centre() - second.centre();
    if apart.length_squared() > THIN * THIN {
        apart
    } else {
        Vec3::X
    }
}

fn gjk(first: &Core, second: &Core) -> Gjk {
    let start = support_pair(first, second, first_direction(first, second));
    let mut simplex = Simplex::starting_at(start);
    for _ in 0..MAX_GJK_ITERATIONS {
        let Some(nearest) = simplex.nearest() else {
            return Gjk::Touching(simplex);
        };
        simplex.keep(&nearest);
        let gap = nearest.point.length_squared();
        if gap <= TOUCHING_SQUARED {
            return Gjk::Touching(simplex);
        }
        let fresh = support_pair(first, second, -nearest.point);
        let progress = gap - nearest.point.dot(fresh.point);
        if progress <= GJK_PROGRESS * gap || simplex.contains(&fresh) {
            break;
        }
        simplex.push(fresh);
    }
    let (on_first, on_second) = simplex.witness();
    Gjk::Apart {
        on_first,
        on_second,
    }
}

/// Whether `point` is off the line, plane or point the simplex spans, so it adds a dimension.
fn adds_dimension(simplex: &Simplex, point: Vec3) -> bool {
    let p = simplex.points();
    let from_first = point - p[0];
    match simplex.len {
        1 => from_first.length() > THIN,
        2 => from_first.cross(p[1] - p[0]).length() > THIN * (p[1] - p[0]).length(),
        _ => {
            let normal = (p[1] - p[0]).cross(p[2] - p[0]);
            normal.dot(from_first).abs() > THIN * normal.length()
        }
    }
}

/// Directions to try for a point that adds a dimension to a simplex of the given size.
fn growth_directions(simplex: &Simplex) -> [Vec3; 6] {
    let p = simplex.points();
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    match simplex.len {
        1 => [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z],
        2 => {
            let line = p[1] - p[0];
            let across = axes.map(|axis| line.cross(axis));
            [
                across[0], -across[0], across[1], -across[1], across[2], -across[2],
            ]
        }
        _ => {
            let normal = (p[1] - p[0]).cross(p[2] - p[0]);
            [normal, -normal, normal, -normal, normal, -normal]
        }
    }
}

/// Whether the three vertices lie on a line, so they span no plane.
fn is_flat(simplex: &Simplex) -> bool {
    let p = simplex.points();
    let (along, other) = (p[1] - p[0], p[2] - p[0]);
    let area = along.cross(other).length_squared();
    area <= THIN * THIN * along.length_squared() * other.length_squared()
}

/// A tetrahedron round the simplex: the simplex plus points that each add a dimension.
fn tetrahedron(first: &Core, second: &Core, mut simplex: Simplex) -> Option<Simplex> {
    if simplex.len == 3 && is_flat(&simplex) {
        simplex.len = 2;
    }
    while simplex.len < 4 {
        let fresh = growth_directions(&simplex)
            .into_iter()
            .map(|direction| support_pair(first, second, direction))
            .find(|vertex| adds_dimension(&simplex, vertex.point))?;
        simplex.push(fresh);
    }
    Some(simplex)
}

/// A triangle of the solid EPA grows: three vertices, the outward unit normal, the distance of its
/// plane from the origin.
#[derive(Clone, Copy)]
struct Face {
    corners: [usize; 3],
    normal: Vec3,
    distance: f32,
}

struct Polytope {
    vertices: [Vertex; MAX_POLYTOPE_VERTICES],
    vertex_count: usize,
    faces: [Face; MAX_POLYTOPE_FACES],
    face_count: usize,
}

/// What EPA finds: the normal from the first core to the second, the depth, and the two points.
struct Penetration {
    normal: Vec3,
    depth: f32,
    on_first: Vec3,
    on_second: Vec3,
}

impl Polytope {
    fn corners_of(&self, face: &Face) -> [Vec3; 3] {
        face.corners.map(|i| self.vertices[i].point)
    }

    /// The face through three vertices, wound so that its normal points away from `inside`.
    fn face_away_from(&self, corners: [usize; 3], inside: Vec3) -> Face {
        let [pa, pb, pc] = corners.map(|i| self.vertices[i].point);
        let raw = (pb - pa).cross(pc - pa);
        let length = raw.length();
        if length <= THIN * THIN {
            return Face {
                corners,
                normal: Vec3::X,
                distance: f32::MAX,
            };
        }
        let normal = raw * (1.0 / length);
        if normal.dot(inside - pa) > 0.0 {
            return Face {
                corners: [corners[0], corners[2], corners[1]],
                normal: -normal,
                distance: -normal.dot(pa),
            };
        }
        Face {
            corners,
            normal,
            distance: normal.dot(pa),
        }
    }

    fn from_tetrahedron(simplex: &Simplex) -> Self {
        let dummy = simplex.vertices[0];
        let mut polytope = Self {
            vertices: [dummy; MAX_POLYTOPE_VERTICES],
            vertex_count: 4,
            faces: [Face {
                corners: [0; 3],
                normal: Vec3::X,
                distance: 0.0,
            }; MAX_POLYTOPE_FACES],
            face_count: 4,
        };
        polytope.vertices[..4].copy_from_slice(&simplex.vertices);
        let points = simplex.points();
        let inside = (points[0] + points[1] + points[2] + points[3]) * 0.25;
        for (slot, corners) in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]]
            .into_iter()
            .enumerate()
        {
            polytope.faces[slot] = polytope.face_away_from(corners, inside);
        }
        polytope
    }

    fn nearest_face(&self) -> usize {
        (0..self.face_count)
            .min_by(|x, y| self.faces[*x].distance.total_cmp(&self.faces[*y].distance))
            .unwrap_or(0)
    }

    fn sees(&self, face: &Face, fresh: &Vertex) -> bool {
        face.normal
            .dot(fresh.point - self.vertices[face.corners[0]].point)
            > VISIBLE
    }

    /// Keeps `face`, or, if `fresh` sees it, drops it and notes its edges in `rim`.
    fn keep_or_note(&mut self, face: Face, fresh: &Vertex, rim: &mut Rim) {
        if self.sees(&face, fresh) {
            rim.note(&face);
        } else {
            self.faces[self.face_count] = face;
            self.face_count += 1;
        }
    }

    /// Adds `fresh`, removing the faces that see it and closing the hole. `false` when full.
    fn expand(&mut self, fresh: Vertex) -> bool {
        let old = self.faces;
        let old_count = self.face_count;
        let visible = old[..old_count]
            .iter()
            .filter(|face| self.sees(face, &fresh))
            .count();
        // Refuse before changing anything when the new faces could not all be stored: each seen
        // face can leave at most three edges on the rim.
        let room = old_count - visible + 3 * visible <= MAX_POLYTOPE_FACES;
        if self.vertex_count == MAX_POLYTOPE_VERTICES || visible == 0 || !room {
            return false;
        }
        let apex = self.vertex_count;
        self.vertices[apex] = fresh;
        self.vertex_count += 1;
        self.face_count = 0;
        let mut rim = Rim::new();
        for face in old[..old_count].iter().copied() {
            self.keep_or_note(face, &fresh, &mut rim);
        }
        self.close_hole(&rim, apex)
    }

    fn push_face(&mut self, corners: [usize; 3], inside: Vec3) -> bool {
        if self.face_count == MAX_POLYTOPE_FACES {
            return false;
        }
        self.faces[self.face_count] = self.face_away_from(corners, inside);
        self.face_count += 1;
        true
    }

    /// New faces from each edge of the hole to the new vertex. An edge is on the rim of the hole
    /// when the face on its other side was kept, that is, when its reverse is not listed.
    fn close_hole(&mut self, rim: &Rim, apex: usize) -> bool {
        let inside = self.interior_point();
        let listed = &rim.edges[..rim.count];
        listed
            .iter()
            .filter(|(from, to)| !listed.contains(&(*to, *from)))
            .all(|&(from, to)| self.push_face([from, to, apex], inside))
    }

    /// A point inside the solid: the mean of its vertices.
    fn interior_point(&self) -> Vec3 {
        let sum = self.vertices[..self.vertex_count]
            .iter()
            .fold(Vec3::ZERO, |total, v| total + v.point);
        sum * (1.0 / f32::from(u8::try_from(self.vertex_count).unwrap_or(u8::MAX)))
    }

    /// The answer for the face `index`: the points of the cores under the origin's projection on it.
    fn penetration(&self, index: usize) -> Penetration {
        let face = self.faces[index];
        let [pa, pb, pc] = self.corners_of(&face);
        let projected = face.normal * face.distance;
        let weights = barycentric(projected, [pa, pb, pc]);
        let mut on_first = Vec3::ZERO;
        let mut on_second = Vec3::ZERO;
        for (corner, weight) in face.corners.into_iter().zip(weights) {
            on_first += self.vertices[corner].on_first * weight;
            on_second += self.vertices[corner].on_second * weight;
        }
        Penetration {
            normal: face.normal,
            depth: face.distance,
            on_first,
            on_second,
        }
    }
}

/// The edges of the faces removed while a vertex is added, each as it was wound.
struct Rim {
    edges: [(usize, usize); MAX_POLYTOPE_FACES * 3],
    count: usize,
}

impl Rim {
    fn new() -> Self {
        Self {
            edges: [(0, 0); MAX_POLYTOPE_FACES * 3],
            count: 0,
        }
    }

    fn note(&mut self, face: &Face) {
        for edge in 0..3 {
            self.edges[self.count] = (face.corners[edge], face.corners[(edge + 1) % 3]);
            self.count += 1;
        }
    }
}

/// The weights of the corners of a triangle that give the point nearest `point` in its plane,
/// clamped to the triangle.
fn barycentric(point: Vec3, triangle: [Vec3; 3]) -> [f32; 3] {
    let [pa, pb, pc] = triangle;
    let (v0, v1, v2) = (pb - pa, pc - pa, point - pa);
    let (d00, d01, d11) = (v0.dot(v0), v0.dot(v1), v1.dot(v1));
    let (d20, d21) = (v2.dot(v0), v2.dot(v1));
    let denominator = d00 * d11 - d01 * d01;
    if denominator.abs() <= f32::MIN_POSITIVE {
        return [1.0 / 3.0; 3];
    }
    let v = ((d11 * d20 - d01 * d21) / denominator).clamp(0.0, 1.0);
    let w = ((d00 * d21 - d01 * d20) / denominator).clamp(0.0, 1.0);
    let u = (1.0 - v - w).max(0.0);
    let total = u + v + w;
    [u / total, v / total, w / total]
}

/// EPA over the difference of two cores that overlap, starting from the simplex GJK ended with.
fn penetrate(first: &Core, second: &Core, simplex: Simplex) -> Option<Penetration> {
    let mut polytope = Polytope::from_tetrahedron(&tetrahedron(first, second, simplex)?);
    for _ in 0..MAX_EPA_ITERATIONS {
        let index = polytope.nearest_face();
        let face = polytope.faces[index];
        let fresh = support_pair(first, second, face.normal);
        let reach = face.normal.dot(fresh.point);
        if reach - face.distance <= EPA_PROGRESS || !polytope.expand(fresh) {
            return Some(polytope.penetration(index));
        }
    }
    Some(polytope.penetration(polytope.nearest_face()))
}

/// The one-point manifold of two convex shapes, normal from the first to the second; `None` when
/// they are further apart than `margin`, or when no answer is found.
pub(super) fn contact(first: &Convex, second: &Convex, margin: f32) -> Option<Manifold3> {
    let radii = first.radius + second.radius;
    let (normal, gap, on_first, on_second) = match gjk(&first.core, &second.core) {
        Gjk::Apart {
            on_first,
            on_second,
        } => {
            let across = on_second - on_first;
            let length = across.length();
            (across * (1.0 / length), length, on_first, on_second)
        }
        Gjk::Touching(simplex) => {
            let found = penetrate(&first.core, &second.core, simplex)?;
            (found.normal, -found.depth, found.on_first, found.on_second)
        }
    };
    let separation = gap - radii;
    if separation > margin {
        return None;
    }
    let middle = (on_first + on_second) * 0.5 + normal * ((first.radius - second.radius) * 0.5);
    Some(Manifold3::new(
        normal,
        &[ContactPoint3 {
            point: middle,
            separation,
        }],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_vertex(x: f32) -> Vertex {
        Vertex {
            point: Vec3::new(x, 0.0, 0.0),
            on_first: Vec3::new(x, 0.0, 0.0),
            on_second: Vec3::ZERO,
        }
    }

    fn unit_box_at(x: f32) -> Core<'static> {
        Core::Box(BoxFrame {
            centre: Vec3::new(x, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            half: Vec3::new(0.5, 0.5, 0.5),
        })
    }

    /// Three vertices on one line span no plane; the simplex is cut back to two and grown again.
    #[test]
    fn a_flat_triangle_is_grown_into_a_tetrahedron_from_its_line() {
        let mut simplex = Simplex::starting_at(flat_vertex(0.0));
        simplex.push(flat_vertex(1.0));
        simplex.push(flat_vertex(2.0));
        assert!(is_flat(&simplex));
        let grown = tetrahedron(&unit_box_at(0.0), &unit_box_at(0.4), simplex);
        let Some(grown) = grown else {
            panic!("a line grows into a solid");
        };
        assert_eq!(grown.len, 4);
        assert!(!is_flat(&Simplex { len: 3, ..grown }));
    }

    /// A push clears the weight of the slot it fills, so the witness never uses a stale one.
    #[test]
    fn a_pushed_vertex_starts_with_no_weight() {
        let mut simplex = Simplex::starting_at(flat_vertex(0.0));
        simplex.weights = [0.25, 0.5, 0.75, 1.0];
        simplex.len = 1;
        simplex.push(flat_vertex(1.0));
        assert!((simplex.weights[1]).abs() < f32::EPSILON);
        assert_eq!(simplex.len, 2);
    }

    fn vertex_at(x: f32, y: f32, z: f32) -> Vertex {
        let point = Vec3::new(x, y, z);
        Vertex {
            point,
            on_first: point,
            on_second: Vec3::ZERO,
        }
    }

    fn simplex_of(points: &[(f32, f32, f32)]) -> Simplex {
        let mut simplex = Simplex::starting_at(vertex_at(points[0].0, points[0].1, points[0].2));
        for p in &points[1..] {
            simplex.push(vertex_at(p.0, p.1, p.2));
        }
        simplex
    }

    /// A line that does not pass through the origin is flat; a real triangle, even a thin one, is not.
    #[test]
    fn only_vertices_on_one_line_are_flat() {
        assert!(is_flat(&simplex_of(&[
            (1.0, 0.0, 0.0),
            (2.0, 1.0, 0.0),
            (3.0, 2.0, 0.0)
        ])));
        assert!(is_flat(&simplex_of(&[
            (5.0, 5.0, 5.0),
            (6.0, 6.0, 6.0),
            (9.0, 9.0, 9.0)
        ])));
        assert!(!is_flat(&simplex_of(&[
            (0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0)
        ])));
        assert!(!is_flat(&simplex_of(&[
            (0.0, 0.0, 0.0),
            (10.0, 0.0, 0.0),
            (5.0, 0.01, 0.0)
        ])));
        assert!(!is_flat(&simplex_of(&[
            (1.0, 1.0, 1.0),
            (2.0, 1.0, 1.0),
            (1.0, 2.0, 3.0)
        ])));
    }

    /// The test is on the sine of the angle at the first vertex, so the lengths of the two sides
    /// must not matter: a sine of 1e-6 is flat, one of 3e-5 is not, whichever side is the long one.
    #[test]
    fn flatness_is_decided_by_the_angle_whatever_the_lengths() {
        for (long, short) in [(100.0, 10.0), (10.0, 100.0)] {
            let flat = [
                (0.0, 0.0, 0.0),
                (short, 0.0, 0.0),
                (long, 1.0e-6 * long, 0.0),
            ];
            let thin = [
                (0.0, 0.0, 0.0),
                (short, 0.0, 0.0),
                (long, 3.0e-5 * long, 0.0),
            ];
            assert!(is_flat(&simplex_of(&flat)), "{long} {short}");
            assert!(!is_flat(&simplex_of(&thin)), "{long} {short}");
        }
    }

    /// A tetrahedron round the origin, as EPA starts from.
    fn solid() -> Polytope {
        let corners = [
            (1.0, 1.0, 1.0),
            (1.0, -1.0, -1.0),
            (-1.0, 1.0, -1.0),
            (-1.0, -1.0, 1.0),
        ];
        Polytope::from_tetrahedron(&simplex_of(&corners))
    }

    /// Fills the face slots after the first four with faces that no new vertex can see.
    fn pad_with_hidden_faces(polytope: &mut Polytope) {
        for face in &mut polytope.faces[4..] {
            *face = Face {
                corners: [0, 1, 2],
                normal: -Vec3::X,
                distance: 100.0,
            };
        }
    }

    /// `expand` refuses, and changes nothing, when the vertex sees nothing, when the vertices are
    /// full, and when the new faces could not all be stored; at the exact limit it goes ahead.
    #[test]
    fn expanding_refuses_before_it_changes_anything_and_works_at_the_exact_limit() {
        let fresh = vertex_at(5.0, 0.0, 0.0);
        let probe = solid();
        let seen = probe.faces[..4]
            .iter()
            .filter(|face| probe.sees(face, &fresh))
            .count();
        assert!(seen >= 1);

        let mut inside = solid();
        assert!(!inside.expand(vertex_at(0.0, 0.0, 0.0)));
        assert_eq!((inside.face_count, inside.vertex_count), (4, 4));

        let mut full = solid();
        full.vertex_count = MAX_POLYTOPE_VERTICES;
        assert!(!full.expand(fresh));
        assert_eq!(full.face_count, 4);

        let mut at_limit = solid();
        pad_with_hidden_faces(&mut at_limit);
        at_limit.face_count = MAX_POLYTOPE_FACES - 2 * seen;
        assert!(at_limit.expand(fresh));
        assert_eq!(at_limit.vertex_count, 5);

        let mut over = solid();
        pad_with_hidden_faces(&mut over);
        over.face_count = MAX_POLYTOPE_FACES - 2 * seen + 1;
        assert!(!over.expand(fresh));
        assert_eq!(
            (over.face_count, over.vertex_count),
            (MAX_POLYTOPE_FACES - 2 * seen + 1, 4)
        );
    }
}
