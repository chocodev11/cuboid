// A full-screen triangle that paints the test pattern of `pattern.rs`: red is the column, green
// the row, blue the exclusive or of both, all modulo 256, and alpha is 1. The values are whole
// numbers divided by 255, so every conforming adapter stores them back unchanged.

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let x = u32(pixel.x);
    let y = u32(pixel.y);
    return vec4<f32>(f32(x & 255u), f32(y & 255u), f32((x ^ y) & 255u), 255.0) / 255.0;
}
