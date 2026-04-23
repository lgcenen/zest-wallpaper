#include <metal_stdlib>
using namespace metal;

struct CompatVertexIn {
    packed_float2 position;
    packed_float2 uv;
    packed_float4 color;
    float opacity;
};

struct CompatVertexOut {
    float4 position [[position]];
    float2 uv;
    float4 color;
};

vertex CompatVertexOut compat_sprite_vertex(
    const device CompatVertexIn* vertices [[buffer(0)]],
    uint vertex_id [[vertex_id]]
) {
    CompatVertexIn input_vertex = vertices[vertex_id];
    CompatVertexOut out_vertex;
    out_vertex.position = float4(float2(input_vertex.position), 0.0, 1.0);
    out_vertex.uv = float2(input_vertex.uv);
    out_vertex.color = float4(input_vertex.color) * input_vertex.opacity;
    return out_vertex;
}

fragment float4 compat_sprite_fragment(
    CompatVertexOut stage_vertex [[stage_in]],
    texture2d<float> color_texture [[texture(0)]]
) {
    constexpr sampler texture_sampler(mag_filter::linear, min_filter::linear);
    return color_texture.sample(texture_sampler, stage_vertex.uv) * stage_vertex.color;
}
