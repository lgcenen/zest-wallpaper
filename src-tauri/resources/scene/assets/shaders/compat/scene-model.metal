#include <metal_stdlib>
using namespace metal;

struct CompatModelVertexIn {
    packed_float2 position;
    packed_float2 uv;
    packed_float4 color;
    float opacity;
};

struct CompatModelVertexOut {
    float4 position [[position]];
    float2 uv;
    float4 color;
};

vertex CompatModelVertexOut compat_model_vertex(
    const device CompatModelVertexIn* vertices [[buffer(0)]],
    uint vertex_id [[vertex_id]]
) {
    CompatModelVertexIn model_vertex = vertices[vertex_id];
    CompatModelVertexOut out_vertex;
    out_vertex.position = float4(float2(model_vertex.position), 0.0, 1.0);
    out_vertex.uv = float2(model_vertex.uv);
    out_vertex.color = float4(model_vertex.color) * model_vertex.opacity;
    return out_vertex;
}

fragment float4 compat_model_fragment(
    CompatModelVertexOut stage_vertex [[stage_in]],
    texture2d<float> color_texture [[texture(0)]]
) {
    constexpr sampler texture_sampler(mag_filter::linear, min_filter::linear);
    return color_texture.sample(texture_sampler, stage_vertex.uv) * stage_vertex.color;
}
