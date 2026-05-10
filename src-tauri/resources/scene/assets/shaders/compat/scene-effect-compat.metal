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
    float2 slot4_uv;
    float2 slot5_uv;
    float2 slot6_uv;
    float2 slot7_uv;
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
    float4 slot4_resolution;
    float4 slot5_resolution;
    float4 slot6_resolution;
    float4 slot7_resolution;
    float2 texel_size;
    float2 aux_texel_size;
    float2 aux2_texel_size;
    float2 aux3_texel_size;
    float2 aux4_texel_size;
    float2 aux5_texel_size;
    float2 aux6_texel_size;
    float2 aux7_texel_size;
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
    float2 slot4_scale = clamp(
        uniforms.slot4_resolution.zw / max(uniforms.slot4_resolution.xy, float2(1.0)),
        float2(0.0),
        float2(1.0)
    );
    float2 slot5_scale = clamp(
        uniforms.slot5_resolution.zw / max(uniforms.slot5_resolution.xy, float2(1.0)),
        float2(0.0),
        float2(1.0)
    );
    float2 slot6_scale = clamp(
        uniforms.slot6_resolution.zw / max(uniforms.slot6_resolution.xy, float2(1.0)),
        float2(0.0),
        float2(1.0)
    );
    float2 slot7_scale = clamp(
        uniforms.slot7_resolution.zw / max(uniforms.slot7_resolution.xy, float2(1.0)),
        float2(0.0),
        float2(1.0)
    );
    out_vertex.primary_uv = base_uv * primary_scale;
    out_vertex.slot1_uv = base_uv * slot1_scale;
    out_vertex.slot2_uv = base_uv * slot2_scale;
    out_vertex.slot3_uv = base_uv * slot3_scale;
    out_vertex.slot4_uv = base_uv * slot4_scale;
    out_vertex.slot5_uv = base_uv * slot5_scale;
    out_vertex.slot6_uv = base_uv * slot6_scale;
    out_vertex.slot7_uv = base_uv * slot7_scale;
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

