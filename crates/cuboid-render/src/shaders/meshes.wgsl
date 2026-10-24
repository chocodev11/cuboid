// Mesh objects: model-space vertices with a normal and a UV, lit like the elements (app 8.3).

struct Sub {
    model: mat4x4<f32>,
    // The inverse transpose of the model's linear part, one column per vec4.
    normal0: vec4<f32>,
    normal1: vec4<f32>,
    normal2: vec4<f32>,
    // Material colour times the mesh tint.
    color: vec4<f32>,
}

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var tex_sampler: sampler;
@group(1) @binding(2) var<uniform> sub: Sub;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: f32,
}

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> VsOut {
    let world = sub.model * vec4<f32>(pos, 1.0);
    let moved = sub.normal0.xyz * normal.x + sub.normal1.xyz * normal.y + sub.normal2.xyz * normal.z;
    let n = moved / max(length(moved), 1e-6);
    return VsOut(view.view_proj * world, uv, lit_factor(n));
}

// The image rows run bottom to top and V runs up, so the UV is the texture coordinate.
fn shade(in: VsOut) -> vec4<f32> {
    let t = textureSampleLevel(tex, tex_sampler, in.uv, 0.0) * sub.color;
    return vec4<f32>(t.rgb * in.light, t.a);
}

@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return shade(in);
}

@fragment
fn fs_cutout(in: VsOut) -> @location(0) vec4<f32> {
    let c = shade(in);
    if c.a < 0.5 {
        discard;
    }
    return c;
}

// The silhouette masks of the outline: the selection or the hover colour where the texel passes
// the test.
@fragment
fn fs_mask(in: VsOut) -> @location(0) vec4<f32> {
    if shade(in).a < 0.5 {
        discard;
    }
    return SELECTED;
}

@fragment
fn fs_mask_hover(in: VsOut) -> @location(0) vec4<f32> {
    if shade(in).a < 0.5 {
        discard;
    }
    return HOVER;
}
