//! Color data model and math for HDR/wide-gamut composition.
//!
//! Pure software, no GPU or hardware dependency. Internal math runs in `f64`
//! for precision; values crossing into shader uniforms are converted to `f32`
//! at the boundary and must pass through [`sanitize`] / [`sanitize_mat3`] so no
//! NaN or infinity ever reaches the GPU.
//!
//! Every color space used by the first HDR slice (sRGB, PQ/BT.2020,
//! Windows-scRGB) is D65-white, so [`chromatic_adaptation`] is an identity stub.
//! Full Bradford/CAT02 adaptation is deferred until ICC support needs it.

use glam::{DMat3, DVec3};

/// Default SDR reference (diffuse) white luminance in cd/m².
///
/// This is the luminance that "SDR white" maps to when composited on an HDR
/// output. Configurable per output; 203 cd/m² is the ITU-R BT.2408 reference.
pub const SDR_REFERENCE_WHITE: f64 = 203.0;

/// PQ (ST 2084) peak luminance in cd/m²: normalized 1.0 encodes to this.
pub const PQ_MAX_LUMINANCE: f64 = 10000.0;

/// Windows scRGB luminance in cd/m² represented by linear channel value 1.0.
pub const SCRGB_UNIT_LUMINANCE: f64 = 80.0;

/// CIE 1931 xy chromaticity coordinates of the RGB primaries and white point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primaries {
    pub red: [f64; 2],
    pub green: [f64; 2],
    pub blue: [f64; 2],
    pub white: [f64; 2],
}

impl Primaries {
    /// BT.709 primaries with a D65 white point (also used by sRGB and scRGB).
    pub const BT709: Primaries = Primaries {
        red: [0.640, 0.330],
        green: [0.300, 0.600],
        blue: [0.150, 0.060],
        white: [0.3127, 0.3290],
    };

    /// BT.2020 primaries with a D65 white point.
    pub const BT2020: Primaries = Primaries {
        red: [0.708, 0.292],
        green: [0.170, 0.797],
        blue: [0.131, 0.046],
        white: [0.3127, 0.3290],
    };

    /// Matrix converting linear RGB in these primaries to CIE XYZ.
    ///
    /// Bruce Lindbloom's derivation: build the primary XYZ matrix, solve for the
    /// per-channel scaling that reproduces the white point at Y=1, then scale.
    pub fn rgb_to_xyz(self) -> DMat3 {
        // xy (with implicit Y=1) -> XYZ.
        let to_xyz = |xy: [f64; 2]| {
            let [x, y] = xy;
            DVec3::new(x / y, 1.0, (1.0 - x - y) / y)
        };

        let m = DMat3::from_cols(to_xyz(self.red), to_xyz(self.green), to_xyz(self.blue));
        let white = to_xyz(self.white);
        let scale = m.inverse() * white;
        m * DMat3::from_diagonal(scale)
    }

    /// Matrix converting CIE XYZ to linear RGB in these primaries.
    pub fn xyz_to_rgb(self) -> DMat3 {
        self.rgb_to_xyz().inverse()
    }
}

/// Chromatic adaptation from one white point to another.
///
/// Identity stub: every first-slice color space is D65, so no adaptation is
/// needed. Replace with Bradford/CAT02 when non-D65 (ICC) spaces are added.
pub fn chromatic_adaptation(_src_white: [f64; 2], _dst_white: [f64; 2]) -> DMat3 {
    // ponytail: identity until a non-D65 space actually exists (plan Phase 1).
    debug_assert_eq!(
        _src_white, _dst_white,
        "chromatic_adaptation is an identity stub; non-D65 spaces are not supported yet"
    );
    DMat3::IDENTITY
}

/// Opto-electronic transfer characteristics of encoded pixel values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFunction {
    /// Linear light (Windows scRGB uses this over BT.709 primaries).
    Linear,
    /// sRGB piecewise curve (IEC 61966-2-1).
    Srgb,
    /// SMPTE ST 2084 (PQ). Normalized so 1.0 == [`PQ_MAX_LUMINANCE`] cd/m².
    St2084Pq,
}

impl TransferFunction {
    /// EOTF: decode an encoded value to linear light.
    ///
    /// sRGB is sign-preserving so extended-range (scRGB-style) values survive;
    /// PQ is not defined for negatives and clamps them to zero.
    pub fn eotf(self, e: f64) -> f64 {
        match self {
            TransferFunction::Linear => e,
            TransferFunction::Srgb => sign_preserving(e, srgb_eotf),
            TransferFunction::St2084Pq => pq_eotf(e.max(0.0)),
        }
    }

