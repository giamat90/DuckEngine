//! The lane path of the solver (card 4-15): four constraints at a time, one per lane of
//! `base::simd::F32x4`, equal bit for bit to four calls of the scalar reference.
//!
//! Why it is equal by construction. Every lane runs the same operations, in the same order, on the
//! same operands as `Constraint::solve` and `Constraint::warm_start`: one IEEE-754 `+ - *` per
//! lane per operation (`F32x4` is the SSE2 or NEON backend, tested bit for bit against its scalar
//! reference in `base`), no fused multiply-add, no reciprocal estimate, no reassociation. The
//! decisions that are not a plain arithmetic operation (the speculative or soft branch, the clamp
//! of the friction impulse, the `max` of the accumulated impulse) are made per lane by the same
//! scalar functions the reference calls. A point a constraint does not have (a manifold of one
//! point) is computed on the zero point and then not written: the old value of the lane is selected
//! back, so no operation on dummy data can reach a result. The four constraints of a group belong to
//! one colour, so they share no moving body and cannot see each other's writes; which four make a
//! group is then irrelevant to the result.
//!
//! The module holds no `unsafe` and no intrinsic: the SIMD backends live in `base`.

use super::{Constraint, Pass, Point, Staged, Sweep, limited};
use crate::d2::MAX_MANIFOLD_POINTS;
use base::math::Vec2;
use base::simd::F32x4 as Lanes;

/// Constraints per group: the width of `F32x4`.
pub const LANES: usize = 4;

/// Runs `pass` on the group; restitution (once per step, not a hot loop) uses the scalar reference.
pub(super) fn run(group: &mut [Staged; LANES], pass: Pass) {
    match pass {
        Pass::WarmStart => warm_start(group),
        Pass::Solve(sweep) => solve(group, sweep),
        Pass::Restitute(_) => group.iter_mut().for_each(|staged| staged.run(pass)),
    }
}

/// A 2D vector per lane.
#[derive(Clone, Copy)]
struct Vec2s {
    x: Lanes,
    y: Lanes,
}

impl Vec2s {
    fn of(group: &[Staged; LANES], pick: impl Fn(&Staged) -> Vec2) -> Self {
        let each = group.each_ref().map(pick);
        Self {
            x: Lanes::from_array(each.map(|v| v.x)),
            y: Lanes::from_array(each.map(|v| v.y)),
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }

    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }

    fn scale(self, factor: Lanes) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
        }
    }

    /// `x * x' + y * y'`, as `Vec2::dot`.
    fn dot(self, other: Self) -> Lanes {
        self.x * other.x + self.y * other.y
    }

    /// `x * y' - y * x'`, as `Vec2::cross`.
    fn cross(self, other: Self) -> Lanes {
        self.x * other.y - self.y * other.x
    }
}

/// `new` in the lanes of `live`, `old` in the others.
fn select(live: [bool; LANES], new: Lanes, old: Lanes) -> Lanes {
    let (new, old) = (new.to_array(), old.to_array());
    Lanes::from_array(std::array::from_fn(|lane| {
        if live[lane] { new[lane] } else { old[lane] }
    }))
}

/// `f` of each lane of `values` and `bounds`: the scalar function the reference calls.
fn per_lane(values: Lanes, bounds: Lanes, f: impl Fn(f32, f32) -> f32) -> Lanes {
    let (values, bounds) = (values.to_array(), bounds.to_array());
    Lanes::from_array(std::array::from_fn(|lane| f(values[lane], bounds[lane])))
}

/// The velocities and the motion of one body of each of the four constraints.
#[derive(Clone, Copy)]
struct BodyLanes {
    velocity: Vec2s,
    spin: Lanes,
    shift: Vec2s,
    cos: Lanes,
    sin: Lanes,
    inv_mass: Lanes,
    inv_inertia: Lanes,
}

impl BodyLanes {
    fn gather(
        group: &[Staged; LANES],
        pick: impl Fn(&Staged) -> &super::SolverBody + Copy,
    ) -> Self {
        let turn = group.each_ref().map(|s| pick(s).turn.direction());
        let float = |field: fn(&super::SolverBody) -> f32| {
            Lanes::from_array(group.each_ref().map(|s| field(pick(s))))
        };
        Self {
            velocity: Vec2s::of(group, |s| pick(s).velocity),
            spin: float(|b| b.spin),
            shift: Vec2s::of(group, |s| pick(s).shift),
            cos: Lanes::from_array(turn.map(|(cos, _)| cos)),
            sin: Lanes::from_array(turn.map(|(_, sin)| sin)),
            inv_mass: float(|b| b.inv_mass),
            inv_inertia: float(|b| b.inv_inertia),
        }
    }

