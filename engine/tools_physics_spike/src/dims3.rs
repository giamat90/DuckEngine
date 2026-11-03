//! The 3D instance of [`Dim`]: 3D vectors, quaternion, diagonal inertia tensor; the locked-axis
//! scene runs on it.

use crate::generic::{Body, Dim, MAX_TANGENTS, RawPoint};
use crate::math3::{
    advance, apply_inverse_inertia, collide_extruded, mass_properties, planar_box, tangents,
};
use crate::scene::{BoxDef, DEPTH_HALF, DT, Fnv, GRAVITY_Y};
use base::math::{Quat, Vec3};

/// The 3D instance, used for a planar scene with locked axes.
pub struct D3;

impl Dim for D3 {
    type Vec = Vec3;
    type Ang = Vec3;
    type Rot = Quat;
    type InvInertia = Vec3;
    const TANGENTS: usize = 2;

    fn dot(a: Vec3, b: Vec3) -> f32 {
        a.dot(b)
    }
    fn length(v: Vec3) -> f32 {
        v.length()
    }
    fn gravity_step() -> Vec3 {
        Vec3::new(0.0, GRAVITY_Y * DT, 0.0)
    }
    fn spin_velocity(w: Vec3, r: Vec3) -> Vec3 {
        w.cross(r)
    }
    fn torque(r: Vec3, f: Vec3) -> Vec3 {
        r.cross(f)
    }
    fn ang_dot(a: Vec3, b: Vec3) -> f32 {
        a.dot(b)
    }
    fn ang_length(a: Vec3) -> f32 {
        a.length()
    }
    fn apply_inverse_inertia(rot: Quat, inv: Vec3, t: Vec3) -> Vec3 {
        apply_inverse_inertia(rot, inv, t)
    }
    fn advance(rot: Quat, w: Vec3, dt: f32) -> Quat {
        advance(rot, w, dt)
    }
    fn tangents(normal: Vec3) -> [Vec3; MAX_TANGENTS] {
        tangents(normal)
    }
    fn make_body(def: &BoxDef) -> Body<Self> {
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
    fn collide(a: &Body<Self>, b: &Body<Self>, out: &mut Vec<RawPoint<Self>>) -> Option<Vec3> {
        collide_extruded(
            planar_box(a.pos, a.rot, a.half),
            planar_box(b.pos, b.rot, b.half),
            a.pos.z,
            |position, separation| {
                out.push(RawPoint {
                    position,
                    separation,
                });
            },
        )
    }
    fn radius(body: &Body<Self>) -> f32 {
        body.half.length()
    }
    fn xy(v: Vec3) -> [f32; 2] {
        [v.x, v.y]
    }
    fn hash_body(h: &mut Fnv, b: &Body<Self>) {
        for v in [
            b.pos.x, b.pos.y, b.pos.z, b.rot.x, b.rot.y, b.rot.z, b.rot.w, b.lin.x, b.lin.y,
            b.lin.z, b.ang.x, b.ang.y, b.ang.z,
        ] {
            h.push(v);
        }
    }
}