    /// Inverse EOTF: encode linear light back to an encoded value.
    pub fn oetf(self, l: f64) -> f64 {
        match self {
            TransferFunction::Linear => l,
            TransferFunction::Srgb => sign_preserving(l, srgb_oetf),
            TransferFunction::St2084Pq => pq_oetf(l.max(0.0)),
        }
    }
}

fn sign_preserving(x: f64, f: impl Fn(f64) -> f64) -> f64 {
    if x < 0.0 {
        -f(-x)
    } else {
        f(x)
    }
}

fn srgb_eotf(e: f64) -> f64 {
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_oetf(l: f64) -> f64 {
    if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

// ST 2084 constants (SMPTE, exact rationals).
const PQ_M1: f64 = 2610.0 / 16384.0;
const PQ_M2: f64 = 2523.0 / 4096.0 * 128.0;
const PQ_C1: f64 = 3424.0 / 4096.0;
const PQ_C2: f64 = 2413.0 / 4096.0 * 32.0;
const PQ_C3: f64 = 2392.0 / 4096.0 * 32.0;

/// PQ EOTF: encoded [0,1] -> normalized linear [0,1] (1.0 == 10000 cd/m²).
fn pq_eotf(e: f64) -> f64 {
    let ep = e.powf(1.0 / PQ_M2);
    let num = (ep - PQ_C1).max(0.0);
    let den = PQ_C2 - PQ_C3 * ep;
    (num / den).powf(1.0 / PQ_M1)
}

/// PQ inverse EOTF: normalized linear [0,1] -> encoded [0,1].
fn pq_oetf(l: f64) -> f64 {
    let lm = l.powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * lm) / (1.0 + PQ_C3 * lm)).powf(PQ_M2)
}

/// Rendering intent, matching the color-management-v1 / ICC enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderingIntent {
    Perceptual,
    RelativeColorimetric,
    Saturation,
    AbsoluteColorimetric,
}

/// Mastering display volume (SMPTE ST 2086), when signalled by the client.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasteringDisplay {
    pub primaries: Primaries,
    pub min_luminance: f64,
    pub max_luminance: f64,
}

/// Immutable description of a color space and its luminance characteristics.
///
/// Construct once via a canonical constructor (or from client protocol state)
/// and treat as read-only; nothing mutates it in place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageDescription {
    pub primaries: Primaries,
    pub transfer: TransferFunction,
    /// Minimum display/mastering luminance in cd/m².
    pub min_luminance: f64,
    /// Maximum display/mastering luminance in cd/m².
    pub max_luminance: f64,
    /// Reference (diffuse) white luminance in cd/m².
    pub reference_white: f64,
    /// Absolute luminance represented by decoded channel value 1.0.
    pub luminance_scale: f64,
    pub mastering: Option<MasteringDisplay>,
    /// Maximum content light level (cd/m²), when supplied.
    pub max_cll: Option<f64>,
    /// Maximum frame-average light level (cd/m²), when supplied.
    pub max_fall: Option<f64>,
    pub rendering_intent: RenderingIntent,
}

impl ImageDescription {
    /// Canonical sRGB (BT.709 primaries, sRGB transfer, SDR white).
    pub fn srgb() -> Self {
        ImageDescription {
            primaries: Primaries::BT709,
            transfer: TransferFunction::Srgb,
            min_luminance: 0.0,
            max_luminance: SDR_REFERENCE_WHITE,
            reference_white: SDR_REFERENCE_WHITE,
            luminance_scale: SDR_REFERENCE_WHITE,
            mastering: None,
            max_cll: None,
            max_fall: None,
            rendering_intent: RenderingIntent::Perceptual,
        }
    }

    /// Canonical HDR10 space: PQ transfer over BT.2020 primaries.
    pub fn pq_bt2020() -> Self {
        ImageDescription {
            primaries: Primaries::BT2020,
            transfer: TransferFunction::St2084Pq,
            min_luminance: 0.0,
            max_luminance: PQ_MAX_LUMINANCE,
            reference_white: SDR_REFERENCE_WHITE,
            luminance_scale: PQ_MAX_LUMINANCE,
            mastering: None,
            max_cll: None,
            max_fall: None,
            rendering_intent: RenderingIntent::Perceptual,
        }
    }

