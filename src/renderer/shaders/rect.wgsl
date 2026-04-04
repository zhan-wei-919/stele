struct ScreenUniform {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> screen: ScreenUniform;

struct RectInstance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

const QUAD_POS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
);

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, instance: RectInstance) -> VertexOut {
    let local = QUAD_POS[vertex_index];
    let pixel_pos = instance.pos + local * instance.size;
    let ndc = vec2<f32>(
        (pixel_pos.x / screen.screen_size.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / screen.screen_size.y) * 2.0,
    );

    var out: VertexOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
