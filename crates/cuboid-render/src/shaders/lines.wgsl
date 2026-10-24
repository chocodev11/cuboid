// Coloured line lists: grid lines, markers, wireframes and gizmos.

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> VsOut {
    return VsOut(view.view_proj * vec4<f32>(pos, 1.0), color);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
