// Shared by every scene shader: the per-view uniform and the lighting of app 8.3.

struct View {
    view_proj: mat4x4<f32>,
    // xyz: camera position.
    camera: vec4<f32>,
    // x: view index, y: viewport width, z: viewport height.
    info: vec4<u32>,
    // x: voxel cell edge, y: grid step.
    misc: vec4<f32>,
    // x: shading mode (0 Cubik, 1 Minecraft block, 2 Minecraft item, 3 flat), y: 1 for the smooth
    // block light, z: 1 for voxel ambient occlusion.
    shading: vec4<u32>,
    // xyz: direction toward item light 0 in the world, w: alpha below which a texel is discarded.
    light0: vec4<f32>,
    // xyz: direction toward item light 1 in the world.
    light1: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;

// A + sum of min(2, I * max(0, n . l)).
fn lit_factor(n: vec3<f32>) -> f32 {
    let main = min(2.0, MAIN_I * max(0.0, dot(n, MAIN_DIR)));
    let second = min(2.0, SECOND_I * max(0.0, dot(n, SECOND_DIR)));
    return AMBIENT + main + second;
}

// The Minecraft block light of a face normal: the constant of its nearest axis (ties go to y,
// then z, then x), or Blockbench's continuous light.
fn block_factor(n: vec3<f32>) -> f32 {
    if view.shading.y != 0u {
        let up = (1.0 + n.y) * 0.5;
        return up * (1.0 - SMOOTH_AMBIENT) + n.x * n.x * SMOOTH_X + n.z * n.z * SMOOTH_Z + SMOOTH_AMBIENT;
    }
    let a = abs(n);
    if a.y >= a.z && a.y >= a.x {
        return select(BLOCK_DOWN, BLOCK_UP, n.y >= 0.0);
    }
    return select(BLOCK_EW, BLOCK_NS, a.z >= a.x);
}

// The Minecraft entity and item light of a face normal under the two lights of the view.
fn item_factor(n: vec3<f32>) -> f32 {
    let sum = max(0.0, dot(n, view.light0.xyz)) + max(0.0, dot(n, view.light1.xyz));
    return min(1.0, sum * ITEM_POWER + ITEM_AMBIENT);
}

// The light of a face by the shading mode; `shaded` is the element's shade flag. Cubik ignores it.
fn face_light(n: vec3<f32>, shaded: bool) -> f32 {
    switch view.shading.x {
        case 0u: { return lit_factor(n); }
        case 1u: { return select(1.0, block_factor(n), shaded); }
        case 2u: { return select(1.0, item_factor(n), shaded); }
        default: { return 1.0; }
    }
}

// Rotates v by the unit quaternion q (x, y, z, w).
fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}
