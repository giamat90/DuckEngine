//! What the sandbox makes: the sound of an impact and the bodies that fall.

use crate::error::Error;
use app_physics_sample::mapping::PIXELS_PER_METRE;
use app_physics_sample::simulation::{CEILING, FLOOR_TOP, HALF_WIDTH, SPAWN_SPREAD};
use audio::sound::Sound;
use base::math::Vec2;
use base::rng::Pcg32;
use physics::d2::{BodyDesc, BodyKind, Rot2, Shape2};

/// Sample rate of the impact sound and of the audio engine.
pub const SOUND_RATE: u32 = 48_000;
/// Length of the impact sound, in frames.
pub const CLICK_FRAMES: u16 = 2400;
/// Pitch of the impact sound, in cycles per sample.
const CLICK_PITCH: f32 = 0.03;
/// Loudest sample of the impact sound.
const CLICK_LEVEL: f32 = 0.5;
/// Half the thickness of the floor and the walls, in metres.
const WALL_HALF: f32 = 0.5;
/// Half the height of a wall, in metres.
const WALL_HALF_HEIGHT: f32 = 12.0;
/// Distance a click keeps from the walls and the floor, in metres.
const MARGIN: f32 = 1.0;
/// The change of velocity at which an impact plays at full gain, metres per second.
const LOUD_SPEED: f32 = 12.0;
/// The quietest gain of an impact.
const QUIET_GAIN: f32 = 0.2;
/// Smallest half side of a box and smallest radius of a disc, in metres.
pub const MIN_HALF: f32 = 0.3;
/// Most a half side adds to [`MIN_HALF`], in metres.
pub const EXTRA_HALF: f32 = 0.3;
/// Most a radius adds to [`MIN_HALF`], in metres.
pub const EXTRA_RADIUS: f32 = 0.25;

/// The sound of an impact: a short decaying tone, mono at [`SOUND_RATE`].
///
/// # Errors
/// [`Error::Sound`] if the sound cannot be made.
pub fn impact_sound() -> Result<Sound, Error> {
    let samples = (0..CLICK_FRAMES)
        .map(|i| {
            let t = f32::from(i);
            let decay = 1.0 - t / f32::from(CLICK_FRAMES);
            (t * CLICK_PITCH * std::f32::consts::TAU).sin() * decay * decay * CLICK_LEVEL
        })
        .collect();
    Ok(Sound::new(SOUND_RATE, 1, samples)?)
}

/// A box or a disc at `at`, of a random size and tilt.
pub(crate) fn body_desc(rng: &mut Pcg32, at: Vec2) -> BodyDesc {
    let circle = rng.next_u32() % 2 == 1;
    let shape = if circle {
        Shape2::Circle {
            radius: MIN_HALF + EXTRA_RADIUS * rng.next_f32(),
        }
    } else {
        Shape2::Box {
            half_extents: Vec2::new(
                MIN_HALF + EXTRA_HALF * rng.next_f32(),
                MIN_HALF + EXTRA_HALF * rng.next_f32(),
            ),
        }
    };
    let mut desc = BodyDesc::new(BodyKind::Dynamic, at, shape);
    desc.rotation = Rot2::from_angle(rng.next_f32() * std::f32::consts::TAU);
    desc
}

/// The floor and the two walls as (centre, half size) in metres: the floor, then the left and the
/// right wall. They surround the inside of the arena, which is [`HALF_WIDTH`] wide on each side.
#[must_use]
pub fn scenery() -> [(Vec2, Vec2); 3] {
    let side = HALF_WIDTH + WALL_HALF;
    let wall = |x: f32| {
        (
            Vec2::new(x, FLOOR_TOP + WALL_HALF_HEIGHT),
            Vec2::new(WALL_HALF, WALL_HALF_HEIGHT),
        )
    };
    [
        (
            Vec2::new(0.0, FLOOR_TOP - WALL_HALF),
            Vec2::new(side + WALL_HALF, WALL_HALF),
        ),
        wall(-side),
        wall(side),
    ]
}

/// `at` moved inside the walls, the floor and the ceiling, by the margin a click keeps.
#[must_use]
pub fn inside_arena(at: Vec2) -> Vec2 {
    Vec2::new(
        at.x.clamp(-(HALF_WIDTH - MARGIN), HALF_WIDTH - MARGIN),
        at.y.clamp(FLOOR_TOP + MARGIN, CEILING),
    )
}

