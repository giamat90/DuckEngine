//! The frame: phases in order, the arena, the stream, clicks and reset.

use crate::common::{Failure, Recorder, VIEW, run, sandbox, step, textures};
use app_physics_sample::simulation::{
    CEILING, FLOOR_TOP, HALF_WIDTH, SPAWN_HEIGHT, SPAWN_SPREAD, STEP_TICKS,
};
use app_sandbox::spawn::scenery;
use app_sandbox::{Error, Requests, Sandbox};
use base::math::Vec2;

#[test]
fn a_new_sandbox_has_an_arena_and_no_bodies() -> Result<(), Failure> {
    let sandbox = sandbox(24)?;
    assert_eq!(sandbox.bodies(), 0);
    assert_eq!(sandbox.steps(), 0);
    Ok(())
}

#[test]
fn a_view_with_a_zero_side_is_refused() {
    assert!(Sandbox::new(textures(), 24, (0, VIEW.1)).is_err());
    assert!(Sandbox::new(textures(), 24, (VIEW.0, 0)).is_err());
}

#[test]
fn a_frame_runs_the_steps_its_time_holds_and_draws_once() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let none = sandbox.frame(Requests::default(), 0, &mut draw)?;
    assert_eq!((none.steps, sandbox.steps(), draw.draws), (0, 0, 1));
    let one = step(&mut sandbox, Requests::default(), &mut draw)?;
    assert_eq!((one.steps, sandbox.steps(), draw.draws), (1, 1, 2));
    let two = sandbox.frame(Requests::default(), STEP_TICKS * 2, &mut draw)?;
    assert_eq!((two.steps, sandbox.steps(), draw.draws), (2, 3, 3));
    Ok(())
}

#[test]
fn the_first_step_drops_the_first_body_of_the_stream() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let report = step(&mut sandbox, Requests::default(), &mut draw)?;
    assert_eq!((report.spawned, sandbox.bodies()), (1, 1));
    // The floor and the two walls, then the body.
    assert_eq!((report.extracted, draw.sprites), (4, 4));
    Ok(())
}

#[test]
fn the_bodies_fall_and_the_sprites_follow_them() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    step(&mut sandbox, Requests::default(), &mut draw)?;
    let start = draw.lowest;
    run(&mut sandbox, 60, &mut draw)?;
    assert!(draw.lowest < start, "{} then {}", start, draw.lowest);
    assert!(draw.lowest > FLOOR_TOP - 1.0, "inside the arena");
    Ok(())
}

#[test]
fn a_click_drops_a_body_inside_the_arena() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    step(&mut sandbox, Requests::default(), &mut draw)?;
    let click = Requests {
        spawn: Some(Vec2::new(HALF_WIDTH * 5.0, CEILING * 5.0)),
        ..Requests::default()
    };
    let report = step(&mut sandbox, click, &mut draw)?;
    assert_eq!((report.spawned, sandbox.bodies()), (1, 2));
    run(&mut sandbox, 240, &mut draw)?;
    let inside = draw.lowest > FLOOR_TOP - 1.0;
    assert!(inside, "a click far outside is moved inside the walls");
    Ok(())
}

#[test]
fn a_full_world_refuses_a_body_without_an_error() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let click = Requests {
        spawn: Some(Vec2::new(0.0, SPAWN_HEIGHT)),
        ..Requests::default()
    };
    let mut last = 0;
    for _ in 0..200 {
        step(&mut sandbox, click, &mut draw)?;
        assert!(sandbox.bodies() >= last);
        last = sandbox.bodies();
    }
    assert!(last > 100 && last < 128, "{last}");
    Ok(())
}

#[test]
fn a_reset_starts_the_run_again_exactly() -> Result<(), Failure> {
    let mut draw = Recorder::default();
    let mut fresh = sandbox(24)?;
    run(&mut fresh, 40, &mut draw)?;
    let mut reset = sandbox(24)?;
    run(&mut reset, 100, &mut draw)?;
    let again = Requests {
        reset: true,
        ..Requests::default()
    };
    // The reset frame itself runs one step of the new arena, like frame zero of a fresh one.
    step(&mut reset, again, &mut draw)?;
    assert_eq!(reset.steps(), 1);
    run(&mut reset, 39, &mut draw)?;
    assert_eq!(reset.state_hash(), fresh.state_hash());
    Ok(())
}

#[test]
fn the_report_of_every_frame_adds_up() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let (mut spawned, mut impacts) = (0, 0);
    for _ in 0..400 {
        let report = step(&mut sandbox, Requests::default(), &mut draw)?;
        spawned += report.spawned;
        impacts += report.impacts;
        // Every impact of the frame is queued, and the queue is emptied in the same frame.
        assert_eq!(
            report.played + report.failed + report.refused,
            report.impacts
        );
        let bodies = u32::try_from(sandbox.bodies())?;
        assert_eq!(report.extracted, 3 + bodies);
    }
    assert_eq!(u32::try_from(sandbox.bodies())?, spawned);
    assert!(impacts > 20, "{impacts}");
    Ok(())
}