static float2 inverse_bilinear_uv(
    float2 point,
    float2 p0,
    float2 p1,
    float2 p2,
    float2 p3
) {
    float2 uv = point;
    for (int iteration = 0; iteration < 8; iteration++) {
        float2 a = mix(p0, p1, uv.x);
        float2 b = mix(p3, p2, uv.x);
        float2 mapped = mix(a, b, uv.y);
        float2 error = mapped - point;
        if (length_squared(error) < 1e-10) {
            break;
        }

        float2 d_du = mix(p1 - p0, p2 - p3, uv.y);
        float2 d_dv = mix(p3 - p0, p2 - p1, uv.x);
        float determinant = d_du.x * d_dv.y - d_du.y * d_dv.x;
        if (fabs(determinant) < 1e-8) {
            return point;
        }

        float2 delta = float2(
            (error.x * d_dv.y - error.y * d_dv.x) / determinant,
            (-error.x * d_du.y + error.y * d_du.x) / determinant
        );
        uv -= delta;
        uv = clamp(uv, float2(-2.0), float2(3.0));
    }
    return uv;
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
    texture2d<float> aux4_texture [[texture(4)]],
    texture2d<float> aux5_texture [[texture(5)]],
    texture2d<float> aux6_texture [[texture(6)]],
    texture2d<float> aux7_texture [[texture(7)]],
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
#elif PHASE10_EFFECT_OPACITY
    float mask = aux_red_mask(aux_texture, texture_sampler, stage_vertex.slot1_uv, uniforms.aux_texel_size);
    sampled.a *= clamp(uniforms.intensity, 0.0, 1.0) * mask;
#elif PHASE10_EFFECT_TRANSFORM
    sampled = sample_input(input_texture, texture_sampler, fract(primary_uv));
#elif PHASE10_EFFECT_SKEW
    float skew_x = uniforms.user0.x;
    float skew_y = uniforms.user0.y;
    float2 anchor = uniforms.user0.zw;
    float2 local = primary_uv - anchor;
    float determinant = 1.0 - skew_x * skew_y;
    float2 skew_uv = primary_uv;
    if (fabs(determinant) >= 1e-6) {
        skew_uv = float2(
            (local.x - skew_x * local.y) / determinant,
            (local.y - skew_y * local.x) / determinant
        ) + anchor;
    }
#if REPEAT
    skew_uv = fract(skew_uv);
#else
    skew_uv = clamp(skew_uv, float2(0.0), float2(1.0));
#endif
    sampled = sample_input(input_texture, texture_sampler, skew_uv);
#elif PHASE10_EFFECT_PERSPECTIVE
    float mask = step(0.0, stage_vertex.position.w);
    float2 p0 = uniforms.user0.xy;
    float2 p1 = uniforms.user0.zw;
    float2 p2 = uniforms.user1.xy;
    float2 p3 = uniforms.user1.zw;
    float2 perspective_uv = inverse_bilinear_uv(primary_uv, p0, p1, p2, p3);
#if REPEAT
    perspective_uv = fract(perspective_uv);
#else
    if (!uv_inside_unit(perspective_uv)) {
        mask = 0.0;
    }
#endif
    sampled = sample_input(input_texture, texture_sampler, perspective_uv);
    sampled.a *= mask;
#elif PHASE10_EFFECT_SPIN
    float2 center = uniforms.user0.xy;
    float size = max(uniforms.user0.z, 0.0001);
    float feather = max(uniforms.user0.w, 0.0001);
    float2 tex_coord = primary_uv;
#if REPEAT
    tex_coord = fract(tex_coord);
#endif
    float2 mask_delta = primary_uv - center;
    float mask = smoothstep(size + feather + 0.00001, size - feather, length(mask_delta));
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        mask *= aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float anim = uniforms.angle * sin(uniforms.time * max(uniforms.speed, 0.001));
    tex_coord -= center;
    tex_coord = rotate2d(tex_coord, anim);
    tex_coord += center;
    float4 rotated = sample_input(input_texture, texture_sampler, tex_coord);
    float4 original = sample_input(input_texture, texture_sampler, primary_uv);
    sampled = mix(original, rotated, mask);
#elif PHASE10_EFFECT_SWING
    float2 p0 = uniforms.user0.xy;
    float2 p1 = uniforms.user0.zw;
    float2 axis_delta = p1 - p0;
    float axis_length = max(length(axis_delta), 0.0001);
    float2 axis = axis_delta / axis_length;
    float2 axis_ortho = float2(-axis.y, axis.x);
    float2 center = mix(p0, p1, clamp(uniforms.angle, 0.0, 1.0));
    float2 uv_delta = primary_uv - center;
    float distance_along_axis = dot(axis, uv_delta);
    float distance_ortho = dot(axis_ortho, uv_delta);
    float anim = sin(uniforms.time * max(uniforms.speed, 0.001) + distance_along_axis * 6.28318530718);
#if NOISE
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        float noise = aux2_texture.sample(
            texture_sampler,
            fract(float2(uniforms.time * 0.08333333, uniforms.time * 0.02777777) * max(uniforms.user1.y, 0.001))
        ).r * 6.28318530718;
        anim = clamp(anim + sin(noise) * uniforms.user1.z, -1.0, 1.0);
    }
#endif
    float size_mod = max(uniforms.radius * (1.0 - abs(anim) * uniforms.intensity * 0.5), 0.0001);
    float feather = max(uniforms.user1.x, 0.00001);
    float distance_right = dot(primary_uv - p1, axis);
    float distance_left = dot(primary_uv - p0, axis);
    float mask = smoothstep(feather, 0.0, distance_right) * smoothstep(-feather, 0.0, distance_left);
    mask *= smoothstep(size_mod + feather, size_mod - feather, distance_ortho);
#if DOUBLESIDED
    mask *= smoothstep(size_mod + feather, size_mod - feather, -distance_ortho);
#else
    mask *= step(0.0, distance_ortho);
#endif
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        mask *= aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float2 uv_distort = axis * anim * distance_ortho * distance_along_axis * uniforms.intensity;
    uv_distort += axis_ortho * anim * anim * distance_ortho * uniforms.intensity * 0.5;
    sampled = sample_input(input_texture, texture_sampler, mix(primary_uv, primary_uv + uv_distort, mask));
#elif PHASE10_EFFECT_TWIRL
    float2 center = uniforms.user0.xy;
    float2 tex_coord = primary_uv - center;
#if ELLIPTICAL
    tex_coord = rotate2d(tex_coord, uniforms.angle);
    tex_coord.x *= max(uniforms.user0.z, 0.0001);
#endif
    float dist = length(tex_coord);
    float feather = smoothstep(uniforms.radius + uniforms.user0.w + 0.00001, uniforms.radius - uniforms.user0.w, dist);
#if INNER
    float falloff = uniforms.radius / max(dist, 0.0001);
#else
    float falloff = dist / max(uniforms.radius, 0.0001);
#endif
    float anim = uniforms.intensity * sin(uniforms.time * max(uniforms.speed, 0.001)) * falloff;
