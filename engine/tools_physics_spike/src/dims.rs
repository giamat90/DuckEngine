//! The 2D instance of [`Dim`]: 2D vectors, scalar angle and inertia.

use crate::generic::{Body, Dim, MAX_TANGENTS, RawPoint};
use crate::narrow2d::{PlanarBox, collide};
use crate::scene::{BoxDef, DT, Fnv, GRAVITY_Y};
use base::math::Vec2;

/// The 2D instance: rotation is a (cos, sin) pair.
pub struct D2;

impl Dim for D2 {
    type Vec = Vec2;
    type Ang = f32;
    type Rot = [f32; 2];
    type InvInertia = f32;
    const TANGENTS: usize = 1;

    fn dot(a: Vec2, b: Vec2) -> f32 {
        a.dot(b)
    }
    fn length(v: Vec2) -> f32 {
        v.length()
    }
    fn gravity_step() -> Vec2 {
        Vec2::new(0.0, GRAVITY_Y * DT)
    }
    fn spin_velocity(w: f32, r: Vec2) -> Vec2 {
        r.perp() * w
    }
    fn torque(r: Vec2, f: Vec2) -> f32 {
        r.cross(f)
    }
    fn ang_dot(a: f32, b: f32) -> f32 {
        a * b
    }
    fn ang_length(a: f32) -> f32 {
        a.abs()
    }
    fn apply_inverse_inertia(_rot: [f32; 2], inv: f32, t: f32) -> f32 {
        inv * t
    }
    fn advance([cos, sin]: [f32; 2], w: f32, dt: f32) -> [f32; 2] {
        let d = w * dt;
        let (c, s) = (cos - sin * d, sin + cos * d);
        let inv = 1.0 / (c * c + s * s).sqrt();
        [c * inv, s * inv]
    }
    fn tangents(normal: Vec2) -> [Vec2; MAX_TANGENTS] {
        [normal.perp(), Vec2::ZERO]
    }
    fn make_body(def: &BoxDef) -> Body<Self> {
        let (inv_mass, inv_inertia) = if def.dynamic {
            let mass = 4.0 * def.half_x * def.half_y;
            let inertia = mass * (def.half_x * def.half_x + def.half_y * def.half_y) / 3.0;
            (1.0 / mass, 1.0 / inertia)
        } else {
            (0.0, 0.0)
        };
        Body {
            pos: Vec2::new(def.x, def.y),
            rot: [1.0, 0.0],
            lin: Vec2::ZERO,
            ang: 0.0,
            inv_mass,
            inv_inertia,
            half: Vec2::new(def.half_x, def.half_y),
        }
    }
    fn collide(a: &Body<Self>, b: &Body<Self>, out: &mut Vec<RawPoint<Self>>) -> Option<Vec2> {
        let planar = |body: &Body<Self>| PlanarBox {
            pos: body.pos,
            cos: body.rot[0],
            sin: body.rot[1],
            half: body.half,
        };
        let m = collide(planar(a), planar(b))?;
        out.extend(m.points.iter().take(m.count).map(|p| RawPoint {
            position: p.position,
            separation: p.separation,
        }));
        Some(m.normal)
    }
    fn radius(body: &Body<Self>) -> f32 {
        body.half.length()
    }
    fn xy(v: Vec2) -> [f32; 2] {
        [v.x, v.y]
    }
    fn hash_body(h: &mut Fnv, b: &Body<Self>) {
        for v in [
            b.pos.x, b.pos.y, b.rot[0], b.rot[1], b.lin.x, b.lin.y, b.ang,
        ] {
            h.push(v);
        }
    }
}