    /// Canonical Windows scRGB: linear light over BT.709 primaries, 1.0 == 80 cd/m².
    pub fn windows_scrgb() -> Self {
        ImageDescription {
            primaries: Primaries::BT709,
            transfer: TransferFunction::Linear,
            min_luminance: 0.0,
            max_luminance: PQ_MAX_LUMINANCE,
            // Windows-scRGB uses 1.0 == 80 cd/m², but the protocol recommends
            // 203 cd/m² when a compositor needs an assumed reference white.
            reference_white: SDR_REFERENCE_WHITE,
            luminance_scale: SCRGB_UNIT_LUMINANCE,
            mastering: None,
            max_cll: None,
            max_fall: None,
            rendering_intent: RenderingIntent::Perceptual,
        }
    }
}

/// Matrix converting linear RGB from `src` primaries to `dst` primaries.
///
/// Chains RGB->XYZ, chromatic adaptation (identity for D65), XYZ->RGB. This is
/// the matrix a shader uses to move a source surface into the output's gamut.
pub fn primary_conversion(src: Primaries, dst: Primaries) -> DMat3 {
    dst.xyz_to_rgb() * chromatic_adaptation(src.white, dst.white) * src.rgb_to_xyz()
}

/// Initial tone-mapping curve: extended Reinhard, mapping linear luminance in
/// `[0, l_white]` toward `[0, 1]` while preserving zero and monotonicity.
///
/// Placeholder for Phase 3's real per-output tone/gamut mapping.
pub fn tone_map_reinhard(l: f64, l_white: f64) -> f64 {
    // ponytail: simple curve; Phase 3 replaces with a proper operator.
    debug_assert!(l_white > 0.0);
    let l = l.max(0.0);
    (l * (1.0 + l / (l_white * l_white))) / (1.0 + l)
}

/// Initial gamut policy: hard-clip each channel into `[min, max]`.
///
/// Out-of-gamut conversion produces small negatives; clipping keeps colors
/// valid. Perceptual gamut compression is a documented follow-up.
pub fn gamut_clip(rgb: DVec3, min: f64, max: f64) -> DVec3 {
    rgb.clamp(DVec3::splat(min), DVec3::splat(max))
}

