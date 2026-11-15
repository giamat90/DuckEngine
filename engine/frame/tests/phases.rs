//! The order of the phases of one frame (design: `wiki/architecture/ecs-and-sandbox-v0.md`
//! section 4, "Glue").

use frame::{FramePlan, Phase};

fn plan(steps: u32) -> Vec<Phase> {
    FramePlan::new(steps).take(1000).collect()
}

#[test]
fn the_phases_come_in_the_order_of_the_design_note() {
    assert_eq!(
        Phase::ORDER,
        [
            Phase::Input,
            Phase::Gameplay,
            Phase::SyncPhysicsIn,
            Phase::PhysicsStep,
            Phase::SyncPhysicsOut,
            Phase::SceneUpdate,
            Phase::SyncRender,
            Phase::Render,
            Phase::SyncAudio,
        ]
    );
}

#[test]
fn only_the_three_physics_phases_run_in_the_fixed_step() {
    let fixed: Vec<Phase> = Phase::ORDER
        .iter()
        .copied()
        .filter(|p| p.runs_in_fixed_step())
        .collect();
    assert_eq!(
        fixed,
        [
            Phase::SyncPhysicsIn,
            Phase::PhysicsStep,
            Phase::SyncPhysicsOut
        ]
    );
}

#[test]
fn a_frame_without_a_fixed_step_skips_the_physics_phases() {
    assert_eq!(
        plan(0),
        [
            Phase::Input,
            Phase::Gameplay,
            Phase::SceneUpdate,
            Phase::SyncRender,
            Phase::Render,
            Phase::SyncAudio,
        ]
    );
}

#[test]
fn a_frame_with_one_fixed_step_runs_the_physics_phases_once_in_order() {
    assert_eq!(
        plan(1),
        [
            Phase::Input,
            Phase::Gameplay,
            Phase::SyncPhysicsIn,
            Phase::PhysicsStep,
            Phase::SyncPhysicsOut,
            Phase::SceneUpdate,
            Phase::SyncRender,
            Phase::Render,
            Phase::SyncAudio,
        ]
    );
}

#[test]
fn several_fixed_steps_repeat_the_three_physics_phases_as_a_block() {
    let phases = plan(3);
    assert_eq!(phases.len(), 6 + 9);
    let block = [
        Phase::SyncPhysicsIn,
        Phase::PhysicsStep,
        Phase::SyncPhysicsOut,
    ];
    for repeat in 0..3 {
        let at = 2 + repeat * 3;
        assert_eq!(phases.get(at..at + 3), Some(&block[..]), "block {repeat}");
    }
    assert_eq!(phases.get(11), Some(&Phase::SceneUpdate));
    assert_eq!(phases.last(), Some(&Phase::SyncAudio));
}

#[test]
fn the_length_is_six_plus_three_per_step_and_is_reported_exactly() {
    for steps in [0_u32, 1, 2, 7] {
        let plan = FramePlan::new(steps);
        let expected = 6 + 3 * steps as usize;
        assert_eq!(plan.size_hint(), (expected, Some(expected)));
        assert_eq!(plan.take(1000).count(), expected);
    }
}

#[test]
fn a_finished_plan_stays_finished_and_a_huge_count_does_not_overflow() {
    let mut done = FramePlan::new(0);
    for _ in 0..6 {
        assert!(done.next().is_some());
    }
    assert_eq!(done.next(), None);
    assert_eq!(done.next(), None);
    assert_eq!(done.size_hint(), (0, Some(0)));
    let big = FramePlan::new(u32::MAX);
    assert_eq!(big.size_hint().1, Some(6 + 3 * u32::MAX as usize));
}

#[test]
fn each_phase_has_a_distinct_name() {
    let names: Vec<&str> = Phase::ORDER.iter().map(|p| p.name()).collect();
    for (i, name) in names.iter().enumerate() {
        assert!(!name.is_empty());
        assert_eq!(names.iter().filter(|n| *n == name).count(), 1, "{i}");
    }
}

#[test]
fn a_plan_ends_after_its_last_phase_even_when_asked_for_more() {
    assert_eq!(FramePlan::new(0).take(100).count(), 6);
    assert_eq!(FramePlan::new(2).take(100).count(), 12);
}
