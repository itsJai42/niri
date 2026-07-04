#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision highp float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
uniform float source_tf;
uniform float luminance_scale;
uniform mat3 gamut_matrix;
uniform float output_sdr;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

float srgb_eotf_channel(float value) {
    float magnitude = abs(value);
    float linear = magnitude <= 0.04045
        ? magnitude / 12.92
        : pow((magnitude + 0.055) / 1.055, 2.4);
    return sign(value) * linear;
}

vec3 srgb_eotf(vec3 value) {
    return vec3(
        srgb_eotf_channel(value.r),
        srgb_eotf_channel(value.g),
        srgb_eotf_channel(value.b)
    );
}

vec3 pq_eotf(vec3 value) {
    const float m1 = 2610.0 / 16384.0;
    const float m2 = 2523.0 / 4096.0 * 128.0;
    const float c1 = 3424.0 / 4096.0;
    const float c2 = 2413.0 / 4096.0 * 32.0;
    const float c3 = 2392.0 / 4096.0 * 32.0;
    vec3 p = pow(max(value, vec3(0.0)), vec3(1.0 / m2));
    return pow(max(p - c1, vec3(0.0)) / (c2 - c3 * p), vec3(1.0 / m1));
}

float srgb_oetf_channel(float value) {
    return value <= 0.0031308
        ? value * 12.92
        : 1.055 * pow(value, 1.0 / 2.4) - 0.055;
}

vec3 srgb_oetf(vec3 value) {
    return vec3(
        srgb_oetf_channel(value.r),
        srgb_oetf_channel(value.g),
        srgb_oetf_channel(value.b)
    );
}

void main() {
    vec4 color = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    color.a = 1.0;
#endif

    if (color.a > 0.0)
        color.rgb /= color.a;

    if (source_tf == 1.0)
        color.rgb = srgb_eotf(color.rgb);
    else if (source_tf == 2.0)
        color.rgb = pq_eotf(color.rgb);

    color.rgb = gamut_matrix * color.rgb * luminance_scale;
    if (output_sdr == 1.0) {
        float peak = max(max(color.r, color.g), color.b);
        if (peak > 1.0)
            color.rgb /= peak;
        color.rgb = srgb_oetf(max(color.rgb, vec3(0.0)));
    }
    color.rgb *= color.a;
    color *= alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