    /// `SolverBody::velocity_at`.
    fn velocity_at(&self, anchor: Vec2s) -> Vec2s {
        Vec2s {
            x: self.velocity.x - self.spin * anchor.y,
            y: self.velocity.y + self.spin * anchor.x,
        }
    }

    /// `Rot2::rotate` of the turn made so far.
    fn rotate(&self, vector: Vec2s) -> Vec2s {
        Vec2s {
            x: self.cos * vector.x - self.sin * vector.y,
            y: self.sin * vector.x + self.cos * vector.y,
        }
    }

    /// `SolverBody::push` in the lanes of `live`; the others keep their velocities.
    fn push(&mut self, live: [bool; LANES], anchor: Vec2s, impulse: Vec2s) {
        let velocity = self.velocity.add(impulse.scale(self.inv_mass));
        let spin = self.spin + self.inv_inertia * anchor.cross(impulse);
        self.velocity = Vec2s {
            x: select(live, velocity.x, self.velocity.x),
            y: select(live, velocity.y, self.velocity.y),
        };
        self.spin = select(live, spin, self.spin);
    }

    /// Writes the velocities back into the staged copies.
    fn store(
        &self,
        group: &mut [Staged; LANES],
        pick: impl Fn(&mut Staged) -> &mut super::SolverBody,
    ) {
        let (x, y) = (self.velocity.x.to_array(), self.velocity.y.to_array());
        let spin = self.spin.to_array();
        for (lane, staged) in group.iter_mut().enumerate() {
            let body = pick(staged);
            body.velocity = Vec2::new(x[lane], y[lane]);
            body.spin = spin[lane];
        }
    }
}

/// Point `index` of each of the four constraints.
#[derive(Clone, Copy)]
struct PointLanes {
    live: [bool; LANES],
    anchor_a: Vec2s,
    anchor_b: Vec2s,
    base_separation: Lanes,
    normal_mass: Lanes,
    tangent_mass: Lanes,
    normal_impulse: Lanes,
    tangent_impulse: Lanes,
    max_normal_impulse: Lanes,
}

impl PointLanes {
    fn gather(group: &[Staged; LANES], index: usize) -> Self {
        let point = |s: &Staged| s.constraint.points.get(index).copied().unwrap_or_default();
        let float = |field: fn(&Point) -> f32| {
            Lanes::from_array(group.each_ref().map(|s| field(&point(s))))
        };
        Self {
            live: group.each_ref().map(|s| index < s.constraint.count),
            anchor_a: Vec2s::of(group, |s| point(s).anchor_a),
            anchor_b: Vec2s::of(group, |s| point(s).anchor_b),
            base_separation: float(|p| p.base_separation),
            normal_mass: float(|p| p.normal_mass),
            tangent_mass: float(|p| p.tangent_mass),
            normal_impulse: float(|p| p.normal_impulse),
            tangent_impulse: float(|p| p.tangent_impulse),
            max_normal_impulse: float(|p| p.max_normal_impulse),
        }
    }

    /// Writes the three impulses back into the points of the staged constraints.
    fn store(&self, group: &mut [Staged; LANES], index: usize) {
        let normal = self.normal_impulse.to_array();
        let tangent = self.tangent_impulse.to_array();
        let peak = self.max_normal_impulse.to_array();
        for (lane, staged) in group.iter_mut().enumerate() {
            let impulses = [normal[lane], tangent[lane], peak[lane]];
            set_impulses(&mut staged.constraint, index, impulses);
        }
    }
}

/// Writes `[normal, tangent, max normal]` impulses into point `index` of `constraint`.
fn set_impulses(constraint: &mut Constraint, index: usize, impulses: [f32; 3]) {
    let [normal, tangent, peak] = impulses;
    if let Some(point) = constraint.points.get_mut(index) {
        point.normal_impulse = normal;
        point.tangent_impulse = tangent;
        point.max_normal_impulse = peak;
    }
}

/// The part of the four constraints every point shares.
struct Shared {
    normal: Vec2s,
    tangent: Vec2s,
    friction: Lanes,
}

impl Shared {
    fn gather(group: &[Staged; LANES]) -> Self {
        let normal = Vec2s::of(group, |s| s.constraint.normal);
        Self {
            normal,
            // `tangent_of`: the normal turned a quarter clockwise.
            tangent: Vec2s {
                x: normal.y,
                y: -normal.x,
            },
            friction: Lanes::from_array(group.each_ref().map(|s| s.constraint.friction)),
        }
    }
}

/// The two bodies of the four constraints and their points, while a pass runs.
struct Workspace {
    a: BodyLanes,
    b: BodyLanes,
    points: [PointLanes; MAX_MANIFOLD_POINTS],
}

impl Workspace {
    fn gather(group: &[Staged; LANES]) -> Self {
        Self {
            a: BodyLanes::gather(group, |s| &s.a),
            b: BodyLanes::gather(group, |s| &s.b),
            points: std::array::from_fn(|index| PointLanes::gather(group, index)),
        }
    }

