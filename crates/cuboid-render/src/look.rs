//! Every visual constant with its spec reference (spec: arch 13.2; app 8.1-8.10, 6.3, 6.4).
//!
//! The pipeline is gamma space: textures are `Rgba8Unorm`, colours are the stored 8-bit values and
//! blending runs on them. The shaders read the same numbers through [`wgsl_prelude`].

use glam::Vec3;

/// Clear colour of a view until the application sets one with `RenderSettings::clear` (the
/// viewport colour of its theme); `#343434` (app 8.1).
pub const CLEAR_VIEW: [u8; 3] = [52, 52, 52];

/// The clear colour of a view from its 8-bit RGB value.
pub fn clear_color(rgb: [u8; 3]) -> wgpu::Color {
    let channel = |v: u8| f64::from(v) / 255.0;
    wgpu::Color {
        r: channel(rgb[0]),
        g: channel(rgb[1]),
        b: channel(rgb[2]),
        a: 1.0,
    }
}

/// Clear colour of a PNG render: transparent black (app 15).
pub const CLEAR_PNG: wgpu::Color = wgpu::Color::TRANSPARENT;

/// Flat ambient light (app 8.2).
pub const AMBIENT: f32 = 0.3953;

/// Main light: direction toward the light and intensity (app 8.2).
#[expect(clippy::approx_constant, reason = "the spec lists 0.7071")]
pub const MAIN_LIGHT: (Vec3, f32) = (Vec3::new(-0.4056, 0.7071, -0.5792), 0.8);

/// Secondary light: direction toward the light and intensity (app 8.2).
pub const SECOND_LIGHT: (Vec3, f32) = (Vec3::new(0.5736, 0.0, 0.8192), 0.5);

/// Alpha below which a texel is discarded in the Cubik shading mode.
pub const CUTOUT_CUBIK: f32 = 0.5;

/// Alpha below which a texel is discarded in the Minecraft block mode: the client's terrain
/// cutout (verified in the 1.21.11 client).
pub const CUTOUT_BLOCK: f32 = 0.5;

/// Alpha below which a texel is discarded in the Minecraft item mode and in the flat mode: the
/// client's item and entity cutout (verified in the 1.21.11 client).
pub const CUTOUT_ITEM: f32 = 0.1;

/// Block light of a face by the nearest axis of its normal, as the client's `getShade` returns
/// it for the overworld (verified in the 1.21.11 client): up 1.0, down 0.5, north and south 0.8,
/// east and west 0.6. An element with `shade` false gets 1.0.
pub const BLOCK_UP: f32 = 1.0;
/// See [`BLOCK_UP`].
pub const BLOCK_DOWN: f32 = 0.5;
/// See [`BLOCK_UP`].
pub const BLOCK_NORTH_SOUTH: f32 = 0.8;
/// See [`BLOCK_UP`].
pub const BLOCK_EAST_WEST: f32 = 0.6;

/// The smooth block light, which follows the normal continuously and gives the same four
/// values on the axes: `(1 + n.y) / 2 * (1 - SMOOTH_AMBIENT) + n.x^2 * SMOOTH_X + n.z^2 *
/// SMOOTH_Z + SMOOTH_AMBIENT` (the per-vertex light of Blockbench's texture shader).
pub const SMOOTH_AMBIENT: f32 = 0.5;
/// See [`SMOOTH_AMBIENT`].
pub const SMOOTH_X: f32 = -0.15;
/// See [`SMOOTH_AMBIENT`].
pub const SMOOTH_Z: f32 = 0.05;

/// Weight of the two directional lights of the entity and item lighting
/// (`MINECRAFT_LIGHT_POWER` of the client's `light.glsl`).
pub const ITEM_POWER: f32 = 0.6;

/// Ambient part of the entity and item lighting (`MINECRAFT_AMBIENT_LIGHT` of `light.glsl`).
pub const ITEM_AMBIENT: f32 = 0.4;

/// The two lights of the world (the client's `DIFFUSE_LIGHT_0` and `DIFFUSE_LIGHT_1`) as
/// directions toward the light in Minecraft axes (x east, y up, z south): `(0.2, 1, -0.7)` and
/// `(-0.2, 1, 0.7)`, normalised. Items in hand, on the ground, in a frame or on the head are lit
/// by them in the world frame of the model.
pub fn level_lights() -> [Vec3; 2] {
    [
        Vec3::new(0.2, 1.0, -0.7).normalize(),
        Vec3::new(-0.2, 1.0, 0.7).normalize(),
    ]
}

