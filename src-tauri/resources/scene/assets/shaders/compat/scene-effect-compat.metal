#include <metal_stdlib>
using namespace metal;

struct Phase10EffectVertexIn {
    packed_float2 position;
    packed_float2 uv;
    packed_float4 color;
    float opacity;
};

struct Phase10EffectVertexOut {
    float4 position [[position]];
    float2 uv;
    float4 color;
};

struct Phase10EffectUniforms {
    float4 color;
    float4 user0;
    float4 user1;
    float2 texel_size;
    float2 aux_texel_size;
    float2 screen_size;
    float time;
    float intensity;
    float speed;
    float radius;
    float angle;
};

vertex Phase10EffectVertexOut phase10_effect_vertex(
    const device Phase10EffectVertexIn* vertices [[buffer(0)]],
    uint vertex_id [[vertex_id]]
) {
    Phase10EffectVertexIn input_vertex = vertices[vertex_id];
    Phase10EffectVertexOut out_vertex;
    out_vertex.position = float4(float2(input_vertex.position), 0.0, 1.0);
    out_vertex.uv = float2(input_vertex.uv);
    out_vertex.color = float4(input_vertex.color) * input_vertex.opacity;
    return out_vertex;
}

static float2 rotate2d(float2 value, float angle) {
    float sine = sin(angle);
    float cosine = cos(angle);
    return float2(
        value.x * cosine - value.y * sine,
        value.x * sine + value.y * cosine
    );
}

static float4 sample_input(
    texture2d<float> input_texture,
    sampler texture_sampler,
    float2 uv
) {
    return input_texture.sample(texture_sampler, clamp(uv, float2(0.0), float2(1.0)));
}

static float aux_red_mask(
    texture2d<float> aux_texture,
    sampler texture_sampler,
    float2 uv,
    float2 aux_texel_size
) {
    if (aux_texel_size.x <= 0.0 || aux_texel_size.y <= 0.0) {
        return 1.0;
    }
    return aux_texture.sample(texture_sampler, clamp(uv, float2(0.0), float2(1.0))).r;
}

static float3 blend_tint(float3 base, float3 blend) {
    return max(base.r, max(base.g, base.b)) * blend;
}

static float3 blend_screen(float3 base, float3 blend) {
    return 1.0 - ((1.0 - base) * (1.0 - blend));
}

static float3 apply_tint_blend(float3 base, float3 blend, float opacity) {
    float amount = clamp(opacity, 0.0, 1.0);
#if BLENDMODE == 2
    return mix(base, base * blend, amount);
#elif BLENDMODE == 7
    return mix(base, blend_screen(base, blend), amount);
#elif BLENDMODE == 9
    return mix(base, min(base + blend, float3(1.0)), amount);
#elif BLENDMODE == 30
    return mix(base, blend_tint(base, blend), amount);
#elif BLENDMODE == 31
    return base + blend * amount;
#elif BLENDMODE == 32
    return mix(base, base + base * blend, amount);
#else
    return mix(base, blend, amount);
#endif
}

