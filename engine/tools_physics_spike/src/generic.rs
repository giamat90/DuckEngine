//! Variant B: the dimension-generic core. [`Dim`] names what differs between dimensions (vector,
//! angular quantity, rotation, inertia, narrowphase, friction basis); [`World`] holds everything
//! else once: body storage, pair loop, warm starting, the velocity solver, integration, hashing.

use crate::scene::{
    BIAS_FACTOR, BoxDef, DT, FRICTION, Fnv, ITERATIONS, PENETRATION_SLOP, Sim, pairs,
};
use std::ops::{Add, Mul, Neg, Sub};

/// The largest number of friction tangents of any dimension. Arrays cannot be sized by the
/// associated constant [`Dim::TANGENTS`] on stable Rust, so every contact reserves this many.
pub const MAX_TANGENTS: usize = 2;

/// Everything that depends on the dimension.
pub trait Dim: Sized {
    /// Position and linear velocity.
    type Vec: Copy
        + Default
        + Add<Output = Self::Vec>
        + Sub<Output = Self::Vec>
        + Neg<Output = Self::Vec>
        + Mul<f32, Output = Self::Vec>;
    /// Angular velocity and torque (a scalar in 2D, a vector in 3D).
    type Ang: Copy
        + Default
        + Add<Output = Self::Ang>
        + Sub<Output = Self::Ang>
        + Mul<f32, Output = Self::Ang>;
    /// Orientation.
    type Rot: Copy;
    /// Inverse inertia in the body frame.
    type InvInertia: Copy;
    /// Friction directions per contact point (1 in 2D, 2 in 3D).
    const TANGENTS: usize;

    fn dot(a: Self::Vec, b: Self::Vec) -> f32;
    fn length(v: Self::Vec) -> f32;
    fn gravity_step() -> Self::Vec;
    /// `w x r`.
    fn spin_velocity(w: Self::Ang, r: Self::Vec) -> Self::Vec;
    /// `r x f`.
    fn torque(r: Self::Vec, f: Self::Vec) -> Self::Ang;
    fn ang_dot(a: Self::Ang, b: Self::Ang) -> f32;
    fn ang_length(a: Self::Ang) -> f32;
    fn apply_inverse_inertia(rot: Self::Rot, inv: Self::InvInertia, t: Self::Ang) -> Self::Ang;
    fn advance(rot: Self::Rot, w: Self::Ang, dt: f32) -> Self::Rot;
    fn tangents(normal: Self::Vec) -> [Self::Vec; MAX_TANGENTS];
    /// Builds a body of this dimension from the planar scene description.
    fn make_body(def: &BoxDef) -> Body<Self>;
    /// Narrowphase: appends the points of a manifold and returns its normal.
    fn collide(a: &Body<Self>, b: &Body<Self>, out: &mut Vec<RawPoint<Self>>) -> Option<Self::Vec>;
    /// Bounding radius, for the cheap pair rejection.
    fn radius(body: &Body<Self>) -> f32;
    fn xy(v: Self::Vec) -> [f32; 2];
    fn hash_body(h: &mut Fnv, body: &Body<Self>);
}

/// Body state, laid out by the dimension's associated types.
pub struct Body<D: Dim> {
    pub pos: D::Vec,
    pub rot: D::Rot,
    pub lin: D::Vec,
    pub ang: D::Ang,
    pub inv_mass: f32,
    pub inv_inertia: D::InvInertia,
    pub half: D::Vec,
}

impl<D: Dim> Clone for Body<D> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<D: Dim> Copy for Body<D> {}

/// A manifold point before it becomes a constraint.
pub struct RawPoint<D: Dim> {
    pub position: D::Vec,
    pub separation: f32,
}

struct Contact<D: Dim> {
    a: usize,
    b: usize,
    key: u64,
    normal: D::Vec,
    tangent: [D::Vec; MAX_TANGENTS],
    point: D::Vec,
    separation: f32,
    ra: D::Vec,
    rb: D::Vec,
    mass_n: f32,
    mass_t: [f32; MAX_TANGENTS],
    bias: f32,
    jn: f32,
    jt: [f32; MAX_TANGENTS],
}

