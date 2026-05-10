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
    float2 primary_uv;
    float2 slot1_uv;
    float2 slot2_uv;
    float2 slot3_uv;
    float4 color;
};

struct Phase10EffectUniforms {
    float4 color;
    float4 user0;
    float4 user1;
    float4 primary_resolution;
    float4 slot1_resolution;
    float4 slot2_resolution;
    float4 slot3_resolution;
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
    constant Phase10EffectUniforms& uniforms [[buffer(1)]],
    uint vertex_id [[vertex_id]]
) {
    Phase10EffectVertexIn input_vertex = vertices[vertex_id];
    Phase10EffectVertexOut out_vertex;
    out_vertex.position = float4(float2(input_vertex.position), 0.0, 1.0);
    float2 base_uv = float2(input_vertex.uv);
    float2 primary_scale = clamp(
        uniforms.primary_resolution.zw / max(uniforms.primary_resolution.xy, float2(1.0)),
        float2(0.0),
        float2(1.0)
    );
    float2 slot1_scale = clamp(
        uniforms.slot1_resolution.zw / max(uniforms.slot1_resolution.xy, float2(1.0)),
        float2(0.0),
        float2(1.0)
    );
    float2 slot2_scale = clamp(
        uniforms.slot2_resolution.zw / max(uniforms.slot2_resolution.xy, float2(1.0)),
        float2(0.0),
        float2(1.0)
    );
    float2 slot3_scale = clamp(
        uniforms.slot3_resolution.zw / max(uniforms.slot3_resolution.xy, float2(1.0)),
        float2(0.0),
        float2(1.0)
    );
    out_vertex.primary_uv = base_uv * primary_scale;
    out_vertex.slot1_uv = base_uv * slot1_scale;
    out_vertex.slot2_uv = base_uv * slot2_scale;
    out_vertex.slot3_uv = base_uv * slot3_scale;
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

static float2 phase10_content_size(float4 resolution) {
    return max(resolution.zw, float2(1.0));
}

static float2 phase10_offset_between_texture_spaces(
    float2 offset,
    float4 source_resolution,
    float4 target_resolution
) {
    return offset * phase10_content_size(source_resolution) / phase10_content_size(target_resolution);
}

static float4 sample_input(
    texture2d<float> input_texture,
    sampler texture_sampler,
    float2 uv
) {
    return input_texture.sample(texture_sampler, clamp(uv, float2(0.0), float2(1.0)));
}

static bool uv_inside_unit(float2 uv) {
    return all(uv >= float2(0.0)) && all(uv <= float2(1.0));
}

static float4 sample_displaced_input(
    texture2d<float> input_texture,
    sampler texture_sampler,
    float2 base_uv,
    float2 offset
) {
    float2 displaced_uv = base_uv + offset;
    if (!uv_inside_unit(displaced_uv)) {
        return sample_input(input_texture, texture_sampler, base_uv);
    }
    return sample_input(input_texture, texture_sampler, displaced_uv);
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
    constexpr float phase_scale = 6.28318530718;
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
    constexpr float phase_scale = 6.28318530718;
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
    float2 primary_uv = stage_vertex.primary_uv;
    float4 sampled = sample_input(input_texture, texture_sampler, primary_uv);

#if PHASE10_EFFECT_PULSE
    float4 original = sampled;
    float thresholds_low = uniforms.user0.z;
    float thresholds_high = uniforms.user0.w;
    if (abs(thresholds_high - thresholds_low) <= 0.0001) {
        thresholds_low = 0.0;
        thresholds_high = 1.0;
    }
    float phase = uniforms.user0.x;
    float power = max(abs(uniforms.user0.y), 0.001);
    float wave = sin(uniforms.time * max(uniforms.speed, 0.001) + (phase - 0.25) * 6.28318530718) * 0.5 + 0.5;
    float pulse = smoothstep(thresholds_low, thresholds_high, wave) * max(uniforms.intensity, 0.0);
    if (has_aux_texture(uniforms.aux_texel_size)) {
        float2 noise_uv = fract(float2(uniforms.time * 0.08333333, uniforms.time * 0.02777777) * max(uniforms.radius, 0.0));
        float noise = aux_texture.sample(texture_sampler, noise_uv).r * uniforms.angle;
        pulse += noise;
    }
    pulse = pow(max(pulse, 0.0), power);
#if PULSEALPHA
    sampled.a *= clamp(pulse, 0.0, 1.0);
#endif
#if PULSECOLOR
    sampled.rgb = apply_tint_blend(sampled.rgb * uniforms.color.rgb, sampled.rgb * uniforms.user1.rgb, pulse);
#endif
#if MASK
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        float mask = aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r;
        sampled = mix(original, sampled, mask);
    }
#endif
#elif PHASE10_EFFECT_SHAKE
    float flow_phase = 0.0;
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        flow_phase = aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r * 6.28318530718;
    }
    float2 flow_mask = float2(0.0);
    if (has_aux_texture(uniforms.aux_texel_size)) {
        float2 flow_colors = aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).rg;
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
    float4 shaken = sample_displaced_input(input_texture, texture_sampler, primary_uv, texCoordOffset);
    if (has_aux_texture(uniforms.aux3_texel_size)) {
        float2 mask_uv = stage_vertex.slot3_uv + phase10_offset_between_texture_spaces(
            texCoordOffset,
            uniforms.primary_resolution,
            uniforms.slot3_resolution
        );
        float mask = aux3_texture.sample(texture_sampler, clamp(mask_uv, float2(0.0), float2(1.0))).r;
        sampled = mix(sampled, shaken, mask);
    } else {
        sampled = shaken;
    }
