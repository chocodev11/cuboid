// Voxel faces: one 8-byte record per face, six vertices per instance (arch 13.5).
//
// Word 0: local x, y, z (4 bits each), chunk x, y, z (6 bits each). Word 1: RGB (bits 0-23) and the
// ambient-occlusion code of v0..v3 (2 bits each). A record whose four codes are all 3 is padding.
// The quad is v0 v1 v2, v0 v2 v3 as in tools 7.6; the corner offsets of the direction come from
// the table of the direction (taken from the CPU expansion, so the viewport and the exporters
// share one source).

struct Dir {
    // Offsets of v0..v3 from the minimum corner of the voxel in (x, y, -z) cell steps.
    corners: array<vec4<f32>, 4>,
    // The unit face normal, the same for the whole direction.
    normal: vec4<f32>,
}

@group(1) @binding(0) var<uniform> dir: Dir;

struct Face {
    @location(0) words: vec2<u32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
}

var<private> QUAD: array<u32, 6> = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, face: Face) -> VsOut {
    let w0 = face.words.x;
    let w1 = face.words.y;
    if (w1 >> 24u) == 255u {
        return VsOut(vec4<f32>(0.0, 0.0, 0.0, 1.0), vec3<f32>(0.0));
    }
    let q = QUAD[vi];
    let local = vec3<f32>(f32(w0 & 15u), f32((w0 >> 4u) & 15u), f32((w0 >> 8u) & 15u));
    let chunk = vec3<f32>(f32((w0 >> 12u) & 63u), f32((w0 >> 18u) & 63u), f32((w0 >> 24u) & 63u));
    let cell = view.misc.x;
    let origin = vec3<f32>(16.0 * chunk.x * cell - 16.0, 16.0 * chunk.y * cell - 16.0, 16.0 - 16.0 * chunk.z * cell);
    let off = dir.corners[q].xyz;
    let pos = origin + vec3<f32>(
        (local.x + off.x) * cell,
        (local.y + off.y) * cell,
        -(local.z) * cell + off.z * cell,
    );
    let rgb = vec3<f32>(
        f32((w1 >> 16u) & 255u),
        f32((w1 >> 8u) & 255u),
        f32(w1 & 255u),
    ) / 255.0;
    let code = (w1 >> (24u + 2u * q)) & 3u;
    var out: VsOut;
    out.clip = view.view_proj * vec4<f32>(pos, 1.0);
    let ao = select(1.0, AO_FACTOR[code], view.shading.z != 0u);
    out.color = rgb * ao * face_light(dir.normal.xyz, true);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