#[test]
fn the_arena_is_drawn_where_the_scenery_stands() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    step(&mut sandbox, Requests::default(), &mut draw)?;
    let mut drawn: Vec<(Vec2, Vec2)> = sandbox
        .scene()
        .sprites()
        .iter()
        .filter(|sprite| sprite.layer == 0)
        .map(|sprite| (sprite.position, sprite.size))
        .collect();
    drawn.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)));
    let mut expected: Vec<(Vec2, Vec2)> = scenery()
        .iter()
        .map(|(at, half)| (*at, *half * 2.0))
        .collect();
    expected.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)));
    assert_eq!(drawn, expected);
    Ok(())
}

fn newest_body(sandbox: &Sandbox) -> Option<Vec2> {
    sandbox
        .scene()
        .sprites()
        .iter()
        .filter(|sprite| sprite.layer == 1)
        .map(|sprite| sprite.position)
        .next_back()
}

#[test]
fn a_click_far_outside_lands_at_the_nearest_inside_point() -> Result<(), Failure> {
    let mut draw = Recorder::default();
    for (click, expected) in [
        (
            Vec2::new(-100.0, 100.0),
            Vec2::new(-(HALF_WIDTH - 1.0), CEILING),
        ),
        (
            Vec2::new(100.0, -100.0),
            Vec2::new(HALF_WIDTH - 1.0, FLOOR_TOP + 1.0),
        ),
        (Vec2::new(3.0, 4.0), Vec2::new(3.0, 4.0)),
    ] {
        let mut sandbox = sandbox(24)?;
        step(&mut sandbox, Requests::default(), &mut draw)?;
        let request = Requests {
            spawn: Some(click),
            ..Requests::default()
        };
        step(&mut sandbox, request, &mut draw)?;
        let at = newest_body(&sandbox).ok_or("no body")?;
        // One step of motion is a few millimetres; the body may have been pushed by a contact.
        assert!(
            (at.x - expected.x).abs() < 0.1 && (at.y - expected.y).abs() < 0.1,
            "{at:?} for {click:?}"
        );
    }
    Ok(())
}

#[test]
fn the_stream_drops_bodies_on_both_sides_of_the_middle() -> Result<(), Failure> {
    let mut draw = Recorder::default();
    let mut xs = Vec::new();
    for seed in 0..30 {
        let mut sandbox = sandbox(seed)?;
        step(&mut sandbox, Requests::default(), &mut draw)?;
        xs.push(newest_body(&sandbox).ok_or("no body")?.x);
    }
    assert!(xs.iter().all(|x| x.abs() <= SPAWN_SPREAD + 0.5));
    assert!(xs.iter().any(|x| *x < -SPAWN_SPREAD / 2.0));
    assert!(xs.iter().any(|x| *x > SPAWN_SPREAD / 2.0));
    Ok(())
}

#[test]
fn a_resize_moves_the_camera_and_a_zero_side_is_refused() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let before = *sandbox.scene().camera();
    assert!(matches!(sandbox.resize((0, 450)), Err(Error::NoView)));
    assert!(matches!(sandbox.resize((800, 0)), Err(Error::NoView)));
    assert_eq!(*sandbox.scene().camera(), before);
    sandbox.resize((1600, 900))?;
    assert!((sandbox.scene().camera().aspect - 1600.0 / 900.0).abs() < 1e-4);
    Ok(())
}

#[test]
fn an_error_says_what_failed() {
    assert_eq!(Error::NoView.to_string(), "the view has a zero side");
    assert_eq!(Error::Render("x".to_owned()).to_string(), "render: x");
    assert!(
        Error::Ecs(ecs::EcsError::StaleEntity)
            .to_string()
            .starts_with("ecs: ")
    );
}

#[test]
fn the_stream_stops_at_its_limit() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    run(&mut sandbox, 1300, &mut draw)?;
    assert_eq!(sandbox.bodies(), 80);
    run(&mut sandbox, 100, &mut draw)?;
    assert_eq!(sandbox.bodies(), 80);
    Ok(())
}

#[test]
fn a_crowd_of_bodies_never_loses_a_cue_without_counting_it() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let drop = Requests {
        spawn: Some(Vec2::new(0.0, SPAWN_HEIGHT)),
        ..Requests::default()
    };
    let mut refused = 0;
    for _ in 0..400 {
        let report = step(&mut sandbox, drop, &mut draw)?;
        refused += report.refused;
        assert_eq!(
            report.played + report.failed + report.refused,
            report.impacts
        );
    }
    // 64 cues fit between two frames and the bodies do not all hit in one step.
    assert_eq!(refused, 0);
    Ok(())
}
