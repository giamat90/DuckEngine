//! World to screen, as pure functions: metres become pixels, the bodies become sprites, and
//! [`SpriteDebugDraw`] is the sample's implementation of the physics crate's `IPhysicsDebugDraw`
//! port, drawing outlines, centres and contacts as thin sprites. No device is touched here.

use base::math::Vec2;
use physics::d2::{Body, BodyKind, DebugColour, IPhysicsDebugDraw, Shape2, World};
use render::ids::TextureId;
use render::sprite::{Sprite, UvRect};

/// Pixels in a metre: the 800 by 450 window shows 40 by 22.5 metres.
pub const PIXELS_PER_METRE: f32 = 20.0;
/// Side of the generated disc texture, in texels.
pub const DISC_SIDE: u16 = 32;
/// Thickness of a debug line, in pixels.
pub const LINE_PIXELS: f32 = 2.0;
/// Side of a debug point, in pixels.
pub const POINT_PIXELS: f32 = 5.0;
/// Layer of the bodies; the debug sprites are above.
pub const BODY_LAYER: i16 = 1;
/// Layer of the debug lines.
pub const LINE_LAYER: i16 = 2;
/// Layer of the debug points.
pub const POINT_LAYER: i16 = 3;

/// The textures the sprites refer to.
#[derive(Clone, Copy, Debug)]
pub struct Textures {
    /// The 1 by 1 white texture: static bodies and the debug sprites.
    pub white: TextureId,
    /// The checker: dynamic boxes.
    pub checker: TextureId,
    /// The disc: circles.
    pub disc: TextureId,
}

/// The slate tint of static bodies (the walls and the floor).
const STATIC_FILL: [u8; 4] = [70, 90, 130, 255];

/// A point in metres as view pixels (origin in the middle, Y up).
#[must_use]
pub fn to_pixels(metres: Vec2) -> Vec2 {
    metres * PIXELS_PER_METRE
}

/// A point in view pixels as metres: the inverse of [`to_pixels`].
#[must_use]
pub fn to_world(pixels: Vec2) -> Vec2 {
    Vec2::new(pixels.x / PIXELS_PER_METRE, pixels.y / PIXELS_PER_METRE)
}

/// RGBA8 pixels of the disc texture: [`DISC_SIDE`] squared, opaque white inside the circle and
/// transparent outside.
#[must_use]
pub fn disc_pixels() -> Vec<u8> {
    let side = DISC_SIDE;
    let radius = f32::from(side) / 2.0;
    (0..side)
        .flat_map(|row| (0..side).map(move |col| (row, col)))
        .flat_map(|(row, col)| {
            let dx = f32::from(col) + 0.5 - radius;
            let dy = f32::from(row) + 0.5 - radius;
            let inside = dx * dx + dy * dy <= radius * radius;
            [255, 255, 255, if inside { 255 } else { 0 }]
        })
        .collect()
}

/// The tint of a body's sprite: static bodies are slate, sleeping ones are dimmed.
fn fill_tint(body: &Body) -> [u8; 4] {
    let circle = matches!(body.shape(), Shape2::Circle { .. });
    match (body.kind(), body.is_asleep(), circle) {
        (BodyKind::Static, _, _) => STATIC_FILL,
        (_, false, false) => [255, 255, 255, 255],
        (_, true, false) => [150, 150, 180, 255],
        (_, false, true) => [240, 140, 80, 255],
        (_, true, true) => [150, 110, 100, 255],
    }
}

/// The sprite of one body: a rectangle or a disc, in pixels, turned like the body; `None` for a
/// shape this sample does not know (`Shape2` can grow).
fn body_sprite(body: &Body, textures: &Textures) -> Option<Sprite> {
    let (texture, extent) = match body.shape() {
        Shape2::Box { half_extents } => (
            if body.kind() == BodyKind::Static {
                textures.white
            } else {
                textures.checker
            },
            half_extents,
        ),
        Shape2::Circle { radius } => (textures.disc, Vec2::new(radius, radius)),
        _ => return None,
    };
    Some(Sprite {
        texture,
        position: to_pixels(body.position()),
        size: to_pixels(extent * 2.0),
        angle: body.rotation().angle(),
        uv: UvRect::FULL,
        tint: fill_tint(body),
        layer: BODY_LAYER,
    })
}

