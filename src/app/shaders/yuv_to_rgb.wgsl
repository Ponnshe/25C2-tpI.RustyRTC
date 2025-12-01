@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var u_tex: texture_2d<f32>;
@group(0) @binding(2) var v_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct Info {
    params: vec4<f32>,
};
@group(0) @binding(4) var<uniform> u_info: Info;

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
    let y_raw = textureSample(y_tex, samp, in.uv).r * 255.0;
    let u_raw = textureSample(u_tex, samp, in.uv).r * 255.0;
    let v_raw = textureSample(v_tex, samp, in.uv).r * 255.0;

    let y_scaled = max(0.0, (y_raw - 16.0) * (255.0 / 219.0));
    let u_scaled = (u_raw - 128.0) * (255.0 / 224.0);
    let v_scaled = (v_raw - 128.0) * (255.0 / 224.0);

    let r_ = y_scaled + 1.402 * v_scaled;
    let g_ = y_scaled - 0.344136 * u_scaled - 0.714136 * v_scaled;
    let b_ = y_scaled + 1.772 * u_scaled;

    let r = clamp(r_ / 255.0, 0.0, 1.0);
    let g = clamp(g_ / 255.0, 0.0, 1.0);
    let b = clamp(b_ / 255.0, 0.0, 1.0);

    return vec4<f32>(r, g, b, 1.0);
}