fragment float4 phase10_effect_fragment(
    Phase10EffectVertexOut stage_vertex [[stage_in]],
    texture2d<float> input_texture [[texture(0)]],
    texture2d<float> aux_texture [[texture(1)]],
    constant Phase10EffectUniforms& uniforms [[buffer(0)]]
) {
    constexpr sampler texture_sampler(address::clamp_to_edge, mag_filter::linear, min_filter::linear);
    float2 uv = stage_vertex.uv;
    float4 sampled = sample_input(input_texture, texture_sampler, uv);

#if PHASE10_EFFECT_PULSE
    float wave = sin(uniforms.time * max(uniforms.speed, 0.001));
    float amount = wave * uniforms.intensity;
    float2 centered = uv - 0.5;
    float scale = max(0.2, 1.0 + amount);
    sampled = sample_input(input_texture, texture_sampler, 0.5 + centered / scale);
#elif PHASE10_EFFECT_SHAKE
    float px = uniforms.intensity * uniforms.texel_size.x;
    float py = uniforms.intensity * uniforms.texel_size.y;
    float2 offset = float2(
        sin(uniforms.time * max(uniforms.speed, 0.001) * 7.0) * px,
        cos(uniforms.time * max(uniforms.speed, 0.001) * 5.0) * py
    );
    sampled = sample_input(input_texture, texture_sampler, uv + offset);
#elif PHASE10_EFFECT_WATERRIPPLE
    float2 ripple_uv = uv + float2(uniforms.time * uniforms.speed * 0.05, -uniforms.time * uniforms.speed * 0.03);
    float2 ripple = aux_texture.sample(texture_sampler, fract(ripple_uv)).rg * 2.0 - 1.0;
    sampled = sample_input(
        input_texture,
        texture_sampler,
        uv + ripple * uniforms.intensity * max(uniforms.texel_size, float2(0.0001))
    );
#elif PHASE10_EFFECT_WATERWAVES
    float mask = aux_red_mask(aux_texture, texture_sampler, uv, uniforms.aux_texel_size);
    float2 direction = uniforms.user0.xy;
    if (length(direction) < 0.0001) {
        direction = float2(-sin(uniforms.angle), cos(uniforms.angle));
    }
    direction = normalize(direction);
    float scale = uniforms.user0.z == 0.0 ? 200.0 : max(abs(uniforms.user0.z), 0.01);
    float exponent = uniforms.user0.w == 0.0 ? 1.0 : max(abs(uniforms.user0.w), 0.51);
    float distance = uniforms.time * max(uniforms.speed, 0.001) + dot(uv, direction) * scale;
    float wave = sin(distance);
    float signed_wave = sign(wave) * pow(abs(wave), exponent);
    float strength = max(uniforms.intensity, 0.0);
    float safe_amplitude = max(max(uniforms.texel_size.x, uniforms.texel_size.y) * 4.0, 0.0001);
    float displacement = min(strength * strength, safe_amplitude) * mask;
    float2 offset = float2(direction.y, -direction.x) * signed_wave * displacement;
    float4 displaced = sample_input(input_texture, texture_sampler, uv + offset);
    float coverage = smoothstep(0.02, 0.15, sampled.a);
    sampled = mix(sampled, displaced, coverage);
#elif PHASE10_EFFECT_BLUR
    float2 step_xy = max(uniforms.texel_size * max(uniforms.radius, 1.0), float2(0.0001));
    sampled =
        sample_input(input_texture, texture_sampler, uv) * 0.28 +
        sample_input(input_texture, texture_sampler, uv + float2(step_xy.x, 0.0)) * 0.18 +
        sample_input(input_texture, texture_sampler, uv - float2(step_xy.x, 0.0)) * 0.18 +
        sample_input(input_texture, texture_sampler, uv + float2(0.0, step_xy.y)) * 0.18 +
        sample_input(input_texture, texture_sampler, uv - float2(0.0, step_xy.y)) * 0.18;
#elif PHASE10_EFFECT_TINT
    float mask = aux_red_mask(aux_texture, texture_sampler, uv, uniforms.aux_texel_size);
    float strength = clamp(uniforms.intensity * mask, 0.0, 1.0);
    sampled.rgb = apply_tint_blend(sampled.rgb, uniforms.color.rgb, strength);
#elif PHASE10_EFFECT_SCROLL
    float2 direction = uniforms.user0.xy;
    if (length(direction) < 0.0001) {
        direction = float2(1.0, 0.0);
    }
    sampled = input_texture.sample(
        texture_sampler,
        fract(uv + direction * uniforms.speed * uniforms.time * 0.05)
    );
#elif PHASE10_EFFECT_SHINE
    float2 rotated = rotate2d(uv - 0.5, uniforms.angle);
    float band = abs(rotated.x - fract(uniforms.time * uniforms.speed * 0.2) + 0.5);
    float highlight = smoothstep(uniforms.radius, 0.0, band) * uniforms.intensity;
    sampled.rgb += uniforms.color.rgb * highlight;
#endif

    sampled *= stage_vertex.color;
    return saturate(sampled);
}
