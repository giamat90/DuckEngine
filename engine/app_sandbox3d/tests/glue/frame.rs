//! The frame: phases in order, the box, the stream, drops, reset, camera and resize.

use crate::common::{Failure, Recorder, SCENERY, STEP_TICKS, VIEW, run, sandbox, step, texture};
use app_sandbox3d::controls::{Piece, Requests};
use app_sandbox3d::mesh::{BOX, CAPSULE, SPHERE};
use app_sandbox3d::sandbox::{MAX_PIECES, STREAM_LIMIT, Sandbox};
use app_sandbox3d::spawn::ARENA_HALF;
use base::math::Vec2;

#[test]
fn a_new_sandbox_has_a_box_and_no_pieces() -> Result<(), Failure> {
    let sandbox = sandbox(24)?;
    assert_eq!((sandbox.pieces(), sandbox.steps()), (0, 0));
    Ok(())
}

#[test]
fn a_view_with_a_zero_side_is_refused() {
    assert!(Sandbox::new(texture(), 24, (0, VIEW.1)).is_err());
    assert!(Sandbox::new(texture(), 24, (VIEW.0, 0)).is_err());
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
fn the_first_step_drops_the_first_piece_of_the_stream() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let report = step(&mut sandbox, Requests::default(), &mut draw)?;
    assert_eq!((report.spawned, sandbox.pieces()), (1, 1));
    assert_eq!(
        (report.extracted as usize, draw.instances.len()),
        (SCENERY + 1, SCENERY + 1)
    );
    Ok(())
}

#[test]
fn the_pieces_fall_and_stay_inside_the_box() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    step(&mut sandbox, Requests::default(), &mut draw)?;
    let start = draw.pieces()[0][1];
    run(&mut sandbox, 40, &mut draw)?;
    assert!(draw.pieces()[0][1] < start, "it fell");
    run(&mut sandbox, 900, &mut draw)?;
    for [x, y, z] in draw.pieces() {
        assert!(x.abs() < ARENA_HALF && z.abs() < ARENA_HALF, "{x} {z}");
        assert!(y > 0.0, "above the floor: {y}");
    }
    Ok(())
}

#[test]
fn the_stream_stops_at_its_limit() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    run(&mut sandbox, 2000, &mut draw)?;
    assert_eq!(sandbox.pieces(), STREAM_LIMIT as usize);
    Ok(())
}

#[test]
fn a_drop_makes_the_chosen_piece() -> Result<(), Failure> {
    for (piece, mesh) in [
        (Piece::Sphere, SPHERE),
        (Piece::Box, BOX),
        (Piece::Capsule, CAPSULE),
    ] {
        let mut sandbox = sandbox(24)?;
        let mut draw = Recorder::default();
        let requests = Requests {
            drop: Some(piece),
            ..Requests::default()
        };
        // Frame zero runs no step, so the stream has not dropped anything of its own.
        sandbox.frame(requests, 0, &mut draw)?;
        assert_eq!(sandbox.pieces(), 1);
        assert_eq!(draw.instances[SCENERY].mesh.0, mesh, "{piece:?}");
    }
    Ok(())
}

#[test]
fn the_box_takes_no_more_pieces_than_it_holds() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let requests = Requests {
        drop: Some(Piece::Sphere),
        ..Requests::default()
    };
    for _ in 0..MAX_PIECES + 20 {
        sandbox.frame(requests, 0, &mut draw)?;
    }
    assert_eq!(sandbox.pieces(), MAX_PIECES);
    assert_eq!(draw.instances.len(), SCENERY + MAX_PIECES);
    Ok(())
}

#[test]
fn a_reset_starts_again_from_the_seed() -> Result<(), Failure> {
    let mut fresh = sandbox(24)?;
    let mut draw = Recorder::default();
    run(&mut fresh, 100, &mut draw)?;
    let mut sandbox = sandbox(24)?;
    run(&mut sandbox, 250, &mut draw)?;
    let reset = Requests {
        reset: true,
        ..Requests::default()
    };
    step(&mut sandbox, reset, &mut draw)?;
    run(&mut sandbox, 99, &mut draw)?;
    assert_eq!(sandbox.steps(), 100);
    assert_eq!(sandbox.state_hash(), fresh.state_hash());
    Ok(())
}

#[test]
fn a_drag_and_the_wheel_move_the_camera_the_scene_is_drawn_with() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let (yaw, distance) = (sandbox.orbit().yaw(), sandbox.orbit().distance());
    let requests = Requests {
        orbit: Vec2::new(50.0, 0.0),
        zoom: 1.0,
        ..Requests::default()
    };
    step(&mut sandbox, requests, &mut draw)?;
    assert!(sandbox.orbit().yaw() < yaw);
    assert!(sandbox.orbit().distance() < distance);
    let seen = sandbox.scene().camera().position;
    assert_eq!(seen, sandbox.orbit().position());
    Ok(())
}

#[test]
fn a_resize_changes_the_aspect_and_a_zero_side_is_refused() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    sandbox.resize((1000, 500))?;
    step(&mut sandbox, Requests::default(), &mut draw)?;
    assert!((draw.aspect - 2.0).abs() < 1e-6);
    assert!(sandbox.resize((0, 500)).is_err());
    step(&mut sandbox, Requests::default(), &mut draw)?;
    assert!((draw.aspect - 2.0).abs() < 1e-6, "nothing changed");
    Ok(())
}

#[test]
fn the_light_comes_from_above_and_the_pieces_have_their_colours_and_sizes() -> Result<(), Failure> {
    let mut colours = Vec::new();
    for piece in [Piece::Sphere, Piece::Box, Piece::Capsule] {
        let mut sandbox = sandbox(24)?;
        let mut draw = Recorder::default();
        let light = sandbox.scene().light().direction;
        assert!(light.x < 0.0 && light.y < 0.0 && light.z < 0.0, "{light:?}");
        let requests = Requests {
            drop: Some(piece),
            ..Requests::default()
        };
        sandbox.frame(requests, 0, &mut draw)?;
        let made = draw.instances[SCENERY];
        assert!((made.material.colour[3] - 1.0).abs() < 1e-6);
        assert!(
            made.material.colour[..3]
                .iter()
                .any(|c| *c > 0.0 && *c < 1.0)
        );
        colours.push(made.material.colour);
        let m = made.model.to_cols_array();
        let size = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
        assert!(
            size > 0.25 && size < if piece == Piece::Capsule { 1.12 } else { 0.56 },
            "the entity scale reaches the model: {size}"
        );
    }
    assert!(
        colours[0]
            .iter()
            .zip(colours[1])
            .any(|(a, b)| (*a - b).abs() > 1e-3)
    );
    assert!(
        colours[1]
            .iter()
            .zip(colours[2])
            .any(|(a, b)| (*a - b).abs() > 1e-3)
    );
    assert!(
        colours[0]
            .iter()
            .zip(colours[2])
            .any(|(a, b)| (*a - b).abs() > 1e-3)
    );
    Ok(())
}