/// Replace a non-finite value with zero. Use at the shader-uniform boundary.
#[inline]
pub fn sanitize(x: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

/// Convert an `f64` color matrix to an `f32` `glam::Mat3` for shader upload,
/// sanitizing any non-finite entry to zero.
pub fn sanitize_mat3(m: DMat3) -> glam::Mat3 {
    let c = m.to_cols_array();
    glam::Mat3::from_cols_array(&[
        sanitize(c[0] as f32),
        sanitize(c[1] as f32),
        sanitize(c[2] as f32),
        sanitize(c[3] as f32),
        sanitize(c[4] as f32),
        sanitize(c[5] as f32),
        sanitize(c[6] as f32),
        sanitize(c[7] as f32),
        sanitize(c[8] as f32),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn srgb_reference_points() {
        // Anchors: endpoints, midpoint, and the piecewise-boundary value.
        assert!(approx(srgb_eotf(0.0), 0.0, 1e-12));
        assert!(approx(srgb_eotf(1.0), 1.0, 1e-12));
        assert!(approx(srgb_eotf(0.5), 0.214041, 1e-6));
        assert!(approx(srgb_eotf(0.04045), 0.0031308, 1e-6));
        assert!(approx(srgb_oetf(0.5), 0.735357, 1e-6));
    }

    #[test]
    fn srgb_round_trip() {
        for i in 0..=1000 {
            let e = i as f64 / 1000.0;
            assert!(approx(srgb_oetf(srgb_eotf(e)), e, 1e-9));
        }
    }

    #[test]
    fn srgb_sign_preserving() {
        let tf = TransferFunction::Srgb;
        assert!(approx(tf.eotf(-0.5), -srgb_eotf(0.5), 1e-12));
        assert!(approx(tf.oetf(tf.eotf(-0.75)), -0.75, 1e-9));
    }

    #[test]
    fn pq_reference_points() {
        // Endpoints and the widely-cited 100 cd/m² (L=0.01) -> ~0.5081 code value.
        assert!(approx(pq_eotf(0.0), 0.0, 1e-9));
        assert!(approx(pq_eotf(1.0), 1.0, 1e-9));
        assert!(approx(pq_oetf(1.0), 1.0, 1e-9));
        assert!(approx(pq_oetf(0.01), 0.508078, 1e-4));
    }

    #[test]
    fn pq_round_trip() {
        for i in 0..=1000 {
            let e = i as f64 / 1000.0;
            assert!(approx(pq_oetf(pq_eotf(e)), e, 1e-6));
        }
    }

    #[test]
    fn pq_clamps_negatives() {
        // PQ is undefined for negatives: input is clamped to zero, so negative
        // input yields the same (finite) result as zero input. Note pq_oetf(0)
        // is c1^m2 (~7e-7), not exactly 0 — a real property of the inverse EOTF.
        let tf = TransferFunction::St2084Pq;
        assert_eq!(tf.eotf(-1.0), tf.eotf(0.0));
        assert_eq!(tf.oetf(-1.0), tf.oetf(0.0));
        assert!(tf.eotf(-1.0).is_finite() && tf.eotf(-1.0) >= 0.0);
        assert!(tf.oetf(-1.0).is_finite() && tf.oetf(-1.0) >= 0.0);
    }

    #[test]
    fn bt709_matrix_matches_published() {
        // Bruce Lindbloom's sRGB D65 RGB->XYZ reference, first row.
        let m = Primaries::BT709.rgb_to_xyz().to_cols_array();
        // Column-major: X row = elements [0], [3], [6].
        assert!(approx(m[0], 0.4124564, 1e-4));
        assert!(approx(m[3], 0.3575761, 1e-4));
        assert!(approx(m[6], 0.1804375, 1e-4));
    }

    #[test]
    fn matrix_round_trips() {
        for p in [Primaries::BT709, Primaries::BT2020] {
            let id = p.xyz_to_rgb() * p.rgb_to_xyz();
            let d = (id - DMat3::IDENTITY).to_cols_array();
            for v in d {
                assert!(v.abs() < 1e-9, "round-trip drift {v}");
            }
        }
        // Cross-gamut conversion must also invert cleanly.
        let fwd = primary_conversion(Primaries::BT709, Primaries::BT2020);
        let back = primary_conversion(Primaries::BT2020, Primaries::BT709);
        let d = (fwd * back - DMat3::IDENTITY).to_cols_array();
        for v in d {
            assert!(v.abs() < 1e-9, "cross round-trip drift {v}");
        }
    }

    #[test]
    fn no_nan_or_inf_reaches_uniforms() {
        // Feed pathological inputs through the conversion + sanitize boundary.
        let mut m = primary_conversion(Primaries::BT2020, Primaries::BT709);
        m.x_axis.x = f64::NAN;
        m.y_axis.y = f64::INFINITY;
        m.z_axis.z = f64::NEG_INFINITY;
        for v in sanitize_mat3(m).to_cols_array() {
            assert!(v.is_finite());
        }

        // Transfer functions over a wide, includes-garbage range stay finite
        // once passed through sanitize.
        for tf in [
            TransferFunction::Linear,
            TransferFunction::Srgb,
            TransferFunction::St2084Pq,
        ] {
            for &x in &[-1e30, -1.0, 0.0, 0.5, 1.0, 1e30] {
                assert!(sanitize(tf.eotf(x) as f32).is_finite());
                assert!(sanitize(tf.oetf(x) as f32).is_finite());
            }
        }
    }

    #[test]
    fn tone_map_is_monotonic_and_bounded() {
        let mut prev = tone_map_reinhard(0.0, 4.0);
        assert_eq!(prev, 0.0);
        let mut x = 0.0;
        while x <= 10.0 {
            let y = tone_map_reinhard(x, 4.0);
            assert!(y >= prev - 1e-12, "not monotonic at {x}");
            assert!(y.is_finite());
            prev = y;
            x += 0.05;
        }
    }

    #[test]
    fn tone_map_at_l_white_one_is_identity() {
        // l_white == 1.0 means the source has no headroom over SDR white
        // (ratio == 1.0): the render shaders skip mapping in this case, so the
        // Rust reference formula must reduce to the identity here too.
        for x in [0.0, 0.25, 0.5, 1.0, 2.0] {
            assert!(approx(tone_map_reinhard(x, 1.0), x, 1e-9));
        }
    }

    #[test]
    fn gamut_clip_bounds() {
        let v = gamut_clip(DVec3::new(-0.2, 0.5, 2.0), 0.0, 1.0);
        assert_eq!(v, DVec3::new(0.0, 0.5, 1.0));
    }

    #[test]
    fn canonical_descriptions() {
        assert_eq!(
            ImageDescription::srgb().reference_white,
            SDR_REFERENCE_WHITE
        );
        assert_eq!(ImageDescription::pq_bt2020().primaries, Primaries::BT2020);
        assert_eq!(ImageDescription::pq_bt2020().luminance_scale, 10000.0);
        let scrgb = ImageDescription::windows_scrgb();
        assert_eq!(scrgb.transfer, TransferFunction::Linear);
        assert_eq!(scrgb.reference_white, SDR_REFERENCE_WHITE);
        assert_eq!(scrgb.luminance_scale, SCRGB_UNIT_LUMINANCE);
        assert_eq!(SCRGB_UNIT_LUMINANCE, 80.0);
    }
}
