#ifdef GL_ES
    // To avoid weird distortion issues when rendering text etc, we want highp if possible.
    // But apparently some devices don't support it, so we have to check first.
    #if defined(GL_FRAGMENT_PRECISION_HIGH) && GL_FRAGMENT_PRECISION_HIGH == 1
        precision highp float;
    #else
        precision mediump float;
    #endif
#endif

uniform sampler2D u_sampler;

// Per text color coverage adjustment (`FontColorTransferFunction::PerColor`):
// (contrast, gamma), set while drawing with the font texture; gamma 0 is off.
uniform vec2 u_coverage_gamma;

#if NEW_SHADER_INTERFACE
    in vec4 v_rgba_in_gamma;
    in vec2 v_tc;
    out vec4 f_color;
    // a dirty hack applied to support webGL2
    #define gl_FragColor f_color
    #define texture2D texture
#else
    varying vec4 v_rgba_in_gamma;
    varying vec2 v_tc;
#endif

// -----------------------------------------------
// Adapted from
// https://www.shadertoy.com/view/llVGzG
// Originally presented in:
// Jimenez 2014, "Next Generation Post-Processing in Call of Duty"
//
// A good overview can be found in
// https://blog.demofox.org/2022/01/01/interleaved-gradient-noise-a-different-kind-of-low-discrepancy-sequence/
// via https://github.com/rerun-io/rerun/
float interleaved_gradient_noise(vec2 n) {
    float f = 0.06711056 * n.x + 0.00583715 * n.y;
    return fract(52.9829189 * fract(f));
}

vec3 dither_interleaved(vec3 rgb, float levels) {
    float noise = interleaved_gradient_noise(gl_FragCoord.xy);
    // scale down the noise slightly to ensure flat colors aren't getting dithered
    noise = (noise - 0.5) * 0.95;
    return rgb + noise / (levels - 1.0);
}

// Skia's mask gamma (SkTMaskGamma_build_correcting_lut) for a coverage and a
// text luminance: blending corrected for `gamma` against the background the
// text's inverse luminance would have, with `contrast` toward light
// backgrounds.
float coverage_for_color(float coverage, float src, float contrast, float gamma) {
    float dst = 1.0 - src;
    float lin_dst = pow(dst, gamma);
    float srca = coverage + (1.0 - coverage) * contrast * lin_dst * coverage;
    if (abs(src - dst) < 1.0 / 256.0) {
        return srca;
    }
    float lin_out = pow(src, gamma) * srca + (1.0 - srca) * lin_dst;
    return clamp((pow(lin_out, 1.0 / gamma) - dst) / (src - dst), 0.0, 1.0);
}

void main() {
    vec4 texture_in_gamma = texture2D(u_sampler, v_tc);

    float coverage = texture_in_gamma.a;
    if (u_coverage_gamma.y > 0.0 && coverage > 0.0 && coverage < 1.0
        && abs(texture_in_gamma.r - coverage) + abs(texture_in_gamma.g - coverage)
            + abs(texture_in_gamma.b - coverage) < 0.02) {
        // A white glyph pixel (colored emoji keep their colors). The text's
        // luminance as Skia takes it, from its unpremultiplied color in gamma
        // space, quantized to 3 bits.
        vec3 rgb = v_rgba_in_gamma.rgb / max(v_rgba_in_gamma.a, 1.0 / 255.0);
        float luminance = dot(rgb, vec3(54.0, 183.0, 19.0) / 256.0);
        float src = floor(clamp(luminance, 0.0, 1.0) * 255.0 / 32.0) / 7.0;
        texture_in_gamma = vec4(coverage_for_color(
            coverage, src, u_coverage_gamma.x, u_coverage_gamma.y));
    }

    // We multiply the colors in gamma space, because that's the only way to get text to look right.
    vec4 frag_color_gamma = v_rgba_in_gamma * texture_in_gamma;

    // Dither the float color down to eight bits to reduce banding.
    // This step is optional for egui backends.
#if DITHERING
    frag_color_gamma.rgb = dither_interleaved(frag_color_gamma.rgb, 256.);
#endif
    gl_FragColor = frag_color_gamma;
}