/// The world, once for every dimension.
pub struct World<D: Dim> {
    bodies: Vec<Body<D>>,
    contacts: Vec<Contact<D>>,
    cache: Vec<(u64, f32, [f32; MAX_TANGENTS])>,
    scratch: Vec<RawPoint<D>>,
    max_penetration: f32,
}

/// The impulses of the previous step for `key`, or zeros.
fn cached(cache: &[(u64, f32, [f32; MAX_TANGENTS])], key: u64) -> (f32, [f32; MAX_TANGENTS]) {
    cache
        .binary_search_by_key(&key, |c| c.0)
        .map_or((0.0, [0.0; MAX_TANGENTS]), |i| (cache[i].1, cache[i].2))
}

fn prepare_contact<D: Dim>(bodies: &[Body<D>], c: &mut Contact<D>) {
    let (ba, bb) = (bodies[c.a], bodies[c.b]);
    c.ra = c.point - ba.pos;
    c.rb = c.point - bb.pos;
    c.mass_n = 1.0 / effective_mass::<D>(&ba, &bb, c.ra, c.rb, c.normal);
    for (mass, dir) in c.mass_t.iter_mut().zip(c.tangent).take(D::TANGENTS) {
        *mass = 1.0 / effective_mass::<D>(&ba, &bb, c.ra, c.rb, dir);
    }
    c.bias = -BIAS_FACTOR / DT * (c.separation + PENETRATION_SLOP).min(0.0);
}

fn accumulated_impulse<D: Dim>(c: &Contact<D>) -> D::Vec {
    let mut p = c.normal * c.jn;
    for (dir, j) in c.tangent.iter().zip(c.jt).take(D::TANGENTS) {
        p = p + *dir * j;
    }
    p
}

fn solve_contact<D: Dim>(bodies: &mut [Body<D>], c: &mut Contact<D>) {
    let dv = relative_velocity::<D>(bodies, c);
    let normal_total = (c.jn + c.mass_n * (c.bias - D::dot(dv, c.normal))).max(0.0);
    let normal_delta = normal_total - c.jn;
    c.jn = normal_total;
    apply_impulse::<D>(bodies, c, c.normal * normal_delta);

    let limit = FRICTION * c.jn;
    for k in 0..D::TANGENTS {
        let dv = relative_velocity::<D>(bodies, c);
        let friction_total =
            (c.jt[k] - c.mass_t[k] * D::dot(dv, c.tangent[k])).clamp(-limit, limit);
        let friction_delta = friction_total - c.jt[k];
        c.jt[k] = friction_total;
        apply_impulse::<D>(bodies, c, c.tangent[k] * friction_delta);
    }
}

fn pair_key(a: usize, b: usize, index: usize) -> u64 {
    ((a as u64) << 40) | ((b as u64) << 8) | index as u64
}

fn effective_mass<D: Dim>(a: &Body<D>, b: &Body<D>, ra: D::Vec, rb: D::Vec, dir: D::Vec) -> f32 {
    let (rna, rnb) = (D::torque(ra, dir), D::torque(rb, dir));
    a.inv_mass
        + b.inv_mass
        + D::ang_dot(rna, D::apply_inverse_inertia(a.rot, a.inv_inertia, rna))
        + D::ang_dot(rnb, D::apply_inverse_inertia(b.rot, b.inv_inertia, rnb))
}

fn relative_velocity<D: Dim>(bodies: &[Body<D>], c: &Contact<D>) -> D::Vec {
    let (a, b) = (&bodies[c.a], &bodies[c.b]);
    b.lin + D::spin_velocity(b.ang, c.rb) - a.lin - D::spin_velocity(a.ang, c.ra)
}

fn apply_impulse<D: Dim>(bodies: &mut [Body<D>], c: &Contact<D>, p: D::Vec) {
    let a = &mut bodies[c.a];
    a.lin = a.lin - p * a.inv_mass;
    a.ang = a.ang - D::apply_inverse_inertia(a.rot, a.inv_inertia, D::torque(c.ra, p));
    let b = &mut bodies[c.b];
    b.lin = b.lin + p * b.inv_mass;
    b.ang = b.ang + D::apply_inverse_inertia(b.rot, b.inv_inertia, D::torque(c.rb, p));
}

