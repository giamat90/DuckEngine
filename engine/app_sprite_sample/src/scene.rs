//! What the sample draws, as plain data and pure functions: which scene a frame shows, the pixels
//! of the generated textures, the triangle and the sprites of a frame. No device is touched here.

use base::math::Vec2;
use render::ids::TextureId;
use render::sprite::{Sprite, SpriteVertex, UvRect};

/// Side of the generated checker texture, in texels.
pub const CHECKER_SIDE: u32 = 8;
/// The one texel of the white texture the triangle and the tinted sprites use.
pub const WHITE_TEXEL: [u8; 4] = [255, 255, 255, 255];
/// Sprites in a sprite frame.
pub const SPRITE_COUNT: usize = 4;

/// What a frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// One coloured triangle.
    Triangle,
    /// The sprites.
    Sprites,
}

/// The scene of frame `frame`: the triangle for the first `triangle_frames` frames, then sprites.
#[must_use]
pub fn scene_for(frame: u32, triangle_frames: u32) -> Scene {
    if frame < triangle_frames {
        Scene::Triangle
    } else {
        Scene::Sprites
    }
}

/// The two texture handles the sprites refer to.
#[derive(Clone, Copy, Debug)]
pub struct SceneTextures {
    /// The 1 by 1 white texture.
    pub white: TextureId,
    /// The generated checker texture.
    pub checker: TextureId,
}

/// RGBA8 pixels of the checker texture: [`CHECKER_SIDE`] squared, alternating light and dark
/// texels, the first (top left) one light.
#[must_use]
pub fn checker_pixels() -> Vec<u8> {
    const LIGHT: [u8; 4] = [240, 200, 60, 255];
    const DARK: [u8; 4] = [60, 40, 20, 255];
    (0..CHECKER_SIDE)
        .flat_map(|row| (0..CHECKER_SIDE).map(move |col| (row + col) % 2 == 0))
        .flat_map(|light| if light { LIGHT } else { DARK })
        .collect()
}

/// The triangle, in pixels around the middle of the view, one primary colour per corner.
#[must_use]
pub fn triangle_vertices() -> [SpriteVertex; 3] {
    let corner = |x: f32, y: f32, color: [u8; 4]| SpriteVertex {
        position: [x, y],
        uv: [0.5, 0.5],
        color,
    };
    [
        corner(0.0, 120.0, [255, 0, 0, 255]),
        corner(-120.0, -90.0, [0, 255, 0, 255]),
        corner(120.0, -90.0, [0, 0, 255, 255]),
    ]
}

const FULL_TURN: u32 = 360;
const ORBIT_RADIUS: f32 = 160.0;
const ORBIT_SIZE: f32 = 48.0;
const CHECKER_SIZE: f32 = 128.0;
const TINTS: [[u8; 4]; 3] = [[255, 60, 60, 200], [60, 255, 60, 200], [60, 60, 255, 200]];

