//! Throw-away spike of card 8-2 (not for merge): a `D=3` instance written against the real `core`,
//! to find out what the generic core carries and what each instance must write alone. One shape
//! (box), one narrowphase (SAT with face clipping and an edge pair), one solver (sub-stepped
//! sequential impulses, no warm start), no joints. Measured by the tests at the end.
#![allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::cast_possible_truncation,
    clippy::needless_range_loop,
    clippy::indexing_slicing
)]

use crate::core::{
    Body, Bounds, ContactManifold, ContactSolver, Dim, HashBits, JointEntry, MassProperties, Motion,
    NoJoint, Placed, PhysicsError, Quantity, SolverConfig, SolverStats, StateHasher, StepFrame,
};
use base::fpenv::FpGuard;
use base::math::{Quat, Vec3};

// ---- instance data -------------------------------------------------------------------------

impl HashBits for Quat {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(self.x);
        hasher.write_f32(self.y);
        hasher.write_f32(self.z);
        hasher.write_f32(self.w);
    }
}

impl Quantity for Quat {
    fn is_finite(&self) -> bool {
        Quat::is_finite(*self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub half: Vec3,
}

impl HashBits for Box3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        self.half.write_bits(hasher);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl HashBits for Aabb3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        self.min.write_bits(hasher);
        self.max.write_bits(hasher);
    }
}

impl Bounds for Aabb3 {
    fn overlaps(&self, o: &Self) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
            && self.min.z <= o.max.z
            && o.min.z <= self.max.z
    }
    fn union(&self, o: &Self) -> Self {
        Self {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }
    fn contains(&self, i: &Self) -> bool {
        self.min.x <= i.min.x
            && self.min.y <= i.min.y
            && self.min.z <= i.min.z
            && i.max.x <= self.max.x
            && i.max.y <= self.max.y
            && i.max.z <= self.max.z
    }
    fn measure(&self) -> f32 {
        let d = self.max - self.min;
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }
    fn fattened(&self, m: f32) -> Self {
        let g = Vec3::new(m, m, m);
        Self {
            min: self.min - g,
            max: self.max + g,
        }
    }
    fn is_valid(&self) -> bool {
        Quantity::is_finite(&self.min)
            && Quantity::is_finite(&self.max)
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
            && self.min.z <= self.max.z
    }
}

pub const MAX_POINTS: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Contact3 {
    pub point: Vec3,
    pub depth: f32,
}

/// Normal points from the first body to the second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Manifold3 {
    pub normal: Vec3,
    pub count: u8,
    pub points: [Contact3; MAX_POINTS],
}

impl HashBits for Manifold3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        self.normal.write_bits(hasher);
        for p in &self.points[..usize::from(self.count)] {
            p.point.write_bits(hasher);
            hasher.write_f32(p.depth);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct D3;

fn axes(q: Quat) -> [Vec3; 3] {
    [
        q.rotate(Vec3::X),
        q.rotate(Vec3::Y),
        q.rotate(Vec3::Z),
    ]
}

fn comp(v: Vec3, i: usize) -> f32 {
    [v.x, v.y, v.z][i]
}

fn mul3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)
}

fn world_inv_inertia(rot: Quat, inv: Vec3, v: Vec3) -> Vec3 {
    rot.rotate(mul3(inv, rot.conjugate().rotate(v)))
}

impl Dim for D3 {
    type Vec = Vec3;
    type Ang = Vec3;
    type Rot = Quat;
    type InvInertia = Vec3;
    type Shape = Box3;
    type Bounds = Aabb3;
    type Manifold = Manifold3;
    type Solver = Solver3;
    type Joint = NoJoint;

    fn apply_inverse_inertia(rot: Quat, inv_inertia: Vec3, torque: Vec3) -> Vec3 {
        world_inv_inertia(rot, inv_inertia, torque)
    }

    fn advance_rotation(rot: Quat, w: Vec3, dt: f32) -> Quat {
        let dq = Quat::new(w.x, w.y, w.z, 0.0) * rot;
        let k = 0.5 * dt;
        Quat::new(
            rot.x + dq.x * k,
            rot.y + dq.y * k,
            rot.z + dq.z * k,
            rot.w + dq.w * k,
        )
        .normalized()
        .unwrap_or(rot)
    }