/// Replaces the contents of `out` with one sprite per body, in slot order.
pub fn body_sprites(world: &World, textures: &Textures, out: &mut Vec<Sprite>) {
    out.clear();
    out.extend(
        world
            .iter()
            .filter_map(|(_, body)| body_sprite(body, textures)),
    );
}

/// The tint of a debug colour role.
#[must_use]
pub fn debug_tint(colour: DebugColour) -> [u8; 4] {
    match colour {
        DebugColour::Awake => [80, 230, 100, 255],
        DebugColour::Asleep => [90, 150, 255, 255],
        DebugColour::Static => [200, 200, 210, 255],
        DebugColour::ContactPoint => [255, 60, 60, 255],
        DebugColour::ContactNormal => [255, 170, 40, 255],
    }
}

/// Appends the debug sprites to `out` and never grows it past its capacity (a sprite that does not
/// fit is counted in [`SpriteDebugDraw::dropped`]).
pub struct SpriteDebugDraw<'a> {
    out: &'a mut Vec<Sprite>,
    white: TextureId,
    dropped: usize,
}

impl<'a> SpriteDebugDraw<'a> {
    /// A drawer that appends to `out`, with the `white` texture.
    pub fn new(out: &'a mut Vec<Sprite>, white: TextureId) -> Self {
        Self {
            out,
            white,
            dropped: 0,
        }
    }

    /// Sprites that did not fit.
    #[must_use]
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    fn push(&mut self, sprite: Sprite) {
        if self.out.len() < self.out.capacity() {
            self.out.push(sprite);
        } else {
            self.dropped += 1;
        }
    }
}