#elif PHASE10_EFFECT_WATERRIPPLE
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        mask = aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        float animation = uniforms.time * uniforms.speed * uniforms.speed;
        float scroll_speed = uniforms.user0.x;
        float ratio = max(abs(uniforms.user0.y), 0.01);
        float scale = max(abs(uniforms.radius), 0.01);
        float2 scroll = rotate2d(float2(0.0, 1.0), uniforms.angle) * scroll_speed * scroll_speed * uniforms.time;
        float ripple_texture_adjustment = uniforms.texel_size.x > 0.0
            ? uniforms.texel_size.y / uniforms.texel_size.x
            : 1.0;

        float4 ripple_uv = float4(stage_vertex.slot2_uv, stage_vertex.slot2_uv * 1.333);
        ripple_uv.xy = ripple_uv.xy + animation + scroll;
        ripple_uv.zw = ripple_uv.zw - animation + scroll;
        ripple_uv *= scale;
        ripple_uv.xz *= ripple_texture_adjustment;
        ripple_uv.yw *= ratio;

        float3 n1 = aux2_texture.sample(texture_sampler, fract(ripple_uv.xy)).xyz * 2.0 - 1.0;
        float3 n2 = aux2_texture.sample(texture_sampler, fract(ripple_uv.zw)).xyz * 2.0 - 1.0;
        float3 normal = normalize(float3(n1.xy + n2.xy, max(n1.z, 0.0001)));
        sampled = sample_displaced_input(
            input_texture,
            texture_sampler,
            primary_uv,
            normal.xy * uniforms.intensity * uniforms.intensity * mask
        );
    }
#elif PHASE10_EFFECT_WATERWAVES
    float mask = aux_red_mask(aux_texture, texture_sampler, stage_vertex.slot1_uv, uniforms.aux_texel_size);
    float2 direction = uniforms.user0.xy;
    if (length(direction) < 0.0001) {
        direction = float2(-sin(uniforms.angle), cos(uniforms.angle));
    }
    direction = normalize(direction);
    float scale = uniforms.user0.z == 0.0 ? 200.0 : max(abs(uniforms.user0.z), 0.01);
    float exponent = uniforms.user0.w == 0.0 ? 1.0 : max(abs(uniforms.user0.w), 0.51);
    float time_offset = 0.0;
#if TIMEOFFSET
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        time_offset = aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r * 1.57079632679;
    }
#endif
    float distance = (uniforms.time + time_offset) * max(uniforms.speed, 0.001) + dot(primary_uv, direction) * scale;
    float wave = sin(distance);
    float signed_wave = sign(wave) * pow(abs(wave), exponent);
    float strength = max(uniforms.intensity, 0.0);
    float safe_amplitude = max(max(uniforms.texel_size.x, uniforms.texel_size.y) * 4.0, 0.0001);
    float displacement = min(strength * strength, safe_amplitude) * mask;
    float2 offset = float2(direction.y, -direction.x) * signed_wave * displacement;
    float4 displaced = sample_displaced_input(input_texture, texture_sampler, primary_uv, offset);
    float coverage = smoothstep(0.02, 0.15, sampled.a);
    sampled = mix(sampled, displaced, coverage);
#elif PHASE10_EFFECT_TINT
    float mask = aux_red_mask(aux_texture, texture_sampler, stage_vertex.slot1_uv, uniforms.aux_texel_size);
    float strength = clamp(uniforms.intensity * mask, 0.0, 1.0);
    sampled.rgb = apply_tint_blend(sampled.rgb, uniforms.color.rgb, strength);
#elif PHASE10_EFFECT_SCROLL
    float2 scroll_speed = uniforms.user0.xy;
    float2 repeat = max(abs(uniforms.user0.zw), float2(0.01));
    float2 signed_scroll = sign(scroll_speed) * pow(abs(scroll_speed), float2(2.0)) * uniforms.time;
    sampled = input_texture.sample(
        texture_sampler,
        fract((primary_uv + signed_scroll) * repeat)
    );
