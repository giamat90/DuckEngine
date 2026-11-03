//! Box against box in the plane: separating-axis test and clipping of the incident edge, after
//! the structure of Box2D-Lite (own code). Every variant uses it: the 2D variants directly, the
//! 3D variants on the planar projection of boxes whose only rotation is about Z (the extruded
//! scene). A real 3D box-box narrowphase (15 axes, face/edge manifolds) is much larger and is
//! not part of this spike; see the spike page.

use base::math::Vec2;

const RELATIVE_TOLERANCE: f32 = 0.95;
const ABSOLUTE_TOLERANCE: f32 = 0.01;

/// A box in the plane: centre, rotation as a (cos, sin) pair, half extents.
#[derive(Clone, Copy, Debug)]
pub struct PlanarBox {
    pub pos: Vec2,
    pub cos: f32,
    pub sin: f32,
    pub half: Vec2,
}

/// One contact point of a manifold.
#[derive(Clone, Copy, Debug, Default)]
pub struct Point {
    pub position: Vec2,
    pub separation: f32,
}

/// Up to two points that share one normal (pointing from the first box to the second).
#[derive(Clone, Copy, Debug)]
pub struct Manifold {
    pub normal: Vec2,
    pub count: usize,
    pub points: [Point; 2],
}

/// A 2x2 matrix stored as columns.
#[derive(Clone, Copy)]
struct Mat2 {
    c1: Vec2,
    c2: Vec2,
}

impl Mat2 {
    fn rotation(cos: f32, sin: f32) -> Self {
        Self {
            c1: Vec2::new(cos, sin),
            c2: Vec2::new(-sin, cos),
        }
    }

    fn mul_vec(self, v: Vec2) -> Vec2 {
        self.c1 * v.x + self.c2 * v.y
    }

    fn transposed_mul_vec(self, v: Vec2) -> Vec2 {
        Vec2::new(self.c1.dot(v), self.c2.dot(v))
    }

