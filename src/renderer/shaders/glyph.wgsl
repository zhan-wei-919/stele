enable dual_source_blending;

struct ScreenUniform {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> screen: ScreenUniform;

@group(0) @binding(1)
var glyph_texture: texture_2d<f32>;

@group(0) @binding(2)
var glyph_sampler: sampler;

struct GlyphInstance {
    @location(0) screen_pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv_min: vec2<f32>,
    @location(3) uv_max: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) bearing: vec2<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct FragmentOut {
    @location(0) @blend_src(0) color: vec4<f32>,
    @location(0) @blend_src(1) blend: vec4<f32>,
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
fn vs_main(@builtin(vertex_index) vertex_index: u32, instance: GlyphInstance) -> VertexOut {
    let local = QUAD_POS[vertex_index];
    let origin = instance.screen_pos + vec2<f32>(instance.bearing.x, -instance.bearing.y);
    let pixel_pos = origin + local * instance.size;
    let ndc = vec2<f32>(
        (pixel_pos.x / screen.screen_size.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / screen.screen_size.y) * 2.0,
    );

    var out: VertexOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = mix(instance.uv_min, instance.uv_max, local);
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> FragmentOut {
    let coverage = textureSample(glyph_texture, glyph_sampler, in.uv).rgb;
    let alpha = max(max(coverage.r, coverage.g), coverage.b) * in.color.a;

    var out: FragmentOut;
    out.color = vec4<f32>(in.color.rgb, alpha);
    out.blend = vec4<f32>(coverage, alpha);
    return out;
}