/// A random x for the stream, within [`SPAWN_SPREAD`] of the middle.
pub(crate) fn stream_x(rng: &mut Pcg32) -> f32 {
    (rng.next_f32() - 0.5) * 2.0 * SPAWN_SPREAD
}

/// The size of the sprite of a body of `shape` in metres, and whether it is a disc; `None` for a
/// shape the sandbox does not draw.
pub(crate) fn sprite_size(shape: &Shape2) -> Option<(Vec2, bool)> {
    match shape {
        Shape2::Circle { radius } => Some((Vec2::new(*radius, *radius) * 2.0, true)),
        Shape2::Box { half_extents } => Some((*half_extents * 2.0, false)),
        _ => None,
    }
}

/// The gain of an impact that changed the velocity of a body by `jolt` metres per second: from the
/// quietest gain up to one at [`LOUD_SPEED`].
pub(crate) fn impact_gain(jolt: f32) -> f32 {
    (jolt / LOUD_SPEED).clamp(QUIET_GAIN, 1.0)
}

/// Half the width of the part of the world a view of `view_width` pixels shows, in metres: the
/// range of the pan.
pub(crate) fn listener_half_width(view_width: u32) -> f32 {
    f32::from(u16::try_from(view_width).unwrap_or(u16::MAX)) / PIXELS_PER_METRE / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn near(a: Vec2, b: Vec2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn the_scenery_surrounds_the_inside_of_the_arena() {
        let [floor, left, right] = scenery();
        // The top of the floor is at FLOOR_TOP, the inner faces of the walls at +-HALF_WIDTH.
        assert!(close(floor.0.y + floor.1.y, FLOOR_TOP));
        assert!(close(floor.0.x, 0.0));
        assert!(close(left.0.x + left.1.x, -HALF_WIDTH));
        assert!(close(right.0.x - right.1.x, HALF_WIDTH));
        // The floor reaches the outer faces of both walls.
        assert!(close(floor.0.x - floor.1.x, left.0.x - left.1.x));
        assert!(close(floor.0.x + floor.1.x, right.0.x + right.1.x));
        // The walls stand on the floor and are as high as the world needs.
        assert!(close(left.0.y - left.1.y, FLOOR_TOP));
        assert!(near(right.0, Vec2::new(-left.0.x, left.0.y)));
        assert!(near(left.1, right.1));
        assert!(close(left.1.x, 0.5) && close(left.1.y, 12.0));
        assert!(close(floor.1.y, 0.5));
    }

    #[test]
    fn a_click_is_moved_inside_by_the_margin() {
        let inside = |x, y| inside_arena(Vec2::new(x, y));
        assert!(near(inside(0.0, 0.0), Vec2::new(0.0, 0.0)));
        assert!(near(
            inside(-100.0, 100.0),
            Vec2::new(-(HALF_WIDTH - 1.0), CEILING)
        ));
        assert!(near(
            inside(100.0, -100.0),
            Vec2::new(HALF_WIDTH - 1.0, FLOOR_TOP + 1.0)
        ));
        assert!(near(inside(5.0, 3.0), Vec2::new(5.0, 3.0)));
    }

    #[test]
    fn the_stream_x_covers_both_sides_within_the_spread() {
        let mut rng = Pcg32::new(1, 24);
        let xs: Vec<f32> = (0..200).map(|_| stream_x(&mut rng)).collect();
        assert!(xs.iter().all(|x| x.abs() <= SPAWN_SPREAD));
        assert!(xs.iter().any(|x| *x < -SPAWN_SPREAD / 2.0));
        assert!(xs.iter().any(|x| *x > SPAWN_SPREAD / 2.0));
        let mean = xs.iter().sum::<f32>() / 200.0;
        assert!(mean.abs() < SPAWN_SPREAD / 4.0, "{mean}");
    }

    #[test]
    fn a_sprite_is_as_big_as_its_shape() {
        let disc = sprite_size(&Shape2::Circle { radius: 0.4 });
        assert_eq!(disc, Some((Vec2::new(0.8, 0.8), true)));
        let half_extents = Vec2::new(0.3, 0.5);
        let boxed = sprite_size(&Shape2::Box { half_extents });
        assert_eq!(boxed, Some((Vec2::new(0.6, 1.0), false)));
    }

    #[test]
    fn the_gain_grows_with_the_hit_between_the_quietest_and_one() {
        assert!(close(impact_gain(0.0), 0.2));
        assert!(close(impact_gain(6.0), 0.5));
        assert!(close(impact_gain(12.0), 1.0));
        assert!(close(impact_gain(500.0), 1.0));
    }

    #[test]
    fn the_listener_hears_half_the_width_of_the_view() {
        assert!(close(listener_half_width(800), 20.0));
        assert!(close(listener_half_width(1600), 40.0));
        assert!(close(
            listener_half_width(u32::MAX),
            f32::from(u16::MAX) / 40.0
        ));
    }

    #[test]
    fn the_impact_sound_is_a_decaying_mono_tone() -> Result<(), Error> {
        let sound = impact_sound()?;
        assert_eq!(sound.sample_rate(), SOUND_RATE);
        assert_eq!(sound.channels(), 1);
        assert_eq!(sound.frame_count(), usize::from(CLICK_FRAMES));
        let samples = sound.samples();
        assert!(close(samples[0], 0.0));
        let peak = |part: &[f32]| part.iter().fold(0.0_f32, |p, s| p.max(s.abs()));
        let quarter = samples.len() / 4;
        let (first, last) = (peak(&samples[..quarter]), peak(&samples[quarter * 3..]));
        assert!(first > 0.3 && first <= CLICK_LEVEL, "{first}");
        assert!(last < first / 10.0, "{last} against {first}");
        Ok(())
    }

    /// The side of a half extent or a radius a body may have: (disc, in range).
    fn size_in_range(shape: &Shape2) -> (bool, bool) {
        match shape {
            Shape2::Circle { radius } => {
                (true, (MIN_HALF..=MIN_HALF + EXTRA_RADIUS).contains(radius))
            }
            Shape2::Box { half_extents: h } => (
                false,
                [h.x, h.y]
                    .iter()
                    .all(|side| (MIN_HALF..=MIN_HALF + EXTRA_HALF).contains(side)),
            ),
            _ => (false, false),
        }
    }

    #[test]
    fn the_impact_sound_follows_its_formula() -> Result<(), Error> {
        let sound = impact_sound()?;
        let samples = sound.samples();
        for i in [1_u16, 17, 400, 1200, 2399] {
            let t = f32::from(i);
            let decay = 1.0 - t / f32::from(CLICK_FRAMES);
            let want =
                (t * CLICK_PITCH * std::f32::consts::TAU).sin() * decay * decay * CLICK_LEVEL;
            assert!(close(samples[usize::from(i)], want), "{i}");
        }
        Ok(())
    }

    #[test]
    fn a_body_is_dynamic_where_it_was_asked_and_of_a_size_in_range() {
        let mut rng = Pcg32::new(7, 24);
        let at = Vec2::new(1.5, -2.5);
        let descs: Vec<BodyDesc> = (0..200).map(|_| body_desc(&mut rng, at)).collect();
        assert!(descs.iter().all(|d| d.kind == BodyKind::Dynamic));
        assert!(descs.iter().all(|d| d.position == at));
        let sizes: Vec<(bool, bool)> = descs.iter().map(|d| size_in_range(&d.shape)).collect();
        assert!(sizes.iter().all(|(_, in_range)| *in_range));
        let round = sizes.iter().filter(|(disc, _)| *disc).count();
        assert!((50..150).contains(&round), "{round} discs of 200");
    }

    #[test]
    fn a_body_is_tilted_anywhere_in_a_turn() {
        let mut rng = Pcg32::new(7, 24);
        let angles: Vec<f32> = (0..200)
            .map(|_| body_desc(&mut rng, Vec2::ZERO).rotation.angle())
            .collect();
        assert!(angles.iter().any(|a| *a > 1.5));
        assert!(angles.iter().any(|a| *a < -1.5));
        assert!(
            angles
                .iter()
                .all(|a| a.abs() <= std::f32::consts::PI + 1e-3)
        );
    }
}