    fn shape_bounds(shape: &Box3, position: Vec3, rot: Quat) -> Aabb3 {
        let u = axes(rot);
        let h = shape.half;
        let e = Vec3::new(
            u[0].x.abs() * h.x + u[1].x.abs() * h.y + u[2].x.abs() * h.z,
            u[0].y.abs() * h.x + u[1].y.abs() * h.y + u[2].y.abs() * h.z,
            u[0].z.abs() * h.x + u[1].z.abs() * h.y + u[2].z.abs() * h.z,
        );
        Aabb3 {
            min: position - e,
            max: position + e,
        }
    }

    fn collide(a: &Placed<Self>, b: &Placed<Self>, margin: f32) -> Option<Manifold3> {
        collide_boxes(a, b, margin)
    }

    fn mass_properties(shape: &Box3, density: f32) -> Result<MassProperties<Self>, PhysicsError> {
        let h = shape.half;
        if !(Quantity::is_finite(&h) && h.x > 0.0 && h.y > 0.0 && h.z > 0.0 && density > 0.0) {
            return Err(PhysicsError::InvalidShape);
        }
        let mass = density * 8.0 * h.x * h.y * h.z;
        let k = mass / 3.0;
        Ok(MassProperties {
            mass,
            inv_inertia: Vec3::new(
                1.0 / (k * (h.y * h.y + h.z * h.z)),
                1.0 / (k * (h.x * h.x + h.z * h.z)),
                1.0 / (k * (h.x * h.x + h.y * h.y)),
            ),
        })
    }
}

// ---- narrowphase: box against box ------------------------------------------------------------

fn clip(poly: &[Vec3], n: Vec3, offset: f32, out: &mut Vec<Vec3>) {
    out.clear();
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (dp, dq) = (n.dot(p) - offset, n.dot(q) - offset);
        if dp <= 0.0 {
            out.push(p);
        }
        if (dp < 0.0) != (dq < 0.0) && (dp - dq).abs() > f32::EPSILON {
            out.push(p + (q - p) * (dp / (dp - dq)));
        }
    }
}

fn collide_boxes(a: &Placed<D3>, b: &Placed<D3>, margin: f32) -> Option<Manifold3> {
    let (ua, ub) = (axes(a.rotation), axes(b.rotation));
    let (ha, hb) = (a.shape.half, b.shape.half);
    let t = b.position - a.position;
    let radius = |u: &[Vec3; 3], h: Vec3, l: Vec3| {
        u[0].dot(l).abs() * h.x + u[1].dot(l).abs() * h.y + u[2].dot(l).abs() * h.z
    };
    // (overlap, axis, kind) with kind 0..3 = face of a, 3..6 = face of b, 6.. = edge pair.
    let mut best_face = (f32::MAX, Vec3::ZERO, 0_usize);
    let mut best_edge = (f32::MAX, Vec3::ZERO, 0_usize);
    let mut consider = |l: Vec3, kind: usize, face: bool| -> bool {
        let s = t.dot(l);
        let overlap = radius(&ua, ha, l) + radius(&ub, hb, l) - s.abs();
        if overlap < -margin {
            return false;
        }
        let l = if s < 0.0 { -l } else { l };
        let slot = if face { &mut best_face } else { &mut best_edge };
        if overlap < slot.0 {
            *slot = (overlap, l, kind);
        }
        true
    };
    for i in 0..3 {
        if !consider(ua[i], i, true) || !consider(ub[i], 3 + i, true) {
            return None;
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            let c = ua[i].cross(ub[j]);
            let len = c.length();
            if len > 1.0e-4 && !consider(c * (1.0 / len), 6 + 3 * i + j, false) {
                return None;
            }
        }
    }
    if best_edge.0 + 0.001 < best_face.0 {
        return Some(edge_contact(a, b, &ua, &ub, best_edge));
    }
    Some(face_contact(a, b, &ua, &ub, best_face, margin))
}

