#version 100

//_DEFINES_

precision highp float;
uniform sampler2D tex;
uniform float alpha;
uniform float sdr_reference_luminance;
uniform float target_peak_luminance;
uniform mat3 gamut_matrix;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

float pq_oetf_channel(float value) {
    const float m1 = 2610.0 / 16384.0;
    const float m2 = 2523.0 / 4096.0 * 128.0;
    const float c1 = 3424.0 / 4096.0;
    const float c2 = 2413.0 / 4096.0 * 32.0;
    const float c3 = 2392.0 / 4096.0 * 32.0;
    float lm = pow(max(value, 0.0), m1);
    return pow((c1 + c2 * lm) / (1.0 + c3 * lm), m2);
}

vec3 pq_oetf(vec3 value) {
    return vec3(
        pq_oetf_channel(value.r),
        pq_oetf_channel(value.g),
        pq_oetf_channel(value.b)
    );
}

void main() {
    vec4 color = texture2D(tex, v_coords);
    if (color.a > 0.0)
        color.rgb /= color.a;

    vec3 nits = gamut_matrix * color.rgb * sdr_reference_luminance;
    float max_channel = max(max(nits.r, nits.g), nits.b);
    if (max_channel > target_peak_luminance)
        nits *= target_peak_luminance / max_channel;

    color.rgb = pq_oetf(max(nits, vec3(0.0)) / 10000.0) * color.a;
    color *= alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    gl_FragColor = color;
}
