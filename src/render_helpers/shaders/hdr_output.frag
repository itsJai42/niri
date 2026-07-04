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

    // Reinhard rolloff toward the output's peak luminance instead of a hard
    // clip, worked in SDR-white-normalized units (matches
    // render_helpers::color::tone_map_reinhard's tested domain) then scaled
    // back to nits for the PQ encode.
    vec3 l = gamut_matrix * max(color.rgb, vec3(0.0));
    float peak = max(max(l.r, l.g), l.b);
    if (peak > 0.0) {
        float l_white = max(target_peak_luminance / sdr_reference_luminance, 1.0);
        float mapped = peak * (1.0 + peak / (l_white * l_white)) / (1.0 + peak);
        l *= mapped / peak;
    }
    vec3 nits = max(l, vec3(0.0)) * sdr_reference_luminance;

    color.rgb = pq_oetf(nits / 10000.0) * color.a;
    color *= alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    gl_FragColor = color;
}