impl IPhysicsDebugDraw for SpriteDebugDraw<'_> {
    fn line(&mut self, a: Vec2, b: Vec2, colour: DebugColour) {
        let (from, to) = (to_pixels(a), to_pixels(b));
        let delta = to - from;
        let sprite = Sprite {
            texture: self.white,
            position: (from + to) * 0.5,
            size: Vec2::new(delta.length().max(LINE_PIXELS), LINE_PIXELS),
            // Presentation only: the std function is fine here, it is not simulation.
            angle: delta.y.atan2(delta.x),
            uv: UvRect::FULL,
            tint: debug_tint(colour),
            layer: LINE_LAYER,
        };
        self.push(sprite);
    }

    fn point(&mut self, p: Vec2, colour: DebugColour) {
        let sprite = Sprite {
            texture: self.white,
            position: to_pixels(p),
            size: Vec2::new(POINT_PIXELS, POINT_PIXELS),
            angle: 0.0,
            uv: UvRect::FULL,
            tint: debug_tint(colour),
            layer: POINT_LAYER,
        };
        self.push(sprite);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use physics::d2::{
        BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Rot2, Shape2,
        SolverConfig, WorldConfig,
    };
    use render::desc::{TextureDesc, TextureFormat};
    use render::device::IRenderDevice;
    use render::null::NullDevice;

    fn textures() -> Textures {
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
        Textures {
            white: make(),
            checker: make(),
            disc: make(),
        }
    }

    fn world() -> World {
        World::new(WorldConfig {
            max_bodies: 8,
            gravity: Vec2::ZERO,
            max_pairs: 16,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 16,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: physics::d2::SleepConfig::DEFAULT,
        })
        .unwrap_or_else(|_| unreachable!())
    }

    fn add(world: &mut World, desc: &BodyDesc) {
        assert!(world.create_body(desc).is_ok());
    }

    fn boxed(kind: BodyKind, at: Vec2, half: Vec2) -> BodyDesc {
        BodyDesc::new(kind, at, Shape2::Box { half_extents: half })
    }

    #[test]
    fn metres_and_pixels_convert_both_ways() {
        let p = to_pixels(Vec2::new(1.5, -2.0));
        assert_eq!(p, Vec2::new(30.0, -40.0));
        assert_eq!(to_world(p), Vec2::new(1.5, -2.0));
    }

    #[test]
    fn the_disc_is_opaque_in_the_middle_and_clear_in_the_corner() {
        let pixels = disc_pixels();
        let side = usize::from(DISC_SIDE);
        assert_eq!(pixels.len(), side * side * 4);
        let alpha = |x: usize, y: usize| pixels[(y * side + x) * 4 + 3];
        assert_eq!(alpha(side / 2, side / 2), 255);
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(side - 1, side - 1), 0);
    }

    #[test]
    fn the_disc_is_centred_in_its_square() {
        let pixels = disc_pixels();
        let side = usize::from(DISC_SIDE);
        let alpha = |x: usize, y: usize| pixels[(y * side + x) * 4 + 3];
        for i in 0..side * side {
            let (x, y) = (i % side, i / side);
            assert_eq!(alpha(x, y), alpha(side - 1 - x, y), "left-right at {x},{y}");
            assert_eq!(alpha(x, y), alpha(x, side - 1 - y), "top-bottom at {x},{y}");
            assert_eq!(alpha(x, y), alpha(y, x), "diagonal at {x},{y}");
        }
    }

    #[test]
    fn the_disc_has_the_area_and_the_shape_of_a_circle() {
        let pixels = disc_pixels();
        let side = usize::from(DISC_SIDE);
        let alpha = |x: usize, y: usize| pixels[(y * side + x) * 4 + 3];
        let inside = (0..side * side)
            .filter(|i| alpha(i % side, i / side) == 255)
            .count();
        // pi r^2 for r = 16 is about 804.
        assert!((780..=830).contains(&inside), "{inside}");
        // On the diagonal, well inside the rim and well outside it.
        assert_eq!(alpha(8, 8), 255);
        assert_eq!(alpha(2, 2), 0);
    }

    #[test]
    fn the_tint_tells_static_from_dynamic_discs_from_boxes_and_asleep_from_awake() {
        let mut w = world();
        // Far apart, so that nothing touches and nothing moves in zero gravity.
        add(
            &mut w,
            &boxed(BodyKind::Static, Vec2::new(0.0, -9.0), Vec2::new(5.0, 0.5)),
        );
        add(
            &mut w,
            &boxed(BodyKind::Dynamic, Vec2::new(-4.0, 0.0), Vec2::new(0.5, 0.5)),
        );
        add(
            &mut w,
            &BodyDesc::new(
                BodyKind::Dynamic,
                Vec2::new(4.0, 0.0),
                Shape2::Circle { radius: 0.5 },
            ),
        );
        let tints = |w: &World| -> Vec<[u8; 4]> { w.iter().map(|(_, b)| fill_tint(b)).collect() };
        let awake = tints(&w);
        assert_eq!(awake[0], STATIC_FILL);
        assert_eq!(awake[1], [255, 255, 255, 255]);
        assert_eq!(awake[2], [240, 140, 80, 255]);
        for _ in 0..300 {
            assert!(w.step(1.0 / 60.0).is_ok());
        }
        let asleep = tints(&w);
        assert!(w.iter().skip(1).all(|(_, b)| b.is_asleep()));
        assert_eq!(asleep[0], STATIC_FILL);
        assert_eq!(asleep[1], [150, 150, 180, 255]);
        assert_eq!(asleep[2], [150, 110, 100, 255]);
    }

    #[test]
    fn every_body_is_one_sprite_with_its_pose_in_pixels() {
        let t = textures();
        let mut w = world();
        let mut tilted = boxed(BodyKind::Dynamic, Vec2::new(1.0, 2.0), Vec2::new(0.5, 0.25));
        tilted.rotation = Rot2::from_angle(0.5);
        add(&mut w, &tilted);
        add(
            &mut w,
            &BodyDesc::new(
                BodyKind::Dynamic,
                Vec2::new(-3.0, 0.0),
                Shape2::Circle { radius: 0.5 },
            ),
        );
        add(
            &mut w,
            &boxed(BodyKind::Static, Vec2::new(0.0, -5.0), Vec2::new(10.0, 1.0)),
        );
        let mut sprites = Vec::new();
        body_sprites(&w, &t, &mut sprites);
        assert_eq!(sprites.len(), 3);
        let [boxy, disc, ground] = [sprites[0], sprites[1], sprites[2]];
        assert_eq!(boxy.texture, t.checker);
        assert_eq!(boxy.position, Vec2::new(20.0, 40.0));
        assert_eq!(boxy.size, Vec2::new(20.0, 10.0));
        assert!((boxy.angle - 0.5).abs() < 1.0e-4);
        assert_eq!(disc.texture, t.disc);
        assert_eq!(disc.size, Vec2::new(20.0, 20.0));
        assert_eq!(ground.texture, t.white);
        assert_eq!(ground.size, Vec2::new(400.0, 40.0));
        assert!(sprites.iter().all(|s| s.layer == BODY_LAYER));
    }

    #[test]
    fn the_sprite_list_is_replaced_not_appended_to() {
        let t = textures();
        let mut w = world();
        add(
            &mut w,
            &boxed(BodyKind::Dynamic, Vec2::ZERO, Vec2::new(1.0, 1.0)),
        );
        let mut sprites = Vec::new();
        body_sprites(&w, &t, &mut sprites);
        body_sprites(&w, &t, &mut sprites);
        assert_eq!(sprites.len(), 1);
    }

    #[test]
    fn a_line_is_a_thin_sprite_between_its_ends_in_pixels() {
        let t = textures();
        let mut out = Vec::with_capacity(4);
        let mut draw = SpriteDebugDraw::new(&mut out, t.white);
        // From (1, 1) m to (1, 3) m: a vertical segment 40 px long centred at (20, 40).
        draw.line(
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 3.0),
            DebugColour::ContactNormal,
        );
        assert_eq!(draw.dropped(), 0);
        let line = out[0];
        assert_eq!(line.texture, t.white);
        assert_eq!(line.position, Vec2::new(20.0, 40.0));
        assert_eq!(line.size, Vec2::new(40.0, LINE_PIXELS));
        assert!((line.angle - std::f32::consts::FRAC_PI_2).abs() < 1.0e-5);
        assert_eq!(line.tint, debug_tint(DebugColour::ContactNormal));
        assert_eq!(line.layer, LINE_LAYER);
    }

    #[test]
    fn a_point_is_a_small_square_on_the_top_layer() {
        let t = textures();
        let mut out = Vec::with_capacity(4);
        let mut draw = SpriteDebugDraw::new(&mut out, t.white);
        draw.point(Vec2::new(-1.0, 0.5), DebugColour::ContactPoint);
        let point = out[0];
        assert_eq!(point.position, Vec2::new(-20.0, 10.0));
        assert_eq!(point.size, Vec2::new(POINT_PIXELS, POINT_PIXELS));
        assert!(point.angle.abs() < f32::EPSILON);
        assert_eq!(point.layer, POINT_LAYER);
        assert_eq!(point.tint, debug_tint(DebugColour::ContactPoint));
    }

    #[test]
    fn a_zero_length_line_is_drawn_as_a_dot_of_the_line_thickness() {
        let t = textures();
        let mut out = Vec::with_capacity(4);
        let mut draw = SpriteDebugDraw::new(&mut out, t.white);
        draw.line(Vec2::ZERO, Vec2::ZERO, DebugColour::Awake);
        assert_eq!(out[0].size, Vec2::new(LINE_PIXELS, LINE_PIXELS));
    }

    #[test]
    fn sprites_past_the_capacity_are_counted_and_the_list_never_grows() {
        let t = textures();
        let mut out = Vec::with_capacity(2);
        let capacity = out.capacity();
        let mut draw = SpriteDebugDraw::new(&mut out, t.white);
        for _ in 0..capacity + 3 {
            draw.point(Vec2::ZERO, DebugColour::Awake);
        }
        assert_eq!(draw.dropped(), 3);
        assert_eq!(out.len(), capacity);
        assert_eq!(out.capacity(), capacity);
    }

    #[test]
    fn the_roles_have_distinct_tints() {
        let roles = [
            DebugColour::Awake,
            DebugColour::Asleep,
            DebugColour::Static,
            DebugColour::ContactPoint,
            DebugColour::ContactNormal,
        ];
        for (i, a) in roles.iter().enumerate() {
            assert!(
                roles[i + 1..]
                    .iter()
                    .all(|b| debug_tint(*a) != debug_tint(*b))
            );
            assert_eq!(debug_tint(*a)[3], 255);
        }
    }
}