#if NOISE
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        float noise = aux2_texture.sample(
            texture_sampler,
            fract(float2(uniforms.time * 0.08333333, uniforms.time * 0.02777777) * max(uniforms.user1.x, 0.001))
        ).r * 6.28318530718;
        anim += sin(noise) * uniforms.user1.y * falloff;
    }
#endif
    tex_coord = rotate2d(tex_coord, anim);
#if ELLIPTICAL
    tex_coord.x /= max(uniforms.user0.z, 0.0001);
    tex_coord = rotate2d(tex_coord, -uniforms.angle);
#endif
    tex_coord += center;
#if REPEAT
    tex_coord = fract(tex_coord);
#endif
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        mask *= aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    sampled = mix(sample_input(input_texture, texture_sampler, primary_uv), sample_input(input_texture, texture_sampler, tex_coord), feather * mask);
#elif PHASE10_EFFECT_CHROMATIC_ABERRATION
    float2 center = uniforms.user0.xy;
    float center_falloff = clamp(uniforms.user0.z, 0.0, 1.0);
    float strength = uniforms.user0.w;
    float2 delta = primary_uv - center;
    float2 coords0 = primary_uv;
    float2 coords1 = primary_uv;
#if MODE == 0
    float falloff = mix(0.5 / (length(delta) + 0.0001), 1.0, center_falloff);
    delta *= strength * 0.01 * falloff;
    coords0 = primary_uv + delta;
    coords1 = primary_uv - delta;
#elif MODE == 1
    float2 direction = float2(-sin(uniforms.angle), cos(uniforms.angle));
    float falloff = mix(1.0, abs(dot(direction, delta)) * 2.0, center_falloff);
    direction *= strength * 0.01 * falloff;
    coords0 = primary_uv + direction;
    coords1 = primary_uv - direction;
#elif MODE == 2
    float falloff = mix(0.5 / (length(delta) + 0.0001), 1.0, center_falloff);
    float amt = strength * 0.01 * falloff;
    coords0 = center + rotate2d(delta, amt);
    coords1 = center + rotate2d(delta, -amt);
#elif MODE == 3
    float2 ref_coords = primary_uv;
    ref_coords -= float2(0.5);
    ref_coords *= float2(1.0 - strength * 0.0125);
    ref_coords += float2(0.5);
    float2 centered0 = ref_coords * 2.0 - 1.0;
    float v0 = dot(centered0, centered0);
    coords0 = (centered0 * (1.0 + strength * 0.05 * v0)) * 0.5 + 0.5;
    float2 centered1 = ref_coords * 2.0 - 1.0;
    float v1 = dot(centered1, centered1);
    coords1 = (centered1 * (1.0 - strength * 0.02 * v1)) * 0.5 + 0.5;
#endif
    float4 sc = sample_input(input_texture, texture_sampler, primary_uv);
    float4 s0 = sample_input(input_texture, texture_sampler, coords0);
    float4 s1 = sample_input(input_texture, texture_sampler, coords1);
    sampled = sc;
#if VARIATION == 0
    sampled.r = s0.r;
    sampled.b = s1.b;
#elif VARIATION == 1
    sampled.g = s1.g;
    sampled.b = s0.b;
#elif VARIATION == 2
    sampled.g = s0.g;
    sampled.r = s1.r;
#endif
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        float mask = aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
        sampled = mix(sc, sampled, mask);
    }
#endif
#elif PHASE10_EFFECT_COLORKEY
    float delta = dot(abs(uniforms.color.rgb - sampled.rgb), float3(1.0, 1.0, 1.0));
    float blend = smoothstep(0.001, 0.002 + max(uniforms.radius, 0.0), delta - uniforms.angle);
#if INVERT == 1
    blend = 1.0 - blend;
#endif
    sampled.a *= mix(clamp(uniforms.intensity, 0.0, 1.0), 1.0, blend);
#if FLATTEN == 1
    sampled.rgb *= sampled.a;
#endif
#elif PHASE10_EFFECT_FISHEYE
    float2 center = uniforms.user0.xy;
    float size = max(uniforms.user0.z, 0.01);
    float scale = uniforms.user0.w;
    constexpr float aperture = 178.0;
    float aperture_half = 0.5 * aperture * (3.14159265359 / 180.0);
    float max_factor = sin(aperture_half);
    float2 xy = (primary_uv - center) * 2.0 / size;
    float d = length(xy);
    float alpha = 1.0;
    float2 uv = primary_uv;
    if (d < (2.0 - max_factor)) {
        d = length(xy * max_factor);
        float z = sqrt(max(1.0 - d * d, 0.0001));
        float r = atan2(d, z) / 3.14159265359;
        float phi = atan2(xy.y, xy.x);
        uv.x = r * cos(phi) * size + center.x;
        uv.y = r * sin(phi) * size + center.y;
    } else {
#if BACKGROUND == 0
        alpha = 0.0;
#endif
    }
    sampled = sample_input(input_texture, texture_sampler, mix(primary_uv, uv, scale));
    sampled.a *= alpha;
