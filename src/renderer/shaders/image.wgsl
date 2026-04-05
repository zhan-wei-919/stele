struct ScreenUniform {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> screen: ScreenUniform;

@group(0) @binding(1)
var image_texture: texture_2d<f32>;

@group(0) @binding(2)
var image_sampler: sampler;

struct ImageInstance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv_min: vec2<f32>,
    @location(3) uv_max: vec2<f32>,
    @location(4) opacity: f32,
    @location(5) _padding: vec3<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) opacity: f32,
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
fn vs_main(@builtin(vertex_index) vertex_index: u32, instance: ImageInstance) -> VertexOut {
    let local = QUAD_POS[vertex_index];
    let pixel_pos = instance.pos + local * instance.size;
    let ndc = vec2<f32>(
        (pixel_pos.x / screen.screen_size.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / screen.screen_size.y) * 2.0,
    );

    var out: VertexOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = mix(instance.uv_min, instance.uv_max, local);
    out.opacity = instance.opacity;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let sampled = textureSample(image_texture, image_sampler, in.uv);
    return vec4<f32>(sampled.rgb, sampled.a * in.opacity);
}
