#include <metal_stdlib>
using namespace metal;

struct CompatCopyVertexIn {
    packed_float2 position;
    packed_float2 uv;
    packed_float4 color;
    float opacity;
};

struct CompatCopyVertexOut {
    float4 position [[position]];
    float2 uv;
};

vertex CompatCopyVertexOut compat_copy_vertex(
    const device CompatCopyVertexIn* vertices [[buffer(0)]],
    uint vertex_id [[vertex_id]]
) {
    CompatCopyVertexIn input_vertex = vertices[vertex_id];
    CompatCopyVertexOut out_vertex;
    out_vertex.position = float4(float2(input_vertex.position), 0.0, 1.0);
    out_vertex.uv = float2(input_vertex.uv);
    return out_vertex;
}

fragment float4 compat_copy_fragment(
    CompatCopyVertexOut stage_vertex [[stage_in]],
    texture2d<float> source_texture [[texture(0)]]
) {
    constexpr sampler texture_sampler(mag_filter::linear, min_filter::linear);
    return source_texture.sample(texture_sampler, stage_vertex.uv);
}
