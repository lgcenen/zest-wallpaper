#include <metal_stdlib>
using namespace metal;

struct CompatMaskVertexIn {
    packed_float2 position;
    packed_float2 uv;
    packed_float4 color;
    float opacity;
};

struct CompatMaskVertexOut {
    float4 position [[position]];
    float2 uv;
};

vertex CompatMaskVertexOut compat_mask_vertex(
    const device CompatMaskVertexIn* vertices [[buffer(0)]],
    uint vertex_id [[vertex_id]]
) {
    CompatMaskVertexIn input_vertex = vertices[vertex_id];
    CompatMaskVertexOut out_vertex;
    out_vertex.position = float4(float2(input_vertex.position), 0.0, 1.0);
    out_vertex.uv = float2(input_vertex.uv);
    return out_vertex;
}

fragment float4 compat_mask_alpha_fragment(
    CompatMaskVertexOut stage_vertex [[stage_in]],
    texture2d<float> source_texture [[texture(0)]],
    texture2d<float> mask_texture [[texture(1)]]
) {
    constexpr sampler texture_sampler(mag_filter::linear, min_filter::linear);
    float source_alpha = source_texture.sample(texture_sampler, stage_vertex.uv).a;
    float mask_alpha = mask_texture.sample(texture_sampler, stage_vertex.uv).r;
    float alpha = source_alpha * mask_alpha;
    return float4(alpha, alpha, alpha, alpha);
}
