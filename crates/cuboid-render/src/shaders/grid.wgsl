// The analytic sub-grid of app 6.3: lines at multiples of the step along both axes of a plane,
// a pixel footprint that thins and fades the lines when zoomed out the way mip-mapping did.

struct Plane {
    origin: vec4<f32>,
    // Spans of the two plane axes, full length.
    u: vec4<f32>,
    v: vec4<f32>,
}

@group(1) @binding(0) var<uniform> plane: Plane;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    // Distance from the plane edge in world units along the two axes.
    @location(0) t: vec2<f32>,
}

var<private> CORNERS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
);

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let c = CORNERS[vi];
    let pos = plane.origin.xyz + plane.u.xyz * c.x + plane.v.xyz * c.y;
    let lens = vec2<f32>(length(plane.u.xyz), length(plane.v.xyz));
    return VsOut(view.view_proj * vec4<f32>(pos, 1.0), c * lens);
}

// Share of a pixel footprint [x - fw/2, x + fw/2] covered by the line of width w nearest to x.
fn line_cover(x: f32, step: f32, fw: f32) -> f32 {
    let w = step / 256.0;
    let d = abs(x - round(x / step) * step);
    let hi = min(0.5 * w, d + 0.5 * fw);
    let lo = max(-0.5 * w, d - 0.5 * fw);
    return max(0.0, hi - lo) / max(fw, 1e-6);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let step = view.misc.y;
    let fw = fwidth(in.t);
    let cover = max(line_cover(in.t.x, step, fw.x), line_cover(in.t.y, step, fw.y));
    let add = GRID_ADD * min(cover, 1.0);
    return vec4<f32>(add, add, add, 0.0);
}
