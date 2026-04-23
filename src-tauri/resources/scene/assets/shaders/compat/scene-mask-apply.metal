#include <metal_stdlib>
using namespace metal;

struct CompatMaskApplyVertexIn {
    packed_float2 position;
    packed_float2 uv;
    packed_float4 color;
    float opacity;
};

struct CompatMaskApplyVertexOut {
    float4 position [[position]];
    float2 uv;
    float4 color;
};

vertex CompatMaskApplyVertexOut compat_mask_apply_vertex(
    const device CompatMaskApplyVertexIn* vertices [[buffer(0)]],
    uint vertex_id [[vertex_id]]
) {
    CompatMaskApplyVertexIn input_vertex = vertices[vertex_id];
    CompatMaskApplyVertexOut out_vertex;
    out_vertex.position = float4(float2(input_vertex.position), 0.0, 1.0);
    out_vertex.uv = float2(input_vertex.uv);
    out_vertex.color = float4(input_vertex.color) * input_vertex.opacity;
    return out_vertex;
}

fragment float4 compat_mask_apply_fragment(
    CompatMaskApplyVertexOut stage_vertex [[stage_in]],
    texture2d<float> source_texture [[texture(0)]],
    texture2d<float> mask_texture [[texture(1)]]
) {
    constexpr sampler texture_sampler(mag_filter::linear, min_filter::linear);
    float4 sampled = source_texture.sample(texture_sampler, stage_vertex.uv) * stage_vertex.color;
    sampled.a *= mask_texture.sample(texture_sampler, stage_vertex.uv).a;
    return sampled;
}
