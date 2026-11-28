//! Contact manifolds for pairs that involve a convex hull: up to four points, by clipping.
//!
//! GJK and EPA ([`super::gjk`]) give a normal and one point, and the normal of a nearly flat contact
//! is noisy (a hundredth of a radian when the shapes just touch). So the face is found again from
//! the corners: a box or a hull has a face when three of its corners near the farthest plane span a
//! plane with every corner on one side ([`fitted_face`]); the exact normal of the larger face then
//! becomes the contact normal, and the other shape's face (or, when it has no face, its corners
//! within [`FACE_TOLERANCE`] of the farthest plane: an edge or one corner) is clipped to the sides of
//! the reference face as box against box does ([`super::box_box`]). The points that are further than
//! the margin from the reference plane are dropped, more than four are cut down to the four that
//! spread most, and when nothing is left the single point of GJK stands.
//!
//! Two more tries serve a contact that is no deeper than [`FACE_WINDOW`] of the reach (card 8-71):
//! when the normal of EPA leans past [`MIN_FACE_COSINE`] from every face, the nearest face within
//! about 45 degrees is tried ([`widened`]); and when the other shape has no face, every corner
//! within the margin of its farthest plane is a candidate, so a slightly tilted edge keeps both
//! ends. The first is accepted only when its deepest point and gap agree with GJK ([`agrees`]); the
//! second only adds points to a manifold that the first rule already gave.
//!
//! A sphere is a point and a capsule a segment, so a capsule that lies on a face gets two points and
//! a sphere one. Only `+ - * /` and `sqrt` are used, in a fixed order.

use super::clip::{CAPACITY, Candidates as AnyCandidates, Polygon as AnyPolygon};
use super::gjk::{self, Convex, Core};
use super::hull::{small_count, triples};
use super::manifold::{ContactPoint3, MAX_MANIFOLD_POINTS, Manifold3};
use base::math::Vec3;

type Polygon = AnyPolygon<CAPACITY>;
type Candidates = AnyCandidates<CAPACITY>;

/// A corner is on a face when it is within this share of the reach of the shape from the plane.
const FACE_TOLERANCE: f32 = 1.0e-3;
/// The smallest tolerance, in metres, for a shape of no size.
const MIN_TOLERANCE: f32 = 1.0e-6;
/// The squared length of an edge below which it has no direction.
const MIN_EDGE_SQUARED: f32 = 1.0e-12;
/// Corners within this share of the reach of the farthest plane are the candidates for a face.
const FACE_WINDOW: f32 = 5.0e-2;
/// The most candidates tried: three of them span each plane tested.
const MAX_WINDOW: usize = 16;
/// How far a corner may be from a face plane, as a share of the reach, and still be on the face.
const PLANE_TOLERANCE: f32 = 1.0e-4;
/// The squared area (share of the reach squared, squared) below which three corners span no plane.
const MIN_TRIANGLE: f32 = 1.0e-8;
/// The cosine of the largest angle between the search direction and a face that is still taken.
const MIN_FACE_COSINE: f32 = 0.9995;
/// The cosine of the largest angle for the second try (card 8-71), when no face is within
/// [`MIN_FACE_COSINE`] of either shape: about 45 degrees.
const WIDE_FACE_COSINE: f32 = 0.7;
/// A triple whose squared cosine with the search direction is below this share of the squared
/// minimum cosine is refused without the square root (card 8-99): 1% of the square is 0.5% of the
/// cosine, thousands of times the rounding error of the exact test.
const CLEARLY_BELOW: f32 = 0.99;
/// The smallest minimum cosine for which the refusal by the square is used: below it the margin is
/// a share of a number as small as the rounding error of the dot product.
const MIN_REFUSED_COSINE: f32 = 0.5;
/// The corners nearest the search direction that the second try spans planes from.
const WIDE_CANDIDATES: usize = 8;
/// How far the deepest point and the gap along the normal of a face of the second try may fall short
/// of those of GJK, as a share of the reach of the smaller shape.
const WIDE_AGREEMENT: f32 = 1.0e-3;

/// How much deeper than GJK's depth a contact point may be, as a share of the reach of the smaller
/// shape (card 8-77): a face that is not where the shapes touch can put points metres behind the
/// reference plane, and a point deeper than this is not a contact of the pair.
const DEPTH_ALLOWANCE: f32 = 0.25;

/// The corners of a shape's support face, in the order round the face when there are three or more.
#[derive(Clone, Copy)]
struct Face {
    corners: Polygon,
    /// The outward unit normal of the face when it was fitted to a plane of the shape.
    normal: Option<Vec3>,
}

impl Face {
    fn is_polygon(&self) -> bool {
        self.corners.len >= 3
    }

    fn first(&self) -> Vec3 {
        self.corners
            .as_slice()
            .first()
            .copied()
            .unwrap_or(Vec3::ZERO)
    }
}

/// How far a shape reaches from its centre: the scale of the tolerance.
fn reach_of(core: &Core) -> f32 {
    match *core {
        Core::Point(_) => 0.0,
        Core::Segment(start, end) => (end - start).length() * 0.5,
        Core::Box(frame) => frame.half.length(),
        Core::Hull { vertices, .. } => vertices
            .iter()
            .map(|v| v.length_squared())
            .fold(0.0, f32::max)
            .sqrt(),
    }
}

/// Keeps in `out` the corners of `candidates` that lie within `tolerance` of the farthest along `n`.
fn keep_farthest(candidates: &[Vec3], n: Vec3, tolerance: f32, out: &mut Polygon) {
    let top = candidates.iter().map(|p| p.dot(n)).fold(f32::MIN, f32::max);
    for p in candidates {
        if p.dot(n) >= top - tolerance {
            out.push(*p);
        }
    }
}

fn box_corners(frame: &super::narrowphase::BoxFrame) -> [Vec3; 8] {
    [0, 1, 2, 3, 4, 5, 6, 7]
        .map(|k| frame.centre + frame.rotation.rotate(local_corner(frame.half, k)))
}

/// The support face of `core` along `n`, unordered.
fn support_corners(core: &Core, n: Vec3) -> Polygon {
    support_corners_within(core, n, 0.0)
}

/// The corners of `core` within `band` (at least the face tolerance) of the farthest plane along `n`.
fn support_corners_within(core: &Core, n: Vec3, band: f32) -> Polygon {
    let tolerance = (reach_of(core) * FACE_TOLERANCE)
        .max(MIN_TOLERANCE)
        .max(band);
    let mut out = Polygon::empty();
    match *core {
        Core::Point(p) => out.push(p),
        Core::Segment(start, end) => keep_farthest(&[start, end], n, tolerance, &mut out),
        Core::Box(frame) => keep_farthest(&box_corners(&frame), n, tolerance, &mut out),
        Core::Hull {
            vertices,
            centre,
            rotation,
        } => {
            let mut world = [Vec3::ZERO; super::MAX_HULL_VERTICES];
            let world = &mut world[..vertices.len().min(super::MAX_HULL_VERTICES)];
            for (slot, v) in world.iter_mut().zip(vertices) {
                *slot = centre + rotation.rotate(*v);
            }
            keep_farthest(world, n, tolerance, &mut out);
        }
    }
    out
}

/// A number that grows with the angle of `(x, y)` from the x axis, from 0 to 4, with no
/// trigonometry: enough to put points in order round a centre.
fn pseudo_angle(x: f32, y: f32) -> f32 {
    let spread = x.abs() + y.abs();
    if spread <= 0.0 {
        return 0.0;
    }
    let t = x / spread;
    if y >= 0.0 { 1.0 - t } else { 3.0 + t }
}

/// Two unit vectors that span the plane perpendicular to `n`.
fn plane_frame(n: Vec3) -> (Vec3, Vec3) {
    let helper = if n.x.abs() < 0.7 { Vec3::X } else { Vec3::Y };
    let u = n.cross(helper);
    let u = u * (1.0 / u.length());
    (u, n.cross(u))
}

