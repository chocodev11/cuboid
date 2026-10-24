//! Every visual constant with its spec reference (spec: arch 13.2; app 8.1-8.10, 6.3, 6.4).
//!
//! The pipeline is gamma space: textures are `Rgba8Unorm`, colours are the stored 8-bit values and
//! blending runs on them. The shaders read the same numbers through [`wgsl_prelude`].

use glam::Vec3;

/// Clear colour of a view, `#343434` (app 8.1).
pub const CLEAR_VIEW: wgpu::Color = wgpu::Color {
    r: 52.0 / 255.0,
    g: 52.0 / 255.0,
    b: 52.0 / 255.0,
    a: 1.0,
};

/// Clear colour of a PNG render: transparent black (app 15).
pub const CLEAR_PNG: wgpu::Color = wgpu::Color::TRANSPARENT;

/// Flat ambient light (app 8.2).
pub const AMBIENT: f32 = 0.3953;

/// Main light: direction toward the light and intensity (app 8.2).
#[expect(clippy::approx_constant, reason = "the spec lists 0.7071")]
pub const MAIN_LIGHT: (Vec3, f32) = (Vec3::new(-0.4056, 0.7071, -0.5792), 0.8);

/// Secondary light: direction toward the light and intensity (app 8.2).
pub const SECOND_LIGHT: (Vec3, f32) = (Vec3::new(0.5736, 0.0, 0.8192), 0.5);

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
         const REACH: i32 = {};\n",
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
}