#elif PHASE10_EFFECT_LIGHTSHAFTS
    float2 fx_coord = primary_uv;
    float mask = 1.0;
    float2 feather = max(abs(uniforms.user0.zw), float2(0.0001));
    float ray_radius = clamp(uniforms.radius, 0.0, 1.0);
    float ray_mode = 0.0;
#if RAYMODE == 1
    ray_mode = 1.0;
#elif RAYMODE == 2
    ray_mode = 2.0;
#endif
    if (ray_mode == 1.0) {
        float2 delta = fx_coord - float2(0.5);
        fx_coord.x = atan2(delta.y, delta.x) / 6.28318530718 + 0.5;
        fx_coord.y = length(delta) * 2.0;
        fx_coord.y = smoothstep(ray_radius, 1.0, fx_coord.y);
        fx_coord.y = (fx_coord.y - 0.0001) * 1.00021;
    } else if (ray_mode == 2.0) {
        float2 delta = fx_coord;
        fx_coord.x = atan2(delta.y, delta.x) / 6.28318530718 * 4.0;
        fx_coord.y = max(delta.x, delta.y);
        float noise_bias = 0.0;
        if (has_aux_texture(uniforms.aux_texel_size)) {
            noise_bias = aux_texture.sample(
                texture_sampler,
                float2(fx_coord.x * 0.054111 * max(uniforms.user1.z, 0.0001), 0.0)
            ).r * uniforms.user1.y - (uniforms.user1.y * 0.5);
        }
        fx_coord.y += noise_bias;
        fx_coord.y = smoothstep(ray_radius, 1.0, fx_coord.y);
    }
    mask *= smoothstep(0.50001, 0.5 - feather.x, abs(fx_coord.x - 0.5));
    mask *= smoothstep(0.50001, 0.5 - feather.y, abs(fx_coord.y - 0.5));
    float grad = 1.0 - fx_coord.y;
    mask *= grad;
    float2 shape_scale = max(abs(uniforms.user0.xy), float2(0.01));
    float2 fx_coord2 = fx_coord;
    fx_coord.xy *= float2(0.054111 * shape_scale.x, 0.003111 * shape_scale.y);
    fx_coord2.xy *= float2(0.07333 * shape_scale.x, 0.005967111 * shape_scale.y);
    fx_coord.xy += uniforms.time * uniforms.speed * float2(0.003, 0.000375111);
    fx_coord2.xy -= uniforms.time * uniforms.speed * float2(0.0047111, 0.0007399);
    float fx0 = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fx_coord).r
        : 1.0;
    float fx1 = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fx_coord2).r
        : 1.0;
    float fx = pow(max(fx0 * fx1, 0.0), max(uniforms.user1.w, 0.0001));
    float smoothness = clamp(uniforms.user1.x, 0.1, 1.0);
    fx = smoothstep((1.0 - smoothness) * 0.29999, 0.3 + smoothness * 0.7, fx);
    float3 fx_color = uniforms.color.rgb * uniforms.intensity;
#if RENDERING == 1
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        fx_color = aux2_texture.sample(texture_sampler, float2(clamp(fx_coord.y, 0.0, 1.0), 0.0)).rgb
            * uniforms.intensity;
    }
#endif
    fx *= mask;
    sampled.rgb = apply_tint_blend(sampled.rgb, fx_color, fx);
    sampled.a = max(sampled.a, fx);
#if WRITEALPHA
    sampled.a = fx;
#endif
#elif PHASE10_EFFECT_FOLIAGESWAY
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        mask *= aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float2 offset = float2(0.0);
    if (mask > 0.0) {
        float noise = has_aux_texture(uniforms.aux2_texel_size)
            ? aux2_texture.sample(texture_sampler, stage_vertex.slot2_uv).g
            : primary_uv.y;
        float phase = uniforms.user0.x;
        float wave = sin((noise * 6.28318530718 + primary_uv.x * 10.0 + primary_uv.y * 5.0) * max(phase, 0.01)
            + uniforms.speed * uniforms.time);
        float signed_wave = sign(wave) * pow(abs(wave), max(uniforms.user0.y, 0.01));
        float amplitude = clamp(uniforms.intensity, 0.0, 2.0) * 0.05 * mask;
        offset.x = signed_wave * amplitude * (primary_uv.y - 0.5);
        offset.y = signed_wave * amplitude * 0.25 * (0.5 - abs(primary_uv.x - 0.5));
    }
    sampled = sample_displaced_input(input_texture, texture_sampler, primary_uv, offset);
#elif PHASE10_EFFECT_CIRCLE
    float2 center_delta = primary_uv - float2(0.5, 0.5);
    sampled.a *= smoothstep(0.5, 0.49, length(center_delta));
#endif

    sampled *= stage_vertex.color;
    return saturate(sampled);
}