#elif PHASE10_EFFECT_EDGEDETECTION
    float2 px = uniforms.texel_size;
    float3 sample00 = sample_input(input_texture, texture_sampler, primary_uv + float2(-px.x, -px.y)).rgb;
    float3 sample10 = sample_input(input_texture, texture_sampler, primary_uv + float2(0.0, -px.y)).rgb;
    float3 sample20 = sample_input(input_texture, texture_sampler, primary_uv + float2(px.x, -px.y)).rgb;
    float3 sample01 = sample_input(input_texture, texture_sampler, primary_uv + float2(-px.x, 0.0)).rgb;
    float3 sample21 = sample_input(input_texture, texture_sampler, primary_uv + float2(px.x, 0.0)).rgb;
    float3 sample02 = sample_input(input_texture, texture_sampler, primary_uv + float2(-px.x, px.y)).rgb;
    float3 sample12 = sample_input(input_texture, texture_sampler, primary_uv + float2(0.0, px.y)).rgb;
    float3 sample22 = sample_input(input_texture, texture_sampler, primary_uv + float2(px.x, px.y)).rgb;
    float3 gx = sample20 - sample00 + (sample21 - sample01) * 2.0 + sample22 - sample02;
    float3 gy = sample00 - sample02 + (sample10 - sample12) * 2.0 + sample20 - sample22;
    float g = abs(dot(gx, float3(0.299, 0.587, 0.114))) + abs(dot(gy, float3(0.299, 0.587, 0.114)));
    float edge_mix = min(1.0, max(0.0, g - uniforms.radius) * uniforms.angle);
    float3 combined_color = mix(uniforms.user0.rgb, uniforms.color.rgb, edge_mix) * uniforms.speed;
    sampled.rgb = apply_tint_blend(sampled.rgb, combined_color, clamp(uniforms.intensity, 0.0, 1.0));
#elif PHASE10_EFFECT_IRIS
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        mask *= aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float2 offset = (primary_uv - float2(0.5)) * (uniforms.user0.xy / max(uniforms.screen_size, float2(1.0)));
    float4 iris = sample_input(input_texture, texture_sampler, primary_uv + offset * mask);
#if BACKGROUND
    float iris_mask = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv + offset * mask, float2(0.0), float2(1.0))).r
        : 1.0;
    iris.rgb = mix(uniforms.color.rgb, iris.rgb, iris_mask);
#endif
    sampled = iris;
#elif PHASE10_EFFECT_CLOUDMOTION
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        mask *= aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float3 noise = has_aux_texture(uniforms.aux2_texel_size)
        ? aux2_texture.sample(texture_sampler, stage_vertex.slot2_uv).rgb
        : float3(0.5, 0.5, 0.5);
    float2 offset = float2((noise.x * 2.0 - 1.0) * uniforms.intensity * mask, 0.0);
    offset = rotate2d(offset, uniforms.angle + 1.57079632679);
    float2 motion_uv = primary_uv + offset;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        float dst_mask = aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv + offset, float2(0.0), float2(1.0))).r;
        motion_uv = mix(primary_uv, motion_uv, dst_mask);
    }
#endif
    sampled = sample_input(input_texture, texture_sampler, motion_uv);
#elif PHASE10_EFFECT_CLOUDS
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        mask *= aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float cloud = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fract(primary_uv * 1.3 + uniforms.time * uniforms.user1.xy)).r
        : 1.0;
    float threshold = uniforms.radius;
    float feather = max(uniforms.user0.x, 0.0001);
    float blend = smoothstep(threshold - feather, threshold + feather, cloud) * mask * uniforms.intensity;
    float3 end_color = float3(uniforms.user0.z, uniforms.user0.w, uniforms.user1.x);
    float3 cloud_color = mix(uniforms.color.rgb, end_color, cloud);
    sampled.rgb = apply_tint_blend(sampled.rgb, cloud_color, blend);
#if WRITEALPHA
    sampled.a = max(sampled.a, blend);
