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
    float4 user2;
    float4 user3;
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

static float3 apply_blending(float3 base, float3 blend, float opacity) {
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

// ─── Color grading helper functions ───

static float3 greyscale_vec3(float3 color) {
    float luma = dot(color, float3(0.2126, 0.7152, 0.0722));
    return float3(luma);
}

static float3 hsv2rgb(float3 c) {
    float4 K = float4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    float3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

static float3 rgb2hsv(float3 RGB) {
    float4 P = (RGB.g < RGB.b) ? float4(RGB.bg, -1.0, 2.0/3.0) : float4(RGB.gb, 0.0, -1.0/3.0);
    float4 Q = (RGB.r < P.x) ? float4(P.xyw, RGB.r) : float4(RGB.r, P.yzx);
    float C = Q.x - min(Q.w, Q.y);
    float H = abs((Q.w - Q.y) / (6.0 * C + 1e-10) + Q.z);
    float3 HCV = float3(H, C, Q.x);
    float S = HCV.y / (HCV.z + 1e-10);
    return float3(HCV.x, S, HCV.z);
}

static float3 ContrastSaturationBrightness(float3 color, float brt, float sat, float con) {
    float3 result = color;
    result = (result - 0.5) * con + 0.5 + (brt - 1.0);
    float3 grey = float3(dot(result, float3(0.2126, 0.7152, 0.0722)));
    result = mix(grey, result, sat);
    return result;
}

static float3 BlendSoftLight(float3 base, float3 blend) {
    float3 result;
    for (int i = 0; i < 3; i++) {
        if (blend[i] <= 0.5) {
            result[i] = base[i] - (1.0 - 2.0 * blend[i]) * base[i] * (1.0 - base[i]);
        } else {
            float d = (base[i] <= 0.25) ? ((16.0 * base[i] - 12.0) * base[i] + 4.0) * base[i] : sqrt(base[i]);
            result[i] = base[i] + (2.0 * blend[i] - 1.0) * (d - base[i]);
        }
    }
    return result;
}

static float3 vibrance(float3 color, float amount) {
    float luma = dot(color, float3(0.2126, 0.7152, 0.0722));
    float max_color = max(color.r, max(color.g, color.b));
    float min_color = min(color.r, min(color.g, color.b));
    float color_saturation = max_color - min_color;
    return mix(float3(luma), color, 1.0 + amount * (1.0 - (sign(amount) * color_saturation)));
}

// Bradford-adapted white balance using LMS matrices
static float3 whiteBalance(float3 color, float temp, float tint) {
    // LIN_2_LMS_MAT
    constant float3x3 LIN_2_LMS_MAT = float3x3(
        float3(3.90405e-1, 5.49941e-1, 8.92632e-3),
        float3(7.08416e-2, 9.63172e-1, 1.35775e-3),
        float3(2.31082e-2, 1.28021e-1, 9.36245e-1)
    );
    // LMS_2_LIN_MAT
    constant float3x3 LMS_2_LIN_MAT = float3x3(
        float3( 2.85847e+0, -1.62879e+0, -2.48910e-2),
        float3(-2.10182e-1,  1.15820e+0,  3.24281e-4),
        float3(-4.18120e-2, -1.18169e-1,  1.06867e+0)
    );

    float t1 = temp * 10.0 / 6.0;
    float t2 = tint * 10.0 / 6.0;
    float x = 0.31271 - t1 * (t1 < 0.0 ? 0.1 : 0.05);
    float standardIlluminantY = 2.87 * x - 3.0 * x * x - 0.27509507;
    float y = standardIlluminantY + t2 * 0.05;
    float3 w1 = float3(0.949237, 1.03542, 1.08728);
    float X = x / y;
    float Z = (1.0 - x - y) / y;
    float L = 0.7328 * X + 0.4296 - 0.1624 * Z;
    float M = -0.7036 * X + 1.6975 + 0.0061 * Z;
    float S = 0.0030 * X + 0.0136 + 0.9834 * Z;
    float3 w2 = float3(L, M, S);
    float3 balance = float3(w1.x / w2.x, w1.y / w2.y, w1.z / w2.z);
    float3 lms = LIN_2_LMS_MAT * color;
    return LMS_2_LIN_MAT * (lms * balance);
}

static float3 liftGammaGain(float3 color, float3 liftFilter, float lift, float3 gammaFilter, float gamma, float3 gainFilter, float gain) {
    color = color + (liftFilter * (lift + 1.0) / 2.0 - 0.5) * (1.0 - color);
    color = saturate(color * (1.5 - 0.5 * liftFilter * (lift + 1.0)) + 0.5 * liftFilter * (lift + 1.0) - 0.5);
    color *= gainFilter * pow(2.0, gain);
    return pow(abs(color), (1.0 / gammaFilter) * pow(2.0, -gamma));
}

static float3 hueTransform(float3 color, float angle) {
    const float3 k = float3(0.57735);
    float cosAngle = cos(radians(angle));
    return color * cosAngle + cross(k, color) * sin(radians(angle)) + k * dot(k, color) * (1.0 - cosAngle);
}

static float3 chromaAdjust(float3 color, float amount) {
    float3 hsv = rgb2hsv(color);
    hsv.y += amount;
    return hsv2rgb(hsv);
}

static float3 invertValue(float3 color) {
    float3 hsv = rgb2hsv(color);
    hsv.z = 1.0 - hsv.z;
    return hsv2rgb(hsv);
}

static float3 splitTone(float3 color, float shadows, float highlights, float balance, float3 shadowTint, float3 highlightTint) {
    float luma = dot(color, float3(0.2126, 0.7152, 0.0722));
    float t = saturate(luma + balance);
    float3 s = mix(0.5, shadowTint * ((1.0 + shadows * 1.5) / 2.0), 1.0 - t);
    float3 h = mix(0.5, highlightTint * ((1.0 + highlights * 1.5) / 2.0), t);
    float3 result = BlendSoftLight(color, s);
    return BlendSoftLight(result, h);
}

// Selective color targeting: returns 0..1 mask based on distance to target
static float selectColor(float3 initColor, float3 color, float3 replaceBaseColor, float tollerance, float smooth_exp) {
    float3 hsv = rgb2hsv(color);
    float3 baseHsv = rgb2hsv(replaceBaseColor);
#if MODE == 1
    float dist = min(abs(hsv.x - baseHsv.x), 1.0 - abs(hsv.x - baseHsv.x)) + max(abs(hsv.y - baseHsv.y), 0.0) + max(abs(hsv.z - baseHsv.z), 0.0);
#elif MODE == 2
    float dist = min(abs(hsv.x - baseHsv.x), 1.0 - abs(hsv.x - baseHsv.x));
#elif MODE == 3
    float dist = max(abs(hsv.y - baseHsv.y), 0.0);
#elif MODE == 4
    float dist = max(abs(hsv.z - baseHsv.z), 0.0);
#else
    float dist = 0.0;
#endif
    return saturate(pow(tollerance / dist, 1.0 / smooth_exp));
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
    sampled.rgb = apply_blending(sampled.rgb * uniforms.color.rgb, sampled.rgb * uniforms.user1.rgb, pulse);
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
    sampled.rgb = apply_blending(sampled.rgb, uniforms.color.rgb, strength);
#elif PHASE10_EFFECT_SCROLL
    float2 scroll_speed = uniforms.user0.xy;
    float2 repeat = max(abs(uniforms.user0.zw), float2(0.01));
    float2 signed_scroll = sign(scroll_speed) * pow(abs(scroll_speed), float2(2.0)) * uniforms.time;
    sampled = input_texture.sample(
        texture_sampler,
        fract((primary_uv + signed_scroll) * repeat)
    );
#elif PHASE10_EFFECT_ACESTONEMAP
    float strength_ace = max(uniforms.intensity, 0.0);
    float3 col = sampled.rgb * strength_ace;
    constant float3x3 ACESInputMat = float3x3(
        float3(0.59719, 0.35458, 0.04823),
        float3(0.07600, 0.90834, 0.01566),
        float3(0.02840, 0.13383, 0.83777)
    );
    constant float3x3 ACESOutputMat = float3x3(
        float3( 1.60475, -0.53108, -0.07367),
        float3(-0.10208,  1.10813, -0.00605),
        float3(-0.00327, -0.07276,  1.07602)
    );
    col = ACESInputMat * col;
    col = (col * (col + 0.0245786) - 0.000090537) / (col * (0.983729 * col + 0.4329510) + 0.238081);
    sampled.rgb = saturate(ACESOutputMat * col);
    sampled.a = 1.0;
#elif PHASE10_EFFECT_GRADIENTCOLOR
    float4 scene = sampled;
    float gradient_mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        gradient_mask = aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
    float timer = sin(uniforms.time * uniforms.user1.x);
    float3 color1 = uniforms.color.rgb;
    float3 color2 = uniforms.user0.xyz;
    float amount = max(abs(uniforms.user0.w), 0.01);
    float speed_hue = uniforms.speed;
    float osc = uniforms.user1.x;
    float opacity_g = max(uniforms.intensity, 0.0);

#if AXIS
    float colorDistanceBlend = pow(primary_uv.y, amount);
#else
    float colorDistanceBlend = pow(primary_uv.x, amount);
#endif
    if (osc > 0.0) {
        colorDistanceBlend += sin(uniforms.time * osc);
    }

    float3 resultColor = mix(color1, color2, colorDistanceBlend);
    float3 hsv = rgb2hsv(resultColor);
    hsv.x = fract(hsv.x + uniforms.time * speed_hue);
    resultColor = hsv2rgb(hsv);

    float3 finalColor = apply_blending(mix(scene.rgb, resultColor, scene.a), resultColor, opacity_g * gradient_mask);
    sampled.rgb = finalColor;
#elif PHASE10_EFFECT_COLORGRADING
    float4 albedo = sampled;
    float4 baseAlbedo = albedo;
    float cg_mask = 1.0;
#if MASK
    if (has_aux_texture(uniforms.aux_texel_size)) {
        cg_mask = aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
    }
#endif
#if INVERTMASK
    cg_mask = 1.0 - cg_mask;
#endif

    float a_alpha = max(uniforms.intensity, 0.0);
    if (cg_mask > 0.0 && a_alpha > 0.0) {
        float startGamma = uniforms.radius; // a_displayInitGamma
        float endGamma = uniforms.angle;    // a_displayGamma
#if !GAMMA
        startGamma = 2.2;
        endGamma = 2.2;
#endif

#if LINEAR
        albedo.rgb = pow(albedo.rgb, float3(startGamma));
        baseAlbedo.rgb = pow(baseAlbedo.rgb, float3(startGamma));
#endif
#if GREYSCALE
        albedo.rgb = greyscale_vec3(albedo.rgb);
#endif
#if INVERTCOLOR
        albedo.rgb = 1.0 - albedo.rgb;
#endif
#if INVERTVALUE
        albedo.rgb = invertValue(albedo.rgb);
#endif

#if MODE != 0
        float colorMultiplier = selectColor(baseAlbedo.rgb, albedo.rgb, uniforms.user3.xyz, uniforms.user1.w, uniforms.user2.w);
#else
        float colorMultiplier = 1.0;
#endif

        // dispatch based on PROPERTIES
#if PROPERTIES == 0
        {
            float c_brightness = uniforms.user0.x;
            float c_contrast = uniforms.user0.y;
            float c_saturation = uniforms.user0.z;
            albedo.rgb = ContrastSaturationBrightness(albedo.rgb, 1.0 + c_brightness, 1.0 + c_saturation, 1.0 + c_contrast);
            float3 ch = uniforms.color.rgb; // a_channelMultiplier
            albedo.rgb = mix(baseAlbedo.rgb, albedo.rgb, ch * colorMultiplier * cg_mask * a_alpha);
        }
#elif PROPERTIES == 1
        {
            float c_exposure = uniforms.user0.x;
            float c_blackLevel = uniforms.user0.y;
            float c_vibrance_val = uniforms.user0.z;
            if (c_exposure != 0.0) albedo.rgb *= pow(2.0, c_exposure);
            if (c_vibrance_val != 0.0) albedo.rgb = vibrance(albedo.rgb, c_vibrance_val);
            if (c_blackLevel != 0.0) albedo.rgb -= c_blackLevel / 10.0;
            float3 ch = uniforms.color.rgb;
            albedo.rgb = mix(baseAlbedo.rgb, albedo.rgb, ch * colorMultiplier * cg_mask * a_alpha);
        }
#elif PROPERTIES == 2
        {
            float c_hueShift = uniforms.user0.x;
            float c_chroma_val = uniforms.user0.y;
            float3 c_colorFilter = uniforms.color.rgb;
            albedo.rgb *= c_colorFilter;
            if (c_hueShift != 0.0) albedo.rgb = hueTransform(albedo.rgb, c_hueShift);
            if (c_chroma_val != 0.0) albedo.rgb = chromaAdjust(albedo.rgb, c_chroma_val);
            float3 ch = uniforms.user1.xyz; // a_channelMultiplier in user1 for this mode
            albedo.rgb = mix(baseAlbedo.rgb, albedo.rgb, ch * colorMultiplier * cg_mask * a_alpha);
        }
#elif PROPERTIES == 3
        {
            float c_colorTemp = uniforms.user0.x;
            float c_whiteTint = uniforms.user0.y;
            albedo.rgb = whiteBalance(albedo.rgb, c_colorTemp, c_whiteTint);
            float3 ch = uniforms.color.rgb;
            albedo.rgb = mix(baseAlbedo.rgb, albedo.rgb, ch * colorMultiplier * cg_mask * a_alpha);
        }
#elif PROPERTIES == 4
        {
            float c_shadows = uniforms.user0.x;
            float c_highlights = uniforms.user0.y;
            float c_HSbalance = uniforms.user0.z;
            float3 c_shadowTint = uniforms.color.rgb;
            float3 c_highlightTint = uniforms.user1.xyz;
            albedo.rgb = splitTone(albedo.rgb, c_shadows, c_highlights, c_HSbalance, c_shadowTint, c_highlightTint);
            float3 ch = uniforms.user2.xyz; // a_channelMultiplier in user2 for this mode
            albedo.rgb = mix(baseAlbedo.rgb, albedo.rgb, ch * colorMultiplier * cg_mask * a_alpha);
        }
#elif PROPERTIES == 5
        {
            float c_gamma = uniforms.user0.x;
            float c_gain = uniforms.user0.y;
            float c_lift = uniforms.user0.z;
            float3 c_LiftColorFilter = uniforms.color.rgb;
            float3 c_GammaColorFilter = uniforms.user1.xyz;
            float3 c_GainColorFilter = uniforms.user2.xyz;
            albedo.rgb = liftGammaGain(albedo.rgb, c_LiftColorFilter, c_lift, c_GammaColorFilter, c_gamma, c_GainColorFilter, c_gain);
            float3 ch = uniforms.user3.xyz; // a_channelMultiplier in user3 for this mode
            albedo.rgb = mix(baseAlbedo.rgb, albedo.rgb, ch * colorMultiplier * cg_mask * a_alpha);
        }
#elif PROPERTIES == 6
        {
            float3 c_red = uniforms.color.rgb;
            float3 c_green = uniforms.user1.xyz;
            float3 c_blue = uniforms.user2.xyz;
            float3 c_matrixOffset = uniforms.user0.xyz;
            float3x3 colorMatrix = float3x3(c_red, c_green, c_blue);
            albedo.rgb = albedo.rgb * colorMatrix + c_matrixOffset;
            float3 ch = uniforms.user3.xyz;
            albedo.rgb = mix(baseAlbedo.rgb, albedo.rgb, ch * colorMultiplier * cg_mask * a_alpha);
        }
#endif

#if LINEAR
        albedo.rgb = pow(albedo.rgb, float3(1.0 / startGamma));
        baseAlbedo.rgb = pow(baseAlbedo.rgb, float3(1.0 / startGamma));
#endif

        float blend_amount = cg_mask * albedo.a * a_alpha;
        albedo.rgb = apply_blending(baseAlbedo.rgb, albedo.rgb, blend_amount);
        albedo.rgb = pow(albedo.rgb, float3(2.2 / endGamma));
    }
    sampled = albedo;
#elif PHASE10_EFFECT_SHARPENFILTER
    float sharpen_mask = 1.0;
#if OPACITY
    if (has_aux_texture(uniforms.aux_texel_size)) {
        sharpen_mask = aux_texture.sample(texture_sampler, clamp(stage_vertex.slot1_uv, float2(0.0), float2(1.0))).r;
#if INVERT
        sharpen_mask = 1.0 - sharpen_mask;
#endif
    }
#endif
    if (sharpen_mask > 0.1) {
        float strength_sh = max(uniforms.intensity, 0.0);
        float radius_sh = max(uniforms.radius, 0.01);
        float2 ts = uniforms.texel_size * radius_sh;
        float4 c1 = sample_input(input_texture, texture_sampler, primary_uv + float2(-ts.x, -ts.y));
        float4 c2 = sample_input(input_texture, texture_sampler, primary_uv + float2(0.0, -ts.y));
        float4 c3 = sample_input(input_texture, texture_sampler, primary_uv + float2(ts.x, -ts.y));
        float4 c4 = sample_input(input_texture, texture_sampler, primary_uv + float2(-ts.x, 0.0));
        float4 c5 = sample_input(input_texture, texture_sampler, primary_uv + float2(ts.x, 0.0));
        float4 c6 = sample_input(input_texture, texture_sampler, primary_uv + float2(-ts.x, ts.y));
        float4 c7 = sample_input(input_texture, texture_sampler, primary_uv + float2(0.0, ts.y));
        float4 c8 = sample_input(input_texture, texture_sampler, primary_uv + float2(ts.x, ts.y));
        float4 blur = (c1 + c3 + c6 + c8 + 2.0 * (c2 + c4 + c5 + c7) + 4.0 * sampled) / 16.0;
        sampled = (1.0 + strength_sh * sharpen_mask) * sampled - strength_sh * sharpen_mask * blur;
    }
#elif PHASE10_EFFECT_LUTLOADER
    float4 textureColor = sampled;
#if CLAMP
    textureColor = saturate(textureColor);
#endif

    float lut_mult = max(uniforms.intensity, 0.0);
    float tc = uniforms.user0.x; // g_TranslucentCompensation
    float blendAmount_lut = lut_mult + tc * (1.0 - textureColor.a);

    if (has_aux_texture(uniforms.aux_texel_size)) {
#if QUAD_SIZE == 64
        float blueColor = textureColor.b * 63.0;
        float quad1y = floor(floor(blueColor) * 0.125);
        float quad2y = floor(ceil(blueColor) * 0.125);
        float2 texPos1;
        texPos1.x = ((floor(blueColor) - (quad1y * 8.0)) * 0.125) + 0.0009765625 + ((0.125 - 0.001953125) * textureColor.r);
        texPos1.y = (quad1y * 0.125) + 0.0009765625 + ((0.125 - 0.001953125) * textureColor.g);
        float2 texPos2;
        texPos2.x = ((ceil(blueColor) - (quad2y * 8.0)) * 0.125) + 0.0009765625 + ((0.125 - 0.001953125) * textureColor.r);
        texPos2.y = (quad2y * 0.125) + 0.0009765625 + ((0.125 - 0.001953125) * textureColor.g);
#else
        // QUAD_SIZE == 16 (default)
        float blueColor = textureColor.b * 15.0;
        float quad1y = floor(floor(blueColor) * 0.25);
        float quad2y = floor(ceil(blueColor) * 0.25);
        float2 texPos1;
        texPos1.x = ((floor(blueColor) - (quad1y * 4.0)) * 0.25) + 0.0078125 + ((0.25 - 0.015625) * textureColor.r);
        texPos1.y = (quad1y * 0.25) + 0.0078125 + ((0.25 - 0.015625) * textureColor.g);
        float2 texPos2;
        texPos2.x = ((ceil(blueColor) - (quad2y * 4.0)) * 0.25) + 0.0078125 + ((0.25 - 0.015625) * textureColor.r);
        texPos2.y = (quad2y * 0.25) + 0.0078125 + ((0.25 - 0.015625) * textureColor.g);
#endif

#if LUT_FLIP_Y
        texPos1.y = 1.0 - texPos1.y;
        texPos2.y = 1.0 - texPos2.y;
#endif

        float3 lut1 = aux_texture.sample(texture_sampler, texPos1).rgb;
        float3 lut2 = aux_texture.sample(texture_sampler, texPos2).rgb;
        float3 lut_color = mix(lut1, lut2, fract(blueColor));

        sampled.rgb = apply_blending(textureColor.rgb, lut_color, blendAmount_lut);
    }
#endif

    sampled *= stage_vertex.color;
    return saturate(sampled);
}