/// The two lights of a GUI item that is shaped like a block (`gui_light` side, the client's
/// `ITEMS_3D`): directions toward the light in the eye frame of the viewer (x right, y up, z
/// toward the viewer).
///
/// The client builds them by turning the level lights with matrices of its own
/// (`scaling(1, -1, 1)`, `rotateYXZ(1.0821041, 3.2375858, 0)`, `rotateYXZ(-0.3926991,
/// 2.3561945, 0)`) in a frame with y down; flipping y gives the eye frame. A test recomputes
/// these numbers from the matrices.
pub const GUI_SIDE_LIGHTS: [Vec3; 2] = [
    Vec3::new(-0.933_439_3, 0.262_694_7, -0.244_300_2),
    Vec3::new(-0.103_571_36, 0.976_606_8, 0.188_446_43),
];

/// The two lights of a flat GUI item (`gui_light` front, the client's `ITEMS_FLAT`), in the eye
/// frame like [`GUI_SIDE_LIGHTS`]: the level lights turned by `rotationY(-0.3926991)` and
/// `rotateX(2.3561945)`.
pub const GUI_FRONT_LIGHTS: [Vec3; 2] = [
    Vec3::new(-0.222_519, 0.171_498_6, 0.959_725_8),
    Vec3::new(-0.215_012_13, 0.971_825_3, 0.096_567_78),
];

/// Brightness of an ambient-occlusion code (tools 7.6).
pub const AO_FACTOR: [f32; 4] = [1.0, 0.9, 0.8, 0.7];

/// Outline colour of a hovered object, `#FFFFFF80` (app 8.7).
pub const HOVER: [f32; 4] = [1.0, 1.0, 1.0, 128.0 / 255.0];

/// Outline colour of a selected object, `#FF6400FF` (app 8.7).
pub const SELECTED: [f32; 4] = [1.0, 100.0 / 255.0, 0.0, 1.0];

/// Reach of the outline taps in pixels (app 8.7).
pub const OUTLINE_REACH: i32 = 2;

/// Vertex alpha of the faint boxes and selection wireframes, 5/255 (app 8.8, 8.9).
pub const FAINT: f32 = 5.0 / 255.0;

/// Colour a fully covered sub-grid line adds to each channel (app 6.3).
pub const GRID_ADD: f32 = 0.124;

/// Main grid lines, `#FFFFFF20` (app 6.4).
pub const MAIN_GRID: [f32; 4] = [1.0, 1.0, 1.0, 32.0 / 255.0];

/// Gizmo and axis colours: X red `#FF4242`, Y blue `#0072FF`, Z green `#82F215` (tools 9.7).
pub const AXIS: [[f32; 4]; 3] = [
    [1.0, 66.0 / 255.0, 66.0 / 255.0, 1.0],
    [0.0, 114.0 / 255.0, 1.0, 1.0],
    [130.0 / 255.0, 242.0 / 255.0, 21.0 / 255.0, 1.0],
];

/// Translucent white of the markers, 25 % (app 8.10).
pub const MARKER: [f32; 4] = [1.0, 1.0, 1.0, 0.25];

/// Opaque white of the north pointer (app 8.10).
pub const POINTER: [f32; 4] = [1.0; 4];

/// Model box handles, `#FFC800` at 25 % (tools 9.7).
pub const BOX_HANDLE: [f32; 4] = [1.0, 200.0 / 255.0, 0.0, 0.25];

/// Voxel pointer, `#E11A1A` at 50 % (tools 9.7).
pub const VOXEL_POINTER: [f32; 4] = [225.0 / 255.0, 26.0 / 255.0, 26.0 / 255.0, 0.5];

/// The light a face with the given unit normal receives, per channel (app 8.3):
/// `A + sum of min(2, I * max(0, n . l))`.
pub fn lit(normal: Vec3) -> f32 {
    [MAIN_LIGHT, SECOND_LIGHT]
        .iter()
        .map(|(dir, intensity)| (intensity * normal.dot(*dir).max(0.0)).min(2.0))
        .sum::<f32>()
        + AMBIENT
}

