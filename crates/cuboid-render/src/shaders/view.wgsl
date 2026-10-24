// Shared by every scene shader: the per-view uniform and the lighting of app 8.3.

struct View {
    view_proj: mat4x4<f32>,
    // xyz: camera position.
    camera: vec4<f32>,
    // x: view index, y: viewport width, z: viewport height.
    info: vec4<u32>,
    // x: voxel cell edge, y: grid step.
    misc: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;

// A + sum of min(2, I * max(0, n . l)).
fn lit_factor(n: vec3<f32>) -> f32 {
    let main = min(2.0, MAIN_I * max(0.0, dot(n, MAIN_DIR)));
    let second = min(2.0, SECOND_I * max(0.0, dot(n, SECOND_DIR)));
    return AMBIENT + main + second;
}

// Rotates v by the unit quaternion q (x, y, z, w).
fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}