impl<D: Dim> World<D> {
    fn find_contacts(&mut self) {
        self.contacts.clear();
        self.max_penetration = 0.0;
        for (a, b) in pairs(self.bodies.len()) {
            self.add_pair(a, b);
        }
    }

    fn add_pair(&mut self, a: usize, b: usize) {
        let (ba, bb) = (self.bodies[a], self.bodies[b]);
        if ba.inv_mass == 0.0 && bb.inv_mass == 0.0 {
            return;
        }
        let reach = D::radius(&ba) + D::radius(&bb);
        let offset = D::length(ba.pos - bb.pos);
        if offset > reach {
            return;
        }
        self.scratch.clear();
        let Some(normal) = D::collide(&ba, &bb, &mut self.scratch) else {
            return;
        };
        let tangent = D::tangents(normal);
        for (index, p) in self.scratch.iter().enumerate() {
            let key = pair_key(a, b, index);
            let (jn, jt) = cached(&self.cache, key);
            self.max_penetration = self.max_penetration.max(-p.separation);
            self.contacts.push(Contact {
                a,
                b,
                key,
                normal,
                tangent,
                point: p.position,
                separation: p.separation,
                ra: D::Vec::default(),
                rb: D::Vec::default(),
                mass_n: 0.0,
                mass_t: [0.0; MAX_TANGENTS],
                bias: 0.0,
                jn,
                jt,
            });
        }
    }

    fn apply_gravity(&mut self) {
        for body in self.bodies.iter_mut().filter(|b| b.inv_mass > 0.0) {
            body.lin = body.lin + D::gravity_step();
        }
    }

    fn prepare(&mut self) {
        for c in &mut self.contacts {
            prepare_contact::<D>(&self.bodies, c);
        }
    }

    fn warm_start(&mut self) {
        for c in &self.contacts {
            apply_impulse::<D>(&mut self.bodies, c, accumulated_impulse::<D>(c));
        }
    }

    fn solve(&mut self) {
        for c in &mut self.contacts {
            solve_contact::<D>(&mut self.bodies, c);
        }
    }

    fn store_cache(&mut self) {
        self.cache.clear();
        self.cache
            .extend(self.contacts.iter().map(|c| (c.key, c.jn, c.jt)));
    }

    fn integrate_positions(&mut self) {
        for b in &mut self.bodies {
            b.pos = b.pos + b.lin * DT;
            b.rot = D::advance(b.rot, b.ang, DT);
        }
    }
}

impl<D: Dim> Sim for World<D> {
    fn from_scene(defs: &[BoxDef]) -> Self {
        Self {
            bodies: defs.iter().map(D::make_body).collect(),
            contacts: Vec::with_capacity(defs.len() * 8),
            cache: Vec::with_capacity(defs.len() * 8),
            scratch: Vec::with_capacity(4),
            max_penetration: 0.0,
        }
    }

    fn step(&mut self) {
        self.find_contacts();
        self.apply_gravity();
        self.prepare();
        self.warm_start();
        for _ in 0..ITERATIONS {
            self.solve();
        }
        self.store_cache();
        self.integrate_positions();
    }

    fn body_bytes() -> usize {
        size_of::<Body<D>>()
    }

    fn contact_bytes() -> usize {
        size_of::<Contact<D>>()
    }

    fn pos_xy(&self, i: usize) -> [f32; 2] {
        D::xy(self.bodies[i].pos)
    }

    fn linear_speed(&self, i: usize) -> f32 {
        D::length(self.bodies[i].lin)
    }

    fn angular_speed(&self, i: usize) -> f32 {
        D::ang_length(self.bodies[i].ang)
    }

    fn max_penetration(&self) -> f32 {
        self.max_penetration
    }

    fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    fn state_hash(&self) -> u64 {
        let mut h = Fnv::new();
        for b in &self.bodies {
            D::hash_body(&mut h, b);
        }
        h.finish()
    }
}