/// Fills `out` with the sprites of frame `frame`: the checker turning in the middle and three
/// tinted squares orbiting it. One degree per frame; the same frame gives the same sprites.
pub fn fill_sprites(frame: u32, textures: &SceneTextures, out: &mut Vec<Sprite>) {
    out.clear();
    let degrees = f32::from(u16::try_from(frame % FULL_TURN).unwrap_or(0));
    let turn = degrees.to_radians();
    out.push(Sprite {
        texture: textures.checker,
        position: Vec2::new(0.0, 0.0),
        size: Vec2::new(CHECKER_SIZE, CHECKER_SIZE),
        angle: turn,
        uv: UvRect::FULL,
        tint: [255, 255, 255, 255],
        layer: 1,
    });
    for (index, tint) in (0u8..).zip(TINTS) {
        let phase = turn * 2.0 + f32::from(index) * std::f32::consts::TAU / 3.0;
        out.push(Sprite {
            texture: textures.white,
            position: Vec2::new(phase.cos() * ORBIT_RADIUS, phase.sin() * ORBIT_RADIUS),
            size: Vec2::new(ORBIT_SIZE, ORBIT_SIZE),
            angle: -turn,
            uv: UvRect::FULL,
            tint,
            layer: 2,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render::desc::{TextureDesc, TextureFormat};
    use render::device::IRenderDevice;
    use render::null::NullDevice;

    fn textures() -> SceneTextures {
        let mut device = NullDevice::new();
        let mut make = || {
            device
                .create_texture(TextureDesc {
                    width: 1,
                    height: 1,
                    mip_count: 1,
                    format: TextureFormat::Rgba8,
                })
                .unwrap_or_else(|_| unreachable!())
        };
        SceneTextures {
            white: make(),
            checker: make(),
        }
    }

    #[test]
    fn the_triangle_comes_first_then_the_sprites() {
        assert_eq!(scene_for(0, 2), Scene::Triangle);
        assert_eq!(scene_for(1, 2), Scene::Triangle);
        assert_eq!(scene_for(2, 2), Scene::Sprites);
        assert_eq!(scene_for(0, 0), Scene::Sprites);
    }

    #[test]
    fn the_checker_alternates_and_starts_light() {
        let pixels = checker_pixels();
        assert_eq!(pixels.len(), (CHECKER_SIDE * CHECKER_SIDE * 4) as usize);
        let texel =
            |col: usize, row: usize| &pixels[(row * CHECKER_SIDE as usize + col) * 4..][..4];
        assert_eq!(texel(0, 0), [240, 200, 60, 255]);
        assert_eq!(texel(1, 0), [60, 40, 20, 255]);
        assert_eq!(texel(0, 1), [60, 40, 20, 255]);
        assert_eq!(texel(1, 1), [240, 200, 60, 255]);
        assert_eq!(texel(7, 7), [240, 200, 60, 255]);
    }

    #[test]
    fn the_triangle_has_one_primary_colour_per_corner() {
        let triangle = triangle_vertices();
        let positions: Vec<_> = triangle.iter().map(|v| v.position).collect();
        assert_eq!(positions, [[0.0, 120.0], [-120.0, -90.0], [120.0, -90.0]]);
        let colors: Vec<_> = triangle.iter().map(|v| v.color).collect();
        assert_eq!(
            colors,
            [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        );
        // Counter-clockwise, Y up: the signed area is positive.
        let [a, b, c] = triangle.map(|v| v.position);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        assert!(area > 0.0, "{area}");
    }

    #[test]
    fn a_frame_has_the_checker_below_three_orbiting_squares() {
        let textures = textures();
        let mut sprites = Vec::new();
        fill_sprites(0, &textures, &mut sprites);
        assert_eq!(sprites.len(), SPRITE_COUNT);
        assert_eq!(sprites[0].texture, textures.checker);
        assert_eq!(sprites[0].layer, 1);
        assert!(
            sprites[1..]
                .iter()
                .all(|s| s.texture == textures.white && s.layer == 2)
        );
        // Frame 0: the first square starts on the positive X axis, at the orbit radius.
        assert!((sprites[1].position.x - ORBIT_RADIUS).abs() < 1e-3);
        assert!(sprites[1].position.y.abs() < 1e-3);
    }

    #[test]
    fn the_squares_stay_on_the_orbit_and_the_sprites_move_with_the_frame() {
        let textures = textures();
        let (mut first, mut later) = (Vec::new(), Vec::new());
        fill_sprites(0, &textures, &mut first);
        fill_sprites(45, &textures, &mut later);
        for sprite in &later[1..] {
            let radius = sprite.position.x.hypot(sprite.position.y);
            assert!((radius - ORBIT_RADIUS).abs() < 1e-2, "{radius}");
        }
        assert!((first[0].angle - 0.0).abs() < 1e-6);
        assert!((later[0].angle - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
        assert_ne!(first[1].position, later[1].position);
    }

    #[test]
    fn the_squares_are_a_third_of_a_turn_apart_and_orbit_twice_as_fast_as_the_checker_turns() {
        let textures = textures();
        let mut sprites = Vec::new();
        fill_sprites(0, &textures, &mut sprites);
        let at = |s: &[Sprite], i: usize| (s[i].position.x, s[i].position.y);
        let half_height = ORBIT_RADIUS * 3.0f32.sqrt() / 2.0;
        let near =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-2 && (a.1 - b.1).abs() < 1e-2;
        assert!(
            near(at(&sprites, 2), (-ORBIT_RADIUS / 2.0, half_height)),
            "{:?}",
            at(&sprites, 2)
        );
        assert!(
            near(at(&sprites, 3), (-ORBIT_RADIUS / 2.0, -half_height)),
            "{:?}",
            at(&sprites, 3)
        );
        // After 45 degrees the checker has turned 45 and the first square 90.
        fill_sprites(45, &textures, &mut sprites);
        assert!(
            near(at(&sprites, 1), (0.0, ORBIT_RADIUS)),
            "{:?}",
            at(&sprites, 1)
        );
        assert!(
            sprites[1..]
                .iter()
                .all(|s| (s.angle + std::f32::consts::FRAC_PI_4).abs() < 1e-5)
        );
    }

    #[test]
    fn the_animation_repeats_after_a_full_turn_and_refilling_replaces_the_sprites() {
        let textures = textures();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        fill_sprites(7, &textures, &mut a);
        fill_sprites(7 + FULL_TURN, &textures, &mut b);
        assert_eq!(a, b);
        fill_sprites(8, &textures, &mut a);
        assert_eq!(a.len(), SPRITE_COUNT);
    }
}
