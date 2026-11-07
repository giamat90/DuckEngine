//! The sample's frame on the Null device (card 4-24): bodies become sprites, the debug drawing adds
//! outlines and contacts, the same seed gives the same sprites, and the controls steer the arena.
//! No window and no GPU needed.

use app_physics_sample::controls::keys;
use app_physics_sample::error::Error;
use app_physics_sample::session::Session;
use app_physics_sample::simulation::{MAX_BODIES, STEP_TICKS};
use app_sprite_sample::{GOLDEN_SHADER, SampleError};
use assets::dshd::Program;
use base::math::Vec2;
use platform::window::{Action, Event, WindowId};
use render::device::IRenderDevice;
use render::null::NullDevice;
use render::sprite::Sprite;

const SIZE: (u32, u32) = (800, 450);

fn session_with(device: &mut NullDevice, seed: u64) -> Session {
    let surface = device
        .create_surface(render::desc::SurfaceDesc {
            native_token: 1,
            width: SIZE.0,
            height: SIZE.1,
        })
        .unwrap_or_else(|_| unreachable!());
    let program = Program::parse(GOLDEN_SHADER).unwrap_or_else(|_| unreachable!());
    Session::new(device, &program, surface, SIZE, seed).unwrap_or_else(|_| unreachable!())
}

fn frames(session: &mut Session, device: &mut NullDevice, count: u32) {
    for _ in 0..count {
        let drawn = session.frame(device, STEP_TICKS);
        assert_eq!(drawn.as_ref().err(), None);
    }
}

fn window() -> WindowId {
    WindowId::from_raw(0, 0)
}

fn press(session: &mut Session, scancode: u32) {
    session.apply(&Event::Key {
        window: window(),
        scancode,
        action: Action::Press,
        mods: 0,
    });
}

/// FNV-1a over the bits of every field of every sprite.
fn hash_sprites(sprites: &[Sprite]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut write = |bytes: &[u8]| {
        for byte in bytes {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for s in sprites {
        for value in [
            s.position.x,
            s.position.y,
            s.size.x,
            s.size.y,
            s.angle,
            s.uv.min.x,
            s.uv.max.y,
        ] {
            write(&value.to_bits().to_le_bytes());
        }
        write(&s.tint);
        write(&s.layer.to_le_bytes());
        write(&s.texture.index().to_le_bytes());
    }
    hash
}

#[test]
fn without_the_debug_drawing_n_bodies_are_n_sprites_with_the_bodies_poses() {
    let mut device = NullDevice::new();
    let mut session = session_with(&mut device, 1);
    press(&mut session, keys::DEBUG);
    frames(&mut session, &mut device, 100);
    let world = session.simulation().world();
    let sprites = session.sprites();
    assert_eq!(sprites.len(), world.len());
    assert!(sprites.len() > 3);
    for ((_, body), sprite) in world.iter().zip(sprites) {
        let at = body.position();
        assert_eq!(sprite.position, Vec2::new(at.x * 20.0, at.y * 20.0));
    }
}

#[test]
fn the_debug_drawing_adds_sprites_and_the_key_takes_them_away() {
    let mut device = NullDevice::new();
    let mut session = session_with(&mut device, 1);
    frames(&mut session, &mut device, 100);
    let with = session.sprites().len();
    assert!(with > session.simulation().world().len() * 4, "{with}");
    press(&mut session, keys::DEBUG);
    frames(&mut session, &mut device, 1);
    assert_eq!(session.sprites().len(), session.simulation().world().len());
    assert_eq!(session.dropped(), 0);
}

#[test]
fn a_frame_submits_draws_to_the_device() {
    let mut device = NullDevice::new();
    let mut session = session_with(&mut device, 1);
    let stats = session.frame(&mut device, STEP_TICKS);
    assert_eq!(stats.as_ref().err(), None);
    // Several textures (white, checker, disc) mean several batches, one draw each.
    assert!(stats.map_or(0, |s| s.draws) >= 1);
}

#[test]
fn the_same_seed_gives_the_same_sprites_and_another_seed_other_ones() {
    let sprites_after = |seed| {
        let mut device = NullDevice::new();
        let mut session = session_with(&mut device, seed);
        frames(&mut session, &mut device, 150);
        hash_sprites(session.sprites())
    };
    assert_eq!(sprites_after(5), sprites_after(5));
    assert_ne!(sprites_after(5), sprites_after(6));
}

#[test]
fn a_click_adds_one_body_where_the_cursor_is() {
    let mut device = NullDevice::new();
    let mut plain = session_with(&mut device, 1);
    let mut clicked = session_with(&mut device, 1);
    clicked.apply(&Event::MouseMove {
        window: window(),
        x: 400.0,
        y: 100.0,
    });
    clicked.apply(&Event::MouseButton {
        window: window(),
        button: 0,
        action: Action::Press,
    });
    frames(&mut plain, &mut device, 1);
    frames(&mut clicked, &mut device, 1);
    assert_eq!(
        clicked.simulation().world().len(),
        plain.simulation().world().len() + 1
    );
    // Once only: the next frame adds just what the stream adds.
    frames(&mut plain, &mut device, 1);
    frames(&mut clicked, &mut device, 1);
    assert_eq!(
        clicked.simulation().world().len(),
        plain.simulation().world().len() + 1
    );
}

#[test]
fn the_reset_key_starts_the_arena_over() {
    let mut device = NullDevice::new();
    let mut session = session_with(&mut device, 1);
    frames(&mut session, &mut device, 1);
    let first = session.simulation().state_hash();
    frames(&mut session, &mut device, 100);
    assert_ne!(session.simulation().state_hash(), first);
    press(&mut session, keys::RESET);
    frames(&mut session, &mut device, 1);
    assert_eq!(session.simulation().state_hash(), first);
    assert_eq!(session.simulation().steps(), 1);
}

#[test]
fn escape_asks_to_quit() {
    let mut device = NullDevice::new();
    let mut session = session_with(&mut device, 1);
    assert!(!session.quit());
    press(&mut session, keys::ESCAPE);
    assert!(session.quit());
}

#[test]
fn a_resize_follows_and_a_zero_side_is_refused_without_a_change() {
    let mut device = NullDevice::new();
    let mut session = session_with(&mut device, 1);
    assert_eq!(session.resize(&mut device, (400, 200)), Ok(()));
    assert_eq!(session.size(), (400, 200));
    frames(&mut session, &mut device, 1);
    assert_eq!(
        session.resize(&mut device, (0, 200)),
        Err(Error::Render(SampleError::NoCamera))
    );
    assert_eq!(session.size(), (400, 200));
}

#[test]
fn a_crowded_arena_still_fits_the_frame() {
    let mut device = NullDevice::new();
    let mut session = session_with(&mut device, 3);
    // Click a body in every frame until the world is full, with the debug drawing on.
    session.apply(&Event::MouseMove {
        window: window(),
        x: 400.0,
        y: 225.0,
    });
    for _ in 0..MAX_BODIES {
        session.apply(&Event::MouseButton {
            window: window(),
            button: 0,
            action: Action::Press,
        });
        frames(&mut session, &mut device, 1);
    }
    frames(&mut session, &mut device, 120);
    assert_eq!(session.simulation().world().len(), MAX_BODIES as usize);
    assert_eq!(session.dropped(), 0);
}
