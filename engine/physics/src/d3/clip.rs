//! Clipping of convex polygons, shared by the contact routines that build a manifold from two faces
//! (box against box, and the pairs with a convex hull).

use super::manifold::{ContactPoint3, MAX_MANIFOLD_POINTS};
use base::math::Vec3;

/// The most corners a polygon of a hull holds: a face of a hull has at most 32, and clipping it
/// against a face with 32 sides leaves at most 64.
pub(super) const CAPACITY: usize = 64;
/// The most corners a polygon of a box holds: a four-sided face clipped by four planes has eight.
/// A smaller capacity is a cheaper copy, and every box contact copies its polygons.
pub(super) const BOX_CAPACITY: usize = 8;

/// A convex polygon of at most [`CAPACITY`] corners.
#[derive(Clone, Copy)]
pub(super) struct Polygon<const N: usize> {
    pub(super) corners: [Vec3; N],
    pub(super) len: usize,
}

impl<const N: usize> Polygon<N> {
    pub(super) fn empty() -> Self {
        Self {
            corners: [Vec3::ZERO; N],
            len: 0,
        }
    }

    pub(super) fn push(&mut self, corner: Vec3) {
        if self.len < N {
            self.corners[self.len] = corner;
            self.len += 1;
        }
    }

    pub(super) fn as_slice(&self) -> &[Vec3] {
        &self.corners[..self.len]
    }

    /// The part of the polygon on the side of the plane where `distance` is not positive.
    pub(super) fn clipped(&self, distance: impl Fn(Vec3) -> f32) -> Self {
        let mut out = Self::empty();
        let corners = self.as_slice();
        for (i, current) in corners.iter().enumerate() {
            let next = corners[(i + 1) % corners.len()];
            clip_edge(
                &mut out,
                (*current, next),
                (distance(*current), distance(next)),
            );
        }
        out
    }
}

/// Adds to `out` what the edge `from_to` leaves of the polygon on the kept side of the plane, given
/// the distances of its two ends from the plane (kept when not positive).
fn clip_edge<const N: usize>(out: &mut Polygon<N>, from_to: (Vec3, Vec3), distances: (f32, f32)) {
    let ((from, to), (d_from, d_to)) = (from_to, distances);
    if d_from <= 0.0 {
        out.push(from);
    }
    if (d_from <= 0.0) != (d_to <= 0.0) {
        out.push(from + (to - from) * (d_from / (d_from - d_to)));
    }
}

/// Contact points kept before they are cut down to four.
pub(super) struct Candidates<const N: usize> {
    pub(super) items: [ContactPoint3; N],
    pub(super) len: usize,
}

impl<const N: usize> Candidates<N> {
    pub(super) fn empty() -> Self {
        Self {
            items: [ContactPoint3::default(); N],
            len: 0,
        }
    }

    /// Adds `contact`; one past the capacity is dropped.
    pub(super) fn push(&mut self, contact: ContactPoint3) {
        if self.len < N {
            self.items[self.len] = contact;
            self.len += 1;
        }
    }

    pub(super) fn from_polygon(
        polygon: &Polygon<N>,
        plane_point: Vec3,
        n: Vec3,
        margin: f32,
    ) -> Self {
        let mut list = Self {
            items: [ContactPoint3::default(); N],
            len: 0,
        };
        let kept = polygon
            .as_slice()
            .iter()
            .map(|corner| {
                let separation = (*corner - plane_point).dot(n);
                ContactPoint3 {
                    point: *corner - n * (separation * 0.5),
                    separation,
                }
            })
            .filter(|contact| contact.separation <= margin);
        for contact in kept {
            list.items[list.len] = contact;
            list.len += 1;
        }
        list
    }

    pub(super) fn as_slice(&self) -> &[ContactPoint3] {
        &self.items[..self.len]
    }