#endif
#elif PHASE10_EFFECT_WATERFLOW
    float flow_phase = has_aux_texture(uniforms.aux2_texel_size)
        ? aux2_texture.sample(texture_sampler, primary_uv * uniforms.radius).r
        : 0.0;
    float2 flow_colors = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, stage_vertex.slot1_uv).rg
        : float2(0.498, 0.498);
    float2 flow_mask = (flow_colors - float2(0.498, 0.498)) * 2.0;
    float flow_amount = length(flow_mask);
    float phase = fract(uniforms.time * 0.5);
    float2 offset_a = flow_mask * uniforms.intensity * 0.1 * (phase - 0.5);
    float2 offset_b = flow_mask * uniforms.intensity * 0.1 * (fract(phase + 0.5) - 0.5);
    float4 flow_a = sample_input(input_texture, texture_sampler, primary_uv + offset_a);
    float4 flow_b = sample_input(input_texture, texture_sampler, primary_uv + offset_b);
    float4 flow = mix(flow_a, flow_b, smoothstep(0.2, 0.8, flow_phase));
    sampled = mix(sampled, flow, clamp(flow_amount, 0.0, 1.0));
#elif PHASE10_EFFECT_NITRO
    float base_noise = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, stage_vertex.slot1_uv).r
        : 0.0;
    float nitro0 = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fract(stage_vertex.slot1_uv + float2(uniforms.time * 0.05, 0.0))).r
        : 0.0;
    float nitro1 = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fract(stage_vertex.slot1_uv * 1.333 - float2(uniforms.time * 0.03, 0.0))).r
        : 0.0;
    float core = smoothstep(nitro0, nitro1, 0.1 + base_noise * 0.8);
    float low = uniforms.user0.y;
    float high = uniforms.user0.x;
    float nitro = smoothstep(low, high, nitro0 * nitro1) * smoothstep(high, low, nitro0 * nitro1);
    nitro = core * nitro * 4.0;
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        mask *= aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float3 end_color = float3(uniforms.user0.z, uniforms.user0.w, uniforms.user1.x);
    float3 nitro_color = mix(uniforms.color.rgb, end_color, nitro);
    sampled.rgb = apply_tint_blend(sampled.rgb, nitro_color, nitro * uniforms.intensity * mask);
#if WRITEALPHA
    sampled.a = max(sampled.a, nitro * mask);
#endif
#elif PHASE10_EFFECT_BLEND
    float2 blend_uv = stage_vertex.slot1_uv;
#if TRANSFORMUV == 1 && TRANSFORMREPEAT == 1
    blend_uv = fract(blend_uv);
#endif
    float blend = 1.0;
#if OPACITYMASK == 1
    if (has_aux_texture(uniforms.aux7_texel_size)) {
        blend *= aux7_texture.sample(texture_sampler, clamp(stage_vertex.slot7_uv, float2(0.0), float2(1.0))).r;
    }
#endif
#if TRANSFORMUV == 1 && TRANSFORMREPEAT == 0
    blend *= step(0.99, dot(step(float2(0.0), blend_uv) * step(blend_uv, float2(1.0)), float2(0.5)));
#endif
    float4 blend_colors = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, clamp(blend_uv, float2(0.0), float2(1.0)))
        : float4(1.0);
#if WRITEALPHA
    float blend_alpha = blend * uniforms.intensity;
    float new_alpha = sampled.a * (1.0 - blend_alpha) + blend_colors.a * blend_alpha;
    sampled.rgb = sampled.rgb * sampled.a * (1.0 - blend_alpha) + blend_colors.rgb * blend_colors.a * blend_alpha;
    sampled.a = new_alpha * uniforms.radius;
#else
    sampled.rgb = apply_tint_blend(sampled.rgb, blend_colors.rgb, blend * uniforms.intensity * blend_colors.a);
    sampled.a *= uniforms.radius;
#endif
#elif PHASE10_EFFECT_DEPTHPARALLAX
    float depth = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r
        : 0.0;
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        mask *= aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float layers = 1.0;
#if QUALITY == 1
    layers = 24.0;
#elif QUALITY == 2
    layers = 64.0;
#endif
    float layer_factor = mix(1.0, 0.35, min(layers / 64.0, 1.0));
    float2 pointer = (primary_uv - float2(0.5)) * 2.0;
    float2 offset = (depth - uniforms.user0.z) * pointer * uniforms.user0.xy * uniforms.user0.w * 0.04 * layer_factor * mask;
    sampled = sample_input(input_texture, texture_sampler, primary_uv + offset);
#elif PHASE10_EFFECT_REFLECTION
    float mask = aux_red_mask(aux_texture, texture_sampler, stage_vertex.slot1_uv, uniforms.aux_texel_size);
    float2 reflected_uv = float2(primary_uv.x, 1.0 - primary_uv.y);
    float4 reflected = sample_input(input_texture, texture_sampler, reflected_uv);
    sampled.rgb = apply_tint_blend(sampled.rgb, reflected.rgb, mask * uniforms.intensity);
    sampled.a = min(1.0, sampled.a + reflected.a * mask * uniforms.intensity);