/// The block light of a face with the given unit normal (Minecraft block modes). Stepped, the
/// constant of the nearest axis; ties go to y, then z, then x as the client's
/// `Direction.getApproximateNearest` does. Smooth, Blockbench's continuous light.
pub fn block_lit(normal: Vec3, smooth: bool) -> f32 {
    if smooth {
        let up = (1.0 + normal.y) * 0.5;
        return up * (1.0 - SMOOTH_AMBIENT)
            + normal.x * normal.x * SMOOTH_X
            + normal.z * normal.z * SMOOTH_Z
            + SMOOTH_AMBIENT;
    }
    let a = normal.abs();
    if a.y >= a.z && a.y >= a.x {
        return if normal.y >= 0.0 {
            BLOCK_UP
        } else {
            BLOCK_DOWN
        };
    }
    if a.z >= a.x {
        BLOCK_NORTH_SOUTH
    } else {
        BLOCK_EAST_WEST
    }
}

/// The entity and item light of a face with the given unit normal under two lights:
/// `min(1, (max(0, n.l0) + max(0, n.l1)) * 0.6 + 0.4)`.
pub fn item_lit(normal: Vec3, lights: [Vec3; 2]) -> f32 {
    let sum: f32 = lights.iter().map(|l| normal.dot(*l).max(0.0)).sum();
    (sum * ITEM_POWER + ITEM_AMBIENT).min(1.0)
}

/// The WGSL constants every shader starts with, generated from the numbers above.
pub fn wgsl_prelude() -> String {
    let v3 = |v: Vec3| format!("vec3<f32>({:?}, {:?}, {:?})", v.x, v.y, v.z);
    format!(
        "const AMBIENT: f32 = {:?};\n\
         const MAIN_DIR: vec3<f32> = {};\n\
         const MAIN_I: f32 = {:?};\n\
         const SECOND_DIR: vec3<f32> = {};\n\
         const SECOND_I: f32 = {:?};\n\
         const AO_FACTOR: array<f32, 4> = array<f32, 4>({:?}, {:?}, {:?}, {:?});\n\
         const HOVER: vec4<f32> = vec4<f32>({:?}, {:?}, {:?}, {:?});\n\
         const SELECTED: vec4<f32> = vec4<f32>({:?}, {:?}, {:?}, {:?});\n\
         const FAINT: f32 = {:?};\n\
         const GRID_ADD: f32 = {:?};\n\
         const REACH: i32 = {};\n\
         const BLOCK_UP: f32 = {:?};\n\
         const BLOCK_DOWN: f32 = {:?};\n\
         const BLOCK_NS: f32 = {:?};\n\
         const BLOCK_EW: f32 = {:?};\n\
         const SMOOTH_AMBIENT: f32 = {:?};\n\
         const SMOOTH_X: f32 = {:?};\n\
         const SMOOTH_Z: f32 = {:?};\n\
         const ITEM_POWER: f32 = {:?};\n\
         const ITEM_AMBIENT: f32 = {:?};\n",
        AMBIENT,
        v3(MAIN_LIGHT.0),
        MAIN_LIGHT.1,
        v3(SECOND_LIGHT.0),
        SECOND_LIGHT.1,
        AO_FACTOR[0],
        AO_FACTOR[1],
        AO_FACTOR[2],
        AO_FACTOR[3],
        HOVER[0],
        HOVER[1],
        HOVER[2],
        HOVER[3],
        SELECTED[0],
        SELECTED[1],
        SELECTED[2],
        SELECTED[3],
        FAINT,
        GRID_ADD,
        OUTLINE_REACH,
        BLOCK_UP,
        BLOCK_DOWN,
        BLOCK_NORTH_SOUTH,
        BLOCK_EAST_WEST,
        SMOOTH_AMBIENT,
        SMOOTH_X,
        SMOOTH_Z,
        ITEM_POWER,
        ITEM_AMBIENT,
    )
}