fn edge_contact(
    a: &Placed<D3>,
    b: &Placed<D3>,
    ua: &[Vec3; 3],
    ub: &[Vec3; 3],
    best: (f32, Vec3, usize),
) -> Manifold3 {
    let (overlap, n, kind) = best;
    let (i, j) = ((kind - 6) / 3, (kind - 6) % 3);
    let support = |centre: Vec3, u: &[Vec3; 3], h: Vec3, skip: usize, dir: Vec3| {
        let mut p = centre;
        for k in 0..3 {
            if k != skip {
                let s = if u[k].dot(dir) >= 0.0 { 1.0 } else { -1.0 };
                p = p + u[k] * (s * comp(h, k));
            }
        }
        p
    };
    let pa = support(a.position, ua, a.shape.half, i, n);
    let pb = support(b.position, ub, b.shape.half, j, -n);
    let (d1, d2, r) = (ua[i], ub[j], pa - pb);
    let (a11, a12, a22) = (d1.dot(d1), d1.dot(d2), d2.dot(d2));
    let (c1, c2) = (d1.dot(r), d2.dot(r));
    let det = a11 * a22 - a12 * a12;
    let (s, u) = if det.abs() > 1.0e-8 {
        ((a12 * c2 - a22 * c1) / det, (a11 * c2 - a12 * c1) / det)
    } else {
        (0.0, 0.0)
    };
    let (qa, qb) = (pa + d1 * s, pb + d2 * u);
    let mut points = [Contact3::default(); MAX_POINTS];
    points[0] = Contact3 {
        point: (qa + qb) * 0.5,
        depth: overlap,
    };
    Manifold3 {
        normal: n,
        count: 1,
        points,
    }
}

fn face_contact(
    a: &Placed<D3>,
    b: &Placed<D3>,
    ua: &[Vec3; 3],
    ub: &[Vec3; 3],
    best: (f32, Vec3, usize),
    margin: f32,
) -> Manifold3 {
    let (_, n, kind) = best;
    let a_is_ref = kind < 3;
    let (rp, ru, rh, ip, iu, ih, nr) = if a_is_ref {
        (a.position, ua, a.shape.half, b.position, ub, b.shape.half, n)
    } else {
        (b.position, ub, b.shape.half, a.position, ua, a.shape.half, -n)
    };
    let ref_axis = kind % 3;
    // Incident face: the one of the other box most against the reference normal.
    let (mut inc_axis, mut inc_sign, mut best_dot) = (0, 1.0, f32::MAX);
    for k in 0..3 {
        for s in [1.0_f32, -1.0] {
            let d = iu[k].dot(nr) * s;
            if d < best_dot {
                (inc_axis, inc_sign, best_dot) = (k, s, d);
            }
        }
    }
    let (m, o) = ((inc_axis + 1) % 3, (inc_axis + 2) % 3);
    let centre = ip + iu[inc_axis] * (inc_sign * comp(ih, inc_axis));
    let (em, eo) = (iu[m] * comp(ih, m), iu[o] * comp(ih, o));
    let mut poly = vec![centre + em + eo, centre - em + eo, centre - em - eo, centre + em - eo];
    let mut tmp = Vec::with_capacity(8);
    for side in 0..3 {
        if side == ref_axis {
            continue;
        }
        let (axis, half) = (ru[side], comp(rh, side));
        clip(&poly, axis, rp.dot(axis) + half, &mut tmp);
        std::mem::swap(&mut poly, &mut tmp);
        clip(&poly, -axis, -rp.dot(axis) + half, &mut tmp);
        std::mem::swap(&mut poly, &mut tmp);
    }
    let face_offset = nr.dot(rp) + comp(rh, ref_axis);
    let mut points = [Contact3::default(); MAX_POINTS];
    let mut count = 0_u8;
    for v in &poly {
        let separation = nr.dot(*v) - face_offset;
        if separation <= margin && usize::from(count) < MAX_POINTS {
            points[usize::from(count)] = Contact3 {
                point: *v - nr * (separation * 0.5),
                depth: -separation,
            };
            count += 1;
        }
    }
    Manifold3 {
        normal: n,
        count,
        points,
    }
}

// ---- solver ---------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Work {
    pos: Vec3,
    p0: Vec3,
    rot: Quat,
    v: Vec3,
    w: Vec3,
    inv_m: f32,
    inv_i: Vec3,
    moves: bool,
}

#[derive(Clone, Copy)]
struct Row {
    a: usize,
    b: usize,
    n: Vec3,
    t1: Vec3,
    t2: Vec3,
    ra: Vec3,
    rb: Vec3,
    k: u8,
    depth: f32,
    mu: f32,
    jn: f32,
    j1: f32,
    j2: f32,
}

pub struct Solver3 {
    config: SolverConfig,
    warm: Vec<(usize, usize, u8, f32, f32, f32)>,
    next: Vec<(usize, usize, u8, f32, f32, f32)>,
}

fn tangents(n: Vec3) -> (Vec3, Vec3) {
    let helper = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let t1 = n.cross(helper).normalized().unwrap_or(Vec3::Z);
    (t1, n.cross(t1))
}

