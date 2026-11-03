//! Variant C: a native 2D world with concrete types (`Vec2`, scalar angle, scalar inertia). This
//! is what a separate 2D library would look like (option C of physics §4) and the reference for
//! the abstraction cost of variant B. Sequential impulses with warm starting.

use crate::narrow2d::{PlanarBox, collide};
use crate::scene::{
    BIAS_FACTOR, BoxDef, DT, FRICTION, Fnv, GRAVITY_Y, ITERATIONS, PENETRATION_SLOP, Sim, pairs,
};
use base::math::Vec2;

/// Body state: 2D vectors, rotation as a (cos, sin) pair, scalar angular velocity and inertia.
#[derive(Clone, Copy)]
pub struct Body {
    pos: Vec2,
    cos: f32,
    sin: f32,
    lin: Vec2,
    ang: f32,
    inv_mass: f32,
    inv_inertia: f32,
    half: Vec2,
}

struct Contact {
    a: usize,
    b: usize,
    key: u64,
    normal: Vec2,
    tangent: Vec2,
    point: Vec2,
    separation: f32,
    ra: Vec2,
    rb: Vec2,
    mass_n: f32,
    mass_t: f32,
    bias: f32,
    jn: f32,
    jt: f32,
}

/// The world of variant C.
pub struct World {
    bodies: Vec<Body>,
    contacts: Vec<Contact>,
    cache: Vec<(u64, f32, f32)>,
    max_penetration: f32,
}

fn make_body(def: &BoxDef) -> Body {
    let (inv_mass, inv_inertia) = if def.dynamic {
        let mass = 4.0 * def.half_x * def.half_y;
        let inertia = mass * (def.half_x * def.half_x + def.half_y * def.half_y) / 3.0;
        (1.0 / mass, 1.0 / inertia)
    } else {
        (0.0, 0.0)
    };
    Body {
        pos: Vec2::new(def.x, def.y),
        cos: 1.0,
        sin: 0.0,
        lin: Vec2::ZERO,
        ang: 0.0,
        inv_mass,
        inv_inertia,
        half: Vec2::new(def.half_x, def.half_y),
    }
}

fn planar(b: &Body) -> PlanarBox {
    PlanarBox {
        pos: b.pos,
        cos: b.cos,
        sin: b.sin,
        half: b.half,
    }
}

fn pair_key(a: usize, b: usize, index: usize) -> u64 {
    ((a as u64) << 40) | ((b as u64) << 8) | index as u64
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
        let Some(m) = collide(planar(&ba), planar(&bb)) else {
            return;
        };
        let tangent = m.normal.perp();
        for (index, p) in m.points.iter().take(m.count).enumerate() {
            let key = pair_key(a, b, index);
            let (jn, jt) = cached(&self.cache, key);
            self.max_penetration = self.max_penetration.max(-p.separation);
            self.contacts.push(Contact {
                a,
                b,
                key,
                normal: m.normal,
                tangent,
                point: p.position,
                separation: p.separation,
                ra: Vec2::ZERO,
                rb: Vec2::ZERO,
                mass_n: 0.0,
                mass_t: 0.0,
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
            let p = c.normal * c.jn + c.tangent * c.jt;
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
            let d = b.ang * DT;
            let (c, s) = (b.cos - b.sin * d, b.sin + b.cos * d);
            let inv = 1.0 / (c * c + s * s).sqrt();
            b.cos = c * inv;
            b.sin = s * inv;
        }
    }
}

/// The impulses of the previous step for `key`, or zeros.
fn cached(cache: &[(u64, f32, f32)], key: u64) -> (f32, f32) {
    cache
        .binary_search_by_key(&key, |c| c.0)
        .map_or((0.0, 0.0), |i| (cache[i].1, cache[i].2))
}

fn prepare_contact(bodies: &[Body], c: &mut Contact) {
    let (ba, bb) = (bodies[c.a], bodies[c.b]);
    c.ra = c.point - ba.pos;
    c.rb = c.point - bb.pos;
    c.mass_n = 1.0 / effective_mass(&ba, &bb, c.ra, c.rb, c.normal);
    c.mass_t = 1.0 / effective_mass(&ba, &bb, c.ra, c.rb, c.tangent);
    c.bias = -BIAS_FACTOR / DT * (c.separation + PENETRATION_SLOP).min(0.0);
}

fn solve_contact(bodies: &mut [Body], c: &mut Contact) {
    let dv = relative_velocity(bodies, c);
    let normal_total = (c.jn + c.mass_n * (c.bias - dv.dot(c.normal))).max(0.0);
    let normal_delta = normal_total - c.jn;
    c.jn = normal_total;
    apply_impulse(bodies, c, c.normal * normal_delta);

    let dv = relative_velocity(bodies, c);
    let limit = FRICTION * c.jn;
    let friction_total = (c.jt - c.mass_t * dv.dot(c.tangent)).clamp(-limit, limit);
    let friction_delta = friction_total - c.jt;
    c.jt = friction_total;
    apply_impulse(bodies, c, c.tangent * friction_delta);
}

fn effective_mass(a: &Body, b: &Body, ra: Vec2, rb: Vec2, dir: Vec2) -> f32 {
    let (rna, rnb) = (ra.cross(dir), rb.cross(dir));
    a.inv_mass + b.inv_mass + a.inv_inertia * rna * rna + b.inv_inertia * rnb * rnb
}

fn relative_velocity(bodies: &[Body], c: &Contact) -> Vec2 {
    let (a, b) = (&bodies[c.a], &bodies[c.b]);
    b.lin + c.rb.perp() * b.ang - a.lin - c.ra.perp() * a.ang
}

fn apply_impulse(bodies: &mut [Body], c: &Contact, p: Vec2) {
    let a = &mut bodies[c.a];
    a.lin -= p * a.inv_mass;
    a.ang -= a.inv_inertia * c.ra.cross(p);
    let b = &mut bodies[c.b];
    b.lin += p * b.inv_mass;
    b.ang += b.inv_inertia * c.rb.cross(p);
}

impl Sim for World {
    fn from_scene(defs: &[BoxDef]) -> Self {
        Self {
            bodies: defs.iter().map(make_body).collect(),
            contacts: Vec::with_capacity(defs.len() * 2),
            cache: Vec::with_capacity(defs.len() * 2),
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
        self.bodies[i].ang.abs()
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

fn hash_body(hash: &mut Fnv, b: &Body) {
    for v in [b.pos.x, b.pos.y, b.cos, b.sin, b.lin.x, b.lin.y, b.ang] {
        hash.push(v);
    }
}