/// A shader source: the generated constants followed by the files.
pub fn shader(parts: &[&str]) -> String {
    let mut source = wgsl_prelude();
    let newline = char::from(10u8);
    for part in parts {
        source.push_str(part);
        source.push(newline);
    }
    source
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_possible_truncation, reason = "test arithmetic")]
    use super::*;

    /// The brightness table of app 8.3, in 8-bit values.
    #[test]
    fn face_brightness_matches_the_spec_table() {
        let byte = |n: Vec3| (lit(n) * 255.0).round() as i32;
        assert_eq!(byte(Vec3::Y), 245);
        assert_eq!(byte(Vec3::NEG_Z), 219);
        assert_eq!(byte(Vec3::Z), 205);
        assert_eq!(byte(Vec3::NEG_X), 184);
        assert_eq!(byte(Vec3::X), 174);
        assert_eq!(byte(Vec3::NEG_Y), 101);
    }

    #[test]
    fn block_light_steps_by_axis_and_smooth_light_agrees_on_the_axes() {
        let axes = [
            (Vec3::Y, BLOCK_UP),
            (Vec3::NEG_Y, BLOCK_DOWN),
            (Vec3::Z, BLOCK_NORTH_SOUTH),
            (Vec3::NEG_Z, BLOCK_NORTH_SOUTH),
            (Vec3::X, BLOCK_EAST_WEST),
            (Vec3::NEG_X, BLOCK_EAST_WEST),
        ];
        for (n, want) in axes {
            assert!((block_lit(n, false) - want).abs() < 1e-6, "{n}");
            assert!((block_lit(n, true) - want).abs() < 1e-6, "{n}");
        }
        // A face turned 22.5 degrees keeps the step of its nearest axis.
        let turned = Vec3::new(0.0, 22.5f32.to_radians().sin(), -22.5f32.to_radians().cos());
        assert!((block_lit(turned, false) - BLOCK_NORTH_SOUTH).abs() < 1e-6);
        assert!((block_lit(turned, true) - BLOCK_NORTH_SOUTH).abs() > 0.05);
        // Ties go to y, then z, then x.
        let tie = |a: Vec3| block_lit(a.normalize(), false);
        assert!((tie(Vec3::new(1.0, 1.0, 0.0)) - BLOCK_UP).abs() < 1e-6);
        assert!((tie(Vec3::new(1.0, 0.0, 1.0)) - BLOCK_NORTH_SOUTH).abs() < 1e-6);
    }

    /// The GUI light pairs follow from the client's matrices (Lighting.java, 1.21.11) applied to
    /// its two level lights, then y flipped from the client's y-down GUI frame to the eye frame.
    #[test]
    fn gui_lights_match_the_client_matrices() {
        use glam::Mat4;
        use std::f32::consts::{FRAC_PI_4, FRAC_PI_8};
        let level = [
            Vec3::new(0.2, 1.0, -0.7).normalize(),
            Vec3::new(-0.2, 1.0, 0.7).normalize(),
        ];
        let flat = Mat4::from_rotation_y(-FRAC_PI_8) * Mat4::from_rotation_x(3.0 * FRAC_PI_4);
        let side = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
            * Mat4::from_rotation_y(1.082_104_1)
            * Mat4::from_rotation_x(3.237_585_8)
            * flat;
        for (matrix, got) in [(flat, GUI_FRONT_LIGHTS), (side, GUI_SIDE_LIGHTS)] {
            for (l, want) in level.iter().zip(got) {
                let m = matrix.transform_vector3(*l);
                let eye = Vec3::new(m.x, -m.y, m.z);
                assert!((eye - want).length() < 1e-5, "{eye} vs {want}");
            }
        }
    }

    #[test]
    fn cutout_thresholds_follow_the_shading_mode() {
        use crate::ShadingMode;
        let want = [
            (ShadingMode::Cubik, 0.5),
            (ShadingMode::MinecraftBlock, 0.5),
            (ShadingMode::MinecraftItem, 0.1),
            (ShadingMode::Flat, 0.1),
        ];
        for (mode, alpha) in want {
            assert!((mode.cutout() - alpha).abs() < 1e-6, "{mode:?}");
        }
    }

    #[test]
    fn item_light_clamps_and_keeps_the_ambient() {
        let lights = level_lights();
        assert!((item_lit(Vec3::Y, lights) - 1.0).abs() < 1e-6);
        assert!((item_lit(Vec3::NEG_Y, lights) - ITEM_AMBIENT).abs() < 1e-6);
        let one = (lights[1].x.abs() * ITEM_POWER) + ITEM_AMBIENT;
        assert!((item_lit(Vec3::NEG_X, lights) - one).abs() < 1e-6);
    }
}
