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
    float2 aux2_texel_size;
    float2 aux3_texel_size;
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

static bool has_aux_texture(float2 texel_size) {
    return texel_size.x > 0.0 && texel_size.y > 0.0;
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

static float phase10_shake_wave(
    constant Phase10EffectUniforms& uniforms,
    float flow_phase
) {
    float2 friction = max(uniforms.user1.xy, float2(0.01));
#if NOISE
    constexpr float phase_scale = 1.57079632679;
    float4 time_phase = flow_phase +
        fract(uniforms.speed * uniforms.time / phase_scale * float4(1.0, -0.16161616, 0.0083333, -0.00019841)) *
            phase_scale;
    float4 cosine_values = cos(time_phase);
    float4 sine_values = sin(time_phase) * 0.498 + 0.5;
    float4 easing = mix(
        1.0 - pow(1.0 - sine_values, float4(friction.x)),
        pow(sine_values, float4(friction.y)),
        step(float4(0.0), cosine_values)
    );
    return dot(float4(0.5), easing);
#else
    constexpr float phase_scale = 1.57079632679;
    float time_phase = uniforms.speed * uniforms.time + flow_phase;
    float wave = sin(fract(time_phase / phase_scale) * phase_scale) * 0.498 + 0.5;
    return mix(
        1.0 - pow(1.0 - wave, friction.x),
        pow(wave, friction.y),
        step(0.0, cos(time_phase))
    );
#endif
}

fragment float4 phase10_effect_fragment(
    Phase10EffectVertexOut stage_vertex [[stage_in]],
    texture2d<float> input_texture [[texture(0)]],
    texture2d<float> aux_texture [[texture(1)]],
    texture2d<float> aux2_texture [[texture(2)]],
    texture2d<float> aux3_texture [[texture(3)]],
    constant Phase10EffectUniforms& uniforms [[buffer(0)]]
) {
    constexpr sampler texture_sampler(address::clamp_to_edge, mag_filter::linear, min_filter::linear);
    float2 uv = stage_vertex.uv;
    float4 sampled = sample_input(input_texture, texture_sampler, uv);

#if PHASE10_EFFECT_PULSE
    float phase = uniforms.user0.x;
    float power = uniforms.user0.y == 0.0 ? 1.0 : max(abs(uniforms.user0.y), 0.001);
    float pulse = sin(uniforms.time * max(uniforms.speed, 0.001) + (phase - 0.25) * 6.28318530718) * 0.5 + 0.5;
    pulse = pow(clamp(pulse, 0.0, 1.0), power) * max(uniforms.intensity, 0.0);
#if PULSEALPHA
    sampled.a *= clamp(pulse, 0.0, 1.0);
#endif
#if PULSECOLOR
    if (any(abs(uniforms.color.rgb - float3(1.0)) > float3(0.001))) {
        sampled.rgb = apply_tint_blend(sampled.rgb, uniforms.color.rgb, pulse);
    }
#endif
#elif PHASE10_EFFECT_SHAKE
    float flow_phase = 0.0;
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        flow_phase = aux2_texture.sample(texture_sampler, clamp(uv, float2(0.0), float2(1.0))).r * 1.57079632679;
    }
    float2 flow_mask = float2(0.0);
    if (has_aux_texture(uniforms.aux_texel_size)) {
        float2 flow_colors = aux_texture.sample(texture_sampler, clamp(uv, float2(0.0), float2(1.0))).rg;
        flow_mask = (flow_colors - float2(0.498)) * 2.0;
    }
    float2 bounds = uniforms.user0.xy;
    float bounds_span = abs(bounds.y - bounds.x);
    if (bounds_span <= 0.0001) {
        bounds = float2(0.0, 1.0);
        bounds_span = 1.0;
    }
    float offset = phase10_shake_wave(uniforms, flow_phase);
    offset = clamp((offset - bounds.x) / bounds_span, 0.0, 1.0);
#if DIRECTION == 0
    offset = offset * 2.0 - 1.0;
#elif DIRECTION == 2
    offset = offset - 1.0;
#endif
    float2 texCoordOffset = offset * uniforms.intensity * uniforms.intensity * flow_mask;
    float4 shaken = sample_input(input_texture, texture_sampler, uv + texCoordOffset);
    if (has_aux_texture(uniforms.aux3_texel_size)) {
        float mask = aux3_texture.sample(texture_sampler, clamp(uv + texCoordOffset, float2(0.0), float2(1.0))).r;
        sampled = mix(sampled, shaken, mask);
    } else {
        sampled = shaken;
    }
#elif PHASE10_EFFECT_WATERRIPPLE
    if (has_aux_texture(uniforms.aux_texel_size)) {
        float2 ripple_uv = uv + float2(uniforms.time * uniforms.speed * 0.05, -uniforms.time * uniforms.speed * 0.03);
        float2 ripple = aux_texture.sample(texture_sampler, fract(ripple_uv)).rg * 2.0 - 1.0;
        sampled = sample_input(
            input_texture,
            texture_sampler,
            uv + ripple * uniforms.intensity * max(uniforms.texel_size, float2(0.0001))
        );
    }
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
