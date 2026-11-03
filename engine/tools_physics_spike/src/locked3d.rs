//! Variant A: a concrete 3D world used for the planar scene. Bodies have `Vec3` position and
//! velocities, a quaternion and a body-frame diagonal inverse inertia whose X and Y entries are
//! zero (the locked axes, as an allowed-DOF mask does in a full engine). Contacts are the four
//! points of an extruded face contact; friction has two tangents. The solver is general 3D code.

use crate::math3::{
    advance, apply_inverse_inertia, collide_extruded, mass_properties, planar_box, tangents,
};
use crate::scene::{
    BIAS_FACTOR, BoxDef, DEPTH_HALF, DT, FRICTION, Fnv, GRAVITY_Y, ITERATIONS, PENETRATION_SLOP,
    Sim, pairs,
};
use base::math::{Quat, Vec3};

/// Body state of the 3D layout.
#[derive(Clone, Copy)]
pub struct Body {
    pos: Vec3,
    rot: Quat,
    lin: Vec3,
    ang: Vec3,
    inv_mass: f32,
    inv_inertia: Vec3,
    half: Vec3,
}

struct Contact {
    a: usize,
    b: usize,
    key: u64,
    normal: Vec3,
    tangent: [Vec3; 2],
    point: Vec3,
    separation: f32,
    ra: Vec3,
    rb: Vec3,
    mass_n: f32,
    mass_t: [f32; 2],
    bias: f32,
    jn: f32,
    jt: [f32; 2],
}

/// The world of variant A.
pub struct World {
    bodies: Vec<Body>,
    contacts: Vec<Contact>,
    cache: Vec<(u64, f32, [f32; 2])>,
    scratch: Vec<(Vec3, f32)>,
    max_penetration: f32,
}

fn make_body(def: &BoxDef) -> Body {
    let (inv_mass, inv_inertia) = if def.dynamic {
        mass_properties(def.half_x, def.half_y)
    } else {
        (0.0, Vec3::ZERO)
    };
    Body {
        pos: Vec3::new(def.x, def.y, 0.0),
        rot: Quat::default(),
        lin: Vec3::ZERO,
        ang: Vec3::ZERO,
        inv_mass,
        inv_inertia,
        half: Vec3::new(def.half_x, def.half_y, DEPTH_HALF),
    }
}

fn pair_key(a: usize, b: usize, index: usize) -> u64 {
    ((a as u64) << 40) | ((b as u64) << 8) | index as u64
}

/// The impulses of the previous step for `key`, or zeros.
fn cached(cache: &[(u64, f32, [f32; 2])], key: u64) -> (f32, [f32; 2]) {
    cache
        .binary_search_by_key(&key, |c| c.0)
        .map_or((0.0, [0.0; 2]), |i| (cache[i].1, cache[i].2))
}

fn prepare_contact(bodies: &[Body], c: &mut Contact) {
    let (ba, bb) = (bodies[c.a], bodies[c.b]);
    c.ra = c.point - ba.pos;
    c.rb = c.point - bb.pos;
    c.mass_n = 1.0 / effective_mass(&ba, &bb, c.ra, c.rb, c.normal);
    for k in 0..2 {
        c.mass_t[k] = 1.0 / effective_mass(&ba, &bb, c.ra, c.rb, c.tangent[k]);
    }
    c.bias = -BIAS_FACTOR / DT * (c.separation + PENETRATION_SLOP).min(0.0);
}

fn solve_contact(bodies: &mut [Body], c: &mut Contact) {
    let dv = relative_velocity(bodies, c);
    let normal_total = (c.jn + c.mass_n * (c.bias - dv.dot(c.normal))).max(0.0);
    let normal_delta = normal_total - c.jn;
    c.jn = normal_total;
    apply_impulse(bodies, c, c.normal * normal_delta);

    let limit = FRICTION * c.jn;
    for k in 0..2 {
        let dv = relative_velocity(bodies, c);
        let friction_total = (c.jt[k] - c.mass_t[k] * dv.dot(c.tangent[k])).clamp(-limit, limit);
        let friction_delta = friction_total - c.jt[k];
        c.jt[k] = friction_total;
        apply_impulse(bodies, c, c.tangent[k] * friction_delta);
    }
}

fn effective_mass(a: &Body, b: &Body, ra: Vec3, rb: Vec3, dir: Vec3) -> f32 {
    let (rna, rnb) = (ra.cross(dir), rb.cross(dir));
    a.inv_mass
        + b.inv_mass
        + rna.dot(apply_inverse_inertia(a.rot, a.inv_inertia, rna))
        + rnb.dot(apply_inverse_inertia(b.rot, b.inv_inertia, rnb))
}