    fn store(&self, group: &mut [Staged; LANES]) {
        self.a.store(group, |s| &mut s.a);
        self.b.store(group, |s| &mut s.b);
        for (index, point) in self.points.iter().enumerate() {
            point.store(group, index);
        }
    }

    /// Pushes `impulse` at the anchors of point `index`: the reverse on `a`, the impulse on `b`.
    fn push_pair(&mut self, index: usize, impulse: Vec2s) {
        let point = self.points[index];
        self.a.push(point.live, point.anchor_a, impulse.neg());
        self.b.push(point.live, point.anchor_b, impulse);
    }

    /// The velocity of `b` at its anchor relative to that of `a` at its anchor.
    fn relative_velocity(&self, index: usize) -> Vec2s {
        let point = self.points[index];
        self.b
            .velocity_at(point.anchor_b)
            .sub(self.a.velocity_at(point.anchor_a))
    }
}

/// `Constraint::warm_start` for four constraints.
fn warm_start(group: &mut [Staged; LANES]) {
    let shared = Shared::gather(group);
    let mut work = Workspace::gather(group);
    for index in 0..MAX_MANIFOLD_POINTS {
        let point = work.points[index];
        let impulse = shared
            .normal
            .scale(point.normal_impulse)
            .add(shared.tangent.scale(point.tangent_impulse));
        work.push_pair(index, impulse);
    }
    work.a.store(group, |s| &mut s.a);
    work.b.store(group, |s| &mut s.b);
}

/// `Constraint::solve` for four constraints: friction on every point, then the normal on every point.
fn solve(group: &mut [Staged; LANES], sweep: &Sweep) {
    let shared = Shared::gather(group);
    let mut work = Workspace::gather(group);
    for index in 0..MAX_MANIFOLD_POINTS {
        friction_point(&mut work, &shared, index);
    }
    for index in 0..MAX_MANIFOLD_POINTS {
        normal_point(&mut work, &shared, group, sweep, index);
    }
    work.store(group);
}

fn friction_point(work: &mut Workspace, shared: &Shared, index: usize) {
    let point = work.points[index];
    let relative = work.relative_velocity(index);
    let delta = -point.tangent_mass * relative.dot(shared.tangent);
    let bound = shared.friction * point.normal_impulse;
    let limit = per_lane(point.tangent_impulse + delta, bound, limited);
    let applied = limit - point.tangent_impulse;
    let total = point.tangent_impulse + applied;
    if let Some(slot) = work.points.get_mut(index) {
        slot.tangent_impulse = select(point.live, total, point.tangent_impulse);
    }
    work.push_pair(index, shared.tangent.scale(applied));
}

fn normal_point(
    work: &mut Workspace,
    shared: &Shared,
    group: &[Staged; LANES],
    sweep: &Sweep,
    index: usize,
) {
    let point = work.points[index];
    let moved = work.b.shift.sub(work.a.shift).add(
        work.b
            .rotate(point.anchor_b)
            .sub(work.a.rotate(point.anchor_a)),
    );
    let separation = moved.dot(shared.normal) + point.base_separation;
    let (bias, mass_scale, impulse_scale) = soft_terms(group, sweep, separation);
    let relative = work.relative_velocity(index);
    let delta = -point.normal_mass * mass_scale * (shared.normal.dot(relative) + bias)
        - impulse_scale * point.normal_impulse;
    // `Constraint::accumulate`: the total may not go below zero.
    let total = per_lane(point.normal_impulse + delta, Lanes::splat(0.0), f32::max);
    let applied = total - point.normal_impulse;
    let peak = per_lane(point.max_normal_impulse, total, f32::max);
    if let Some(slot) = work.points.get_mut(index) {
        slot.normal_impulse = select(point.live, total, point.normal_impulse);
        slot.max_normal_impulse = select(point.live, peak, point.max_normal_impulse);
    }
    work.push_pair(index, shared.normal.scale(applied));
}

/// `Sweep::terms` per lane: the bias and the two scales of each constraint at its separation.
fn soft_terms(group: &[Staged; LANES], sweep: &Sweep, separation: Lanes) -> (Lanes, Lanes, Lanes) {
    let separation = separation.to_array();
    let terms: [(f32, f32, f32); LANES] =
        std::array::from_fn(|lane| sweep.terms(separation[lane], &group[lane].constraint.softness));
    (
        Lanes::from_array(terms.map(|(bias, _, _)| bias)),
        Lanes::from_array(terms.map(|(_, mass, _)| mass)),
        Lanes::from_array(terms.map(|(_, _, impulse)| impulse)),
    )
}

#[cfg(test)]
mod tests;