/// The corners put in order round their centre as seen along `n` (insertion sort by angle).
fn order_round(corners: &mut Polygon, n: Vec3) {
    let len = corners.len;
    if len < 3 {
        return;
    }
    let slice = &mut corners.corners[..len];
    let centre = slice.iter().fold(Vec3::ZERO, |sum, p| sum + *p) * (1.0 / small_count(len));
    let (u, v) = plane_frame(n);
    let key = |p: &Vec3| pseudo_angle((*p - centre).dot(u), (*p - centre).dot(v));
    for i in 1..len {
        let mut j = i;
        while j > 0 && key(&slice[j - 1]) > key(&slice[j]) {
            slice.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// The corners of a box or a hull as (corners in the shape's own frame, centre, rotation).
struct Solid<'a> {
    corners: &'a [Vec3],
    centre: Vec3,
    rotation: base::math::Quat,
}

impl Solid<'_> {
    fn world(&self, local: Vec3) -> Vec3 {
        self.centre + self.rotation.rotate(local)
    }

    fn reach(&self) -> f32 {
        self.corners
            .iter()
            .map(|c| c.length_squared())
            .fold(0.0, f32::max)
            .sqrt()
    }
}

/// What a plane must satisfy to be a face of a solid: the tolerance and the reach of the solid, and
/// the cosine of the largest angle to the search direction.
#[derive(Clone, Copy)]
struct Fit {
    tolerance: f32,
    reach: f32,
    min_cosine: f32,
}

/// The corners no further than `window` from the farthest plane along `n` (in the shape's frame),
/// in the order of the solid and at most [`MAX_WINDOW`] of them.
fn window_of(solid: &Solid, n: Vec3, window: f32) -> ([Vec3; MAX_WINDOW], usize) {
    let top = solid
        .corners
        .iter()
        .map(|c| c.dot(n))
        .fold(f32::MIN, f32::max);
    let mut found = [Vec3::ZERO; MAX_WINDOW];
    let mut len = 0;
    for c in solid.corners.iter().filter(|c| c.dot(n) >= top - window) {
        if len < MAX_WINDOW {
            found[len] = *c;
            len += 1;
        }
    }
    (found, len)
}

/// The unit normal of the plane through `a`, `b`, `c`, turned to face `n`, when the triangle is not
/// a sliver and the normal is within `min_cosine` of `n`. This is the cheap half of
/// [`face_plane`]: it looks at no other corner of the solid.
fn plane_facing(
    reach: f32,
    (a, b, c): (Vec3, Vec3, Vec3),
    n: Vec3,
    min_cosine: f32,
) -> Option<Vec3> {
    let across = (b - a).cross(c - a);
    let square = across.length_squared();
    if square <= MIN_TRIANGLE * (reach * reach) * (reach * reach) {
        return None;
    }
    // PERF: most triples of a round hull lean far from `n`; they are refused here by the squared
    // cosine, before the square root and the division of the unit normal (benchmark: the
    // `d3_hull_contacts` rolling prisms, card 8-99). The margin is far above the rounding error of
    // the exact test below, so a triple refused here is one the exact test refuses too, and a
    // triple that is not leaves the exact test alone (also a NaN, for which `<` is false): the
    // answer is the same bit for bit. Only for a minimum cosine of one half or more, where 1% of the
    // square is far above the rounding error of `along` (a relative margin does not hold near 0).
    let along = across.dot(n);
    if min_cosine >= MIN_REFUSED_COSINE
        && along * along < CLEARLY_BELOW * (min_cosine * min_cosine) * square
    {
        return None;
    }
    let mut normal = across * (1.0 / across.length());
    if normal.dot(n) < 0.0 {
        normal = -normal;
    }
    (normal.dot(n) > min_cosine).then_some(normal)
}

/// Whether every corner of the solid is on one side of the plane through `on` with outward
/// `normal`, within the tolerance: the costly half of [`face_plane`], a scan of every corner.
fn bounds(solid: &Solid, on: Vec3, normal: Vec3, tolerance: f32) -> bool {
    solid
        .corners
        .iter()
        .all(|v| (*v - on).dot(normal) <= tolerance)
}

/// The outward unit normal of the plane through `a`, `b`, `c` when it is a face plane of the solid:
/// within the cosine of `n` (a cheap test that comes first, since most triples of a round hull
/// fail it) and with every corner of the solid on one side of it (within the tolerance).
fn face_plane(solid: &Solid, triple: (Vec3, Vec3, Vec3), n: Vec3, fit: Fit) -> Option<Vec3> {
    let normal = plane_facing(fit.reach, triple, n, fit.min_cosine)?;
    bounds(solid, triple.0, normal, fit.tolerance).then_some(normal)
}

/// The face of a box or hull that `n` (in the world) points at: three corners near the farthest
/// plane that span a plane with the whole solid below it and within [`MIN_FACE_COSINE`] of `n` (the
/// first such plane in a fixed order; two such planes are less than four degrees apart); the corners
/// on it, in order round the face. `None` when no face is that close to `n`.
fn fitted_face(solid: &Solid, n: Vec3) -> Option<Face> {
    let local = solid.rotation.conjugate().rotate(n);
    let reach = solid.reach();
    let (window, len) = window_of(solid, local, reach * FACE_WINDOW);
    let fit = Fit {
        tolerance: reach * PLANE_TOLERANCE,
        reach,
        min_cosine: MIN_FACE_COSINE,
    };
    let found = triples(len).find_map(|(i, j, k)| {
        let triple = (window[i], window[j], window[k]);
        face_plane(solid, triple, local, fit).map(|p| (p, window[i]))
    });
    let (normal, on_plane) = found?;
    Some(face_on(solid, normal, on_plane, fit.tolerance))
}

/// The face of the solid on the plane through `on_plane` with outward `normal` (both in the solid's
/// frame): the corners on it, in order round the face, in the world.
fn face_on(solid: &Solid, normal: Vec3, on_plane: Vec3, tolerance: f32) -> Face {
    let mut corners = Polygon::empty();
    for v in solid.corners {
        if (*v - on_plane).dot(normal) >= -tolerance {
            corners.push(solid.world(*v));
        }
    }
    let world_normal = solid.rotation.rotate(normal);
    order_round(&mut corners, world_normal);
    Face {
        corners,
        normal: Some(world_normal),
    }
}

/// The [`WIDE_CANDIDATES`] corners farthest along `n` (in the shape's frame), the farthest first.
fn nearest_corners(solid: &Solid, n: Vec3) -> ([Vec3; WIDE_CANDIDATES], usize) {
    let mut found = [(f32::MIN, Vec3::ZERO); WIDE_CANDIDATES];
    let mut len = 0;
    for c in solid.corners {
        let height = c.dot(n);
        let at = found[..len]
            .iter()
            .position(|(h, _)| height > *h)
            .unwrap_or(len);
        if at < WIDE_CANDIDATES {
            let end = len.min(WIDE_CANDIDATES - 1);
            found.copy_within(at..end, at + 1);
            found[at] = (height, *c);
            len = (len + 1).min(WIDE_CANDIDATES);
        }
    }
    (found.map(|(_, c)| c), len)
}

/// A plane through three of the nearest corners that faces the search direction within
/// [`WIDE_FACE_COSINE`]: how much it faces it, its normal, and which corner it passes through.
#[derive(Clone, Copy)]
struct Leaning {
    cosine: f32,
    normal: Vec3,
    on: usize,
}

/// The triples of [`WIDE_CANDIDATES`] corners.
const WIDE_TRIPLES: usize = WIDE_CANDIDATES * (WIDE_CANDIDATES - 1) * (WIDE_CANDIDATES - 2) / 6;

/// The place in `list` of the plane that faces most, the first of equal ones.
fn most_leaning(list: &[Leaning]) -> usize {
    let mut best = 0;
    for (at, plane) in list.iter().enumerate() {
        if plane.cosine > list[best].cosine {
            best = at;
        }
    }
    best
}

/// The face of a box or hull nearest to `n` within [`WIDE_FACE_COSINE`] (card 8-71): the second try
/// when [`fitted_face`] found none, because the normal of EPA at a corner of a round hull leans
/// further than 1.8 degrees from the face that touches. Of the planes spanned by the corners nearest
/// `n` that bound the whole solid, the one that faces `n` most (the first of equal ones); `None`
/// when there is none.
///
/// The planes are put in the order of how much they face `n` and the first that bounds the solid is
/// taken, so the costly scan of every corner stops at the first plane that passes (card 8-78): the
/// answer is the one the scan of every plane gives, bit for bit.
fn widest_face(solid: &Solid, n: Vec3) -> Option<Face> {
    let local = solid.rotation.conjugate().rotate(n);
    let reach = solid.reach();
    let (near, len) = nearest_corners(solid, local);
    let tolerance = reach * PLANE_TOLERANCE;
    let mut leaning = [Leaning {
        cosine: f32::NEG_INFINITY,
        normal: Vec3::ZERO,
        on: 0,
    }; WIDE_TRIPLES];
    let mut count = 0;
    for (i, j, k) in triples(len) {
        let triple = (near[i], near[j], near[k]);
        if let Some(normal) = plane_facing(reach, triple, local, WIDE_FACE_COSINE) {
            leaning[count] = Leaning {
                cosine: normal.dot(local),
                normal,
                on: i,
            };
            count += 1;
        }
    }
    for _ in 0..count {
        let at = most_leaning(&leaning[..count]);
        let plane = leaning[at];
        if bounds(solid, near[plane.on], plane.normal, tolerance) {
            return Some(face_on(solid, plane.normal, near[plane.on], tolerance));
        }
        // A plane that has been tried faces nothing, so it is not picked again.
        leaning[at].cosine = f32::NEG_INFINITY;
    }
    None
}

/// Corner `k` (its three low bits are the signs of x, y, z) of a box of half sizes `half`.
fn local_corner(half: Vec3, k: usize) -> Vec3 {
    let sign = |bit: usize| if k >> bit & 1 == 1 { 1.0 } else { -1.0 };
    Vec3::new(half.x * sign(0), half.y * sign(1), half.z * sign(2))
}

/// The solid of a box or a hull, with its corners in its own frame (`scratch` holds a box's).
fn solid_of<'a>(core: &Core<'a>, scratch: &'a mut [Vec3; 8]) -> Option<Solid<'a>> {
    match *core {
        Core::Box(frame) => {
            for (k, slot) in scratch.iter_mut().enumerate() {
                *slot = local_corner(frame.half, k);
            }
            Some(Solid {
                corners: scratch,
                centre: frame.centre,
                rotation: frame.rotation,
            })
        }
        Core::Hull {
            vertices,
            centre,
            rotation,
        } => Some(Solid {
            corners: vertices,
            centre,
            rotation,
        }),
        _ => None,
    }
}

/// The support face of `core` along `n`: a fitted face of a box or hull, else the corners within
/// the tolerance of the farthest plane (a polygon, an edge or a point).
fn support_face(core: &Core, n: Vec3, wide: bool) -> Face {
    let mut scratch = [Vec3::ZERO; 8];
    let fitted = solid_of(core, &mut scratch).and_then(|solid| {
        if wide {
            widest_face(&solid, n)
        } else {
            fitted_face(&solid, n)
        }
    });
    if let Some(face) = fitted {
        return face;
    }
    let mut corners = support_corners(core, n);
    order_round(&mut corners, n);
    Face {
        corners,
        normal: None,
    }
}

/// The inward side planes of a reference face as (point, inward direction in the plane) pairs.
struct Sides {
    planes: [(Vec3, Vec3); CAPACITY],
    len: usize,
}

impl Sides {
    fn of(face: &Face, n: Vec3) -> Self {
        let corners = face.corners.as_slice();
        let mut sides = Self {
            planes: [(Vec3::ZERO, Vec3::ZERO); CAPACITY],
            len: 0,
        };
        let turn = winding(corners, n);
        let edges = corners
            .iter()
            .enumerate()
            .map(|(i, from)| (*from, corners[(i + 1) % corners.len()] - *from))
            .filter(|(_, edge)| edge.length_squared() > MIN_EDGE_SQUARED);
        for (slot, (from, edge)) in sides.planes.iter_mut().zip(edges) {
            *slot = (from, n.cross(edge) * turn);
            sides.len += 1;
        }
        sides
    }

    /// How far `x` is outside side `k`, in the units of the side's direction (positive: outside).
    fn outside(&self, k: usize, x: Vec3) -> f32 {
        let (from, inward) = self.planes[k];
        -(x - from).dot(inward)
    }
}

/// Plus one when the corners run counter-clockwise seen from the tip of `n`, minus one otherwise.
fn winding(corners: &[Vec3], n: Vec3) -> f32 {
    let first = corners.first().copied().unwrap_or(Vec3::ZERO);
    let area: f32 = corners
        .windows(2)
        .map(|pair| (pair[0] - first).cross(pair[1] - first).dot(n))
        .sum();
    if area >= 0.0 { 1.0 } else { -1.0 }
}

/// The part of the incident face that is inside the sides of the reference face.
fn clip_to(sides: &Sides, incident: &Face) -> Polygon {
    let corners = incident.corners.as_slice();
    match corners {
        [] => Polygon::empty(),
        [point] => {
            let inside = (0..sides.len).all(|k| sides.outside(k, *point) <= 0.0);
            let mut out = Polygon::empty();
            if inside {
                out.push(*point);
            }
            out
        }
        [from, to] => clip_segment(sides, (*from, *to)),
        _ => {
            let mut polygon = incident.corners;
            for k in 0..sides.len {
                polygon = polygon.clipped(|x| sides.outside(k, x));
            }
            polygon
        }
    }
}

fn clip_segment(sides: &Sides, (from, to): (Vec3, Vec3)) -> Polygon {
    let (mut start, mut end) = (0.0_f32, 1.0_f32);
    for k in 0..sides.len {
        let (d_from, d_to) = (sides.outside(k, from), sides.outside(k, to));
        if d_from > 0.0 && d_to > 0.0 {
            return Polygon::empty();
        }
        if d_from > 0.0 {
            start = start.max(d_from / (d_from - d_to));
        } else if d_to > 0.0 {
            end = end.min(d_from / (d_from - d_to));
        }
    }
    let mut out = Polygon::empty();
    if start <= end {
        let (first, second) = (from + (to - from) * start, from + (to - from) * end);
        out.push(first);
        // A segment of no length (or cut to nothing) is one point, not two on top of each other.
        if (second - first).length_squared() > MIN_EDGE_SQUARED {
            out.push(second);
        }
    }
    out
}

/// Which shape's face is the reference.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reference {
    First,
    Second,
}