fn apply(w: &mut [Option<Work>], i: usize, r: Vec3, impulse: Vec3, sign: f32) {
    if let Some(b) = w[i].as_mut().filter(|b| b.moves) {
        b.v = b.v + impulse * (sign * b.inv_m);
        let torque = r.cross(impulse) * sign;
        b.w = b.w + world_inv_inertia(b.rot, b.inv_i, torque);
    }
}

fn velocity_at(w: &[Option<Work>], i: usize, r: Vec3) -> Vec3 {
    w[i].map_or(Vec3::ZERO, |b| b.v + b.w.cross(r))
}

fn inv_mass_along(w: &[Option<Work>], i: usize, r: Vec3, d: Vec3) -> f32 {
    w[i].filter(|b| b.moves).map_or(0.0, |b| {
        let rn = r.cross(d);
        b.inv_m + rn.dot(world_inv_inertia(b.rot, b.inv_i, rn))
    })
}

impl ContactSolver<D3> for Solver3 {
    fn new(config: SolverConfig, _mb: u32, _mm: u32, _mj: u32) -> Result<Self, PhysicsError> {
        config.validate()?;
        Ok(Self {
            config,
            warm: Vec::new(),
            next: Vec::new(),
        })
    }

    fn solve(
        &mut self,
        bodies: &mut [Option<Body<D3>>],
        contacts: &[ContactManifold<D3>],
        _joints: &[JointEntry<D3>],
        frame: StepFrame<D3>,
    ) {
        let _guard = FpGuard::deterministic();
        let sub = self.config.sub_steps.max(1);
        let h = frame.dt / sub as f32;
        let mut w: Vec<Option<Work>> = bodies
            .iter()
            .map(|b| {
                b.as_ref().map(|b| {
                    let moves = b.inverse_mass() > 0.0 && !b.is_asleep();
                    Work {
                        pos: b.position(),
                        p0: b.position(),
                        rot: b.rotation(),
                        v: b.linear_velocity(),
                        w: b.angular_velocity(),
                        inv_m: if moves { b.inverse_mass() } else { 0.0 },
                        inv_i: if moves { b.inverse_inertia() } else { Vec3::ZERO },
                        moves,
                    }
                })
            })
            .collect();
        let mut rows = Vec::new();
        for c in contacts {
            let (a, b) = (c.first.index() as usize, c.second.index() as usize);
            let (Some(ba), Some(bb)) = (&bodies[a], &bodies[b]) else {
                continue;
            };
            let m = &c.manifold;
            let (t1, t2) = tangents(m.normal);
            for (k, p) in m.points[..usize::from(m.count)].iter().enumerate() {
                let key = (a, b, k as u8);
                let (jn, j1, j2) = self
                    .warm
                    .binary_search_by(|e| (e.0, e.1, e.2).cmp(&key))
                    .map_or((0.0, 0.0, 0.0), |i| (self.warm[i].3, self.warm[i].4, self.warm[i].5));
                rows.push(Row {
                    k: k as u8,
                    a,
                    b,
                    n: m.normal,
                    t1,
                    t2,
                    ra: p.point - ba.position(),
                    rb: p.point - bb.position(),
                    depth: p.depth,
                    mu: (ba.friction() * bb.friction()).sqrt(),
                    jn,
                    j1,
                    j2,
                });
            }
        }
        for _ in 0..sub {
            for b in w.iter_mut().flatten().filter(|b| b.moves) {
                b.v = b.v + frame.gravity * h;
            }
            for r in &rows {
                let total = r.n * r.jn + r.t1 * r.j1 + r.t2 * r.j2;
                apply(&mut w, r.a, r.ra, total, -1.0);
                apply(&mut w, r.b, r.rb, total, 1.0);
            }
            for _ in 0..60 {
                for r in &mut rows {
                    relax(&mut w, r, h, &self.config);
                }
            }
            for b in w.iter_mut().flatten().filter(|b| b.moves) {
                b.pos = b.pos + b.v * h;
                b.rot = D3::advance_rotation(b.rot, b.w, h);
            }
        }
        self.next.clear();
        self.next.extend(rows.iter().map(|r| (r.a, r.b, r.k, r.jn, r.j1, r.j2)));
        for (slot, body) in bodies.iter_mut().enumerate() {
            if let (Some(body), Some(b)) = (body.as_mut(), w[slot]) {
                if b.moves {
                    body.set_motion(&Motion {
                        position: b.pos,
                        rotation: b.rot,
                        linear_velocity: b.v,
                        angular_velocity: b.w,
                    });
                }
            }
        }
    }