fn relative_velocity(bodies: &[Body], c: &Contact) -> Vec3 {
    let (a, b) = (&bodies[c.a], &bodies[c.b]);
    b.lin + b.ang.cross(c.rb) - a.lin - a.ang.cross(c.ra)
}

fn apply_impulse(bodies: &mut [Body], c: &Contact, p: Vec3) {
    let a = &mut bodies[c.a];
    a.lin -= p * a.inv_mass;
    a.ang -= apply_inverse_inertia(a.rot, a.inv_inertia, c.ra.cross(p));
    let b = &mut bodies[c.b];
    b.lin += p * b.inv_mass;
    b.ang += apply_inverse_inertia(b.rot, b.inv_inertia, c.rb.cross(p));
}

impl World {
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
        let reach = ba.half.length() + bb.half.length();
        if (ba.pos - bb.pos).length() > reach {
            return;
        }
        let scratch = &mut self.scratch;
        scratch.clear();
        let planar_a = planar_box(ba.pos, ba.rot, ba.half);
        let planar_b = planar_box(bb.pos, bb.rot, bb.half);
        let Some(normal) =
            collide_extruded(planar_a, planar_b, ba.pos.z, |p, s| scratch.push((p, s)))
        else {
            return;
        };
        let tangent = tangents(normal);
        for (index, &(position, separation)) in self.scratch.iter().enumerate() {
            let key = pair_key(a, b, index);
            let (jn, jt) = cached(&self.cache, key);
            self.max_penetration = self.max_penetration.max(-separation);
            self.contacts.push(Contact {
                a,
                b,
                key,
                normal,
                tangent,
                point: position,
                separation,
                ra: Vec3::ZERO,
                rb: Vec3::ZERO,
                mass_n: 0.0,
                mass_t: [0.0; 2],
                bias: 0.0,
                jn,
                jt,
            });
        }
    }

    fn apply_gravity(&mut self) {
        for body in self.bodies.iter_mut().filter(|b| b.inv_mass > 0.0) {
            body.lin.y += GRAVITY_Y * DT;
        }
    }

    fn prepare(&mut self) {
        for c in &mut self.contacts {
            prepare_contact(&self.bodies, c);
        }
    }

    fn warm_start(&mut self) {
        for c in &self.contacts {
            let p = c.normal * c.jn + c.tangent[0] * c.jt[0] + c.tangent[1] * c.jt[1];
            apply_impulse(&mut self.bodies, c, p);
        }
    }

    fn solve(&mut self) {
        for c in &mut self.contacts {
            solve_contact(&mut self.bodies, c);
        }
    }

    fn store_cache(&mut self) {
        self.cache.clear();
        self.cache
            .extend(self.contacts.iter().map(|c| (c.key, c.jn, c.jt)));
    }

    fn integrate_positions(&mut self) {
        for b in &mut self.bodies {
            b.pos += b.lin * DT;
            b.rot = advance(b.rot, b.ang, DT);
        }
    }
}

impl Sim for World {
    fn from_scene(defs: &[BoxDef]) -> Self {
        Self {
            bodies: defs.iter().map(make_body).collect(),
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
        size_of::<Body>()
    }

    fn contact_bytes() -> usize {
        size_of::<Contact>()
    }

    fn pos_xy(&self, i: usize) -> [f32; 2] {
        [self.bodies[i].pos.x, self.bodies[i].pos.y]
    }

    fn linear_speed(&self, i: usize) -> f32 {
        self.bodies[i].lin.length()
    }

    fn angular_speed(&self, i: usize) -> f32 {
        self.bodies[i].ang.length()
    }

    fn max_penetration(&self) -> f32 {
        self.max_penetration
    }

    fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    fn state_hash(&self) -> u64 {
        let mut h = Fnv::new();
        self.bodies.iter().for_each(|b| hash_body(&mut h, b));
        h.finish()
    }
}

fn hash_body(hash: &mut Fnv, body: &Body) {
    let (pos, rot) = (body.pos, body.rot);
    for value in [pos.x, pos.y, pos.z, rot.x, rot.y, rot.z, rot.w] {
        hash.push(value);
    }
    let (lin, ang) = (body.lin, body.ang);
    for value in [lin.x, lin.y, lin.z, ang.x, ang.y, ang.z] {
        hash.push(value);
    }
}