    fn abs(self) -> Self {
        Self {
            c1: self.c1.abs(),
            c2: self.c2.abs(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    FaceAX,
    FaceAY,
    FaceBX,
    FaceBY,
}

struct Candidate {
    axis: Axis,
    separation: f32,
    normal: Vec2,
    tolerance_half: f32,
}

/// The two boxes in each other's frames plus the face separations of the SAT.
struct Sat {
    a: PlanarBox,
    b: PlanarBox,
    ra: Mat2,
    rb: Mat2,
    d_a: Vec2,
    d_b: Vec2,
    face_a: Vec2,
    face_b: Vec2,
}

fn signed(positive: bool, v: Vec2) -> Vec2 {
    if positive { v } else { -v }
}

impl Sat {
    /// `None` when a face axis separates the boxes.
    fn new(a: PlanarBox, b: PlanarBox) -> Option<Self> {
        let ra = Mat2::rotation(a.cos, a.sin);
        let rb = Mat2::rotation(b.cos, b.sin);
        let dp = b.pos - a.pos;
        let d_a = ra.transposed_mul_vec(dp);
        let d_b = rb.transposed_mul_vec(dp);
        let c = Mat2 {
            c1: ra.transposed_mul_vec(rb.c1),
            c2: ra.transposed_mul_vec(rb.c2),
        }
        .abs();
        let face_a = d_a.abs() - a.half - c.mul_vec(b.half);
        let face_b = d_b.abs() - c.transposed_mul_vec(a.half) - b.half;
        let separated = face_a.x > 0.0 || face_a.y > 0.0 || face_b.x > 0.0 || face_b.y > 0.0;
        (!separated).then_some(Self {
            a,
            b,
            ra,
            rb,
            d_a,
            d_b,
            face_a,
            face_b,
        })
    }

    /// The axis of least penetration, with a bias towards the faces of the first box.
    fn best_axis(&self) -> (Axis, Vec2) {
        let candidates = [
            Candidate {
                axis: Axis::FaceAX,
                separation: self.face_a.x,
                normal: signed(self.d_a.x > 0.0, self.ra.c1),
                tolerance_half: 0.0,
            },
            Candidate {
                axis: Axis::FaceAY,
                separation: self.face_a.y,
                normal: signed(self.d_a.y > 0.0, self.ra.c2),
                tolerance_half: self.a.half.y,
            },
            Candidate {
                axis: Axis::FaceBX,
                separation: self.face_b.x,
                normal: signed(self.d_b.x > 0.0, self.rb.c1),
                tolerance_half: self.b.half.x,
            },
            Candidate {
                axis: Axis::FaceBY,
                separation: self.face_b.y,
                normal: signed(self.d_b.y > 0.0, self.rb.c2),
                tolerance_half: self.b.half.y,
            },
        ];
        let better = |best: &Candidate, c: &Candidate| {
            c.separation
                > RELATIVE_TOLERANCE * best.separation + ABSOLUTE_TOLERANCE * c.tolerance_half
        };
        let best =
            candidates[1..].iter().fold(
                &candidates[0],
                |best, c| if better(best, c) { c } else { best },
            );
        (best.axis, best.normal)
    }
}

/// The reference face and the clip planes of its two sides.
struct Reference {
    front_normal: Vec2,
    front: f32,
    side_normal: Vec2,
    neg_side: f32,
    pos_side: f32,
    incident: [Vec2; 2],
}

fn incident_edge(half: Vec2, pos: Vec2, rot: Mat2, front_normal: Vec2) -> [Vec2; 2] {
    let n = -rot.transposed_mul_vec(front_normal);
    let n_abs = n.abs();
    let local = if n_abs.x > n_abs.y {
        if n.x > 0.0 {
            [Vec2::new(half.x, -half.y), Vec2::new(half.x, half.y)]
        } else {
            [Vec2::new(-half.x, half.y), Vec2::new(-half.x, -half.y)]
        }
    } else if n.y > 0.0 {
        [Vec2::new(half.x, half.y), Vec2::new(-half.x, half.y)]
    } else {
        [Vec2::new(-half.x, -half.y), Vec2::new(half.x, -half.y)]
    };
    [pos + rot.mul_vec(local[0]), pos + rot.mul_vec(local[1])]
}

fn reference(sat: &Sat, axis: Axis, normal: Vec2) -> Reference {
    let (reference_box, reference_rot, incident_box, incident_rot, front_normal) = match axis {
        Axis::FaceAX | Axis::FaceAY => (sat.a, sat.ra, sat.b, sat.rb, normal),
        Axis::FaceBX | Axis::FaceBY => (sat.b, sat.rb, sat.a, sat.ra, -normal),
    };
    let along_x = axis == Axis::FaceAX || axis == Axis::FaceBX;
    let (front_half, side_normal, side_half) = if along_x {
        (reference_box.half.x, reference_rot.c2, reference_box.half.y)
    } else {
        (reference_box.half.y, reference_rot.c1, reference_box.half.x)
    };
    let side = reference_box.pos.dot(side_normal);
    Reference {
        front_normal,
        front: reference_box.pos.dot(front_normal) + front_half,
        side_normal,
        neg_side: -side + side_half,
        pos_side: side + side_half,
        incident: incident_edge(
            incident_box.half,
            incident_box.pos,
            incident_rot,
            front_normal,
        ),
    }
}

/// Clips the segment against `normal . p <= offset`; `None` when fewer than two points remain.
fn clip_segment(input: [Vec2; 2], normal: Vec2, offset: f32) -> Option<[Vec2; 2]> {
    let d0 = normal.dot(input[0]) - offset;
    let d1 = normal.dot(input[1]) - offset;
    let mut out = [Vec2::ZERO; 2];
    let mut count = 0;
    if d0 <= 0.0 {
        out[count] = input[0];
        count += 1;
    }
    if d1 <= 0.0 {
        out[count] = input[1];
        count += 1;
    }
    if d0 * d1 < 0.0 && count < 2 {
        out[count] = input[0] + (input[1] - input[0]) * (d0 / (d0 - d1));
        count += 1;
    }
    (count == 2).then_some(out)
}

/// The contact manifold of two boxes, or `None` when they do not touch.
#[must_use]
pub fn collide(a: PlanarBox, b: PlanarBox) -> Option<Manifold> {
    let sat = Sat::new(a, b)?;
    let (axis, normal) = sat.best_axis();
    let r = reference(&sat, axis, normal);
    let clipped = clip_segment(r.incident, -r.side_normal, r.neg_side)?;
    let clipped = clip_segment(clipped, r.side_normal, r.pos_side)?;
    let mut manifold = Manifold {
        normal,
        count: 0,
        points: [Point::default(); 2],
    };
    for v in clipped {
        let separation = r.front_normal.dot(v) - r.front;
        if separation <= 0.0 {
            manifold.points[manifold.count] = Point {
                position: v - r.front_normal * separation,
                separation,
            };
            manifold.count += 1;
        }
    }
    (manifold.count > 0).then_some(manifold)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(x: f32, y: f32) -> PlanarBox {
        PlanarBox {
            pos: Vec2::new(x, y),
            cos: 1.0,
            sin: 0.0,
            half: Vec2::new(0.5, 0.5),
        }
    }

    #[test]
    fn stacked_boxes_give_two_points_and_an_upward_normal() {
        let m = collide(unit(0.0, 0.0), unit(0.0, 0.99));
        assert!(
            matches!(m, Some(m) if m.count == 2 && m.normal.y > 0.99 && m.points[0].separation < 0.0)
        );
    }

    #[test]
    fn separated_boxes_give_nothing() {
        assert!(collide(unit(0.0, 0.0), unit(0.0, 1.5)).is_none());
    }
}