#elif PHASE10_EFFECT_SHIMMER
    float mask = aux_red_mask(aux_texture, texture_sampler, stage_vertex.slot1_uv, uniforms.aux_texel_size);
    float offset = 0.0;
#if OFFSET
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        offset += aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r * uniforms.user0.w;
    }
#endif
    float2 shimmer_coord = rotate2d(primary_uv, -uniforms.angle + 1.57079632679) * uniforms.user0.x;
#if MODE == 1
    shimmer_coord.x += uniforms.user0.z + uniforms.user0.y * sin(uniforms.speed * uniforms.time + offset);
#else
    shimmer_coord.x += uniforms.user0.z + uniforms.speed * (uniforms.time + offset);
#endif
    shimmer_coord.x = clamp(fract(shimmer_coord.x / max(uniforms.user0.x * uniforms.radius, 0.0001)) * uniforms.user0.x * uniforms.radius, 0.0, 1.0);
    float3 shimmer_color = has_aux_texture(uniforms.aux3_texel_size)
        ? aux3_texture.sample(texture_sampler, fract(shimmer_coord)).rgb
        : float3(1.0, 1.0, 1.0);
    float3 effect_albedo = shimmer_color * uniforms.color.rgb;
    effect_albedo = apply_tint_blend(sampled.rgb, effect_albedo, 1.0);
    sampled.rgb = mix(sampled.rgb, effect_albedo, mask * max(max(shimmer_color.r, shimmer_color.g), shimmer_color.b) * uniforms.intensity);
#elif PHASE10_EFFECT_FILMGRAIN
    float4 noise_sample = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fract(primary_uv + float2(uniforms.time * 0.011, uniforms.time * 0.017)))
        : float4(1.0);
    float4 noise_sample2 = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fract(primary_uv * 1.333 + float2(uniforms.time * 0.023, uniforms.time * -0.019)))
        : float4(1.0);
    float3 noise = noise_sample.rgb;
    float3 noise2 = noise_sample2.gbr;
#if GREYSCALE
    float grey0 = dot(noise, float3(0.299, 0.587, 0.114));
    float grey1 = dot(noise2, float3(0.299, 0.587, 0.114));
    noise = float3(grey0);
    noise2 = float3(grey1);
#endif
    noise = saturate(noise * noise2);
    noise = pow(noise, float3(max(uniforms.radius, 0.0001)));
    float blend = uniforms.intensity;
#if MASK
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        blend *= aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    sampled.rgb = apply_tint_blend(sampled.rgb, noise, blend);
#elif PHASE10_EFFECT_VHS
    float dblend = sin(uniforms.time);
    dblend = sign(dblend) * pow(abs(max(0.00001, dblend)), 4.0);
    float2 distortion = float2(
        dblend * uniforms.radius * 0.02 *
            smoothstep(0.01 * uniforms.angle, 0.0, abs(fract(uniforms.time * uniforms.speed) - primary_uv.y)),
        0.0
    ) * uniforms.intensity;
    float vhs_blend = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux2_texel_size)) {
        vhs_blend *= aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float noise0 = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fract(primary_uv * max(uniforms.user0.x, 0.01) + float2(uniforms.time * 0.031, 0.0))).r
        : 0.0;
    float noise1 = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fract(primary_uv * 1.777 + float2(0.0, uniforms.time * 0.027))).g
        : 0.0;
    float artifact_alpha = step(0.9, noise0 * pow(max(uniforms.user0.y, 0.0001), 0.2)) * noise1;
    float x_offset = uniforms.intensity * artifact_alpha * uniforms.user0.z * 0.1;
    float4 orig = sample_input(input_texture, texture_sampler, primary_uv + distortion + float2(x_offset * vhs_blend, 0.0));
    float4 shifted = sample_input(input_texture, texture_sampler, primary_uv + distortion - float2(x_offset, 0.0));
    float3 noise = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, fract(primary_uv * 2.5 + float2(uniforms.time * 0.013, uniforms.time * 0.021))).rgb
        : float3(0.5);
#if GREYSCALE
    float grey = dot(noise, float3(0.299, 0.587, 0.114));
    noise = float3(grey);
#endif
    float3 blended = apply_tint_blend(orig.rgb, noise, 0.1);
    blended = mix(blended, 1.0 - blended, artifact_alpha * vhs_blend);
    sampled = mix(orig, float4(mix(blended, shifted.rgb, clamp(uniforms.user0.w * 0.1, 0.0, 1.0)), orig.a), uniforms.intensity * vhs_blend);