/// The reference is the larger of the fitted faces; with none fitted, the larger polygon.
fn choose_reference(a: &Face, b: &Face, n: Vec3) -> Option<Reference> {
    let (a_fits, b_fits) = (a.normal.is_some(), b.normal.is_some());
    let (usable_a, usable_b) = if a_fits || b_fits {
        (a_fits, b_fits)
    } else {
        (a.is_polygon(), b.is_polygon())
    };
    match (usable_a, usable_b) {
        (false, false) => None,
        (true, false) => Some(Reference::First),
        (false, true) => Some(Reference::Second),
        (true, true) => Some(if area(a, n) >= area(b, n) {
            Reference::First
        } else {
            Reference::Second
        }),
    }
}

fn area(face: &Face, n: Vec3) -> f32 {
    let corners = face.corners.as_slice();
    let first = corners.first().copied().unwrap_or(Vec3::ZERO);
    corners
        .windows(2)
        .map(|pair| (pair[0] - first).cross(pair[1] - first).dot(n))
        .sum::<f32>()
        .abs()
}

/// The contact points of the clipped incident corners against the reference plane.
fn contacts(
    clipped: &Polygon,
    reference: (Reference, Vec3),
    n: Vec3,
    radii: (f32, f32),
    (margin, floor): (f32, f32),
) -> Candidates {
    let (which, plane_point) = reference;
    let shift = (radii.0 - radii.1) * 0.5;
    let mut list = Candidates::empty();
    for corner in clipped.as_slice() {
        let (gap, middle) = match which {
            Reference::First => {
                let gap = (*corner - plane_point).dot(n);
                (gap, *corner - n * (gap * 0.5))
            }
            Reference::Second => {
                let gap = (plane_point - *corner).dot(n);
                (gap, *corner + n * (gap * 0.5))
            }
        };
        let separation = gap - radii.0 - radii.1;
        if (floor..=margin).contains(&separation) {
            list.push(ContactPoint3 {
                point: middle + n * shift,
                separation,
            });
        }
    }
    list
}

/// The incident face when the reference is fitted: its own fitted face, or else the corners of its
/// shape within the tolerance (at least `margin`, see below) of the farthest plane along `toward`
/// (the reference's normal).
fn incident_for(core: &Core, fitted: &Face, toward: Vec3, margin: f32) -> Face {
    if fitted.normal.is_some() {
        return *fitted;
    }
    // Every corner within the margin of the farthest plane is a contact that may be kept: a tilted
    // edge keeps both its ends (card 8-71); [`contacts`] drops what is beyond the margin.
    let mut corners = support_corners_within(core, toward, margin);
    order_round(&mut corners, toward);
    Face {
        corners,
        normal: None,
    }
}

/// What GJK and EPA said about a pair: the one-point manifold, the margin, and the band of the incident shape
/// (how far from its farthest plane a corner may be and still be a contact point, see
/// [`incident_for`]).
#[derive(Clone, Copy)]
struct Ask {
    /// The one-point answer of GJK and EPA: its normal is the normal of the pair, and its depth
    /// bounds the points (see [`depth_floor`]).
    single: Manifold3,
    margin: f32,
    band: f32,
}

/// Whether a contact is shallow: no deeper than [`FACE_WINDOW`] of the reach of the smaller shape.
/// The normal of a shallow contact is ambiguous and a face may be looked for wider; that of a deep
/// overlap is the true way out and the answer of the first try stands.
fn is_shallow(first: &Convex, second: &Convex, single: &Manifold3) -> bool {
    let reach = reach_of(&first.core).min(reach_of(&second.core));
    deepest(single) >= -reach * FACE_WINDOW
}

/// Whether `manifold` says what GJK and EPA say, to within [`WIDE_AGREEMENT`] of the reach of the
/// smaller shape: its deepest point is as deep as theirs, and its normal separates the pair as
/// well. A face may replace the normal of EPA only then (a ridge against a ridge has faces within
/// 45 degrees that lose much of the gap, and a plane that is not where the shapes touch puts the
/// points metres off).
fn agrees(first: &Convex, second: &Convex, manifold: &Manifold3, single: &Manifold3) -> bool {
    let slack = reach_of(&first.core).min(reach_of(&second.core)) * WIDE_AGREEMENT;
    let gap = gjk::gap_along(first, second, manifold.normal);
    (deepest(manifold) - deepest(single)).abs() <= slack
        && gap >= gjk::gap_along(first, second, single.normal) - slack
}

/// The deepest separation of a manifold.
fn deepest(manifold: &Manifold3) -> f32 {
    manifold
        .active()
        .iter()
        .map(|p| p.separation)
        .fold(f32::MAX, f32::min)
}

/// The lowest separation a contact point of the pair may have: GJK's depth less [`DEPTH_ALLOWANCE`]
/// of the reach (with the radius) of the smaller shape, and at least [`MIN_TOLERANCE`] so that
/// rounding cannot drop a point of a shape with no reach.
fn depth_floor(first: &Convex, second: &Convex, single: &Manifold3) -> f32 {
    // A round shape's radius is part of how far it can reach: a sphere has a point as its core.
    let reach = (reach_of(&first.core) + first.radius).min(reach_of(&second.core) + second.radius);
    deepest(single) - (reach * DEPTH_ALLOWANCE).max(MIN_TOLERANCE)
}

/// The manifold of `reference` (the face of `which`) against the other face `other`, clipped, with
/// `normal` the one of EPA; `None` when no point is within the margin.
fn clip_manifold(
    (first, second): (&Convex, &Convex),
    which: Reference,
    (reference, other): (&Face, &Face),
    ask: Ask,
) -> Option<Manifold3> {
    let Ask {
        single,
        margin,
        band,
    } = ask;
    let normal = single.normal;
    let floor = depth_floor(first, second, &single);
    let other_core = match which {
        Reference::First => &second.core,
        Reference::Second => &first.core,
    };
    // The exact normal of a fitted reference replaces the one from EPA; it points from the first
    // shape to the second.
    let n = match (reference.normal, which) {
        (Some(fitted), Reference::First) => fitted,
        (Some(fitted), Reference::Second) => -fitted,
        (None, _) => normal,
    };
    let toward = if which == Reference::First { -n } else { n };
    let incident = if reference.normal.is_some() {
        incident_for(other_core, other, toward, band)
    } else {
        *other
    };
    let clipped = clip_to(&Sides::of(reference, n), &incident);
    let radii = (first.radius, second.radius);
    let candidates = contacts(
        &clipped,
        (which, reference.first()),
        n,
        radii,
        (margin, floor),
    );
    if candidates.len == 0 {
        return None;
    }
    if candidates.len > MAX_MANIFOLD_POINTS {
        return Some(Manifold3::new(n, &candidates.reduced(n)));
    }
    Some(Manifold3::new(n, candidates.as_slice()))
}