    fn is_finite(&self) -> bool {
        true
    }

    fn commit(&mut self) {
        std::mem::swap(&mut self.warm, &mut self.next);
    }

    fn stats(&self) -> SolverStats {
        SolverStats::default()
    }

    fn write_bits(&self, _hasher: &mut StateHasher) {}
}

fn relax(w: &mut [Option<Work>], r: &mut Row, h: f32, config: &SolverConfig) {
    let (a, b) = (r.a, r.b);
    let moved = |i: usize| w[i].map_or(Vec3::ZERO, |x| x.pos - x.p0);
    let s = -r.depth + r.n.dot(moved(b) - moved(a));
    let relative = |w: &[Option<Work>], d: Vec3| d.dot(velocity_at(w, b, r.rb) - velocity_at(w, a, r.ra));
    // Normal.
    let k = inv_mass_along(w, a, r.ra, r.n) + inv_mass_along(w, b, r.rb, r.n);
    if k > 0.0 {
        let bias = if s > 0.0 {
            s / h
        } else {
            ((s + config.linear_slop).min(0.0) * 0.2 / h).max(-config.contact_push_speed)
        };
        let lambda = -(relative(w, r.n) + bias) / k;
        let new = (r.jn + lambda).max(0.0);
        let d = new - r.jn;
        r.jn = new;
        apply(w, a, r.ra, r.n * d, -1.0);
        apply(w, b, r.rb, r.n * d, 1.0);
    }
    // Friction, two tangents, each clamped by the cone.
    let limit = r.mu * r.jn;
    for (t, j) in [(r.t1, &mut r.j1), (r.t2, &mut r.j2)] {
        let k = inv_mass_along(w, a, r.ra, t) + inv_mass_along(w, b, r.rb, t);
        if k > 0.0 {
            let lambda = -relative(w, t) / k;
            let new = (*j + lambda).clamp(-limit, limit);
            let d = new - *j;
            *j = new;
            apply(w, a, r.ra, t * d, -1.0);
            apply(w, b, r.rb, t * d, 1.0);
        }
    }
}

