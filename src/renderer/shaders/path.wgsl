struct ScreenUniform {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> screen: ScreenUniform;

struct PathVertex {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) coverage: f32,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) coverage: f32,
};

@vertex
fn vs_main(vertex: PathVertex) -> VertexOut {
    let ndc = vec2<f32>(
        (vertex.position.x / screen.screen_size.x) * 2.0 - 1.0,
        1.0 - (vertex.position.y / screen.screen_size.y) * 2.0,
    );

    var out: VertexOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = vertex.color;
    out.coverage = vertex.coverage;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color.rgb, in.color.a * in.coverage);
}