/// The second try when neither shape has a face within 1.8 degrees of the normal of EPA (card
/// 8-71): the normal at a corner of a round hull leans because it is ambiguous, and the contact is
/// that of the nearest face of either shape. Only for a contact no deeper than [`FACE_WINDOW`] of
/// the reach of the smaller shape (the normal of a deep overlap is the true way out), and only when
/// its normal separates the pair as well (see [`agrees`]).
fn widened(first: &Convex, second: &Convex, single: &Manifold3, margin: f32) -> Option<Manifold3> {
    if !is_shallow(first, second, single) {
        return None;
    }
    let (a, b) = (
        support_face(&first.core, single.normal, true),
        support_face(&second.core, -single.normal, true),
    );
    let which = choose_reference(&a, &b, single.normal)?;
    let faces = if which == Reference::First {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let ask = Ask {
        single: *single,
        margin,
        band: margin,
    };
    let manifold = clip_manifold((first, second), which, faces, ask)?;
    agrees(first, second, &manifold, single).then_some(manifold)
}

/// The manifold of two convex shapes, at most four points, normal from the first to the second;
/// `None` when they are further apart than `margin`.
pub(super) fn contact(first: &Convex, second: &Convex, margin: f32) -> Option<Manifold3> {
    let single = gjk::contact(first, second, margin)?;
    let (a, b) = (
        support_face(&first.core, single.normal, false),
        support_face(&second.core, -single.normal, false),
    );
    let Some(which) = choose_reference(&a, &b, single.normal) else {
        return Some(widened(first, second, &single, margin).unwrap_or(single));
    };
    let faces = if which == Reference::First {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let pair = (first, second);
    let ask = Ask {
        single,
        margin,
        band: margin,
    };
    // The answer of a fitted reference stands as it was (the corners on the farthest plane of the
    // other shape); when that shape has no face and the contact is shallow, the corners within the
    // margin of that plane are added (card 8-71): a tilted edge keeps both ends. The normal and the
    // reference are the same, so only points beyond the old ones can appear.
    let narrow = clip_manifold(pair, which, faces, Ask { band: 0.0, ..ask });
    // Only a manifold of one point can gain: two points are an edge or a face already.
    let lone = narrow.is_some_and(|m| m.count == 1);
    let wider = (lone && faces.1.normal.is_none() && is_shallow(first, second, &single))
        .then(|| clip_manifold(pair, which, faces, ask))
        .flatten();
    let manifold = wider.or(narrow);
    Some(manifold.unwrap_or(single))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3::narrowphase::BoxFrame;
    use base::math::Quat;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1.0e-4
    }

    fn polygon(points: &[Vec3]) -> Polygon {
        let mut out = Polygon::empty();
        for p in points {
            out.push(*p);
        }
        out
    }

    fn face(points: &[Vec3], normal: Option<Vec3>) -> Face {
        Face {
            corners: polygon(points),
            normal,
        }
    }

    /// The corners of a square of half side `h` in the plane y = 0, counter-clockwise from above.
    fn square(h: f32) -> [Vec3; 4] {
        [
            Vec3::new(h, 0.0, h),
            Vec3::new(h, 0.0, -h),
            Vec3::new(-h, 0.0, -h),
            Vec3::new(-h, 0.0, h),
        ]
    }

    /// The fit of a face of the first try: the tolerance, the reach and 1.8 degrees.
    fn fit(tolerance: f32, reach: f32) -> Fit {
        Fit {
            tolerance,
            reach,
            min_cosine: MIN_FACE_COSINE,
        }
    }

    fn unit_box_corners() -> [Vec3; 8] {
        [0, 1, 2, 3, 4, 5, 6, 7].map(|k| local_corner(Vec3::new(0.5, 0.5, 0.5), k))
    }

    fn cube() -> Solid<'static> {
        // The corners live as long as the program, which keeps the test short.
        let corners: &'static [Vec3; 8] = Box::leak(Box::new(unit_box_corners()));
        Solid {
            corners,
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        }
    }

    #[test]
    fn a_face_is_a_polygon_from_three_corners() {
        assert!(!face(&[Vec3::ZERO, Vec3::X], None).is_polygon());
        assert!(face(&[Vec3::ZERO, Vec3::X, Vec3::Y], None).is_polygon());
        assert!(near(face(&[Vec3::Y, Vec3::X], None).first(), Vec3::Y));
        assert!(near(face(&[], None).first(), Vec3::ZERO));
    }

    #[test]
    fn the_reach_of_a_shape_is_its_farthest_corner_from_the_centre() {
        let hull = [Vec3::new(3.0, 4.0, 0.0), Vec3::new(1.0, 0.0, 0.0)];
        let core = Core::Hull {
            vertices: &hull,
            centre: Vec3::new(9.0, 9.0, 9.0),
            rotation: Quat::IDENTITY,
        };
        assert!((reach_of(&core) - 5.0).abs() < 1e-5);
        assert!((reach_of(&Core::Point(Vec3::X)) - 0.0).abs() < 1e-9);
        let segment = Core::Segment(Vec3::new(-3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
        assert!((reach_of(&segment) - 3.0).abs() < 1e-5);
        let frame = BoxFrame {
            centre: Vec3::new(5.0, 5.0, 5.0),
            rotation: Quat::IDENTITY,
            half: Vec3::new(1.0, 2.0, 2.0),
        };
        assert!((reach_of(&Core::Box(frame)) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn box_corners_are_the_eight_corners_turned_and_moved() {
        let turn =
            Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let frame = BoxFrame {
            centre: Vec3::new(10.0, 0.0, 0.0),
            rotation: turn,
            half: Vec3::new(1.0, 2.0, 3.0),
        };
        let corners = box_corners(&frame);
        // Corner 1 is (+1, -2, -3); a quarter turn about z takes (x, y) to (-y, x): (2, 1, -3).
        assert!(
            near(corners[1], Vec3::new(12.0, 1.0, -3.0)),
            "{:?}",
            corners[1]
        );
        assert!(
            near(corners[6], Vec3::new(8.0, -1.0, 3.0)),
            "{:?}",
            corners[6]
        );
        let distinct = corners
            .iter()
            .enumerate()
            .all(|(i, a)| corners[i + 1..].iter().all(|b| !near(*a, *b)));
        assert!(distinct);
    }

    #[test]
    fn the_corner_of_a_box_has_the_sign_of_each_bit_times_the_half_size() {
        let half = Vec3::new(1.0, 2.0, 3.0);
        assert!(near(local_corner(half, 0), Vec3::new(-1.0, -2.0, -3.0)));
        assert!(near(local_corner(half, 1), Vec3::new(1.0, -2.0, -3.0)));
        assert!(near(local_corner(half, 2), Vec3::new(-1.0, 2.0, -3.0)));
        assert!(near(local_corner(half, 4), Vec3::new(-1.0, -2.0, 3.0)));
        assert!(near(local_corner(half, 7), half));
    }

    #[test]
    fn the_support_corners_of_a_shape_are_those_within_the_tolerance_of_the_farthest_plane() {
        // A segment of length 2 has reach 1 and tolerance 1e-3.
        let tilted =
            |rise: f32| Core::Segment(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, rise, 0.0));
        assert_eq!(support_corners(&tilted(5.0e-4), Vec3::Y).len, 2);
        assert_eq!(support_corners(&tilted(2.0e-3), Vec3::Y).len, 1);
        assert_eq!(support_corners(&Core::Point(Vec3::X), Vec3::Y).len, 1);
        let frame = BoxFrame {
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            half: Vec3::new(0.5, 0.5, 0.5),
        };
        assert_eq!(support_corners(&Core::Box(frame), Vec3::Y).len, 4);
        let hull = [
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0005, 1.0),
        ];
        let core = Core::Hull {
            vertices: &hull,
            centre: Vec3::new(0.0, 5.0, 0.0),
            rotation: Quat::IDENTITY,
        };
        let found = support_corners(&core, Vec3::Y);
        assert_eq!(found.len, 1);
        assert!(near(found.as_slice()[0], Vec3::new(0.0, 6.0, 0.0)));
    }

    #[test]
    fn a_pseudo_angle_grows_round_the_circle() {
        let at = |x: f32, y: f32| pseudo_angle(x, y);
        assert!((at(1.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((at(0.5, 0.5) - 0.5).abs() < 1e-6);
        assert!((at(0.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((at(-1.0, 0.0) - 2.0).abs() < 1e-6);
        assert!((at(0.0, -1.0) - 3.0).abs() < 1e-6);
        assert!((at(1.0, -1.0) - 3.5).abs() < 1e-6);
        assert!((at(0.0, 0.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn a_plane_frame_is_two_unit_vectors_across_the_normal() {
        for n in [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::new(0.6, 0.0, 0.8),
            Vec3::new(0.0, 0.8, 0.6),
        ] {
            let (u, v) = plane_frame(n);
            assert!((u.length() - 1.0).abs() < 1e-5 && (v.length() - 1.0).abs() < 1e-5);
            assert!(u.dot(n).abs() < 1e-5 && v.dot(n).abs() < 1e-5 && u.dot(v).abs() < 1e-5);
        }
        // A normal along x gets the y axis as its helper (x cross y is z); a normal along y gets the
        // x axis (y cross x is minus z).
        assert!(near(plane_frame(Vec3::X).0, Vec3::Z));
        assert!(near(plane_frame(Vec3::Y).0, -Vec3::Z));
    }

    #[test]
    fn scattered_corners_are_put_in_order_round_the_face() {
        // An irregular convex pentagon far from the origin, in the plane y = 3, given out of order.
        let ring = [
            Vec3::new(10.0, 3.0, 20.0),
            Vec3::new(14.0, 3.0, 21.0),
            Vec3::new(15.0, 3.0, 24.0),
            Vec3::new(12.0, 3.0, 26.0),
            Vec3::new(9.0, 3.0, 24.0),
        ];
        let shuffled = [ring[3], ring[0], ring[4], ring[2], ring[1]];
        let mut corners = polygon(&shuffled);
        order_round(&mut corners, Vec3::Y);
        let sorted = corners.as_slice();
        let turns: Vec<f32> = (0..5)
            .map(|i| {
                let (a, b, c) = (sorted[i], sorted[(i + 1) % 5], sorted[(i + 2) % 5]);
                (b - a).cross(c - b).dot(Vec3::Y)
            })
            .collect();
        let all_left = turns.iter().all(|t| *t > 0.0);
        let all_right = turns.iter().all(|t| *t < 0.0);
        assert!(all_left || all_right, "not a convex ring: {turns:?}");
        // Fewer than three corners are left as they are, and three are put in order too.
        let mut two = polygon(&[Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 1.0)]);
        order_round(&mut two, Vec3::Y);
        assert!(near(two.as_slice()[0], Vec3::new(-1.0, 0.0, -1.0)));
        let mut three = polygon(&[ring[2], ring[0], ring[1]]);
        order_round(&mut three, Vec3::Y);
        let t = three.as_slice();
        assert!((t[1] - t[0]).cross(t[2] - t[1]).dot(Vec3::Y).abs() > 0.0);
        let first_turn = (t[1] - t[0]).cross(t[2] - t[1]).dot(Vec3::Y);
        let second_turn = (t[2] - t[1]).cross(t[0] - t[2]).dot(Vec3::Y);
        assert!(first_turn * second_turn > 0.0);
    }

    #[test]
    fn the_solid_reach_is_the_farthest_corner() {
        let corners = [Vec3::new(3.0, 4.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        let solid = Solid {
            corners: &corners,
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        assert!((solid.reach() - 5.0).abs() < 1e-5);
        assert!(near(solid.world(Vec3::X), Vec3::X));
    }

    #[test]
    fn the_window_holds_the_corners_within_the_distance_of_the_farthest_plane_up_to_its_size() {
        let corners = [
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 0.96, 0.0),
            Vec3::new(2.0, 0.9, 0.0),
        ];
        let solid = Solid {
            corners: &corners,
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        assert_eq!(window_of(&solid, Vec3::Y, 0.05).1, 2);
        assert_eq!(window_of(&solid, Vec3::Y, 0.1).1, 3);
        // The corner exactly one window below the top is in (a distance of 0.25 is exact).
        let exact = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.75, 0.0)];
        let solid = Solid {
            corners: &exact,
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        assert_eq!(window_of(&solid, Vec3::Y, 0.25).1, 2);
        // Many corners on one plane: only the first MAX_WINDOW are kept.
        let many = [Vec3::Y; 20];
        let solid = Solid {
            corners: &many,
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        assert_eq!(window_of(&solid, Vec3::Y, 0.1).1, MAX_WINDOW);
    }

    #[test]
    fn three_corners_span_a_face_plane_only_when_every_corner_is_below_it() {
        let solid = cube();
        let top = (
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(0.5, 0.5, -0.5),
            Vec3::new(-0.5, 0.5, -0.5),
        );
        let normal = face_plane(&solid, top, Vec3::Y, fit(1e-4, solid.reach()));
        assert!(normal.is_some_and(|n| near(n, Vec3::Y)), "{normal:?}");
        // The same corners in the other order give the other cross product: it is turned to `n`.
        let reversed = (top.2, top.1, top.0);
        assert!(
            face_plane(&solid, reversed, Vec3::Y, fit(1e-4, solid.reach()))
                .is_some_and(|n| near(n, Vec3::Y))
        );
        // The plane of the bottom face seen from above has the cube on its wrong side.
        let bottom = (
            Vec3::new(0.5, -0.5, 0.5),
            Vec3::new(0.5, -0.5, -0.5),
            Vec3::new(-0.5, -0.5, -0.5),
        );
        assert!(face_plane(&solid, bottom, Vec3::Y, fit(1e-4, solid.reach())).is_none());
        // A plane through the middle of the cube has corners on both sides.
        let middle = (
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(0.5, -0.5, -0.5),
        );
        assert!(face_plane(&solid, middle, Vec3::Y, fit(1e-4, solid.reach())).is_none());
        // Corners on a line span no plane.
        let line = (Vec3::ZERO, Vec3::X, Vec3::new(2.0, 0.0, 0.0));
        assert!(face_plane(&solid, line, Vec3::Y, fit(1e-4, solid.reach())).is_none());
        // A tiny triangle spans no plane worth the name.
        let tiny = (
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(0.5, 0.5, 0.4999),
            Vec3::new(0.4999, 0.5, 0.5),
        );
        assert!(face_plane(&solid, tiny, Vec3::Y, fit(1e-4, solid.reach())).is_none());
    }

    #[test]
    fn a_plane_that_does_not_face_the_search_direction_is_not_taken() {
        // The +x face of the cube is a face plane, but it is a quarter turn from `n = y`: it is
        // refused before the corners are scanned (the cheap test that makes a search of many
        // triples cheap).
        let side = (
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(0.5, 0.5, -0.5),
            Vec3::new(0.5, -0.5, -0.5),
        );
        let scale = fit(1.0e-4, cube().reach());
        assert!(face_plane(&cube(), side, Vec3::X, scale).is_some());
        assert!(face_plane(&cube(), side, Vec3::Y, scale).is_none());
        // Just inside and just outside the angle (1.8 degrees: a lean of 0.0314).
        let lean = |tilt: f32| {
            let n = Vec3::new(1.0, tilt, 0.0);
            n * (1.0 / n.length())
        };
        assert!(face_plane(&cube(), side, lean(0.025), scale).is_some());
        assert!(face_plane(&cube(), side, lean(0.04), scale).is_none());
    }

    #[test]
    fn a_corner_just_over_the_plane_is_on_the_face_up_to_the_tolerance() {
        let lifted = |by: f32| {
            let mut corners = unit_box_corners();
            corners[7] = Vec3::new(0.5, 0.5 + by, 0.5);
            corners
        };
        let top = (
            Vec3::new(0.5, 0.5, -0.5),
            Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(-0.5, 0.5, 0.5),
        );
        for (by, found) in [(0.5e-4, true), (2.0e-4, false)] {
            let corners = lifted(by);
            let solid = Solid {
                corners: &corners,
                centre: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            };
            assert_eq!(
                face_plane(&solid, top, Vec3::Y, fit(1e-4, solid.reach())).is_some(),
                found,
                "{by}"
            );
        }
    }

    /// The face of the cube that a normal leaning by `lean` finds, with the checks of a top face.
    fn top_face_found_for(lean: f32) -> bool {
        let n = Vec3::new(lean, 1.0, -lean * 0.5);
        let n = n * (1.0 / n.length());
        fitted_face(&cube(), n).is_some_and(|found| {
            found.normal.is_some_and(|f| near(f, Vec3::Y))
                && found.corners.len == 4
                && found
                    .corners
                    .as_slice()
                    .iter()
                    .all(|c| (c.y - 0.5).abs() < 1e-5)
        })
    }

    #[test]
    fn a_face_is_fitted_to_the_cube_whatever_the_normal_leans_by_a_little() {
        for lean in [0.0_f32, 0.01, -0.016, 0.02] {
            assert!(top_face_found_for(lean), "no face for a lean of {lean}");
        }
        // Past 1.8 degrees (0.04 here) and at 10 degrees it is no face of a cube: the edge or corner
        // is what touches.
        for lean in [0.04_f32, 0.2] {
            assert!(!top_face_found_for(lean), "a face for a lean of {lean}");
        }
    }

    #[test]
    fn a_fitted_face_is_turned_and_moved_with_the_shape() {
        let corners = unit_box_corners();
        let turn =
            Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let solid = Solid {
            corners: &corners,
            centre: Vec3::new(5.0, 0.0, 0.0),
            rotation: turn,
        };
        // The local +y face now looks along -x.
        let Some(found) = fitted_face(&solid, -Vec3::X) else {
            panic!("a face");
        };
        assert!(found.normal.is_some_and(|f| near(f, -Vec3::X)));
        assert!(
            found
                .corners
                .as_slice()
                .iter()
                .all(|c| (c.x - 4.5).abs() < 1e-4)
        );
    }

    #[test]
    fn a_solid_is_a_box_or_a_hull_and_nothing_else() {
        let mut scratch = [Vec3::ZERO; 8];
        assert!(solid_of(&Core::Point(Vec3::ZERO), &mut scratch).is_none());
        assert!(solid_of(&Core::Segment(Vec3::ZERO, Vec3::X), &mut scratch).is_none());
        let frame = BoxFrame {
            centre: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::IDENTITY,
            half: Vec3::new(1.0, 1.0, 1.0),
        };
        let solid = solid_of(&Core::Box(frame), &mut scratch);
        assert!(
            solid.is_some_and(|s| s.corners.len() == 8 && near(s.centre, Vec3::new(1.0, 2.0, 3.0)))
        );
        let vertices = [Vec3::X, Vec3::Y];
        let hull = Core::Hull {
            vertices: &vertices,
            centre: Vec3::Z,
            rotation: Quat::IDENTITY,
        };
        assert!(solid_of(&hull, &mut scratch).is_some_and(|s| s.corners.len() == 2));
    }

    #[test]
    fn the_support_face_of_a_segment_or_point_has_no_normal() {
        let found = support_face(
            &Core::Segment(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)),
            Vec3::Y,
            false,
        );
        assert!(found.normal.is_none() && found.corners.len == 2);
        let found = support_face(&Core::Point(Vec3::X), Vec3::Y, false);
        assert!(found.normal.is_none() && found.corners.len == 1);
    }

    /// The largest distance outside any side of `sides` for the point `at`.
    fn most_outside(sides: &Sides, at: Vec3) -> f32 {
        (0..sides.len)
            .map(|k| sides.outside(k, at))
            .fold(f32::MIN, f32::max)
    }

    #[test]
    fn the_sides_of_a_face_point_inward_whichever_way_it_is_wound() {
        let ccw = square(1.0);
        let cw = [ccw[3], ccw[2], ccw[1], ccw[0]];
        for corners in [ccw, cw] {
            let sides = Sides::of(&face(&corners, None), Vec3::Y);
            assert_eq!(sides.len, 4);
            // The centre is inside every side, a point beyond one edge is outside it, and the
            // distance grows with the distance from the edge (an edge of length 2).
            assert!(most_outside(&sides, Vec3::ZERO) < 0.0);
            let (near_edge, far) = (
                most_outside(&sides, Vec3::new(2.0, 0.0, 0.0)),
                most_outside(&sides, Vec3::new(3.0, 0.0, 0.0)),
            );
            assert!(near_edge > 0.0);
            assert!((far - near_edge - 2.0).abs() < 1e-4, "{far} {near_edge}");
        }
        // A repeated corner makes an edge of no length, which gives no side.
        let repeated = [ccw[0], ccw[0], ccw[1], ccw[2], ccw[3]];
        assert_eq!(Sides::of(&face(&repeated, None), Vec3::Y).len, 4);
    }

    #[test]
    fn the_winding_is_plus_one_counter_clockwise_and_minus_one_clockwise() {
        let ccw = square(1.0);
        let cw = [ccw[3], ccw[2], ccw[1], ccw[0]];
        assert!((winding(&ccw, Vec3::Y) - 1.0).abs() < 1e-6);
        assert!((winding(&cw, Vec3::Y) + 1.0).abs() < 1e-6);
        assert!((winding(&ccw, -Vec3::Y) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_area_of_a_face_is_twice_its_surface_as_the_sum_of_the_fan_of_triangles() {
        assert!((area(&face(&square(1.0), None), Vec3::Y) - 8.0).abs() < 1e-4);
        assert!((area(&face(&square(2.0), None), Vec3::Y) - 32.0).abs() < 1e-3);
        assert!((area(&face(&[], None), Vec3::Y)).abs() < 1e-9);
    }

    fn sides_of_unit_square() -> Sides {
        Sides::of(&face(&square(1.0), None), Vec3::Y)
    }

    #[test]
    fn nothing_is_left_of_no_corners_and_a_point_is_kept_only_inside() {
        let sides = sides_of_unit_square();
        assert_eq!(clip_to(&sides, &face(&[], None)).len, 0);
        let inside = clip_to(&sides, &face(&[Vec3::new(0.5, 0.0, 0.5)], None));
        assert_eq!(inside.len, 1);
        assert_eq!(
            clip_to(&sides, &face(&[Vec3::new(1.5, 0.0, 0.0)], None)).len,
            0
        );
        // On the border counts as inside.
        assert_eq!(
            clip_to(&sides, &face(&[Vec3::new(1.0, 0.0, 0.0)], None)).len,
            1
        );
    }

    #[test]
    fn a_segment_is_cut_to_the_sides_of_the_reference() {
        let sides = sides_of_unit_square();
        let cut = |from: Vec3, to: Vec3| clip_to(&sides, &face(&[from, to], None));
        // Across the whole square: both ends are cut off.
        let across = cut(Vec3::new(-2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(across.len, 2);
        assert!(near(across.as_slice()[0], Vec3::new(-1.0, 0.0, 0.0)));
        assert!(near(across.as_slice()[1], Vec3::new(1.0, 0.0, 0.0)));
        // From inside to outside, and from outside to inside.
        let out = cut(Vec3::new(0.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
        assert!(
            out.len == 2
                && near(out.as_slice()[0], Vec3::ZERO)
                && near(out.as_slice()[1], Vec3::new(1.0, 0.0, 0.0))
        );
        let back = cut(Vec3::new(0.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 0.0));
        assert!(
            back.len == 2
                && near(back.as_slice()[0], Vec3::new(0.0, 0.0, 1.0))
                && near(back.as_slice()[1], Vec3::ZERO)
        );
        // Entirely inside is left as it is; entirely outside on one side is dropped.
        assert_eq!(
            cut(Vec3::new(-0.5, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0)).len,
            2
        );
        assert_eq!(
            cut(Vec3::new(2.0, 0.0, -1.0), Vec3::new(2.0, 0.0, 1.0)).len,
            0
        );
        // Outside two different sides at each end but through the corner region: x + z = -1.2.
        let diagonal = cut(Vec3::new(-2.0, 0.0, 0.8), Vec3::new(0.8, 0.0, -2.0));
        assert_eq!(diagonal.len, 2, "{:?}", diagonal.as_slice());
        assert!(near(diagonal.as_slice()[0], Vec3::new(-1.0, 0.0, -0.2)));
        assert!(near(diagonal.as_slice()[1], Vec3::new(-0.2, 0.0, -1.0)));
        // A segment that only passes the corner outside is dropped: x + z = -2.5, the square's
        // nearest corner is at -2.
        assert_eq!(
            cut(Vec3::new(-3.0, 0.0, 0.5), Vec3::new(0.5, 0.0, -3.0)).len,
            0
        );
        // A segment of no length inside is one point.
        assert_eq!(
            cut(Vec3::new(0.2, 0.0, 0.2), Vec3::new(0.2, 0.0, 0.2)).len,
            1
        );
    }

    #[test]
    fn a_polygon_is_cut_to_the_sides_of_the_reference() {
        let sides = sides_of_unit_square();
        let big = clip_to(&sides, &face(&square(3.0), None));
        assert_eq!(big.len, 4);
        assert!(
            big.as_slice()
                .iter()
                .all(|c| (c.x.abs() - 1.0).abs() < 1e-5 && (c.z.abs() - 1.0).abs() < 1e-5)
        );
        let shifted: Vec<Vec3> = square(1.0)
            .iter()
            .map(|c| *c + Vec3::new(1.0, 0.0, 0.0))
            .collect();
        let half = clip_to(&sides, &face(&shifted, None));
        assert_eq!(half.len, 4);
        assert!(
            half.as_slice()
                .iter()
                .all(|c| c.x >= -1e-5 && c.x <= 1.0 + 1e-5)
        );
    }

    #[test]
    fn the_reference_is_the_larger_fitted_face_else_the_larger_polygon() {
        let small = face(&square(1.0), Some(Vec3::Y));
        let large = face(&square(2.0), Some(Vec3::Y));
        let bare = face(&square(1.0), None);
        let edge = face(&[Vec3::ZERO, Vec3::X], None);
        let pick = |a: &Face, b: &Face| choose_reference(a, b, Vec3::Y);
        assert_eq!(pick(&small, &large), Some(Reference::Second));
        assert_eq!(pick(&large, &small), Some(Reference::First));
        assert_eq!(pick(&small, &small), Some(Reference::First));
        // A fitted face beats a bigger polygon that was not fitted, on either side.
        assert_eq!(
            pick(&small, &face(&square(3.0), None)),
            Some(Reference::First)
        );
        assert_eq!(
            pick(&face(&square(3.0), None), &small),
            Some(Reference::Second)
        );
        // With none fitted the polygon is the reference; two polygons give the larger.
        assert_eq!(pick(&bare, &edge), Some(Reference::First));
        assert_eq!(pick(&edge, &bare), Some(Reference::Second));
        assert_eq!(
            pick(&bare, &face(&square(2.0), None)),
            Some(Reference::Second)
        );
        assert_eq!(pick(&edge, &edge), None);
    }

    #[test]
    fn contacts_sit_halfway_between_the_surfaces_and_count_the_radii() {
        let clipped = polygon(&[Vec3::new(1.0, -0.1, 0.0), Vec3::new(2.0, 0.4, 0.0)]);
        // The reference is the first shape's face at y = 0; the incident corners are the second's.
        let first = contacts(
            &clipped,
            (Reference::First, Vec3::ZERO),
            Vec3::Y,
            (0.2, 0.0),
            (0.5, f32::MIN),
        );
        assert_eq!(first.len, 2);
        // Corner 0: gap -0.1, separation -0.3, point halfway (y = -0.05) shifted by half the radius.
        assert!((first.as_slice()[0].separation + 0.3).abs() < 1e-6);
        assert!(near(first.as_slice()[0].point, Vec3::new(1.0, 0.05, 0.0)));
        assert!((first.as_slice()[1].separation - 0.2).abs() < 1e-6);
        assert!(near(
            first.as_slice()[1].point,
            Vec3::new(2.0, 0.4 - 0.2 + 0.1, 0.0)
        ));
        // Past the margin a point is dropped.
        let tight = contacts(
            &clipped,
            (Reference::First, Vec3::ZERO),
            Vec3::Y,
            (0.2, 0.0),
            (0.1, f32::MIN),
        );
        assert_eq!(tight.len, 1);
        // The reference is the second shape's face at y = 1 (corners on the first shape's face).
        let second = contacts(
            &polygon(&[Vec3::new(0.0, 0.9, 0.0)]),
            (Reference::Second, Vec3::new(0.0, 1.0, 0.0)),
            Vec3::Y,
            (0.0, 0.1),
            (0.5, f32::MIN),
        );
        // Gap 0.1, separation 0.1 - 0.1 = 0; the point is halfway (y = 0.95), less half the radii.
        assert!(second.as_slice()[0].separation.abs() < 1e-6);
        assert!(near(
            second.as_slice()[0].point,
            Vec3::new(0.0, 0.95 - 0.05, 0.0)
        ));
    }

    /// A unit cube turned about z by `degrees`, so a top-pointing search direction is tilted that far
    /// from the face it should find.
    fn lean(degrees: f32) -> Vec3 {
        let angle = degrees.to_radians();
        Vec3::new(base::dmath::sin(angle), base::dmath::cos(angle), 0.0)
    }

    #[test]
    fn the_widest_face_is_the_face_nearest_the_direction_up_to_forty_five_degrees() {
        // At 20 and 40 degrees no face is fitted (1.8 degrees) but the top face is the nearest.
        for degrees in [0.0, 20.0, 40.0, -30.0] {
            let n = lean(degrees);
            let found = widest_face(&cube(), n);
            assert!(
                found.is_some_and(
                    |f| f.normal.is_some_and(|f| near(f, Vec3::Y)) && f.corners.len == 4
                ),
                "no top face for {degrees} degrees"
            );
        }
        assert!(fitted_face(&cube(), lean(20.0)).is_none());
        // At 60 degrees the nearest face is the side (30 degrees away), and a corner direction is
        // further than 45 degrees from every face.
        let side = widest_face(&cube(), lean(60.0));
        assert!(side.is_some_and(|f| f.normal.is_some_and(|f| near(f, Vec3::X))));
        let corner = Vec3::new(1.0, 1.0, 1.0) * (1.0 / 3.0_f32.sqrt());
        assert!(widest_face(&cube(), corner).is_none());
    }

    #[test]
    fn the_widest_face_of_a_round_hull_is_a_face_not_a_cut_across_the_corners() {
        // A 12-gon prism (radius 1, half length 1): its faces are 30 degrees apart, so the normal at
        // a corner leans 15 degrees from both neighbours.
        let mut corners = Vec::new();
        for i in 0..12_u8 {
            let angle = std::f32::consts::TAU * f32::from(i) / 12.0;
            let rim = Vec3::new(base::dmath::cos(angle), base::dmath::sin(angle), 0.0);
            corners.push(rim - Vec3::Z);
            corners.push(rim + Vec3::Z);
        }
        let solid = Solid {
            corners: &corners,
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        // The corner at 90 degrees: the direction at 91 degrees is 14 degrees off the face between
        // the corners at 90 and 120 degrees (normal at 105), which no fitted face accepts.
        let at = 91.0_f32.to_radians();
        let n = Vec3::new(base::dmath::cos(at), base::dmath::sin(at), 0.0);
        assert!(fitted_face(&solid, n).is_none());
        let found = widest_face(&solid, n);
        assert!(found.is_some_and(|f| f.corners.len == 4));
    }

    /// `widest_face` as card 8-71 wrote it: every triple is tested against every corner, and the
    /// triple that faces the direction most wins (the first of equal ones). The reference of card
    /// 8-78, which makes the search cheaper without changing a bit of its answer.
    fn widest_face_by_every_triple(solid: &Solid, n: Vec3) -> Option<Face> {
        let local = solid.rotation.conjugate().rotate(n);
        let reach = solid.reach();
        let (near, len) = nearest_corners(solid, local);
        let fit = Fit {
            tolerance: reach * PLANE_TOLERANCE,
            reach,
            min_cosine: WIDE_FACE_COSINE,
        };
        let planes = triples(len).filter_map(|(i, j, k)| {
            face_plane(solid, (near[i], near[j], near[k]), local, fit)
                .map(|normal| (normal.dot(local), normal, near[i]))
        });
        // The first of equal cosines stays: a later one replaces it only when it is larger.
        let best = planes.fold(None, |best: Option<(f32, Vec3, Vec3)>, plane| {
            best.filter(|(cosine, _, _)| plane.0 <= *cosine)
                .or(Some(plane))
        });
        let (_, normal, on_plane) = best?;
        Some(face_on(solid, normal, on_plane, fit.tolerance))
    }

    /// `plane_facing` as it was before card 8-99: the unit normal is made first, the cosine tested
    /// after. The reference of the squared-cosine refusal, which changes no bit of the answer.
    fn plane_facing_by_the_square_root(
        reach: f32,
        (a, b, c): (Vec3, Vec3, Vec3),
        n: Vec3,
        min_cosine: f32,
    ) -> Option<Vec3> {
        let across = (b - a).cross(c - a);
        if across.length_squared() <= MIN_TRIANGLE * (reach * reach) * (reach * reach) {
            return None;
        }
        let mut normal = across * (1.0 / across.length());
        if normal.dot(n) < 0.0 {
            normal = -normal;
        }
        (normal.dot(n) > min_cosine).then_some(normal)
    }

    fn same_normal(a: Option<Vec3>, b: Option<Vec3>) -> bool {
        a.map(|v| v.to_array().map(f32::to_bits)) == b.map(|v| v.to_array().map(f32::to_bits))
    }

    fn lcg(seed: &mut u64) -> f32 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        f32::from(u16::try_from(*seed >> 48).unwrap_or(0)) / 32_768.0 - 1.0
    }

    /// Whether the two versions agree to the bit on a triple; the normal when they take it.
    fn compared(triple: (Vec3, Vec3, Vec3), n: Vec3, min: f32) -> Option<Vec3> {
        let now = plane_facing(1.0, triple, n, min);
        let before = plane_facing_by_the_square_root(1.0, triple, n, min);
        assert!(same_normal(now, before), "{triple:?} {n:?} {min}");
        now
    }

    /// Triples whose plane leans from `Vec3::Y` by a cosine just below and just above `min`, in
    /// both senses of the winding.
    fn near_the_edge(min: f32) -> Vec<(Vec3, Vec3, Vec3)> {
        (-40_i16..=40)
            .flat_map(|step| {
                let cosine = (min * (1.0 + f32::from(step) * 5.0e-4)).min(1.0);
                let normal = Vec3::new((1.0 - cosine * cosine).sqrt(), cosine, 0.0);
                let (u, v) = plane_frame(normal);
                [
                    (Vec3::ZERO, u * 0.8, v * 0.8),
                    (Vec3::ZERO, v * 0.8, u * 0.8),
                ]
            })
            .collect()
    }

    /// A triple, a direction and a minimum cosine.
    type Case = ((Vec3, Vec3, Vec3), Vec3, f32);

    fn point(seed: &mut u64) -> Vec3 {
        Vec3::new(lcg(seed), lcg(seed), lcg(seed))
    }

    /// Random triples and directions for the two minimum cosines of the two tries and for two
    /// below one half (a negative and a small one), which the square does not refuse.
    fn random_cases(seed: &mut u64) -> Vec<Case> {
        let mut cases = Vec::new();
        for _ in 0..20_000 {
            let triple = (point(seed), point(seed), point(seed));
            let n = point(seed).normalized().unwrap_or(Vec3::Y);
            cases
                .extend([-0.5, 0.1, WIDE_FACE_COSINE, MIN_FACE_COSINE].map(|min| (triple, n, min)));
        }
        cases
    }

    #[test]
    fn a_triple_is_refused_by_its_squared_cosine_only_when_the_exact_test_refuses_it_too() {
        let mut seed = 99_u64;
        let random = random_cases(&mut seed);
        let edge: Vec<Case> = [WIDE_FACE_COSINE, MIN_FACE_COSINE]
            .into_iter()
            .flat_map(|min| {
                near_the_edge(min)
                    .into_iter()
                    .map(move |t| (t, Vec3::Y, min))
            })
            .collect();
        let taken = |cases: &[Case]| {
            let found = cases
                .iter()
                .filter(|(triple, n, min)| compared(*triple, *n, *min).is_some());
            found.count()
        };
        // The refusal is used (most random triples of the two tries are refused) and the exact test
        // is used at the edge (some of the near-edge triples are taken).
        let refused_random = random.len() - taken(&random);
        assert!(refused_random > 25_000, "{refused_random}");
        let taken_at_the_edge = taken(&edge);
        assert!(taken_at_the_edge > 100, "{taken_at_the_edge}");
    }

    fn same_face(a: Option<&Face>, b: Option<&Face>) -> bool {
        let bits = |v: Vec3| v.to_array().map(f32::to_bits);
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                let (ours, theirs) = (&a.corners, &b.corners);
                ours.len == theirs.len
                    && ours.corners[..ours.len]
                        .iter()
                        .zip(&theirs.corners[..theirs.len])
                        .all(|(x, y)| bits(*x) == bits(*y))
                    && a.normal.map(bits) == b.normal.map(bits)
            }
            _ => false,
        }
    }

    /// The corners of a prism with `sides` sides, radius 1, half length 1, along z.
    fn prism_corners(sides: u8) -> Vec<Vec3> {
        let mut corners = Vec::new();
        for i in 0..sides {
            let angle = std::f32::consts::TAU * f32::from(i) / f32::from(sides);
            let rim = Vec3::new(base::dmath::cos(angle), base::dmath::sin(angle), 0.0);
            corners.push(rim - Vec3::Z);
            corners.push(rim + Vec3::Z);
        }
        corners
    }

    /// Directions all round, in 5 degree steps in three planes, plus the ties: a diagonal of a box,
    /// and an edge.
    fn search_directions() -> Vec<Vec3> {
        let mut directions = vec![
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.0, 1.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
        ];
        for step in 0..72_u8 {
            let angle = f32::from(step) * 5.0_f32.to_radians();
            let (c, s) = (base::dmath::cos(angle), base::dmath::sin(angle));
            directions.push(Vec3::new(c, s, 0.0));
            directions.push(Vec3::new(0.0, c, s));
            directions.push(Vec3::new(c, 0.2, s));
        }
        directions
            .into_iter()
            .map(|d| d.normalized().unwrap_or(Vec3::Y))
            .collect()
    }

    /// How many directions were compared and how many found a face; panics (asserts) at the first
    /// direction where the two searches differ.
    fn compare_searches(corners: &[Vec3], rotation: Quat) -> (u32, u32) {
        let solid = Solid {
            corners,
            centre: Vec3::new(0.2, -0.1, 0.4),
            rotation,
        };
        let (mut compared, mut found) = (0, 0);
        for n in search_directions() {
            let now = widest_face(&solid, n);
            let before = widest_face_by_every_triple(&solid, n);
            assert!(same_face(now.as_ref(), before.as_ref()), "direction {n:?}");
            compared += 1;
            found += u32::from(now.is_some());
        }
        (compared, found)
    }

    #[test]
    fn the_widest_face_is_the_one_the_search_of_every_triple_finds_bit_for_bit() {
        let turns = [
            Quat::IDENTITY,
            Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 0.7).unwrap_or(Quat::IDENTITY),
            Quat::from_axis_angle(Vec3::Y, 0.3).unwrap_or(Quat::IDENTITY),
        ];
        let mut shapes: Vec<Vec<Vec3>> = vec![unit_box_corners().to_vec()];
        shapes.extend([8_u8, 12, 16].map(prism_corners));
        // Fewer than eight corners: a square pyramid and a tetrahedron.
        shapes.push(vec![
            Vec3::new(1.0, -0.5, 1.0),
            Vec3::new(1.0, -0.5, -1.0),
            Vec3::new(-1.0, -0.5, -1.0),
            Vec3::new(-1.0, -0.5, 1.0),
            Vec3::new(0.0, 1.0, 0.0),
        ]);
        shapes.push(vec![
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(1.0, -1.0, -1.0),
            Vec3::new(-1.0, 1.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
        ]);
        let (mut compared, mut found) = (0, 0);
        for (corners, rotation) in shapes
            .iter()
            .flat_map(|corners| turns.iter().map(move |turn| (corners, *turn)))
        {
            let (more, hits) = compare_searches(corners, rotation);
            compared += more;
            found += hits;
        }
        // The comparison is not empty: most directions find a face and some find none.
        assert!(compared > 3000 && found > compared / 2 && found < compared);
    }

    #[test]
    fn the_nearest_corners_are_the_farthest_along_the_direction_and_at_most_sixteen() {
        let (found, len) = nearest_corners(&cube(), Vec3::Y);
        assert_eq!(len, 8);
        assert!(found[..4].iter().all(|c| c.y > 0.0));
        let ladder: Vec<Vec3> = (0..20_u8)
            .map(|i| Vec3::new(f32::from(i), 0.0, 0.0))
            .collect();
        let solid = Solid {
            corners: &ladder,
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        let (found, len) = nearest_corners(&solid, Vec3::X);
        assert_eq!(len, WIDE_CANDIDATES);
        // The sixteen largest x, the largest first: 19 down to 4.
        for (rank, c) in (4_u8..20).rev().zip(found.iter()) {
            assert!((c.x - f32::from(rank)).abs() < 1e-6, "{rank}: {}", c.x);
        }
    }

    #[test]
    fn a_tilted_edge_keeps_both_ends_within_the_margin_and_only_the_lower_one_beyond() {
        // A segment 1 m long, one end 5 mm above the other along y.
        let core = Core::Segment(Vec3::new(-0.5, 0.005, 0.0), Vec3::new(0.5, 0.0, 0.0));
        let fitted = Face {
            corners: Polygon::empty(),
            normal: None,
        };
        let wide = incident_for(&core, &fitted, -Vec3::Y, 0.02);
        assert_eq!(wide.corners.len, 2);
        let fitted = Face {
            corners: Polygon::empty(),
            normal: None,
        };
        let narrow = incident_for(&core, &fitted, -Vec3::Y, 0.0);
        assert_eq!(narrow.corners.len, 1);
    }

    /// A manifold of one point with the given normal and separation.
    fn one_point(normal: Vec3, separation: f32) -> Manifold3 {
        let point = ContactPoint3 {
            point: Vec3::ZERO,
            separation,
        };
        Manifold3::new(normal, &[point])
    }

    fn point_shape(at: Vec3) -> Convex<'static> {
        Convex {
            core: Core::Point(at),
            radius: 0.0,
        }
    }

    #[test]
    fn a_face_replaces_the_normal_of_gjk_only_when_it_loses_neither_depth_nor_gap() {
        // Two points 1 m apart along y: the gap along y is 1, along a normal leaning by 0.436 it is
        // 0.9. The shapes have no reach, so no slack.
        let (first, second) = (point_shape(Vec3::ZERO), point_shape(Vec3::Y));
        let single = one_point(Vec3::Y, 1.0);
        assert!(agrees(&first, &second, &one_point(Vec3::Y, 1.0), &single));
        // The same normal with a deepest point that is not where GJK put it.
        assert!(!agrees(&first, &second, &one_point(Vec3::Y, 0.5), &single));
        // The right depth with a normal that separates the pair less.
        let leaning = Vec3::new(0.436, 0.9, 0.0);
        let leaning = leaning * (1.0 / leaning.length());
        assert!(!agrees(&first, &second, &one_point(leaning, 1.0), &single));
    }

    #[test]
    fn a_contact_is_shallow_up_to_the_window_share_of_the_smaller_reach() {
        // A box of half sizes (0.5, 0.5, 0.5) has a reach of 0.87: a window of 0.043 m.
        let frame = BoxFrame {
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            half: Vec3::new(0.5, 0.5, 0.5),
        };
        let boxed = Convex {
            core: Core::Box(frame),
            radius: 0.0,
        };
        let other = Convex {
            core: Core::Box(frame),
            radius: 0.0,
        };
        assert!(is_shallow(&boxed, &other, &one_point(Vec3::Y, -0.04)));
        assert!(!is_shallow(&boxed, &other, &one_point(Vec3::Y, -0.05)));
        assert!(is_shallow(&boxed, &other, &one_point(Vec3::Y, 0.3)));
    }

    /// A floor box (top at y = 0) and an octagonal prism along z (radius 0.5, half length 0.5)
    /// with a corner edge down, tilted about x so that the two ends of that edge are `rise` apart,
    /// the lower end `sunk` below the floor.
    fn prism_on_the_floor(rise: f32, sunk: f32) -> Option<Manifold3> {
        let ring = |i: u8| {
            let angle = std::f32::consts::TAU * (f32::from(i) + 0.5) / 8.0;
            Vec3::new(
                0.5 * base::dmath::cos(angle),
                0.5 * base::dmath::sin(angle),
                0.0,
            )
        };
        let mut corners = Vec::new();
        for i in 0..8 {
            corners.push(ring(i) - Vec3::Z * 0.5);
            corners.push(ring(i) + Vec3::Z * 0.5);
        }
        let floor = Convex {
            core: Core::Box(BoxFrame {
                centre: Vec3::new(0.0, -5.0, 0.0),
                rotation: Quat::IDENTITY,
                half: Vec3::new(50.0, 5.0, 50.0),
            }),
            radius: 0.0,
        };
        // Corners at 22.5 degrees + k 45: the lowest edge (at 247.5 and 292.5 degrees) is a flat
        // face; turn by 22.5 degrees about z so a corner edge points down.
        let down =
            Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_8).unwrap_or(Quat::IDENTITY);
        let tilt = Quat::from_axis_angle(Vec3::X, base::dmath::sin(rise)).unwrap_or(Quat::IDENTITY);
        let rotation = tilt * down;
        let lowest = corners
            .iter()
            .map(|c| rotation.rotate(*c).y)
            .fold(f32::MAX, f32::min);
        let prism = Convex {
            core: Core::Hull {
                vertices: &corners,
                centre: Vec3::new(0.0, -lowest - sunk, 0.0),
                rotation,
            },
            radius: 0.0,
        };
        contact(&floor, &prism, 0.02)
    }

    #[test]
    fn a_prism_with_an_edge_down_keeps_both_ends_of_the_edge_when_it_is_tilted_a_little() {
        // Level, and with the ends 0.2 mm, 5 mm and 15 mm apart (inside the margin): two points.
        for rise in [0.0, 0.0002, 0.005, 0.015] {
            let found = prism_on_the_floor(rise, 0.001);
            assert!(
                found.is_some_and(|m| m.count == 2),
                "rise {rise}: {found:?}"
            );
        }
    }

    #[test]
    fn a_point_far_behind_the_reference_plane_is_not_a_contact() {
        // The reference face is at y = 0; one incident corner is 50 m behind it (a face that is not
        // where the shapes touch), one is 5 mm behind it.
        let clipped = polygon(&[Vec3::new(0.0, -50.0, 0.0), Vec3::new(1.0, -0.005, 0.0)]);
        let found = contacts(
            &clipped,
            (Reference::First, Vec3::ZERO),
            Vec3::Y,
            (0.0, 0.0),
            (0.02, -0.1),
        );
        assert_eq!(found.len, 1);
        assert!((found.as_slice()[0].separation + 0.005).abs() < 1e-6);
    }

    #[test]
    fn the_depth_floor_is_the_depth_of_gjk_less_a_share_of_the_smaller_reach() {
        // Two unit cubes have a reach of 0.87 each: an allowance of 0.25 x 0.87 = 0.2165 m.
        let frame = BoxFrame {
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            half: Vec3::new(0.5, 0.5, 0.5),
        };
        let cube = || Convex {
            core: Core::Box(frame),
            radius: 0.0,
        };
        let floor = depth_floor(&cube(), &cube(), &one_point(Vec3::Y, -0.03));
        assert!(
            (floor - (-0.03 - 0.25 * 0.866_025_4)).abs() < 1e-5,
            "floor {floor}"
        );
    }

    #[test]
    fn the_depth_floor_counts_the_radius_of_a_round_shape_and_never_drops_to_nothing() {
        let frame = BoxFrame {
            centre: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            half: Vec3::new(0.5, 0.5, 0.5),
        };
        let cube = Convex {
            core: Core::Box(frame),
            radius: 0.0,
        };
        // A sphere is a point grown by 0.35: its reach is the radius, not zero.
        let sphere = Convex {
            core: Core::Point(Vec3::ZERO),
            radius: 0.35,
        };
        let single = one_point(Vec3::Y, -0.01);
        let floor = depth_floor(&cube, &sphere, &single);
        assert!(
            (floor - (-0.01 - 0.25 * 0.35)).abs() < 1e-6,
            "floor {floor}"
        );
        // Two points: no reach and no radius, still room for rounding.
        let bare = depth_floor(&point_shape(Vec3::ZERO), &point_shape(Vec3::Y), &single);
        assert!(bare < -0.01 && bare > -0.011, "floor {bare}");
    }

    #[test]
    fn a_clip_against_a_face_that_is_not_where_the_shapes_touch_gives_no_manifold() {
        // Two unit cubes overlap by 1 mm (GJK's depth). The reference face is the top of the first, at
        // y = 0.5 (not fitted, so the incident face handed in is used as it is), 50 m below it, as when a face is chosen that is not
        // where the shapes touch: its points are 50 m behind the plane and are not contacts.
        let frame = |y: f32| BoxFrame {
            centre: Vec3::new(0.0, y, 0.0),
            rotation: Quat::IDENTITY,
            half: Vec3::new(0.5, 0.5, 0.5),
        };
        let (first, second) = (
            Convex {
                core: Core::Box(frame(0.0)),
                radius: 0.0,
            },
            Convex {
                core: Core::Box(frame(0.999)),
                radius: 0.0,
            },
        );
        let reference = face(&square_at(0.5), None);
        let ask = Ask {
            single: one_point(Vec3::Y, -0.001),
            margin: 0.02,
            band: 0.0,
        };
        let far = face(&square_at(-49.5), None);
        assert!(
            clip_manifold((&first, &second), Reference::First, (&reference, &far), ask).is_none()
        );
        // The same call with the incident face where the cubes touch keeps its four points.
        let near = face(&square_at(0.499), None);
        let kept = clip_manifold(
            (&first, &second),
            Reference::First,
            (&reference, &near),
            ask,
        );
        assert_eq!(kept.map(|m| m.count), Some(4));
    }

    /// The four corners of a unit square (half 0.5) at height `y`.
    fn square_at(y: f32) -> [Vec3; 4] {
        [
            Vec3::new(-0.5, y, -0.5),
            Vec3::new(0.5, y, -0.5),
            Vec3::new(0.5, y, 0.5),
            Vec3::new(-0.5, y, 0.5),
        ]
    }
}
