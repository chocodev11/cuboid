// The full-view composite pass: scene colour with the outline of app 8.7, alpha forced to 1.

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var mask: texture_2d<f32>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // One triangle that covers the target.
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    return VsOut(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0));
}

@fragment
fn fs_plain(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureLoad(scene, vec2<i32>(in.clip.xy), 0);
    return vec4<f32>(c.rgb, 1.0);
}

fn tap(p: vec2<i32>, dims: vec2<i32>) -> vec4<f32> {
    if any(p < vec2<i32>(0)) || any(p >= dims) {
        return vec4<f32>(0.0);
    }
    return textureLoad(mask, p, 0);
}

// Four diagonal taps at (+-REACH, +-REACH): per-channel maximum colour, summed alpha, cleared
// wherever a highlighted silhouette covers the pixel, then mix(scene, rgb, a).
@fragment
fn fs_outline(in: VsOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.clip.xy);
    let c = textureLoad(scene, p, 0);
    let dims = vec2<i32>(textureDimensions(mask));
    let a = tap(p + vec2<i32>(REACH, REACH), dims);
    let b = tap(p + vec2<i32>(REACH, -REACH), dims);
    let d = tap(p + vec2<i32>(-REACH, REACH), dims);
    let e = tap(p + vec2<i32>(-REACH, -REACH), dims);
    var rgb = max(max(a.rgb, b.rgb), max(d.rgb, e.rgb));
    var alpha = min(1.0, a.a + b.a + d.a + e.a);
    if tap(p, dims).a > 0.0 {
        alpha = 0.0;
    }
    return vec4<f32>(mix(c.rgb, rgb, alpha), 1.0);
}