// ---- measurements -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, World,
        WorldConfig,
    };
    use std::time::Instant;

    const DT: f32 = 1.0 / 60.0;

    fn cube(half: f32) -> Box3 {
        Box3 {
            half: Vec3::new(half, half, half),
        }
    }

    fn placed(shape: Box3, x: f32, y: f32, z: f32) -> Placed<D3> {
        Placed::new(shape, Vec3::new(x, y, z), Quat::IDENTITY)
    }

    fn stack_world(boxes: u32) -> Result<(World<D3>, Vec<crate::core::BodyHandle<D3>>), PhysicsError> {
        let mut world = World::<D3>::new(WorldConfig {
            max_bodies: boxes + 2,
            gravity: Vec3::new(0.0, -9.806_65, 0.0),
            max_pairs: 1024,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 1024,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: crate::core::SleepConfig::DEFAULT,
        })?;
        let mut ground = BodyDesc::<D3>::new(BodyKind::Static, Vec3::new(0.0, -0.5, 0.0), Box3 {
            half: Vec3::new(20.0, 0.5, 20.0),
        });
        ground.friction = 0.6;
        world.create_body(&ground)?;
        let mut handles = Vec::new();
        for i in 0..boxes {
            let mut d = BodyDesc::<D3>::new(
                BodyKind::Dynamic,
                Vec3::new(0.0, 0.5 + i as f32 * 1.0, 0.0),
                cube(0.5),
            );
            d.friction = 0.6;
            handles.push(world.create_body(&d)?);
        }
        Ok((world, handles))
    }

    #[test]
    fn two_stacked_unit_cubes_give_a_four_point_face_manifold() {
        let m = collide_boxes(&placed(cube(0.5), 0.0, 0.0, 0.0), &placed(cube(0.5), 0.0, 0.95, 0.0), 0.02)
            .expect("overlap");
        assert_eq!(m.count, 4);
        assert!((m.normal.y - 1.0).abs() < 1e-5);
        assert!((m.points[0].depth - 0.05).abs() < 1e-4);
    }

    #[test]
    fn separated_cubes_give_no_manifold_and_edge_contact_gives_one_point() {
        assert!(collide_boxes(&placed(cube(0.5), 0.0, 0.0, 0.0), &placed(cube(0.5), 3.0, 0.0, 0.0), 0.02).is_none());
        let q = Quat::from_axis_angle(Vec3::X, std::f32::consts::FRAC_PI_4).expect("axis");
        let a = Placed::new(cube(0.5), Vec3::ZERO, q);
        let q2 = Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_4).expect("axis");
        let b = Placed::new(cube(0.5), Vec3::new(0.0, 1.35, 0.0), q2);
        let m = collide_boxes(&a, &b, 0.02).expect("edge touch");
        assert!(m.count >= 1);
    }

    fn run_stack(boxes: u32, steps: u32) -> Result<(f32, f32, f32), PhysicsError> {
        let (mut world, handles) = stack_world(boxes)?;
        for _ in 0..steps {
            world.step(DT)?;
        }
        let top = world.body(*handles.last().expect("a box"))?;
        let drift = (top.position().y - (0.5 + (boxes - 1) as f32)).abs();
        let speed = top.linear_velocity().length();
        let lateral = (top.position().x.abs()).max(top.position().z.abs());
        Ok((drift, speed, lateral))
    }

    #[test]
    fn a_stack_of_five_boxes_settles() -> Result<(), PhysicsError> {
        let (drift, speed, lateral) = run_stack(5, 600)?;
        eprintln!("stack5: drift {drift:.4} speed {speed:.5} lateral {lateral:.5}");
        assert!(drift < 0.1 && speed < 0.05 && lateral < 0.05);
        Ok(())
    }

    #[test]
    fn a_stack_of_ten_boxes_settles() -> Result<(), PhysicsError> {
        let (drift, speed, lateral) = run_stack(10, 900)?;
        eprintln!("stack10: drift {drift:.4} speed {speed:.5} lateral {lateral:.5}");
        assert!(drift < 0.15 && speed < 0.1 && lateral < 0.1);
        Ok(())
    }

    #[test]
    fn the_replay_hash_is_the_same_twice() -> Result<(), PhysicsError> {
        let (mut a, _) = stack_world(6)?;
        let (mut b, _) = stack_world(6)?;
        for _ in 0..120 {
            a.step(DT)?;
            b.step(DT)?;
            assert_eq!(a.state_hash(), b.state_hash());
        }
        Ok(())
    }

    #[test]
    fn sizes_and_timing() -> Result<(), PhysicsError> {
        eprintln!(
            "size Body<D3> {} Body<D2> {} ContactManifold<D3> {} ContactManifold<D2> {}",
            std::mem::size_of::<Body<D3>>(),
            std::mem::size_of::<Body<crate::d2::D2>>(),
            std::mem::size_of::<ContactManifold<D3>>(),
            std::mem::size_of::<ContactManifold<crate::d2::D2>>(),
        );
        let (mut world, _) = stack_world(10)?;
        let start = Instant::now();
        for _ in 0..600 {
            world.step(DT)?;
        }
        eprintln!("10 boxes, 600 steps: {:?} per step", start.elapsed() / 600);
        Ok(())
    }
}

#[cfg(test)]
mod debug {
    use super::*;
    use crate::core::{BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, World, WorldConfig};

    #[test]
    fn trace_one_box_on_ground() -> Result<(), PhysicsError> {
        let mut world = World::<D3>::new(WorldConfig {
            max_bodies: 4,
            gravity: Vec3::new(0.0, -9.806_65, 0.0),
            max_pairs: 64,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 64,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: crate::core::SleepConfig::DEFAULT,
        })?;
        let g = BodyDesc::<D3>::new(BodyKind::Static, Vec3::new(0.0, -0.5, 0.0), Box3 { half: Vec3::new(20.0, 0.5, 20.0) });
        world.create_body(&g)?;
        let _low = world.create_body(&BodyDesc::<D3>::new(BodyKind::Dynamic, Vec3::new(0.0, 0.5, 0.0), Box3 { half: Vec3::new(0.5, 0.5, 0.5) }))?;
        let b = world.create_body(&BodyDesc::<D3>::new(BodyKind::Dynamic, Vec3::new(0.0, 1.5, 0.0), Box3 { half: Vec3::new(0.5, 0.5, 0.5) }))?;
        for i in 0..40 {
            world.step(1.0 / 60.0)?;
            let body = world.body(b)?;
            if i < 8 || i % 8 == 0 {
                eprintln!("{i}: pos {:?} v {:?} w {:?}", body.position(), body.linear_velocity(), body.angular_velocity());
            }
        }
        Ok(())
    }
}