#elif PHASE10_EFFECT_BLENDGRADIENT
    float2 blend_uv = stage_vertex.slot1_uv;
#if TRANSFORMUV == 1 && TRANSFORMREPEAT == 1
    blend_uv = fract(blend_uv);
#endif
    float4 blend_colors = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, clamp(blend_uv, float2(0.0), float2(1.0)))
        : float4(1.0);
    float gradient = has_aux_texture(uniforms.aux2_texel_size)
        ? aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r
        : 0.0;
    float blend = smoothstep(saturate(gradient - uniforms.speed), saturate(gradient + uniforms.speed), uniforms.intensity);
#if OPACITYMASK == 1
    if (has_aux_texture(uniforms.aux3_texel_size)) {
        blend *= aux3_texture.sample(texture_sampler, clamp(stage_vertex.slot3_uv, float2(0.0), float2(1.0))).r;
    }
#endif
#if WRITEALPHA
    float new_alpha = sampled.a * (1.0 - blend) + blend_colors.a * blend * uniforms.radius;
    sampled.rgb = sampled.rgb * sampled.a * (1.0 - blend) + blend_colors.rgb * blend_colors.a * blend;
    sampled.a = new_alpha;
#else
    sampled.rgb = apply_tint_blend(sampled.rgb, blend_colors.rgb, blend * uniforms.radius);
#endif
#if EDGEGLOW
    float burn_width = uniforms.speed * 0.5;
    float burn_amount = step(gradient - burn_width, uniforms.intensity) *
        step(uniforms.intensity, gradient + burn_width) *
        step(0.01, uniforms.intensity) *
        step(uniforms.intensity, 0.999);
    sampled.rgb = max(float3(0.0), mix(sampled.rgb, uniforms.color.rgb, burn_amount * uniforms.angle));
#endif
#elif PHASE10_EFFECT_WATERCAUSTICS
    float mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        mask *= aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float2 caustics_coords = stage_vertex.slot2_uv;
    float ratio = uniforms.primary_resolution.x / max(uniforms.primary_resolution.y, 1.0);
    caustics_coords.x *= ratio;
    caustics_coords *= max(uniforms.radius, 0.1);
    float time = uniforms.time * uniforms.speed + uniforms.user0.w;
    float2 noise_coords = caustics_coords * 0.02 + float2(time * 0.005, time * 0.004111);
    float2 blend_coords = caustics_coords * 0.01333 + float2(time * 0.003777);
    float2 shift_coords = caustics_coords * 0.05 + float2(time * 0.01);
    float4 shift_color = has_aux_texture(uniforms.aux4_texel_size)
        ? aux4_texture.sample(texture_sampler, fract(stage_vertex.slot4_uv + shift_coords)) * 2.0 - 1.0
        : float4(0.0);
    float4 noise_color = has_aux_texture(uniforms.aux3_texel_size)
        ? aux3_texture.sample(texture_sampler, fract(stage_vertex.slot3_uv + noise_coords)) * 2.0 - 1.0
        : float4(0.0);
    float4 noise_color2 = has_aux_texture(uniforms.aux3_texel_size)
        ? aux3_texture.sample(texture_sampler, fract(stage_vertex.slot3_uv + noise_coords * 1.666)) * 2.0 - 1.0
        : float4(0.0);
    caustics_coords += noise_color.xy * 0.025 * uniforms.user0.x;
    caustics_coords += noise_color2.xy * 0.025 * uniforms.user0.x;
    caustics_coords += shift_color.rg * uniforms.user0.x;
    float2 left_coords = caustics_coords;
    float2 right_coords = caustics_coords;
    left_coords.x -= 0.01 * uniforms.user0.y;
    right_coords.x += 0.01 * uniforms.user0.y;
    float3 caustics = float3(
        has_aux_texture(uniforms.aux2_texel_size) ? aux2_texture.sample(texture_sampler, fract(left_coords)).r : 0.0,
        has_aux_texture(uniforms.aux2_texel_size) ? aux2_texture.sample(texture_sampler, fract(caustics_coords)).r : 0.0,
        has_aux_texture(uniforms.aux2_texel_size) ? aux2_texture.sample(texture_sampler, fract(right_coords)).r : 0.0
    );
    float glow_sample = has_aux_texture(uniforms.aux5_texel_size)
        ? aux5_texture.sample(texture_sampler, fract(stage_vertex.slot5_uv + caustics_coords)).r
        : caustics.g;
    float blend_color = has_aux_texture(uniforms.aux3_texel_size)
        ? aux3_texture.sample(texture_sampler, fract(stage_vertex.slot3_uv + blend_coords)).r
        : 0.5;
    caustics = mix(caustics, float3(glow_sample), uniforms.user0.z);
