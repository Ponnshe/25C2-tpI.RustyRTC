@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var u_tex: texture_2d<f32>;
@group(0) @binding(2) var v_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct VertexOutput {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Leer raw [0,1]
    let Y = textureSample(y_tex, samp, in.uv).r * 255.0;
    let U = textureSample(u_tex, samp, in.uv).r * 255.0 - 128.0;
    let V = textureSample(v_tex, samp, in.uv).r * 255.0 - 128.0;

    let r = (Y + 1.402 * V) / 255.0;
    let g = (Y - 0.344136 * U - 0.714136 * V) / 255.0;
    let b = (Y + 1.772 * U) / 255.0;

    return vec4<f32>(r, g, b, 1.0);
}
