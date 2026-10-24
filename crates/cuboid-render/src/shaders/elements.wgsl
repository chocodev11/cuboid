// Instanced cuboids: 36 vertices per instance, no vertex buffer for the corners (arch 13.3).
//
// The 176-byte record is read as instance-rate vertex attributes. Word layout:
//   0-2 position (f32), 3 flags (bits 0-3 view mask, 4 hover highlight, 5 inverted normals, 6 hidden element drawn
//   as a faint box, 7 shade off), 4-6 size (f32), 7 list slot (unused here), 8-11 rotation quaternion xyzw, 12-35 per face tl.x tl.y br.x br.y in atlas page
//   space (V up, page rows run bottom to top), 36-41 per face vertex colour RGBA8, 42-43 per
//   face state byte (bit 0 active, bits 1-2 UV quarter turns, bits 3-7 atlas page; face i is
//   byte i of the 8 bytes).

@group(1) @binding(0) var atlas: texture_2d_array<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct Instance {
    @location(0) pos_flags: vec4<u32>,
    @location(1) size_slot: vec4<u32>,
    @location(2) rot: vec4<f32>,
    @location(3) uv0: vec4<f32>,
    @location(4) uv1: vec4<f32>,
    @location(5) uv2: vec4<f32>,
    @location(6) uv3: vec4<f32>,
    @location(7) uv4: vec4<f32>,
    @location(8) uv5: vec4<f32>,
    @location(9) colors_a: vec4<u32>,
    @location(10) colors_b: vec4<u32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    // Centroid, so a pixel partly covered by the face (MSAA) samples inside the face.
    @location(0) @interpolate(perspective, centroid) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) page: u32,
    @location(3) @interpolate(flat) hover: u32,
}

var<private> CORNERS: array<vec3<f32>, 8> = array<vec3<f32>, 8>(
    vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 0.0, 0.0),
    vec3<f32>(1.0, 0.0, -1.0), vec3<f32>(0.0, 0.0, -1.0),
    vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 1.0, 0.0),
    vec3<f32>(1.0, 1.0, -1.0), vec3<f32>(0.0, 1.0, -1.0),
);

// Face slots down, up, north, south, west, east; four corners each (tools 4.3).
var<private> OUTER: array<vec4<u32>, 6> = array<vec4<u32>, 6>(
    vec4<u32>(2u, 3u, 0u, 1u), vec4<u32>(7u, 6u, 5u, 4u), vec4<u32>(4u, 5u, 1u, 0u),
    vec4<u32>(6u, 7u, 3u, 2u), vec4<u32>(7u, 4u, 0u, 3u), vec4<u32>(5u, 6u, 2u, 1u),
);
var<private> INNER: array<vec4<u32>, 6> = array<vec4<u32>, 6>(
    vec4<u32>(6u, 7u, 4u, 5u), vec4<u32>(3u, 2u, 1u, 0u), vec4<u32>(7u, 6u, 2u, 3u),
    vec4<u32>(5u, 4u, 0u, 1u), vec4<u32>(6u, 5u, 1u, 2u), vec4<u32>(4u, 7u, 3u, 0u),
);
var<private> NORMALS: array<vec3<f32>, 6> = array<vec3<f32>, 6>(
    vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0),
    vec3<f32>(0.0, 0.0, -1.0), vec3<f32>(-1.0, 0.0, 0.0), vec3<f32>(1.0, 0.0, 0.0),
);
// Triangles (b+3, b+1, b) and (b+3, b+2, b+1) of tools 4.3.
var<private> TRIS: array<u32, 6> = array<u32, 6>(3u, 1u, 0u, 3u, 2u, 1u);

fn face_rect(inst: Instance, face: u32) -> vec4<f32> {
    switch face {
        case 0u: { return inst.uv0; }
        case 1u: { return inst.uv1; }
        case 2u: { return inst.uv2; }
        case 3u: { return inst.uv3; }
        case 4u: { return inst.uv4; }
        default: { return inst.uv5; }
    }
}