    /// The index not in `taken` with the largest `key`; keys within `slack` of the largest are
    /// tied, and of the tied the point farthest along `prefer` wins, so that the choice follows the
    /// geometry and not the order of the list.
    fn best_by(
        &self,
        taken: &[usize],
        key: impl Fn(&ContactPoint3) -> f32,
        (slack, prefer): (f32, Vec3),
    ) -> usize {
        let keyed = || {
            self.as_slice()
                .iter()
                .enumerate()
                .filter(|(i, _)| !taken.contains(i))
                .map(|(i, item)| (i, key(item), item.point.dot(prefer)))
        };
        let top = keyed().map(|(_, k, _)| k).fold(f32::MIN, f32::max);
        keyed()
            .filter(|(_, k, _)| *k >= top - slack * top.abs().max(1.0))
            .max_by(|x, y| x.2.total_cmp(&y.2))
            .map_or(0, |(i, _, _)| i)
    }

    /// Four points that spread over the contact: the deepest, the farthest from it, and the two that
    /// enclose the largest area on either side of the line between them. Points that are as good as
    /// each other (to a thousandth) are told apart by where they are, not by their place in the
    /// list, so the four do not change when the list starts at another corner.
    pub(super) fn reduced(&self, n: Vec3) -> [ContactPoint3; MAX_MANIFOLD_POINTS] {
        let prefer = preferred_direction(n);
        let tie = (TIE_SLACK, prefer);
        let deepest = self.best_by(&[], |p| -p.separation, tie);
        let from = self.items[deepest].point;
        let far = self.best_by(&[deepest], |p| (p.point - from).length_squared(), tie);
        let line = self.items[far].point - from;
        let side = |p: &ContactPoint3| line.cross(p.point - from).dot(n);
        let third = self.best_by(&[deepest, far], side, tie);
        let fourth = self.best_by(&[deepest, far, third], |p| -side(p), tie);
        [deepest, far, third, fourth].map(|i| self.items[i])
    }
}

/// How close two keys must be to be told apart by position instead (a share of the larger, with a
/// floor of one metre-scale unit, so for small keys it is an absolute slack of this size).
const TIE_SLACK: f32 = 1.0e-3;