#if MODE == 1
    float caustics_sample = saturate(caustics.g + glow_sample * uniforms.angle);
    float3 caustics_color = uniforms.intensity * mix(uniforms.color.rgb, uniforms.user1.rgb, smoothstep(0.0, 0.5, blend_color));
#else
    float caustics_sample = smoothstep(blend_color * 0.8, 1.0 - blend_color * 0.2, dot(caustics, float3(0.33333)) + glow_sample * uniforms.angle);
    float3 caustics_color = uniforms.intensity * mix(uniforms.color.rgb, uniforms.user1.rgb, blend_color);
    caustics_color *= caustics;
#endif
    sampled.rgb = apply_tint_blend(sampled.rgb, caustics_color, mask * caustics_sample);
#elif PHASE10_EFFECT_FIRE
    float2 flow_colors = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).rg
        : float2(0.498, 0.498);
    float2 flow_mask = (flow_colors - float2(0.498, 0.498)) * 2.0;
    float scaled_time = uniforms.time * uniforms.speed;
    float cycle0 = fract(scaled_time);
    float cycle1 = fract(scaled_time + 0.5);
    float blend = 2.0 * abs(cycle0 - 0.5);
    float cloud_scale = max(uniforms.user0.x, 0.01);
    float2 flow_uv_offset0 = cloud_scale * flow_mask * 0.15 * (cycle0 - 0.5);
    float2 flow_uv_offset1 = cloud_scale * flow_mask * 0.15 * (cycle1 - 0.5);
    float cloud_background = has_aux_texture(uniforms.aux2_texel_size)
        ? aux2_texture.sample(texture_sampler, fract(primary_uv * cloud_scale + scaled_time * 0.1)).r
        : 1.0;
    float cloud0 = has_aux_texture(uniforms.aux2_texel_size)
        ? aux2_texture.sample(texture_sampler, fract(primary_uv * cloud_scale + flow_uv_offset0)).r
        : 0.0;
    float cloud1 = has_aux_texture(uniforms.aux2_texel_size)
        ? aux2_texture.sample(texture_sampler, fract(primary_uv * cloud_scale + flow_uv_offset1)).r
        : 0.0;
    float stream_noise = mix(cloud0, cloud1, blend);
    float2 base_uv = primary_uv;
#if REFRACT
    float flow_mask_length = pow(length(flow_mask), 2.0);
    base_uv += mix(flow_mask, -flow_mask, stream_noise) * cloud_background * 0.5 * stream_noise * flow_mask_length * uniforms.angle;
#endif
    sampled = sample_input(input_texture, texture_sampler, base_uv);
    stream_noise = fract(stream_noise + scaled_time * 0.2);
    float color_noise = smoothstep(0.0, 0.5, stream_noise) * smoothstep(1.0, 0.5, stream_noise);
    float flow_mask_length = pow(length(flow_mask), 2.0);
    float3 fire_color = mix(uniforms.user1.rgb, uniforms.color.rgb, color_noise);
    float blend_noise = mix(color_noise * flow_mask_length, 1.0, pow(flow_mask_length, 4.0));
    blend_noise = smoothstep(uniforms.user0.y, uniforms.user0.y + uniforms.user0.z, blend_noise);
    float stream_blend = uniforms.intensity * blend_noise;
    sampled.rgb = apply_tint_blend(sampled.rgb, fire_color, stream_blend);
#elif PHASE10_EFFECT_XRAY
    float4 mask = has_aux_texture(uniforms.aux_texel_size)
        ? aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0)))
        : float4(1.0);
    float blend = mask.a * uniforms.intensity;
#if OPACITYMASK == 1
    if (has_aux_texture(uniforms.aux3_texel_size)) {
        blend *= aux3_texture.sample(texture_sampler, clamp(stage_vertex.slot3_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float2 sprite_uv = (primary_uv - float2(0.5)) * uniforms.radius + float2(0.5);
    float2 sprite_sample = has_aux_texture(uniforms.aux2_texel_size)
        ? aux2_texture.sample(texture_sampler, clamp(sprite_uv, float2(0.0), float2(1.0))).ra
        : float2(1.0, 1.0);
    blend *= sprite_sample.x * sprite_sample.y;
    sampled.rgb = apply_tint_blend(sampled.rgb, mask.rgb, blend);
#endif

    sampled *= stage_vertex.color;
    return saturate(sampled);
}