fn face_color(inst: Instance, face: u32) -> u32 {
    switch face {
        case 0u: { return inst.colors_a.x; }
        case 1u: { return inst.colors_a.y; }
        case 2u: { return inst.colors_a.z; }
        case 3u: { return inst.colors_a.w; }
        case 4u: { return inst.colors_b.x; }
        default: { return inst.colors_b.y; }
    }
}

fn face_state(inst: Instance, face: u32) -> u32 {
    let word = select(inst.colors_b.z, inst.colors_b.w, face >= 4u);
    return (word >> ((face % 4u) * 8u)) & 255u;
}

fn degenerate() -> VsOut {
    return VsOut(vec4<f32>(0.0, 0.0, 0.0, 1.0), vec2<f32>(0.0), vec4<f32>(0.0), 0u, 0u);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let face = vi / 6u;
    let flags = inst.pos_flags.w;
    let state = face_state(inst, face);
    if ((flags >> view.info.x) & 1u) == 0u || (state & 1u) == 0u {
        return degenerate();
    }
    let slot = TRIS[vi % 6u];
    let inverted = (flags & 32u) != 0u;
    let corner_ids = select(OUTER[face], INNER[face], inverted);
    var corner = corner_ids.x;
    switch slot {
        case 1u: { corner = corner_ids.y; }
        case 2u: { corner = corner_ids.z; }
        case 3u: { corner = corner_ids.w; }
        default: {}
    }
    let size = bitcast<vec3<f32>>(inst.size_slot.xyz);
    let origin = bitcast<vec3<f32>>(inst.pos_flags.xyz);
    let world = origin + rotate(inst.rot, CORNERS[corner] * size);

    // Rotated UV corners: Q = [P00, P10, P11, P01], C_m = Q[(k + m) mod 4]; the up face takes
    // C_slot, the others C_(slot + 2) (tools 4.3, 4.5).
    let rect = face_rect(inst, face);
    let q = array<vec2<f32>, 4>(
        vec2<f32>(rect.x, rect.y), vec2<f32>(rect.z, rect.y),
        vec2<f32>(rect.z, rect.w), vec2<f32>(rect.x, rect.w),
    );
    let k = (state >> 1u) & 3u;
    let m = select((slot + 2u) % 4u, slot, face == 1u);
    var uv = q[0];
    switch (k + m) % 4u {
        case 1u: { uv = q[1]; }
        case 2u: { uv = q[2]; }
        case 3u: { uv = q[3]; }
        default: {}
    }

    let n = rotate(inst.rot, NORMALS[face]);
    let vertex = unpack4x8unorm(face_color(inst, face));
    var out: VsOut;
    out.clip = view.view_proj * vec4<f32>(world, 1.0);
    out.uv = uv;
    out.color = vec4<f32>(vertex.rgb * face_light(n, (flags & 128u) == 0u), vertex.a);
    out.page = (state >> 3u) & 31u;
    out.hover = ((flags >> 4u) & 1u) | (((flags >> 6u) & 1u) << 1u);
    return out;
}

fn shade(in: VsOut) -> vec4<f32> {
    // A hidden element drawn as a faint white box ignores its texture.
    if (in.hover & 2u) != 0u {
        return vec4<f32>(1.0, 1.0, 1.0, FAINT);
    }
    let t = textureSampleLevel(atlas, atlas_sampler, in.uv, i32(in.page), 0.0);
    return vec4<f32>(t.rgb * in.color.rgb, t.a * in.color.a);
}

// The blended colour pass of a translucent object.
@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return shade(in);
}

// The alpha-tested pass of a binary object, and the depth pre-pass of a translucent one.
@fragment
fn fs_cutout(in: VsOut) -> @location(0) vec4<f32> {
    let c = shade(in);
    if c.a < view.light0.w {
        discard;
    }
    return c;
}

// The silhouette mask of the outline: the highlight colour where the texel passes the test; the
// hover bit of the instance picks the hover colour.
@fragment
fn fs_mask(in: VsOut) -> @location(0) vec4<f32> {
    if shade(in).a < view.light0.w {
        discard;
    }
    return select(SELECTED, HOVER, (in.hover & 1u) != 0u);
}