/// A direction in the plane across `n` that depends on `n` alone and lines up with no symmetry of a
/// regular polygon.
fn preferred_direction(n: Vec3) -> Vec3 {
    let helper = if n.x.abs() < 0.7 { Vec3::X } else { Vec3::Y };
    let u = n.cross(helper);
    let u = u * (1.0 / u.length());
    u + n.cross(u) * 0.37
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1.0e-5
    }

    fn contact(x: f32, z: f32, separation: f32) -> ContactPoint3 {
        ContactPoint3 {
            point: Vec3::new(x, 0.0, z),
            separation,
        }
    }

    #[test]
    fn the_polygon_a_box_clips_stays_small() {
        // A box face clipped by four planes has at most eight corners; the copies made while
        // clipping are a cost of every box contact, so they must not carry the size of a hull's.
        assert!(std::mem::size_of::<Polygon<8>>() <= 128);
        assert!(std::mem::size_of::<Candidates<8>>() <= 160);
    }

    /// The four points kept from sixteen round a circle, all as deep as each other, when the list
    /// starts at corner `start`; sorted so that they can be compared.
    fn kept_from_a_ring(start: usize) -> Vec<Vec3> {
        let ring: Vec<ContactPoint3> = (0..16_u8)
            .map(|k| {
                let (sin, cos) = base::dmath::sin_cos(f32::from(k) * std::f32::consts::TAU / 16.0);
                contact(cos, sin, -0.01)
            })
            .collect();
        let mut list = Candidates::<CAPACITY>::empty();
        for k in 0..16 {
            list.push(ring[(start + k) % 16]);
        }
        let mut points: Vec<Vec3> = list.reduced(Vec3::Y).iter().map(|p| p.point).collect();
        points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
        points
    }

    #[test]
    fn the_four_points_kept_do_not_depend_on_where_the_list_starts() {
        // Whichever corner the list starts at, the same four are kept (the choice must not hop
        // from step to step).
        let reference = kept_from_a_ring(0);
        for start in 1..16 {
            let kept = kept_from_a_ring(start);
            let same = kept.iter().zip(&reference).all(|(a, b)| near(*a, *b));
            assert!(same, "start {start}: {kept:?} against {reference:?}");
        }
    }

    #[test]
    fn a_polygon_and_a_candidate_list_stop_at_their_capacity() {
        let mut polygon = Polygon::<CAPACITY>::empty();
        let mut list = Candidates::<CAPACITY>::empty();
        for k in 0..=u8::try_from(CAPACITY).unwrap_or(u8::MAX) {
            polygon.push(Vec3::new(f32::from(k), 0.0, 0.0));
            list.push(contact(0.0, 0.0, 0.0));
        }
        assert_eq!((polygon.len, list.len), (CAPACITY, CAPACITY));
        assert_eq!(polygon.as_slice().last().map(|p| p.x), Some(63.0));
    }

    #[test]
    fn an_edge_that_crosses_the_plane_adds_its_crossing_at_the_right_place() {
        // From x = 0 (distance -1) to x = 4 (distance 3): the plane is at x = 1.
        let mut out = Polygon::<CAPACITY>::empty();
        clip_edge(
            &mut out,
            (Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)),
            (-1.0, 3.0),
        );
        assert_eq!(out.len, 2);
        assert!(near(out.as_slice()[0], Vec3::ZERO));
        assert!(near(out.as_slice()[1], Vec3::new(1.0, 0.0, 0.0)));
        // An edge that starts outside and comes in adds only the crossing.
        let mut out = Polygon::<CAPACITY>::empty();
        clip_edge(
            &mut out,
            (Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)),
            (3.0, -1.0),
        );
        assert_eq!(out.len, 1);
        assert!(near(out.as_slice()[0], Vec3::new(3.0, 0.0, 0.0)));
    }

    #[test]
    fn candidates_from_a_polygon_sit_halfway_to_the_plane_and_drop_what_is_past_the_margin() {
        let mut polygon = Polygon::<CAPACITY>::empty();
        for corner in [
            Vec3::new(0.0, -0.2, 0.0),
            Vec3::new(1.0, 0.1, 0.0),
            Vec3::new(2.0, 0.5, 0.0),
        ] {
            polygon.push(corner);
        }
        let list = Candidates::from_polygon(&polygon, Vec3::new(0.0, 0.3, 0.0), Vec3::Y, 0.0);
        // Distances from the plane y = 0.3 along y: -0.5, -0.2, +0.2; the last is past the margin.
        assert_eq!(list.len, 2);
        assert!((list.as_slice()[0].separation + 0.5).abs() < 1e-6);
        assert!(near(list.as_slice()[0].point, Vec3::new(0.0, 0.05, 0.0)));
        assert!(near(list.as_slice()[1].point, Vec3::new(1.0, 0.2, 0.0)));
    }

    #[test]
    fn five_candidates_are_cut_to_the_deepest_the_farthest_and_the_two_widest_on_either_side() {
        let mut list = Candidates::<CAPACITY>::empty();
        // Seen from above (+y), the deepest is at the origin and the farthest from it at (4, 0).
        for point in [
            contact(0.0, 0.0, -0.3),
            contact(4.0, 0.0, -0.1),
            contact(2.0, 1.0, -0.1),
            contact(2.0, -2.0, -0.1),
            contact(2.0, 0.2, -0.1),
        ] {
            list.push(point);
        }
        let kept = list.reduced(Vec3::Y);
        let at = |x: f32, z: f32| kept.iter().any(|p| near(p.point, Vec3::new(x, 0.0, z)));
        assert!(at(0.0, 0.0) && at(4.0, 0.0), "{kept:?}");
        assert!(at(2.0, 1.0) && at(2.0, -2.0), "{kept:?}");
    }
}
